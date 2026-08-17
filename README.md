# graph-core

`graph-core` is a small Rust workspace that first validated, and now runs, three different kinds of graph-shaped runtime structure that are often incorrectly collapsed into one abstraction:

1. **Capability Graph** — what capabilities/services/plugins exist and depend on each other.
2. **Workflow Graph / DAG** — how a task is decomposed, ordered, retried, resumed, and completed.
3. **Execution Streams** — high-frequency ordered runtime data such as model deltas, tool output, events, and progress. These are intentionally modeled as typed streams, not as a mutable general-purpose graph.

Research Baseline v0 is complete. M1 Runtime Core is implemented as a
synchronous, single-process,
deterministic Runtime Core over the validated structures. It is still an
in-memory candidate: there is no persistence layer, async runtime, provider
SDK, plugin loader, or distributed execution.

## Why this exists

The original `workflow_engine` direction grew from a ComfyUI/Dify-like DAG workflow system toward Agent-controlled dynamic workflows. More recent harness/runtime designs suggest that capability composition, task orchestration, and high-frequency execution data have different lifecycles and should not be forced into one graph model.

`graph-core` exists to validate that separation experimentally before committing to a production architecture.

## Workspace

```text
crates/
  core/               Shared identifiers and neutral primitives only
  capability-graph/   Capability/service/plugin dependency experiments
  workflow-graph/     Task DAG and orchestration experiments
  execution-stream/   Typed ordered runtime stream primitives
  runtime-core/       Deterministic runtime coordination and recovery
  graph-lab/          Small executable for experiments and examples

docs/
  architecture/       Lightweight architecture decisions
  research/           Baseline, questions, reference notes, experiment plan
```

## Baseline rules

- Do not create a `Graph` trait shared by all three domains merely for reuse.
- Semantic invariants live in the domain crate that owns them.
- Execution streams are not represented as graph mutations.
- Dynamic composition does not automatically imply dynamic task topology.
- Workflow completion and external effect facts remain authoritative in their domain crates; Runtime Core coordinates them without creating a second truth.
- New dependencies require an experiment or a concrete implementation need.

## M1 Runtime Core

M1 composes capability pinning, workflow scheduling and mutation, effect
intent/dispatch/outcome recovery, and non-authoritative execution streams.
`graph-lab` includes a real smoke run. See
[`docs/runtime/M1-runtime-core.md`](docs/runtime/M1-runtime-core.md) for the
authority model, invariants, and current limitations.

## First commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -p graph-lab
```

## Research entrypoint

Start with [`docs/research/BASELINE.md`](docs/research/BASELINE.md).
