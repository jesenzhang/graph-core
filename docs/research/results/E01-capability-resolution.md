# E01 — Capability dependency resolution

Date: 2026-08-14
Status: Complete

## Research question

Can a small Rust capability graph provide deterministic dependency resolution,
useful cycle diagnostics, and an explicit construction/teardown order without a
graph library or runtime-specific concepts?

The experiment deliberately excludes providers, tools, agents, MCP, network,
configuration, serialization, and persistence. It evaluates the composition
kernel only.

## Cordis correspondence

The current local Cordis checkout (`8cc9e33`) treats a plugin's `inject` list
as a declaration of required services. A plugin remains pending until those
services exist; the declaration, rather than configuration-file order, decides
when the plugin can start. DeepSeek Harness documents the same behavior in its
Cordis primer and services tutorial. The relevant source snapshot and links
are recorded in [`CORDIS-CAPABILITY-RESEARCH.md`](../CORDIS-CAPABILITY-RESEARCH.md).

The Rust experiment keeps the useful invariant — dependencies are explicit and
validated before construction — but does not model Cordis's pending fiber,
proxy property lookup, or plugin loader.

## Candidate designs

| Concern | Candidate | Result |
|---|---|---|
| Definition storage | `HashMap` plus sorting at each use | Rejected: easy to accidentally expose iteration-order behavior. |
| Definition storage | `BTreeMap` keyed by `CapabilityId` | Chosen: sorted roots and direct access. |
| Dependencies | unordered vector | Rejected unless every traversal sorts it. |
| Dependencies | `BTreeSet<Dependency>` | Chosen: equivalent definitions have the same traversal order and duplicate edges disappear. |
| Resolution | Kahn queue | Possible, but cycle reporting needs extra predecessor/path bookkeeping. |
| Resolution | deterministic DFS with an active path | Chosen: dependency-first order and `A -> B -> C -> A` diagnostics are direct. |
| Missing dependency | ignore until construction | Rejected: composition must fail before runtime work. |

No third-party crate was necessary. The standard library collections express
the ordering invariant directly, and the graph is small enough that the
straightforward DFS has better diagnostic behavior than a more generic graph
algorithm.

## Final experiment design

`CapabilityDefinition` contains:

- a stable `CapabilityId`;
- a human-readable capability kind;
- a sorted set of `Dependency` values.

`CapabilityGraph::resolve()` walks capability identifiers and dependencies in
sorted order. A dependency is appended to the resolved construction order
only after all of its own dependencies have been visited. The returned
`ResolvedCapabilityGraph` exposes:

- `construction_order()` — dependencies first;
- `teardown_order()` — the exact reverse.

Missing dependencies are represented as
`CapabilityGraphError::MissingDependency { capability, dependency }`. A cycle
is represented as `CapabilityGraphError::Cycle { path }`, with the repeated
start identifier included in the path.

The legacy baseline `Capability { id, kind }` remains accepted by
`CapabilityGraph::insert()` and is converted into a dependency-free definition.
New code should use `CapabilityDefinition` when it needs dependencies.

## Implementation

The implementation lives in
[`crates/capability-graph/src/lib.rs`](../../../crates/capability-graph/src/lib.rs).
It uses only:

- `BTreeMap<CapabilityId, CapabilityDefinition>` for nodes;
- `BTreeSet<Dependency>` for direct requirements;
- a DFS state map (`Active`/`Done`);
- an active recursion path for structured cycle errors.

`require()` validates both endpoints immediately. Definitions assembled with
`depends_on()` are allowed to arrive in any insertion order and are validated
by `resolve()`, so the graph builder remains order-independent.

## Test result

Targeted validation:

```text
cargo test -p capability-graph --all-features
16 passed; 0 failed
```

The E01 cases are:

- `resolve_simple_dependency`
- `resolve_multi_level_dependency`
- `resolution_is_deterministic`
- `missing_dependency_is_rejected`
- `cycle_is_rejected`
- `cycle_error_contains_path`
- `teardown_order_is_reverse_resolution_order`

The remaining targeted tests exercise E02 in the same crate.

## Findings

1. Deterministic resolution is cheap when ordering is part of the data model;
   it does not require a graph dependency.
2. DFS is sufficient for this experiment and produces a more useful cycle
   error than a boolean cycle flag.
3. Construction and teardown order are graph semantics, not an incidental
   property of a runtime registry. They can therefore be tested before any
   resource is constructed.
4. A definition can be assembled before its provider definition is inserted,
   but resolution must reject the incomplete graph. There is no safe default
   for silently ignoring a missing requirement.

## Ownership impact

E01 does not own runtime instances. It returns an immutable ordering result that
the scope/lifecycle experiment can consume. Keeping graph validation separate
from resource ownership avoids making the dependency graph responsible for
disposing arbitrary objects.

This boundary differs from a literal Cordis port: Cordis combines dependency
availability with fiber activation and unload/reload transitions. Rust
`graph-core` can retain the dependency invariant while leaving activation,
ownership, and concurrency to `Scope`.

## What to keep, defer, and reject

- Keep: explicit dependency declarations, fail-fast validation, deterministic
  order, structured cycle paths.
- Defer: pending activation, dependency-triggered reactivation, event dispatch,
  configuration overlays, and async lifecycle state machines.
- Do not copy: proxy-based `ctx.service` lookup, TypeScript declaration
  merging, or a plugin loader inside the graph crate.

## Open questions

- Should a later graph revision support replacing a definition's dependency
  set while instances are active?
- Should resolution return definitions as well as identifiers once runtime
  construction needs the metadata?
- Do async resource dependencies require a separate ready/active state, or can
the scope layer own that protocol without changing the graph?
