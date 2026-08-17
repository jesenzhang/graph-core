# M1 Runtime Core

Status: implemented

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
capabilities, creates a `TaskAttempt`, and retains exact `CapabilityHandle`s.
Tasks without an effect record completion immediately. Effect tasks persist an
intent and wait for explicit `dispatch_effect` and
`record_effect_outcome` calls. Workflow mutation goes through the existing
expected-revision API and the next scheduler step reads a fresh graph view.

## Capability pinning

An attempt stores `CapabilityId`, `Generation`, `EntryId`, and the exact
`CapabilityHandle`. Replacing a scope entry changes future lookups only. An
attempt that already started retains its old entry and value; a later attempt
resolves the current entry. M1 does not rebind or restart in-flight attempts.

## Recovery rule

Recovery classification reads only the workflow and journal authorities:

- intent without dispatch: execute is allowed;
- idempotent dispatch with unknown outcome: retry the same `OperationId` with a
  new `AttemptId`;
- non-idempotent dispatch with unknown outcome: reconcile, never auto-retry;
- known success with incomplete workflow: complete the task without executing
  the effect again.

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

`graph-lab` runs the same public Runtime Core API as a smoke demonstration.

## Remaining limitations

M1 is intentionally in-memory and synchronous. It has no database-backed
durability, async cancellation token, real external provider executor,
distributed scheduler, plugin loader, or automatic dependent restart.
Unknown non-idempotent outcomes require an external reconciliation operation.

## Next milestone

M2 can add persistence and an asynchronous execution boundary only after
preserving the authority and identity rules proven here.
