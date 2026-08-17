# E04 — Crash / Recovery Boundary

Date: 2026-08-17
Status: Complete
Decision: **PASS**

## Research Question

When local durable workflow knowledge and an external side effect cannot be
committed in one atomic transaction, can the runtime recover after a crash
without silently duplicating a non-idempotent effect, while producing a
deterministic and explainable decision from durable facts?

E04 is a synchronous semantic experiment. It is not a scheduler, database, or
Agent implementation.

## Failure Model

The local runtime may crash after any of these boundaries:

1. the effect intent is durable;
2. dispatch-started is durable;
3. the external call commits;
4. the local outcome checkpoint is durable;
5. the workflow completion fact is durable.

The external world is modeled separately from the local journal and survives
the simulated process restart. A crash between dispatch and outcome means the
external result may be either committed or not committed; local recovery must
behave safely in both cases.

## Local Knowledge vs External Reality

`KnownEffectOutcome` contains only `Succeeded` and `Failed`: these are results
known by the local runtime. The external simulator also has an actual result
and an observable commit count. `OutcomeUnknown` is not an external result; it
means that the local runtime has no checkpoint proving which external result
occurred.

This separation prevents a missing local record from being silently treated as
an external failure.

## Durable Protocol

The protocol uses distinct typed durable facts:

```text
EffectIntent -> DispatchRecord -> external invocation -> OutcomeRecord
                                                        -> workflow completion fact
```

An intent alone does not prove dispatch. Therefore an intent with no dispatch
is safe to execute, while a dispatch with no known outcome is explicitly
unknown. The journal admission API rejects dispatches without intents,
outcomes without matching dispatches, wrong attempts, duplicate attempts, and
contradictory outcomes.

## Operation and Attempt Identity

`OperationId` identifies one logical external side effect, for example
`send-contract/contract-123/v1`. `AttemptId` identifies one transport attempt,
for example `attempt-1` or `attempt-2`.

An idempotent retry keeps the same `OperationId` and may use a new
`AttemptId`. The simulator deduplicates idempotent external commits by
`OperationId`, never by a human-readable task label.

## Effect Ownership Cardinality

E04 v0 intentionally uses:

```text
TaskId -> 0..1 OperationId
```

because `WorkflowGraph` completion is task-level. Once an `EffectIntent` is
durable, its `OperationId` owns exactly one workflow task. Multiple attempts
remain allowed for the same `OperationId`.

Multiple logical external operations per workflow task are not modeled. If
production requires:

```text
Task -> multiple effects
```

future work must introduce explicit semantics such as a subtask per effect,
an `EffectGroup`, or an operation-set completion rule before task completion
can be derived safely. E04-F1 therefore rejects a second operation for the
same task without replacing the existing owner or corrupting its recovery
state.

## Idempotency Semantics

`EffectSemantics::Idempotent` permits retrying an unknown outcome with the same
logical operation identity. `EffectSemantics::NonIdempotent` forbids automatic
retry after dispatch when the outcome is unknown.

Known success completes the local workflow without invoking the external
operation again. Known failure is represented distinctly and produces an
observe/block decision; E04 does not invent backoff, retry limits, or a retry
queue.

## OutcomeUnknown Semantics

The journal derives `OutcomeUnknown` exactly when an intent and at least one
dispatch are durable but no known outcome has been checkpointed. It covers both
the crash-before-call and crash-after-external-commit windows. That ambiguity
is why non-idempotent automatic retry is forbidden even when the external call
may not have run.

## Recovery Decision Model

`classify_recovery(workflow, journal, operation_id)` is a pure function that
returns a structured `RecoveryDecision { action, reason }`:

| Durable facts | Decision |
|---|---|
| Intent, no dispatch | `Execute` |
| Dispatch, unknown outcome, idempotent | `RetrySameOperation` |
| Dispatch, unknown outcome, non-idempotent | `Reconcile` |
| Known success, workflow completion missing | `CompleteWithoutReexecution` |
| Known success, workflow already complete | `NoAction` |
| Known failure | `ObserveFailure` |
| Contradictory workflow/effect facts | `InvariantViolation` |

Classification performs no external invocation and no workflow mutation. A
caller applies an allowed action separately and then persists the next durable
fact.

## Crash Windows

The experiment covers all required partial states:

| Window | Local durable facts | External reality | Result |
|---|---|---|---|
| A | Intent | Not dispatched | `Execute` |
| B | Intent + DispatchRecord | No local result; external call uncertain | `Reconcile` for non-idempotent, same-operation retry for idempotent |
| C | Intent + DispatchRecord | External commit, local outcome missing | Same conservative result as B |
| D | Intent + DispatchRecord + known success | Committed | `CompleteWithoutReexecution` |
| E | Known success + workflow completion | Committed | `NoAction` |

## Non-Idempotent Safety Proof

The test `non_idempotent_external_commit_is_not_duplicated_after_crash`
increments an observable external counter after `DispatchRecord`, then
simulates a crash before `OutcomeRecord`. Recovery returns `Reconcile` and the
recovery path does not invoke the effect again. The external counter remains
exactly `1`.

This proves the policy prevents a second non-idempotent invocation in the
ambiguous window. It does not claim that the missing result can be inferred
without reconciliation.

## Idempotent Retry Proof

The idempotent retry tests invoke the same `OperationId` first with
`attempt-1`, classify the missing outcome as `RetrySameOperation`, then invoke
the same operation with `attempt-2`. The simulated external world deduplicates
by `OperationId`; the logical external commit count remains `1`. The second
attempt's known success can then lead to local completion without a third
invocation.

## Workflow Completion Boundary

Recording completion through the existing `WorkflowGraph::complete` API does
not advance topology revision and does not create a topology mutation. A known
successful effect can therefore recover the execution fact while preserving
E03's invariant:

```text
Topology Revision != Execution Progress
```

Existing prerequisite completion facts remain in the completion set and log.

## Durable Fact Invariants

The in-memory `DurableJournal` rejects:

- duplicate intents;
- dispatch without intent;
- duplicate attempt identities;
- outcome without dispatch;
- outcome for an attempt owned by another operation;
- conflicting duplicate outcomes;
- more than one known outcome for one operation;
- dispatch after an outcome is known.

Malformed facts fail closed at admission. Workflow/effect contradictions are
returned as an explicit `InvariantViolation` recovery decision.

## Experiment Results

The focused recovery suite contains 19 passing tests covering classification,
all crash windows, observable non-idempotent and idempotent simulator behavior,
determinism, journal integrity, and Workflow Graph integration. The workspace
adds no third-party dependency, no async runtime, no database, and no
execution-stream dependency.

The E04 graph-lab smoke output is:

```text
e04: non-idempotent outcome unknown -> reconcile, external_commits=1
```

## Rejected Alternatives

1. **Intent exists => retry.** Rejected because intent does not prove that an
   external effect was dispatched.
2. **Missing outcome => failure.** Rejected because local ignorance is not an
   external failure result.
3. **Automatically retry non-idempotent unknown outcomes.** Rejected because
   the effect may already have committed and would be duplicated.
4. **Use execution streams as recovery truth.** Rejected because stream
   durability and backpressure semantics are not established by E04, and a
   lossy stream cannot prove effect safety.
5. **Distributed transaction with an arbitrary external system.** Rejected
   for this baseline because it is generally unavailable and outside scope.
6. **Full event-sourcing framework.** Rejected because typed E04 facts are
   sufficient to prove the boundary without introducing Aggregate,
   Projection, CQRS, or a universal event store.

## Remaining Limitations

- `DurableJournal` is an in-memory semantic simulator. E04 does not prove
  `fsync`, WAL, SQLite/PostgreSQL durability, power-loss safety, or network
  partition behavior.
- Reconciliation is represented as a safe stop; no provider-specific lookup or
  compensating action is implemented.
- Known-failure handling is explicit but intentionally has no retry policy.
- There is no scheduler, async runtime, concurrency model, or execution-stream
  integration.
- The simulated external world is deterministic and local; real provider
  idempotency contracts still need separate validation.

## Decision

**PASS.** E04 proves the crash/recovery boundary required for the current
research stage: intent and dispatch are distinct durable facts, unknown local
outcomes are explicit, non-idempotent unknown outcomes never auto-retry,
idempotent retries preserve logical operation identity, known success avoids
re-execution, malformed facts fail closed, and recovery classification is
deterministic and explainable from durable workflow facts.

Workflow Graph v0 remains frozen. E05 remains not started.
