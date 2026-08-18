# M2-B0 Primary Sources

Evidence notebook for durable execution / recovery mapping in `graph-core`.
Primary sources only. No implementation notes, no Rust edits, no persistence
design beyond the evidence-backed calls below.

## Decision Snapshot

- Pre-dispatch cancellation attempt retention: `RETAIN`.
- `retry_execution_event` evolution: `VALIDATE stream-id + sequence identity`;
  do not rely on a rename-only event shape.
- Compaction: `DEFER` to storage / replay infrastructure, not graph-core
  execution semantics.

## Reference 1: Temporal Server

- Repository: `temporalio/temporal`
- Release tag: `v1.31.2`
- Commit: `19a774302c613da9adc4436ab14278ccdca8e0a5`

### Relevant paths

- `docs/architecture/history-service.md`
- `docs/worker-versioning.md`
- `tests/workflow_timer_test.go`
- `tests/stickytq_test.go`
- `tests/gethistory_test.go`
- `tests/pause_workflow_execution_test.go`
- `api/token/v1/message.pb.go`
- `api/workflow/v1/message.pb.go`

### Mechanism

- Workflow history is the durable authority. The server doc says history events
  alone are sufficient to recover workflow state.
- Mutable state is reloaded from persistence, and queue processors checkpoint
  ack levels.
- Timer processing appends timer-firing events and schedules follow-up workflow
  tasks.
- Sticky queue tests prove replay after timeout / failure, and timer history
  shows canceled / rescheduled paths.
- Protobuf tokens expose `RunId`, `TaskId`, `ScheduledEventId`, and `Attempt`
  style identity fields for server-side history / task handles.

### Problem solved

- Crash recovery from persisted history.
- Deterministic replay after worker eviction / failover.
- Timer and workflow-task advancement without trusting local memory.

### Semantic assumptions

- `RunId` is run-generation identity, not a transport attempt id.
- History event order is authoritative over transient worker state.
- Task handles are derived from history, not from execution-stream buffers.

### Assessment for graph-core

- `PORT`: history/replay authority, timer durability, worker-crash recovery,
  latest-history-event validation, sticky eviction semantics.
- `ADAPT`: `RunId` and task-event identities are close to graph-core run /
  dispatch identities, but not a full `OperationId` / `AttemptId` model.
- `GAP`: no graph-core-style capability-entry identity primitive.
- `DEFER`: compaction is storage-tier work, not graph semantics.

### Source URLs

- <https://github.com/temporalio/temporal/blob/19a774302c613da9adc4436ab14278ccdca8e0a5/docs/architecture/history-service.md>
- <https://github.com/temporalio/temporal/blob/19a774302c613da9adc4436ab14278ccdca8e0a5/tests/workflow_timer_test.go>
- <https://github.com/temporalio/temporal/blob/19a774302c613da9adc4436ab14278ccdca8e0a5/tests/stickytq_test.go>
- <https://github.com/temporalio/temporal/blob/19a774302c613da9adc4436ab14278ccdca8e0a5/api/token/v1/message.pb.go>
- <https://github.com/temporalio/temporal/blob/19a774302c613da9adc4436ab14278ccdca8e0a5/api/workflow/v1/message.pb.go>

## Reference 2: Temporal Rust SDK

- Repository: `temporalio/sdk-rust`
- Release tag: `v0.7.0`
- Commit: `46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71`

### Relevant paths

- `ARCHITECTURE.md`
- `crates/sdk/src/workflow_replayer.rs`
- `crates/workflow/src/cancellation.rs`
- `crates/sdk-core/tests/integ_tests/workflow_tests/timers.rs`
- `crates/sdk-core/tests/integ_tests/workflow_tests/stickyness.rs`
- `crates/sdk/examples/cancellation/workflows.rs`
- `crates/workflow/src/runtime/instance.rs`

### Mechanism

- The architecture doc defines `HistoryEvent`, `WorkflowTask`, `WorkflowActivation`,
  and `WorkflowActivationJob` as the replay/activation boundary.
- `WorkflowReplayer` classifies malformed history, nondeterminism, workflow-task
  failures, and internal replay failures.
- `WorkflowCancellationToken` is deterministic and detached tokens stay
  detached from workflow cancellation.
- Timer and stickiness tests prove replay after cache eviction / task failure
  does not duplicate durable timer intent.

### Problem solved

- Rust-side durable workflow replay.
- Deterministic cancellation propagation.
- Timer and sticky-cache behavior under replay / eviction.

### Semantic assumptions

- Replay is history-driven, not memory-driven.
- Cached workflow state may be evicted and reconstructed from history.
- Deterministic cancellation tokens are a runtime primitive, not a persisted
  effect record.

### Assessment for graph-core

- `PORT`: replay, cancellation, timer semantics, worker-eviction recovery.
- `ADAPT`: useful for graph-core runtime boundaries and effect replay, but not a
  direct durable journal design.
- `GAP`: leases / fencing and compaction are not modeled as first-class graph
  core semantics here.

### Source URLs

- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/ARCHITECTURE.md>
- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/crates/sdk/src/workflow_replayer.rs>
- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/crates/workflow/src/cancellation.rs>
- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/crates/sdk-core/tests/integ_tests/workflow_tests/timers.rs>
- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/crates/sdk-core/tests/integ_tests/workflow_tests/stickyness.rs>
- <https://github.com/temporalio/sdk-rust/blob/46c50fc8540fd3b1a1f1e02ea2fa1291f8ec0c71/crates/sdk/examples/cancellation/workflows.rs>

## Reference 3: Restate

- Repository: `restatedev/restate`
- Release tag: `v1.7.3`
- Commit: `ad0151655289e11ee5e6165d2f4ce758cfddf6eb`

### Relevant paths

- `crates/types/src/invocation/mod.rs`
- `crates/types/src/journal_v2/command.rs`
- `crates/worker/src/partition/state_machine/lifecycle/cancel.rs`
- `crates/worker/src/partition/state_machine/lifecycle/manual_pause.rs`
- `crates/worker/src/partition/state_machine/lifecycle/purge_journal.rs`
- `crates/worker/src/partition/state_machine/lifecycle/restart_as_new.rs`
- `crates/worker/src/partition/state_machine/tests/idempotency.rs`
- `crates/worker/src/partition/state_machine/tests/kill_cancel.rs`
- `crates/partition-store/src/journal_table_v2/mod.rs`
- `crates/partition-store/src/tests/vqueue_table_test/mod.rs`
- `crates/partition-store/src/tests/snapshots_test/mod.rs`

### Mechanism

- `InvocationRequestHeader` carries an `InvocationId`, optional
  `idempotency_key`, and an `is_idempotent` check.
- `ServiceInvocation` and journal v2 command types keep invocation identity and
  idempotency as first-class state.
- `manual_pause` explicitly says resume re-invokes and replays the journal so
  the SDK re-derives what it is waiting on.
- `cancel` distinguishes scheduled / inboxed / completed / missing cases and
  can append abort state rather than erasing the invocation.
- `JournalEntryId::from_parts` / `InvocationId::from_parts` encode exact journal
  sequence identity.
- Vqueue snapshot tests prove readers hold a fixed view.
- `purge_journal` and retention fields make cleanup explicit.

### Problem solved

- Idempotent request deduplication.
- Durable invocation replay after pause / resume / restart.
- Exact journal sequence identity for recovery and cleanup.
- Bounded retention with explicit purge.

### Semantic assumptions

- `InvocationId` is the durable logical unit, not a worker attempt.
- `JournalEntryId` is the durable sequence id for replay and validation.
- Snapshot readers must not observe post-snapshot writes.

### Assessment for graph-core

- `PORT`: intent-before-effect, idempotent retry, non-idempotent recovery,
  cancellation, checkpoint/replay, retention / purge.
- `ADAPT`: `InvocationId` maps well to graph-core `OperationId`; journal entry
  index maps well to stream / sequence identity.
- `GAP`: no direct capability-fiber / entry-instance analogue.

### Source URLs

- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/types/src/invocation/mod.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/types/src/journal_v2/command.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/worker/src/partition/state_machine/lifecycle/cancel.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/worker/src/partition/state_machine/lifecycle/manual_pause.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/worker/src/partition/state_machine/lifecycle/purge_journal.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/worker/src/partition/state_machine/tests/idempotency.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/worker/src/partition/state_machine/tests/kill_cancel.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/partition-store/src/journal_table_v2/mod.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/partition-store/src/tests/vqueue_table_test/mod.rs>
- <https://github.com/restatedev/restate/blob/ad0151655289e11ee5e6165d2f4ce758cfddf6eb/crates/partition-store/src/tests/snapshots_test/mod.rs>

## Reference 4: Durable Task Framework

- Repository: `Azure/durabletask`
- Release tag: `durabletask.core-v3.9.0`
- Commit: `af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8`

### Relevant paths

- `docs/concepts/replay-and-durability.md`
- `docs/features/timers.md`
- `docs/features/retries.md`
- `docs/advanced/testing.md`
- `src/DurableTask.AzureStorage/AzureStorageOrchestrationServiceSettings.cs`
- `src/DurableTask.AzureStorage/AnalyticsEventSource.cs`
- `src/DurableTask.Emulator/LocalOrchestrationService.cs`
- `test/DurableTask.AzureStorage.Tests/OrchestrationSessionTests.cs`
- `test/DurableTask.AzureStorage.Tests/AzureStorageScaleTests.cs`
- `test/DurableTask.ServiceBus.Tests/SessionIdCaseInsensitiveTests.cs`
- `test/DurableTask.Core.Tests/ScheduleTaskOptionsTests.cs`

### Mechanism

- Official docs describe event-sourced replay after crash.
- `IsReplaying` gates side effects during replay.
- Durable timers and retry policy are first-class orchestration features.
- Azure Storage settings expose lease renew / acquire intervals, app leases,
  and the option to replay terminal instances.
- The emulator stores state by `InstanceId` and `ExecutionId`, which is a
  generation-style identity split.
- `AnalyticsEventSource` logs lease acquisition, renewal, stealing, removal,
  and purge-history operations.

### Problem solved

- Recovery after host crash.
- Durable timers and retry behavior.
- Partition ownership / fencing.
- History cleanup and terminal-instance replay recovery.

### Semantic assumptions

- `InstanceId` is the stable logical orchestration identity.
- `ExecutionId` is the restart / generation identity.
- Lease ownership is external to orchestration history but required for safe
  processing.

### Assessment for graph-core

- `PORT`: crash replay, timers, retries, and lease/fencing concepts.
- `ADAPT`: `InstanceId` / `ExecutionId` are the closest upstream analogue for
  `OperationId` / generation-style attempt boundaries.
- `GAP`: the framework is orchestration-centric rather than capability-entry
  centric, so `Capability Generation` / `EntryId` still need graph-core native
  definitions.
- `DEFER`: compaction remains a storage concern.

### Source URLs

- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/docs/concepts/replay-and-durability.md>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/docs/features/timers.md>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/docs/features/retries.md>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/docs/advanced/testing.md>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/src/DurableTask.AzureStorage/AzureStorageOrchestrationServiceSettings.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/src/DurableTask.AzureStorage/AnalyticsEventSource.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/src/DurableTask.Emulator/LocalOrchestrationService.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/test/DurableTask.AzureStorage.Tests/OrchestrationSessionTests.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/test/DurableTask.AzureStorage.Tests/AzureStorageScaleTests.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/test/DurableTask.ServiceBus.Tests/SessionIdCaseInsensitiveTests.cs>
- <https://github.com/Azure/durabletask/blob/af8078ff7073facf5bb6ec8b7ba3beeb7efcf2d8/test/DurableTask.Core.Tests/ScheduleTaskOptionsTests.cs>

## Cross-Source Identity Map

| graph-core term | Closest upstream analogue | Notes |
|---|---|---|
| `OperationId` | Restate `InvocationId`, Temporal workflow/run identity, DurableTask `InstanceId` | Logical external effect / invocation boundary. |
| `AttemptId` | Temporal task attempt, DurableTask `ExecutionId`, transport retry attempt | Must remain distinct from logical operation identity. |
| `RunId` | Temporal `RunId`, DurableTask `ExecutionId` | Generation / replay instance identity. |
| `TaskId` | Temporal event/task handles, DurableTask scheduled-event IDs | Dispatch-side handle, not business identity. |
| `Capability Generation` | DurableTask `ExecutionId` style generation, Temporal worker-version assignment | Fencing / compatibility boundary. |
| `EntryId` | Restate `JournalEntryId`, Temporal history event identity | Exact durable entry sequence identity. |

## Final Call for M2-B0

- Keep pre-dispatch cancellation attempts as durable facts. Do not erase the
  attempt just because it never reached dispatch.
- Make retry validation identity-first. If `retry_execution_event` stays in the
  model, it must validate stream identity and sequence continuity before it is
  accepted.
- Do not push compaction into graph-core semantics. Treat it as storage /
  journal maintenance.
