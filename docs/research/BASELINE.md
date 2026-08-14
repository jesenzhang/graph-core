# Research Baseline

Date: 2026-08-14
Status: E01/E02/E02R/E02R-F1/E03 research baseline; E04/E05 not started

## Experiment status

| Experiment | Status |
|---|---|
| E01 — Capability dependency resolution | PASS |
| E02 — Scoped capability replacement | PASS |
| E02R — Capability runtime integrity | PASS |
| E02R-F1 — Scope hierarchy closure | PASS |
| E03 — Versioned mutable workflow graph | PASS |
| E04 — Crash/recovery boundary | NOT STARTED |
| E05 — Typed stream backpressure | NOT STARTED |

## 1. Research thesis

The project starts from a falsifiable thesis:

> Capability composition, workflow orchestration, and execution dataflow are related but semantically different structures. A production Agent/workflow runtime should compose them explicitly instead of hiding them behind one universal dynamic graph abstraction.

The immediate goal is **not** to build a Cordis clone or a complete Agent runtime. The goal is to determine which primitives are truly shared, where lifecycle ownership belongs, and which semantics should remain independent.

## 2. Three structures under study

### 2.1 Capability Graph

Represents relatively long-lived runtime composition:

- model/provider capabilities;
- tools;
- services;
- plugins/extensions;
- session/runtime services;
- dependency requirements;
- lifecycle ownership and teardown order;
- scoped overrides and runtime reconfiguration.

Primary questions:

- Can dependency resolution remain deterministic under runtime changes?
- What is the minimum scope model: global, runtime, session, turn, task?
- Who owns cleanup when a capability is replaced?
- Can changes be applied transactionally or reversibly?
- Which failures should reject composition before execution starts?

Cordis is a key reference for this axis because it combines context-based dependency injection, services, plugin lifecycle, hot reloading, and service isolation. It is a reference to study, not a design to port literally.

### 2.2 Workflow Graph / DAG

Represents task-level orchestration:

- task/node dependencies;
- conditional branches;
- parallelism;
- retries;
- interrupt/resume;
- checkpointing;
- recovery;
- agent-directed topology changes;
- audit/replay.

Primary questions:

- What mutations are legal after execution has started?
- Is topology append-only, versioned, or freely mutable?
- How are deterministic replay and dynamic replanning reconciled?
- What state must be durable for crash recovery?
- How should a scheduler observe graph changes without owning business semantics?

This is the direct successor research axis to the old `workflow_engine`.

E03 PASS establishes the first workflow-graph v0 semantics: immutable task
identity, DAG topology, topology-only revision, immutable completed facts,
typed future mutations, atomic batches, expected-revision conflict detection,
deterministic scheduler view, and deterministic replay. See
[`E03-versioned-mutable-workflow-graph.md`](results/E03-versioned-mutable-workflow-graph.md).

### 2.3 Execution Streams

Represents high-frequency ordered data:

- model token/reasoning deltas;
- stdout/stderr;
- tool progress;
- structured runtime events;
- telemetry and UI updates.

Primary questions:

- Which streams require lossless delivery?
- Which streams require replay?
- Which streams allow coalescing/backpressure/drop policies?
- How should stream sequence relate to durable domain events?

Baseline decision: do **not** represent these streams as generic graph nodes/edges. Use typed channels/envelopes and only project them into a graph when a specific analysis requires it.

## 3. Cross-cutting research axes

The following are evaluated independently for each structure:

| Axis | Capability Graph | Workflow Graph | Execution Streams |
|---|---|---|---|
| Change frequency | low/medium | medium | high |
| Typical lifetime | runtime/session | task/run | milliseconds to run |
| Durability need | config/snapshot dependent | usually high | selective |
| Replay need | configuration history | strong | selective |
| Hot mutation | useful | required for agentic cases | continuous data |
| Backpressure | low relevance | scheduler-level | first-class |
| Ownership | lifecycle/scopes | run/task | producer/consumer |

## 4. Reference projects

Initial reference set:

- **Cordis** (`cordiverse/cordis`) — context/service/plugin composition, dependency injection, lifecycle and hot reload.
- **Hydro + Cordis** — evidence of Cordis used as an application/plugin substrate with service isolation and dynamic loading.
- **DeepSeek Harness-related work** — study the “model + harness” split and plugin/config-driven harness architecture; distinguish official projects from third-party experiments.
- **Goose / prior workflow_engine experiments** — plugin boundary lessons already explored in the predecessor project.
- **Effect / ZIO-style layers** — typed service dependency and scoped resource-management concepts; useful as conceptual comparison rather than a direct implementation target.

Every reference must be evaluated by a reproducible experiment before its mechanism is adopted.

## 5. Baseline non-goals

Not part of the initialization milestone:

- production Agent loop;
- LLM provider adapters;
- MCP implementation;
- plugin dynamic libraries/WASM ABI;
- distributed scheduler;
- database schema;
- event sourcing implementation;
- UI protocol;
- sandboxing;
- multi-process orchestration.

These are intentionally deferred until the graph boundaries survive the first experiments.

## 6. Initial success criteria

The research baseline is useful when:

1. each of the three structures has an independent Rust representation;
2. no cross-domain universal `Graph` abstraction is required to compile or test them;
3. each structure has explicit invariants and at least one unit test;
4. experiments can be added without pulling production runtime concerns into `core`;
5. the workspace passes fmt, clippy, and tests on a normal Rust toolchain.

## 7. Source baseline captured on 2026-08-14

The research set should track source snapshots, not only project names:

- `deepseek-ai/deepseek-harness` — official DeepSeek repository; current baseline inspection found dedicated Cordis primers/tutorials, Cordis tooling scripts, and Cordis-backed core packages. Research against a recorded commit before copying behavior.
- `cordiverse/cordis` — upstream Cordis implementation; track release/API drift separately from the vendored/forked version used by DeepSeek Harness.

This distinction matters: “what Cordis currently does” and “what DeepSeek Harness expects from its Cordis baseline” may diverge over time.
