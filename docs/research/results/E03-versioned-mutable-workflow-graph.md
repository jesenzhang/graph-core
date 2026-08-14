# E03 — Versioned Mutable Workflow Graph

Date: 2026-08-14  
Status: Complete  
Decision: **PASS**

## Research Question

Can an executing workflow safely change its future topology while preserving
completed execution facts, versioning each topology transition, and replaying a
deterministic scheduler-view graph?

The experiment deliberately excludes a scheduler, workers, agents, planner
models, persistence, distributed execution, retries, async runtime, MCP, tools,
and UI. The planner input is a deterministic typed mutation batch.

## Topology vs Execution Facts

`WorkflowGraph` keeps two explicit views:

- `WorkflowTopology` contains task definitions and incoming dependency edges;
- `ExecutionFacts` contains completed task identities and a separate typed
  `CompletionRecord` log.

Topology mutations do not write completion facts. Recording a completion does
not advance the topology revision. This preserves the boundary:

```text
Topology Revision != Execution Progress
```

The workflow graph remains independent from `CapabilityGraph`; it does not
inherit from or implement a universal graph trait.

## Revision Semantics

The initial topology revision is zero. Every successful non-empty mutation
batch advances the topology revision exactly once. A batch containing six
operations therefore represents one transition, for example:

```text
Revision 1 -> apply batch -> Revision 2
```

Failed batches, empty batches, rejected cycles, completed-fact violations,
unknown tasks, duplicate operations, and stale planner submissions do not
advance the revision. Completion facts also do not advance it.

## Completed Fact Immutability

Completed task identity is immutable. The v0 mutation surface intentionally
contains no remove or replace operation, and `AddTask` rejects an existing
identity rather than rewriting its label or definition.

An incoming dependency cannot be added to a completed task. This freezes the
completed task's historical prerequisite set, so a planner cannot change the
meaning of an already completed fact. Future tasks may still depend on a
completed task, and future topology may be extended around it.

`complete(task_id)` requires every direct prerequisite to be completed and
returns a structured `IncompletePrerequisite` error otherwise.

## Typed Mutations

The v0 mutation enum is intentionally small:

```rust
enum WorkflowMutation {
    AddTask { task: Task },
    AddDependency { task_id: Id, dependency_id: Id },
}
```

`MutationBatch` preserves operation order and `WorkflowMutationRecord` stores
the base revision, resulting revision, and owned typed operations. The history
does not depend on a clone of the whole mutable graph and does not use closures,
JSON patches, or generic maps.

## Atomic Mutation Batch

`apply_batch(expected_revision, batch)` clones the candidate state, applies and
validates every operation, checks the DAG invariant, and commits only after the
whole batch succeeds. A rejected final operation leaves earlier candidate
operations uncommitted; no inverse rollback is required.

## Cycle Safety

Workflow dependencies are validated by a small deterministic DFS local to the
workflow crate. A dependency cycle returns a structured `Cycle` error with the
cycle path. The implementation does not call or reuse capability dependency
resolution because the two graphs have different domain semantics.

## Planner Revision Conflict

The expected topology revision is checked before candidate preparation. A
planner that read revision 4 cannot commit after another transition has moved
the graph to revision 5; it receives `RevisionConflict { expected: 4,
actual: 5 }` and the newer state remains unchanged.

## Scheduler View

`ready_tasks()` is a deterministic query, not a scheduler. It returns tasks
that are not completed and whose direct prerequisites are all completed. Tasks
are stored in `BTreeMap`/`BTreeSet`, so task insertion order and edge insertion
order do not change the result or topology representation.

For the planner append experiment:

```text
Plan -> Research -> Execute

Plan, Research completed

Research -> Validate -> Execute
Research -> Review   -> Execute
```

`Validate` and `Review` are ready, while `Execute` remains blocked until both
future branch tasks complete. The original `Research -> Execute` edge remains
because v0 only proves append operations; removing or rewriting future edges is
deferred.

## Replay Model

`WorkflowGraph::replay(records)` starts from an empty topology and re-applies
the ordered `WorkflowMutationRecord` values, checking each base and resulting
revision. It reconstructs the same task set, edge set, topology revision, and
mutation history.

Execution facts are replayed separately with
`WorkflowGraph::replay_with_facts(records, completion_records)`. The resulting
state has the same completed identities and the same deterministic
`ready_tasks()` result as the original state. No universal event enum or event
sourcing framework is introduced.

## Experiment Results

Focused workflow-graph validation covered 20 tests, including:

- successful and failed revision transitions;
- one-revision batch commits and atomic cycle rejection;
- incomplete-prerequisite rejection;
- completed identity and historical prerequisite protection;
- dynamic future branch append;
- all-or-nothing invalid batches;
- stale planner revision conflict;
- topology, revision, fact, and scheduler-view replay;
- deterministic ready-task queries and insertion-order stability.

The graph-lab demonstration preserves the Capability Kernel smoke test and
shows the E03 transition from topology revision 1 to 2 with `plan` and
`research` completed and `review,validate` ready.

## Rejected Alternatives

- **Universal graph trait:** rejected because capability composition and
  workflow scheduling have different invariants and lifecycle semantics.
- **Put `completed` on replaceable graph nodes:** rejected because topology
  mutation could rewrite historical execution facts.
- **One revision per mutation inside a planner proposal:** rejected because a
  proposal is one atomic topology transition.
- **Partial commit followed by inverse rollback:** rejected because candidate
  clone-and-commit gives the required atomicity with simpler proof obligations.
- **Reuse CapabilityGraph's resolver:** rejected because workflow DAG admission
  and capability dependency admission are separate domain contracts.
- **Remove/replace CRUD in v0:** deferred because the minimum append surface is
  sufficient to answer the E03 question.
- **Event sourcing framework or third-party graph crate:** rejected because
  typed mutation history and standard ordered collections are sufficient for
  this experiment.

## Remaining Limitations

- The v0 surface proves append-only future mutation; future-task removal,
  metadata replacement, and edge removal need a separate safety experiment.
- Replay starts from an empty workflow assembled by mutation records; a
  durable baseline or snapshot format is not modeled.
- Completion replay is an in-memory typed fact log, not a durable execution
  journal.
- No scheduler, worker, concurrent runtime, persistence, crash recovery, retry,
  or distributed ownership semantics are implemented.
- Revision overflow and long-lived storage concerns are outside this small
  standard-library experiment.

## Decision

**PASS.** Workflow Graph v0 research semantics are:

- immutable logical task identity;
- DAG topology with local cycle validation;
- topology-only monotonic revision;
- immutable completed execution facts;
- typed future mutations;
- atomic mutation batches;
- expected-revision conflict detection;
- deterministic scheduler view;
- deterministic topology and fact replay.

This is sufficient to answer the E03 question: a deterministic planner can
atomically append future DAG topology without invalidating completed execution
facts, and the resulting scheduler view can be reconstructed from typed
records. E04 is not started by this experiment.
