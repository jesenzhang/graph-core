//! Typed execution stream primitives.
//!
//! This crate intentionally does not expose graph nodes or edges.

use graph_core::Id;

/// Monotonic sequence number within one stream.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Sequence(u64);

impl Sequence {
    /// First valid sequence number.
    pub const FIRST: Self = Self(1);

    /// Returns the numeric sequence value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the following sequence number.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
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
