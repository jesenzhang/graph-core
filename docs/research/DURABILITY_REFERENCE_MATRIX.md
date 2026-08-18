# M2-B0 Durability Reference Matrix

Status: M2-B0 complete

M2-B0 is a reference-mapping milestone. It does not add a database adapter,
SQL schema, WAL, async executor, Tokio scheduler, distributed worker, or
provider SDK. `WorkflowGraph`, `DurableJournal`, the capability runtime, and
Execution Streams remain separate authorities.

## Reference set

The references below were checked at source level, including implementation
and test paths. Links are pinned to the exact commit used for this mapping.

| Project | Exact version / commit | Relevant implementation and test paths | Mechanism observed |
| --- | --- | --- | --- |
| [Temporal Server](https://github.com/temporalio/temporal/tree/19a774302c613da9adc4436ab14278ccdca8e0a5) | `v1.31.2`, `19a774302c613da9adc4436ab14278ccdca8e0a5` | `service/history/workflow/mutable_state_impl.go`; `service/history/history_engine.go`; `common/persistence/execution_manager.go`; `common/persistence/history_manager.go`; `service/history/workflow/mutable_state_impl_test.go`; `service/history/tests/history_engine_test.go`; `tests/workflow_timer_test.go`; `tests/workflow_retry_test.go` | Event history plus mutable state, scheduled/started/completed event identities, persistence transactions, timers, retries, and workflow completion facts. |
| [Temporal Rust SDK](https://github.com/temporalio/sdk-rust/tree/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71) | `v0.7.0`, `46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71` | `ARCHITECTURE.md`; `crates/sdk/src/workflow_replayer.rs`; `crates/workflow/src/cancellation.rs`; `crates/workflow/src/runtime/instance.rs`; `crates/sdk-core/tests/integ_tests/workflow_tests/timers.rs`; `crates/sdk-core/tests/integ_tests/workflow_tests/stickyness.rs` | Rust-side history replay, activation boundaries, deterministic cancellation, timer replay, and cache-eviction reconstruction. |
| [Restate](https://github.com/restatedev/restate/tree/ad0151655289e11ee5e6165d2f4ce758cfddf6eb) | `v1.7.3`, tag dereferenced to `ad0151655289e11ee5e6165d2f4ce758cfddf6eb` | `crates/types/src/journal/entries.rs`; `crates/invoker-impl/src/invocation_state_machine.rs`; `crates/partition-store/src/journal_table/mod.rs`; `crates/worker/src/partition/state_machine/lifecycle/cancel.rs`; inline tests in `invocation_state_machine.rs`; `crates/worker/src/partition/state_machine/tests/idempotency.rs` | Per-invocation journal entries, entry indexes, durable command/notification acknowledgements, retry timer keys, fencing tokens, and persisted journal storage. |
| [Azure Durable Task Go](https://github.com/microsoft/durabletask-go/tree/9c9e2d6d4cc3609c28bc2cc660ab5311f0217593) | exact `main` commit `9c9e2d6d4cc3609c28bc2cc660ab5311f0217593` | `backend/orchestration.go`; `backend/taskhub.go`; `task/orchestrator.go`; `internal/helpers/history.go`; `task/orchestrator_test.go`; `tests/orchestrations_test.go`; `tests/task_executor_test.go` | History-driven orchestration, deterministic replay, sequence-numbered actions, duplicate-event filtering, durable timers, work-item completion/abandon, and checkpoint-shaped state transitions. |
| [Azure Durable Task Framework](https://github.com/Azure/durabletask/tree/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8) | `durabletask.core-v3.9.0`, `af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8` | `docs/concepts/replay-and-durability.md`; `docs/features/timers.md`; `docs/features/retries.md`; `src/DurableTask.AzureStorage/AzureStorageOrchestrationServiceSettings.cs`; `src/DurableTask.AzureStorage/AnalyticsEventSource.cs`; `src/DurableTask.Emulator/LocalOrchestrationService.cs`; `test/DurableTask.AzureStorage.Tests/OrchestrationSessionTests.cs`; `test/DurableTask.AzureStorage.Tests/AzureStorageScaleTests.cs` | Event-sourced replay, durable timers/retries, instance/execution identity split, lease renewal/stealing, and explicit history purge. |

These systems solve different parts of the problem. Temporal is the strongest
reference for workflow history and mutable-state transactions; Restate is the
strongest Rust reference for journal entries, fencing, and command
acknowledgement; Durable Task is the clearest small implementation of replay
and sequence validation. None is copied as an API or treated as proof that a
retry is recovery.

## Concern mapping

`PORT`, `ADAPT`, `GAP`, `DEFER`, and `REJECT` are the only strategy statuses in
this matrix.

| Concern | Reference | Mechanism | graph-core strategy |
| --- | --- | --- | --- |
| intent before effect | Temporal history/mutable state; Durable Task orchestration actions | Commit the logical command/history fact before a worker performs the external action | ADAPT |
| dispatch identity | Temporal scheduled/started event IDs; Restate invocation journal entry index | Separate logical operation identity from each dispatch attempt and persist the latest dispatch identity | PORT |
| unknown outcome | Temporal activity/workflow history; M1 E04 recovery model | Preserve `OutcomeUnknown`; do not infer success or retry safety from process state or stream state | PORT |
| idempotent retry | Temporal retry policy; Restate retry state machine and journal tracker | Retry the same `OperationId` with a new `AttemptId` only when semantics explicitly allow it | PORT |
| non-idempotent recovery | Temporal history plus application reconciliation; M1 E04 | Reconcile or block when dispatch exists without a known outcome; provider-specific reconciliation is not automatic | GAP |
| cancellation | Restate cancel lifecycle; Durable Task cancellation events; M1 cancellation rule | Persist cancellation as a fact at the same authority boundary as intent/dispatch and retain prior dispatch facts | ADAPT |
| checkpoint | Durable Task work-item state; Temporal mutable state; Restate journal/snapshot storage | Persist a materialized correctness state with a monotonic revision; checkpoint is not a Fiber snapshot | ADAPT |
| replay | Durable Task `task/orchestrator.go`; Temporal event history replay | Reconstruct correctness state and validate sequence identity; do not replay UI observations or Fiber pointers | ADAPT |
| timer durability | Temporal timer events; Durable Task durable timers; Restate sleep/retry timers | Durable timer identity is required, but timer backend semantics are outside M2-B0 | DEFER |
| lease/fencing | Restate `FencingToken` and invoker generation checks | Add a worker-execution/lease identity and reject stale ownership; exact lease protocol remains a design task | ADAPT |
| worker crash | All three: reload history/journal, deduplicate events, and resume from durable facts | Reconstruct from authority, classify unknown outcomes, and never reuse a prior `AttemptId` for a new dispatch | PORT |
| compaction | Temporal archival/continue-as-new; Restate snapshots and journal compaction | Keep compaction semantics separate from correctness replay until history growth is measured | DEFER |
| Fiber object graph as durable truth | M2-A runtime plus all references' replay/state boundaries | Persist configuration and correctness facts, not handles, mutexes, disposer closures, or scheduler queues | REJECT |
| Execution Stream as durable authority | M1 E05 plus the reference systems' history/journal separation | Streams are observations; loss/backpressure cannot change workflow, effect, or capability identity | REJECT |

## Crash boundary mapping

| Crash point | Durable facts present | Restart decision |
| --- | --- | --- |
| before intent | None for the operation | No operation exists; do not dispatch. |
| after intent, before dispatch | `RunId`, `TaskId`, `OperationId`, intent semantics, and ownership | The operation is prepared but not dispatched; cancellation can prevent dispatch, otherwise allocate the first `AttemptId`. |
| after dispatch, before outcome | Intent plus one latest `DispatchRecord`/`AttemptId`, no outcome | Outcome is unknown. Idempotent operations may retry with a new attempt; non-idempotent operations require reconciliation. |
| after outcome, before workflow completion | Known outcome tied to exact `AttemptId`, workflow completion fact absent | Apply workflow completion without executing the effect again. |
| during cancellation | Cancellation fact may race dispatch | Compare cancellation with the latest durable dispatch; cancellation never erases an existing dispatch or outcome. |
| during retry | Previous attempt and retry decision are durable | A retry creates a new `AttemptId` under the same `OperationId`; an old outcome is late and cannot overwrite the latest attempt. |

## Identity mapping

| Durable/runtime identity | graph-core meaning | Rule |
| --- | --- | --- |
| `RunId` | One workflow execution | Stable across task attempts in one run. |
| `TaskId` | Logical workflow node/task | Owned by `WorkflowGraph`; not an attempt identity. |
| `OperationId` | One logical external side effect | Stable across retries and cancellation/recovery. |
| `AttemptId` | One dispatch/execution attempt | New for every retry; never reused after an uncertain dispatch. |
| Capability generation | Version of a capability definition | Captured by `CapabilityPin` at attempt start. |
| `EntryId` | Exact published capability entry | Captured with generation and handle; distinguishes replacement entries in one generation-aware scope. |
| Worker execution identity | Future worker/lease owner | Must be separate from `AttemptId`; it identifies ownership of processing, not the logical external call. |

## Atomicity and replay conclusions

The first durable seam should be a transaction that validates the expected
workflow revision and operation ownership, appends intent/cancellation and
dispatch facts as applicable, and advances the monotonic correctness revision.
The external effect is invoked only after the committed intent/dispatch fact.
Outcome recording is a second idempotent compare-and-set boundary keyed by
`OperationId` and `AttemptId`. Workflow completion consumes a known outcome and
must not invoke the external effect again.

The first implementation should not adopt a general event-sourcing framework.
Use a small typed fact/journal interface with monotonic revisions and
materialized workflow state; an embedded transactional journal/snapshot store
is the recommended backend *type*. SQLite or an embedded log are possible
future adapters, but neither is selected or implemented by M2-B0.

## M1 follow-up decisions

### Pre-dispatch cancellation attempt retention

When an attempt is created and `TaskStarted` is published, but intent is
persisted and dispatch is not yet recorded, the attempt is a durable historical
fact once the attempt admission boundary commits. Cancellation is also durable
at that boundary. The cancelled attempt is never reused: restart records the
cancellation against the same logical operation and, if execution is still
needed, creates a new `AttemptId`. The cancelled attempt remains history even
though no provider dispatch occurred.

The ordering must prevent a non-durable `TaskStarted` observation from creating
an attempt that recovery cannot explain. `TaskStarted` is an observation of an
already admitted attempt, not the authority itself.

### Execution retry item API

`retry_execution_event(item)` is too permissive for a durable boundary if the
caller can pass an item from an older stream generation or sequence. The
follow-up should evolve toward `retry_pending_execution_event()` or require
validation of stream ID, event identity, and sequence before requeueing. The
retry operation must preserve the original attempt identity for the same
lossless lifecycle item; a new `AttemptId` belongs to an external-effect retry,
not to transport replay.

## M2-B1 implementation plan (not executed)

1. Define a minimal `DurableStore` trait for load/commit of run state,
   operation facts, cancellation facts, dispatch records, and outcomes.
2. Define one atomic commit request with expected workflow revision,
   idempotency key, monotonic revision, and append-before-effect semantics.
3. Define recovery queries that return the latest dispatch/outcome by
   `OperationId`, reject stale `AttemptId` outcomes, and expose unknown outcome
   explicitly.
4. Add a deterministic in-memory conformance backend and crash-boundary tests;
   select the embedded transactional backend only after the interface tests
   pass.
5. Integrate the store behind `runtime-core` while preserving the existing
   `WorkflowGraph`, `DurableJournal`, capability pinning, and non-authoritative
   stream boundaries.
