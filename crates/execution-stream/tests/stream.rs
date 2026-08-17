//! Semantic tests for typed synchronous stream policies.

use execution_stream::{
    CoalescingBuffer, KeyedStreamItem, LosslessBuffer, LossyBuffer, PushError, Sequence,
    SequenceObservation, SequenceTracker, StreamError, StreamItem,
};
use graph_core::Id;

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn sequence(value: u64) -> Sequence {
    (1..value).fold(Sequence::FIRST, |current, _| current.next())
}

fn item(value: u64, payload: u32) -> StreamItem<u32> {
    StreamItem {
        stream_id: id("runtime-events"),
        sequence: sequence(value),
        payload,
    }
}

fn keyed(key: &'static str, value: u64, payload: u32) -> KeyedStreamItem<&'static str, u32> {
    KeyedStreamItem {
        key,
        item: item(value, payload),
    }
}

#[test]
fn sequence_overflow_is_not_silent() {
    assert_eq!(Sequence::MAX.checked_next(), None);
    assert!(std::panic::catch_unwind(|| Sequence::MAX.next()).is_err());
}

#[test]
fn sequence_tracker_accepts_contiguous_items() {
    let mut tracker = SequenceTracker::new(id("runtime-events"));

    assert_eq!(
        tracker.observe(&item(1, 10)).expect("same stream"),
        SequenceObservation::First {
            actual: Sequence::FIRST,
        }
    );
    assert_eq!(
        tracker.observe(&item(2, 20)).expect("same stream"),
        SequenceObservation::InOrder {
            actual: sequence(2),
        }
    );
}

#[test]
fn sequence_tracker_detects_gap() {
    let mut tracker = SequenceTracker::new(id("runtime-events"));
    tracker.observe(&item(1, 10)).expect("same stream");

    assert_eq!(
        tracker.observe(&item(4, 40)).expect("same stream"),
        SequenceObservation::Gap {
            expected: sequence(2),
            actual: sequence(4),
        }
    );
}

#[test]
fn sequence_tracker_detects_missing_prefix() {
    let mut tracker = SequenceTracker::new(id("runtime-events"));

    assert_eq!(
        tracker.observe(&item(3, 30)).expect("same stream"),
        SequenceObservation::Gap {
            expected: Sequence::FIRST,
            actual: sequence(3),
        }
    );
}

#[test]
fn sequence_tracker_rejects_duplicate_or_reordered_sequence() {
    let mut tracker = SequenceTracker::new(id("runtime-events"));
    tracker.observe(&item(1, 10)).expect("same stream");
    tracker.observe(&item(2, 20)).expect("same stream");

    assert_eq!(
        tracker.observe(&item(2, 20)).expect("same stream"),
        SequenceObservation::DuplicateOrReordered {
            last: sequence(2),
            actual: sequence(2),
        }
    );
}

#[test]
fn sequence_tracking_is_isolated_per_stream() {
    let stream_a = id("model-output");
    let stream_b = id("stdout");
    let mut tracker_a = SequenceTracker::new(stream_a.clone());
    let mut tracker_b = SequenceTracker::new(stream_b.clone());
    let item_b = StreamItem {
        stream_id: stream_b.clone(),
        sequence: sequence(6),
        payload: "line",
    };

    assert_eq!(
        tracker_a.observe(&item_b),
        Err(StreamError::StreamMismatch {
            expected: stream_a,
            actual: stream_b.clone(),
        })
    );
    assert_eq!(
        tracker_b.observe(&item_b).expect("stream b is bound here"),
        SequenceObservation::Gap {
            expected: Sequence::FIRST,
            actual: sequence(6),
        }
    );
}

#[test]
fn lossless_buffer_preserves_fifo() {
    let mut buffer = LosslessBuffer::new(2).expect("capacity is valid");
    buffer.try_push(item(1, 10)).expect("space is available");
    buffer.try_push(item(2, 20)).expect("space is available");

    assert_eq!(buffer.pop().expect("item 1").sequence, sequence(1));
    assert_eq!(buffer.pop().expect("item 2").sequence, sequence(2));
}

#[test]
fn lossless_buffer_backpressures_when_full() {
    let mut buffer = LosslessBuffer::new(2).expect("capacity is valid");
    buffer.try_push(item(1, 10)).expect("space is available");
    buffer.try_push(item(2, 20)).expect("space is available");

    assert!(matches!(
        buffer.try_push(item(3, 30)),
        Err(PushError::Backpressure(_))
    ));
}

#[test]
fn lossless_backpressure_preserves_rejected_item() {
    let mut buffer = LosslessBuffer::new(2).expect("capacity is valid");
    buffer.try_push(item(1, 10)).expect("space is available");
    buffer.try_push(item(2, 20)).expect("space is available");
    let third = item(3, 30);

    assert_eq!(
        buffer.try_push(third.clone()),
        Err(PushError::Backpressure(third))
    );
}

#[test]
fn lossless_retry_produces_no_sequence_gap() {
    let mut buffer = LosslessBuffer::new(2).expect("capacity is valid");
    buffer.try_push(item(1, 10)).expect("space is available");
    buffer.try_push(item(2, 20)).expect("space is available");
    let third = match buffer.try_push(item(3, 30)) {
        Err(PushError::Backpressure(item)) => item,
        Ok(()) => panic!("full lossless buffer must backpressure"),
    };
    let delivered_one = buffer.pop().expect("item 1");
    buffer.try_push(third).expect("retry after pop");

    let mut tracker = SequenceTracker::new(id("runtime-events"));
    assert!(matches!(
        tracker.observe(&delivered_one).expect("same stream"),
        SequenceObservation::First { .. }
    ));
    let delivered_two = buffer.pop().expect("item 2");
    assert!(matches!(
        tracker.observe(&delivered_two).expect("same stream"),
        SequenceObservation::InOrder { .. }
    ));
    let delivered_three = buffer.pop().expect("item 3");
    assert!(matches!(
        tracker.observe(&delivered_three).expect("same stream"),
        SequenceObservation::InOrder { .. }
    ));
}

#[test]
fn coalescing_replaces_same_key_with_latest_item() {
    let mut buffer = CoalescingBuffer::new(2).expect("capacity is valid");
    buffer.try_push(keyed("progress", 1, 10)).expect("space");
    buffer
        .try_push(keyed("progress", 2, 20))
        .expect("same key coalesces");
    buffer
        .try_push(keyed("progress", 3, 30))
        .expect("same key coalesces");

    let delivered = buffer.pop().expect("latest progress");
    assert_eq!(delivered.key, "progress");
    assert_eq!(delivered.item.payload, 30);
    assert_eq!(delivered.item.sequence, sequence(3));
    assert!(buffer.is_empty());
}

#[test]
fn coalescing_preserves_latest_sequence() {
    let mut buffer = CoalescingBuffer::new(1).expect("capacity is valid");
    buffer.try_push(keyed("progress", 1, 10)).expect("space");
    buffer
        .try_push(keyed("progress", 4, 40))
        .expect("same key coalesces");

    assert_eq!(
        buffer.pop().expect("latest item").item.sequence,
        sequence(4)
    );
}

#[test]
fn coalescing_different_key_backpressures_when_full() {
    let mut buffer = CoalescingBuffer::new(2).expect("capacity is valid");
    buffer.try_push(keyed("A", 1, 10)).expect("space");
    buffer.try_push(keyed("B", 2, 20)).expect("space");
    let third = keyed("C", 3, 30);

    assert_eq!(
        buffer.try_push(third.clone()),
        Err(PushError::Backpressure(third))
    );
}

#[test]
fn coalescing_delivery_gap_is_detectable() {
    let mut buffer = CoalescingBuffer::new(2).expect("capacity is valid");
    buffer.try_push(keyed("progress", 1, 10)).expect("space");
    buffer
        .try_push(keyed("progress", 2, 20))
        .expect("same key coalesces");
    buffer
        .try_push(keyed("progress", 3, 30))
        .expect("same key coalesces");
    let delivered = buffer.pop().expect("latest progress");
    let mut tracker = SequenceTracker::new(id("runtime-events"));

    assert_eq!(
        tracker.observe(&delivered.item).expect("same stream"),
        SequenceObservation::Gap {
            expected: Sequence::FIRST,
            actual: sequence(3),
        }
    );
}

#[test]
fn lossy_buffer_drops_oldest_when_full() {
    let mut buffer = LossyBuffer::new(2).expect("capacity is valid");
    buffer.push(item(1, 10));
    buffer.push(item(2, 20));

    assert_eq!(
        buffer
            .push(item(3, 30))
            .expect("item 1 was dropped")
            .sequence,
        sequence(1)
    );
    assert_eq!(buffer.pop().expect("item 2").sequence, sequence(2));
    assert_eq!(buffer.pop().expect("item 3").sequence, sequence(3));
}

#[test]
fn lossy_buffer_retains_latest_items() {
    let mut buffer = LossyBuffer::new(2).expect("capacity is valid");
    for value in 1..=5 {
        buffer.push(item(value, value as u32 * 10));
    }

    assert_eq!(buffer.pop().expect("item 4").sequence, sequence(4));
    assert_eq!(buffer.pop().expect("item 5").sequence, sequence(5));
    assert!(buffer.is_empty());
}

#[test]
fn lossy_delivery_gap_is_detectable() {
    let mut buffer = LossyBuffer::new(2).expect("capacity is valid");
    buffer.push(item(1, 10));
    buffer.push(item(2, 20));
    buffer.push(item(3, 30));
    let delivered = buffer.pop().expect("item 2");
    let mut tracker = SequenceTracker::new(id("runtime-events"));

    assert_eq!(
        tracker.observe(&delivered).expect("same stream"),
        SequenceObservation::Gap {
            expected: Sequence::FIRST,
            actual: sequence(2),
        }
    );
}

#[test]
fn multiple_lossy_drops_produce_detectable_gap() {
    let mut buffer = LossyBuffer::new(2).expect("capacity is valid");
    for value in 1..=5 {
        buffer.push(item(value, value as u32 * 10));
    }
    let first_delivered = buffer.pop().expect("item 4");
    let second_delivered = buffer.pop().expect("item 5");
    let mut tracker = SequenceTracker::new(id("runtime-events"));

    assert_eq!(
        tracker.observe(&first_delivered).expect("same stream"),
        SequenceObservation::Gap {
            expected: Sequence::FIRST,
            actual: sequence(4),
        }
    );
    assert!(matches!(
        tracker.observe(&second_delivered).expect("same stream"),
        SequenceObservation::InOrder { .. }
    ));
}
