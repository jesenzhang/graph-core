//! Typed execution stream primitives.
//!
//! This crate intentionally does not expose graph nodes or edges. Its bounded
//! policies describe transport behavior only; workflow truth remains in the
//! workflow and recovery crates.

use graph_core::Id;
use std::collections::VecDeque;
use std::fmt;

/// Monotonic sequence number within one stream.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Sequence(u64);

impl Sequence {
    /// First valid sequence number.
    pub const FIRST: Self = Self(1);

    /// Largest representable sequence number.
    pub const MAX: Self = Self(u64::MAX);

    /// Returns the numeric sequence value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the following sequence number, or `None` at exhaustion.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }

    /// Returns the following sequence number.
    ///
    /// This is deliberately fail-fast at [`Sequence::MAX`] rather than
    /// silently wrapping to the first sequence.
    #[must_use]
    pub const fn next(self) -> Self {
        self.checked_next().expect("sequence overflow")
    }
}

/// Typed item emitted by a runtime producer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamItem<T> {
    /// Stream identifier.
    pub stream_id: Id,
    /// Monotonic sequence within the stream.
    pub sequence: Sequence,
    /// Typed payload.
    pub payload: T,
}

impl<T> StreamItem<T> {
    /// Maps the payload without changing stream identity or ordering metadata.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> StreamItem<U> {
        StreamItem {
            stream_id: self.stream_id,
            sequence: self.sequence,
            payload: f(self.payload),
        }
    }
}

/// Error returned when a sequence-owning operation cannot advance any further.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceError {
    /// The stream has already emitted its maximum sequence.
    Exhausted {
        /// Stream whose sequence is exhausted.
        stream_id: Id,
        /// Last sequence that was emitted.
        sequence: Sequence,
    },
}

impl fmt::Display for SequenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted {
                stream_id,
                sequence,
            } => write!(
                f,
                "sequence exhausted for stream {stream_id} after {}",
                sequence.get()
            ),
        }
    }
}

impl std::error::Error for SequenceError {}

/// Synchronous producer-side sequence allocator for one stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamSequencer {
    stream_id: Id,
    next_sequence: Sequence,
    exhausted: bool,
}

impl StreamSequencer {
    /// Creates a sequencer starting at [`Sequence::FIRST`].
    #[must_use]
    pub fn new(stream_id: Id) -> Self {
        Self {
            stream_id,
            next_sequence: Sequence::FIRST,
            exhausted: false,
        }
    }

    /// Returns the stream owned by this sequencer.
    #[must_use]
    pub const fn stream_id(&self) -> &Id {
        &self.stream_id
    }

    /// Emits one item and advances the sequence without wrapping.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError::Exhausted`] after the maximum sequence has
    /// already been emitted.
    pub fn emit<T>(&mut self, payload: T) -> Result<StreamItem<T>, SequenceError> {
        if self.exhausted {
            return Err(SequenceError::Exhausted {
                stream_id: self.stream_id.clone(),
                sequence: Sequence::MAX,
            });
        }

        let sequence = self.next_sequence;
        match sequence.checked_next() {
            Some(next) => self.next_sequence = next,
            None => self.exhausted = true,
        }
        Ok(StreamItem {
            stream_id: self.stream_id.clone(),
            sequence,
            payload,
        })
    }
}

/// Result of observing one sequence in a stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequenceObservation {
    /// The first observed item is the first sequence.
    First {
        /// Sequence observed.
        actual: Sequence,
    },
    /// The observed item directly follows the previous item.
    InOrder {
        /// Sequence observed.
        actual: Sequence,
    },
    /// One or more sequences were not observed before this item.
    Gap {
        /// Sequence that was expected next.
        expected: Sequence,
        /// Sequence actually observed.
        actual: Sequence,
    },
    /// The observed sequence is not newer than the latest accepted sequence.
    DuplicateOrReordered {
        /// Latest sequence previously observed.
        last: Sequence,
        /// Sequence observed out of order.
        actual: Sequence,
    },
}

/// Error returned when a tracker receives an item from another stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamError {
    /// The tracker is bound to one stream identity.
    StreamMismatch {
        /// Stream identity bound to the tracker.
        expected: Id,
        /// Stream identity carried by the item.
        actual: Id,
    },
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StreamMismatch { expected, actual } => {
                write!(
                    f,
                    "sequence tracker expected stream {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for StreamError {}

/// Consumer-side tracker for one stream's sequence continuity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceTracker {
    stream_id: Id,
    last: Option<Sequence>,
}

impl SequenceTracker {
    /// Creates a tracker bound to `stream_id`.
    #[must_use]
    pub fn new(stream_id: Id) -> Self {
        Self {
            stream_id,
            last: None,
        }
    }

    /// Returns the stream identity bound to this tracker.
    #[must_use]
    pub const fn stream_id(&self) -> &Id {
        &self.stream_id
    }

    /// Returns the newest sequence observed, if any.
    #[must_use]
    pub const fn last(&self) -> Option<Sequence> {
        self.last
    }

    /// Observes one item and classifies its sequence relationship.
    ///
    /// A first item greater than [`Sequence::FIRST`] reports a missing prefix
    /// as a [`SequenceObservation::Gap`]. A gap advances the tracker to the
    /// observed sequence so subsequent transport observations remain local and
    /// deterministic.
    ///
    /// # Errors
    ///
    /// Returns [`StreamError::StreamMismatch`] for an item from another stream.
    pub fn observe<T>(&mut self, item: &StreamItem<T>) -> Result<SequenceObservation, StreamError> {
        if item.stream_id != self.stream_id {
            return Err(StreamError::StreamMismatch {
                expected: self.stream_id.clone(),
                actual: item.stream_id.clone(),
            });
        }

        let observation = match self.last {
            None if item.sequence == Sequence::FIRST => SequenceObservation::First {
                actual: item.sequence,
            },
            None => SequenceObservation::Gap {
                expected: Sequence::FIRST,
                actual: item.sequence,
            },
            Some(last) => match last.checked_next() {
                Some(expected) if item.sequence == expected => SequenceObservation::InOrder {
                    actual: item.sequence,
                },
                Some(expected) if item.sequence > expected => SequenceObservation::Gap {
                    expected,
                    actual: item.sequence,
                },
                _ => SequenceObservation::DuplicateOrReordered {
                    last,
                    actual: item.sequence,
                },
            },
        };

        if self.last.is_none_or(|last| item.sequence > last) {
            self.last = Some(item.sequence);
        }
        Ok(observation)
    }
}

/// Error returned when a bounded policy cannot be created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferError {
    /// A bounded policy must have at least one slot.
    ZeroCapacity,
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("bounded stream capacity must be greater than zero"),
        }
    }
}

impl std::error::Error for BufferError {}

/// Error returned when a producer cannot enqueue an item immediately.
#[derive(Debug, Eq, PartialEq)]
pub enum PushError<T> {
    /// The item remains owned by the caller for a later retry.
    Backpressure(T),
}

/// A fixed-capacity FIFO that never silently drops accepted items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LosslessBuffer<T> {
    capacity: usize,
    items: VecDeque<StreamItem<T>>,
}

impl<T> LosslessBuffer<T> {
    /// Creates a bounded lossless FIFO.
    ///
    /// # Errors
    ///
    /// Returns [`BufferError::ZeroCapacity`] for a zero-sized buffer.
    pub fn new(capacity: usize) -> Result<Self, BufferError> {
        if capacity == 0 {
            return Err(BufferError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        })
    }

    /// Returns the fixed capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of pending items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether no items are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Attempts to append an item without dropping or coalescing anything.
    ///
    /// # Errors
    ///
    /// Returns the original item in [`PushError::Backpressure`] when full.
    pub fn try_push(&mut self, item: StreamItem<T>) -> Result<(), PushError<StreamItem<T>>> {
        if self.items.len() == self.capacity {
            return Err(PushError::Backpressure(item));
        }
        self.items.push_back(item);
        Ok(())
    }

    /// Removes the oldest pending item.
    pub fn pop(&mut self) -> Option<StreamItem<T>> {
        self.items.pop_front()
    }
}

/// A stream item paired with a semantic key for coalescing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyedStreamItem<K, T> {
    /// Semantic identity used to select a coalescing target.
    pub key: K,
    /// Stream item carrying sequence and payload.
    pub item: StreamItem<T>,
}

/// A fixed-capacity FIFO that replaces only pending items with the same key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoalescingBuffer<K, T> {
    capacity: usize,
    items: VecDeque<KeyedStreamItem<K, T>>,
}

impl<K: Eq, T> CoalescingBuffer<K, T> {
    /// Creates a bounded same-key coalescing buffer.
    ///
    /// # Errors
    ///
    /// Returns [`BufferError::ZeroCapacity`] for a zero-sized buffer.
    pub fn new(capacity: usize) -> Result<Self, BufferError> {
        if capacity == 0 {
            return Err(BufferError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        })
    }

    /// Returns the fixed capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of pending keyed items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether no keyed items are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Attempts to add or replace one keyed pending item.
    ///
    /// A matching key replaces its existing pending item and moves the newer
    /// item to the pending queue's tail, preserving FIFO order among retained
    /// keys. A new key at capacity returns the original item as explicit
    /// backpressure.
    ///
    /// # Errors
    ///
    /// Returns [`PushError::Backpressure`] when full and no same-key item can
    /// be coalesced.
    pub fn try_push(
        &mut self,
        item: KeyedStreamItem<K, T>,
    ) -> Result<(), PushError<KeyedStreamItem<K, T>>> {
        if let Some(index) = self
            .items
            .iter()
            .position(|existing| existing.key == item.key)
        {
            let _ = self.items.remove(index);
            self.items.push_back(item);
            return Ok(());
        }
        if self.items.len() == self.capacity {
            return Err(PushError::Backpressure(item));
        }
        self.items.push_back(item);
        Ok(())
    }

    /// Removes the oldest pending keyed item.
    pub fn pop(&mut self) -> Option<KeyedStreamItem<K, T>> {
        self.items.pop_front()
    }
}

/// A fixed-capacity telemetry buffer that deterministically drops its oldest item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LossyBuffer<T> {
    capacity: usize,
    items: VecDeque<StreamItem<T>>,
}

impl<T> LossyBuffer<T> {
    /// Creates a bounded drop-oldest telemetry buffer.
    ///
    /// # Errors
    ///
    /// Returns [`BufferError::ZeroCapacity`] for a zero-sized buffer.
    pub fn new(capacity: usize) -> Result<Self, BufferError> {
        if capacity == 0 {
            return Err(BufferError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        })
    }

    /// Returns the fixed capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of pending items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether no telemetry items are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Pushes an item, returning the oldest dropped item when the buffer was full.
    pub fn push(&mut self, item: StreamItem<T>) -> Option<StreamItem<T>> {
        let dropped = if self.items.len() == self.capacity {
            self.items.pop_front()
        } else {
            None
        };
        self.items.push_back(item);
        dropped
    }

    /// Removes the oldest pending item.
    pub fn pop(&mut self) -> Option<StreamItem<T>> {
        self.items.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_mapping_preserves_order_metadata() {
        let stream_id = Id::new("model-output").expect("test id is valid");
        let item = StreamItem {
            stream_id: stream_id.clone(),
            sequence: Sequence::FIRST,
            payload: "42",
        };
        let mapped = item.map(str::len);

        assert_eq!(mapped.stream_id, stream_id);
        assert_eq!(mapped.sequence, Sequence::FIRST);
        assert_eq!(mapped.payload, 2);
    }
}
