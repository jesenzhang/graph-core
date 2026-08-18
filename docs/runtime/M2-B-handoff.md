# M2-B Handoff: Durable Runtime Extension

Status: M2-A integrated; M2-B0 not started

M2-A keeps Cordis-derived Context, Registry, Fiber, and Effect state
process-local. The next milestone must persist authoritative facts without
serializing the in-memory Fiber object graph.

## Data that must remain durable

- workflow revisions and completion facts;
- effect intent;
- dispatch records and latest-dispatch identity;
- known outcomes;
- `OperationId` and `AttemptId` lineage;
- the recovery identity required to distinguish late outcomes from the latest
  dispatch;
- capability configuration identity when replay needs to select the same
  logical capability configuration.

## Observations that are not authoritative

- token streams;
- stdout/progress observations;
- telemetry;
- UI observations;
- disposable Execution Stream buffer contents.

## Runtime boundary

Capability Fiber lifecycle is process-local runtime state by default. A
restart after a process crash reconstructs the required runtime from durable
configuration and authoritative workflow/effect facts; it does not deserialize
old Fiber pointers, disposers, mutexes, or async tasks.

M2-B must preserve M1's latest-dispatch authority, late-outcome rejection,
exact `AttemptId` identity, capability pinning for in-flight attempts, and the
separation between WorkflowGraph, DurableJournal, Capability Runtime, and
Execution Streams.
