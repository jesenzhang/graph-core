# Initial Experiments

Experiments should stay small, measurable, and disposable until a result justifies promotion into a stable crate API.

## Status

| Experiment | Status |
|---|---|
| E01 — Capability dependency resolution | PASS |
| E02 — Scoped capability replacement | PASS |
| E02R — Capability runtime integrity | PASS |
| E02R-F1 — Scope hierarchy closure | PASS |
| E03 — Versioned mutable workflow graph | PASS |
| E04 — Crash/recovery boundary | PASS |
| E05 — Typed stream backpressure | NOT STARTED |

## E01 — Capability dependency resolution

Implement deterministic dependency resolution with cycle detection and explicit lifecycle order.

Acceptance:

- insertion order does not change the resolved order;
- a dependency cycle is rejected with a useful path;
- teardown order is the reverse of construction order.

## E02 — Scoped capability replacement

Prototype root/runtime/session/task scopes and replace one provider inside a child scope.

Acceptance:

- parent scope remains unchanged;
- in-flight readers keep a valid owned reference;
- replacement cleanup is deterministic;
- failed replacement leaves the previous scope usable.

## E03 — Mutable workflow revision — PASS

Start a DAG, complete one task, then append a new branch selected by a mock planner.

Acceptance:

- completed facts are immutable;
- every topology change produces a monotonic revision;
- replay can reconstruct the exact graph seen by each scheduling decision.

Result: PASS. The implementation and evidence are recorded in
[`E03-versioned-mutable-workflow-graph.md`](results/E03-versioned-mutable-workflow-graph.md).

## E04 — Crash/recovery boundary — PASS

Model “effect committed, checkpoint missing” and “checkpoint committed, effect unknown”.

Acceptance:

- outcome-unknown is represented explicitly;
- recovery never silently duplicates a non-idempotent effect;
- scheduler decisions remain explainable from durable facts.

Result: PASS. The separate `workflow-recovery` crate distinguishes durable
intent from dispatch, keeps external reality separate from local knowledge,
reuses `OperationId` across idempotent retries, and returns structured recovery
decisions from `WorkflowGraph` plus `DurableJournal`. A deterministic external
simulator proves that a committed non-idempotent effect remains at one commit
after a crash, while an idempotent retry also remains at one logical commit.
See [`E04-crash-recovery-boundary.md`](results/E04-crash-recovery-boundary.md).

## E05 — Typed stream backpressure

Compare bounded lossless, bounded coalescing, and lossy telemetry channels.

Acceptance:

- policy is declared per stream type;
- sequence gaps are detectable when loss is allowed;
- workflow state does not depend on a lossy stream.

## Promotion rule

A mechanism may move into a stable public API only after an experiment documents:

- problem statement;
- competing designs;
- measured/observed result;
- failure modes;
- API consequence.
