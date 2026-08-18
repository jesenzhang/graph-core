# M1 Runtime Core

Status: implemented; contract frozen for M2-A

M1 is a synchronous, single-process, deterministic coordinator. It composes
the existing structures without merging their authority.

## Authority model

- `WorkflowGraph` owns task topology, topology revisions, ready-task view, and
  completion facts.
- `DurableJournal` in `workflow-recovery` owns effect intent, dispatch, and
  outcome facts, including task-to-operation ownership.
- `capability_graph::Scope` owns capability definitions, generations, exact
  entry identity, runtime instances, and lifecycle.
- `execution-stream` owns only bounded observations. Stream loss is never used
  to reconstruct workflow or effect truth.
- `runtime-core` owns coordination, task configuration inputs, attempt records,
  cancellation markers, and disposable stream buffers.

## Runtime flow

`Runtime::step` reads `WorkflowGraph::ready_tasks`, resolves the configured
capabilities through the runtime's `CapabilityContext`, creates a
`TaskAttempt`, and retains exact `CapabilityHandle`s. The default context is a
view over the M1 scope, so this integration does not change M1 authority or
task configuration inputs.
Tasks without an effect record completion immediately. Effect tasks persist an
intent and wait for explicit `dispatch_effect` and
`record_effect_outcome` calls. Workflow mutation goes through the existing
expected-revision API and the next scheduler step reads a fresh graph view.

## Capability pinning

An attempt stores `CapabilityId`, `Generation`, `EntryId`, and the exact
`CapabilityHandle`. Replacing a scope entry changes future context lookups
only. An attempt that already started retains its old entry and value; a later
attempt resolves the current entry. M1 does not rebind or restart in-flight
attempts.

## Recovery rule

Recovery classification reads only the workflow and journal authorities:

- intent without dispatch: execute is allowed;
- idempotent dispatch with unknown outcome: retry the same `OperationId` with a
  new `AttemptId`;
- non-idempotent dispatch with unknown outcome: reconcile, never auto-retry;
- known success with incomplete workflow: complete the task without executing
  the effect again.

The latest dispatch is the recovery authority for an operation. An outcome
from an older attempt is late once a newer dispatch exists and must not
overwrite that newer attempt's `OutcomeUnknown` state. This is an identity
rule, not an execution-stream ordering rule.

The journal enforces one logical operation owner per task.

## Cancellation rule

Cancellation before dispatch prevents a task from starting or being
dispatched. Cancellation after dispatch reports the existing dispatch and
does not erase or reinterpret its effect fact. The caller must still follow
outcome or recovery/reconciliation semantics.

## Proven scenarios

`runtime-core/tests/runtime.rs` covers:

1. deterministic `A -> B` execution and single effect completion;
2. capability replacement during an in-flight attempt;
3. mutation adding `C` after `A` completes;
4. idempotent unknown-outcome retry with the same operation and new attempt;
5. non-idempotent unknown-outcome reconciliation;
6. progress coalescing and telemetry dropping without authority changes;
7. cancellation before dispatch;
8. cancellation after dispatch.

## Frozen M1 invariants

The following are part of the M1 contract and must remain true while the
capability runtime evolves:

1. latest dispatch is the recovery authority;
2. a late outcome for an older attempt cannot cover or replace the latest
   unknown dispatch;
3. `StreamSequencer` is single-owner and is not `Clone`;
4. retrying a lossless lifecycle item retains the original attempt identity;
5. authoritative recovery does not depend on recovering an Execution Stream;
6. `WorkflowGraph`, `DurableJournal`, Capability Runtime, and Execution
   Streams remain separate authorities.

The M1 follow-ups retained for M2-A design are pre-dispatch cancellation
attempt retention and the execution-retry-item identity/API ambiguity. They do
not reopen the M1 implementation or its acceptance result.

`graph-lab` runs the same public Runtime Core API as a smoke demonstration.

## Remaining limitations

M1 is intentionally in-memory and synchronous. It has no database-backed
durability, async cancellation token, real external provider executor,
distributed scheduler, plugin loader, or automatic dependent restart.
Unknown non-idempotent outcomes require an external reconciliation operation.

## Next milestone

M2 can add persistence and an asynchronous execution boundary only after
preserving the authority and identity rules proven here.
