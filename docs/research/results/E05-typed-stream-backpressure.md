# E05 — Typed Stream Backpressure

Date: 2026-08-17
Status: Complete — integrity closure
Decision: **PASS**

## Research Question

Can different runtime data streams declare different bounded delivery
policies without conflating stream transport with durable workflow correctness,
while making loss and coalescing observable through sequence semantics?

E05 is a synchronous policy experiment. It does not implement an async
runtime, scheduler, persistence, or production channel library.

## Stream vs Durable State

`execution-stream` contains transport items and bounded delivery policies only.
It does not depend on `workflow-graph` or `workflow-recovery`. Workflow
topology, completion facts, effect intent, dispatch proof, and effect outcome
remain authoritative in their existing layers.

The cross-structure tests deliberately drop and coalesce stream items while
leaving workflow completion facts and topology revision unchanged. A lossy
telemetry item is disposable and must never be the only copy of a durable fact.

Integrity closure added after the initial experiment:

- sequence emission at `u64::MAX` is followed by `SequenceError::Exhausted`
  without wrapping;
- coalescing identity includes `stream_id` as well as the semantic key;
- replacing a pending key moves it to the pending tail, preserving the
  documented multi-key ordering.

## Sequence Semantics

Every `StreamItem` carries a stream identity and a `Sequence` before entering a
buffer policy. `Sequence::MAX`, `checked_next()`, and fail-fast `next()` close
the previous unchecked overflow path. `StreamSequencer` owns one stream id and
advances sequences without wrapping.

Sequence numbers describe logical producer emission order, not queue slots.
Items later coalesced or dropped still retain their distinct sequence numbers.

## Sequence Gap Detection

`SequenceTracker` is bound to one stream identity. It reports:

- `First` for the first sequence;
- `InOrder` for a contiguous successor;
- `Gap { expected, actual }` for skipped sequences, including a missing prefix
  before the first observed item;
- `DuplicateOrReordered` for a sequence that is not newer than the latest
  observed sequence.

An item from another stream is rejected as `StreamMismatch`; independent
streams cannot accidentally share sequence state.

## Bounded Lossless Policy

`LosslessBuffer<T>` is a fixed-capacity FIFO. It preserves accepted item order,
never overwrites, never coalesces, and returns explicit
`PushError::Backpressure(item)` when full. The rejected item remains owned by
the caller and can be retried after a consumer pop.

## Bounded Coalescing Policy

`CoalescingBuffer<K, T>` is a fixed-capacity keyed FIFO. A new item replaces
only a pending item with the same semantic key and moves the newer item to the
pending tail, preserving FIFO order among retained keys while retaining the
newer sequence and payload. A different key when the buffer is full returns
explicit backpressure and is not silently dropped.

## Lossy Telemetry Policy

`LossyBuffer<T>` is a fixed-capacity drop-oldest queue. A new item is accepted
when full and the oldest pending item is returned to the caller as the dropped
item. Sequence tracking makes the loss observable even if a caller ignores the
drop return value.

## Backpressure Semantics

Bounded lossless does not promise immediate producer acceptance. It promises
that capacity exhaustion is explicit backpressure rather than silent loss.
Bounded coalescing may accept a full-buffer update only when it has a matching
key to replace; a new key remains backpressured.

Lossy telemetry intentionally accepts and drops according to its declared
policy. It is suitable only for disposable observations, not durable workflow
facts.

## Workflow Correctness Boundary

The cross-structure experiment proves:

1. a lossy stream can drop telemetry without changing `WorkflowGraph` completion
   facts;
2. coalesced progress can skip sequence delivery without changing topology
   revision;
3. recovery classification can run from `WorkflowGraph` and
   `DurableJournal` without execution-stream input.

There is no `workflow-graph -> execution-stream` or
`workflow-recovery -> execution-stream` correctness dependency.

## Experiment Results

The execution-stream suite contains 19 passing tests: one existing payload
mapping test plus 18 E05 policy, sequence, overflow, gap, FIFO, backpressure,
coalescing, and lossy-drop tests. The graph-lab smoke output is:

```text
e05: lossless=backpressured, coalesced_gap=true, telemetry_gap=true
```

The workspace adds no third-party dependency and remains synchronous and
deterministic.

## Rejected Alternatives

1. **One queue policy for every stream.** Rejected because model output,
   progress, and telemetry have different delivery semantics.
2. **Unbounded queue as lossless.** Rejected because it avoids bounded
   backpressure instead of defining it.
3. **Silent dropping without sequence metadata.** Rejected because consumers
   cannot distinguish loss from inactivity.
4. **Use a lossy stream to reconstruct workflow truth.** Rejected because
   correctness would depend on transport loss policy.
5. **Universal event bus.** Rejected because durable domain facts and runtime
   transport have different semantics and ownership.
6. **Tokio channel implementation first.** Rejected because policy semantics
   should be proven before selecting an async library.

## Remaining Limitations

- The policies are synchronous in-memory structures, not concurrent channels.
- No fairness, wakeup, cancellation, or async producer/consumer protocol is
  modeled.
- Coalescing keys are supplied by callers; the experiment does not infer
  semantic identity from payloads.
- Sequence tracking detects transport observations, but does not recover
  missing payloads.
- Lossy telemetry policy is deterministic drop-oldest; other policies require
  a separate experiment.

## Decision

**PASS.** E05 proves that policy-visible bounded stream types can distinguish
lossless backpressure, same-key coalescing, and lossy telemetry while preserving
sequence observability. Workflow correctness remains independent of stream
transport. The initial three-structure research baseline is complete.

Agent Runtime, production scheduler, persistence, async runtime, and provider
integration remain not started.
