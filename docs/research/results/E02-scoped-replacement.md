# E02 — Scoped capability replacement

English | [中文](E02-scoped-replacement.zh.md)

Date: 2026-08-14
Status: Complete

## Research question

Can a minimal parent/child scope model support inherited capabilities, local
overrides, safe in-flight readers, and transactional replacement without
mutating or disposing a parent-owned value?

The experiment intentionally uses only a root scope and arbitrary child scopes.
It does not assume that `Root -> Runtime -> Session -> Task` is the final
hierarchy. The tested primitive is “child inherits and may override”; named
application layers can be added later if evidence requires them.

## Cordis correspondence

The current local Cordis checkout (`8cc9e33`) provides service lookup through a
context, registers services as reversible effects, and unloads plugin-owned
effects when a fiber is disposed. Its loader can isolate a service name inside
a group so sibling groups see different providers. DeepSeek Harness also
describes scoped contexts whose registration context determines both visibility
and ownership.

The most relevant source snapshot and direct links are in
[`CORDIS-CAPABILITY-RESEARCH.md`](../CORDIS-CAPABILITY-RESEARCH.md), especially
the sections on scope ownership, replacement, lifecycle, and service isolation.

The Rust experiment retains three ideas:

1. child lookup falls back to the parent;
2. a local provider shadows without changing the parent;
3. cleanup belongs to the scope that published the local provider.

It intentionally does not reproduce proxy contexts, plugin fibers, async
effect generators, or dynamic module loading.

## Candidate designs

| Design | Problem | Decision |
|---|---|---|
| Mutate one shared instance in place | Readers can observe a partially changed value and cannot retain V1 safely. | Rejected. |
| Remove V1, construct V2, then publish | A failed constructor leaves the scope empty or damaged. | Rejected. |
| Clone all inherited values into every child | Teardown and ownership become ambiguous; parent changes do not propagate. | Rejected. |
| `Arc` value with a scope-local map and child fallback | Publication is local, readers own a stable snapshot, and parent ownership remains separate. | Chosen. |
| Third-party atomic-swap crate | Not required for this synchronous `RwLock` experiment; would constrain the design before measuring need. | Deferred. |

## Final experiment design

`Scope` owns a local `BTreeMap<CapabilityId, CapabilityEntry>` behind an
`RwLock` and keeps an optional parent `Scope`. `get()` checks the local map
first, then recursively checks the parent.

`CapabilityEntry` holds:

- an `Arc<CapabilityDefinition>` for the published metadata;
- an `Arc<InstanceSlot>` for the runtime resource.

`CapabilityHandle` is a reader-owned clone of those arcs. `InstanceSlot` owns a
boxed `CapabilityInstance`; its `Drop` implementation calls `dispose()` exactly
when the last scope/reader reference is gone. This gives the required behavior:

```text
scope owns V1
reader owns V1
replace publishes V2 and drops scope's V1 reference
reader continues using V1
reader releases V1 -> V1 is disposed
```

The replacement path is:

```text
check scope is open
→ validate all declared dependencies through current-scope lookup
→ run the constructor
→ acquire the write lock and atomically replace the local map entry
→ release the old entry outside the lock
→ Arc lifetime determines when old disposal is safe
```

Construction errors happen before the map write. A failed replacement therefore
leaves the previous entry visible and usable. A child replacement writes only
the child's map; a parent entry is never removed by child teardown.

## Implementation

The implementation lives in
[`crates/capability-graph/src/lib.rs`](../../../crates/capability-graph/src/lib.rs).
The public surface is intentionally small:

- `Scope::root()` and `Scope::child()`;
- `Scope::get()`;
- `Scope::provide()` / `Scope::replace()`;
- `Scope::teardown()`;
- `CapabilityInstance`, `CapabilityHandle`, and `ScopeError`.

`CapabilityInstance` is a synchronous resource boundary with an explicit
`dispose()` method and `as_any()` for experiment-only typed inspection. No
Tokio, async trait, dynamic library, or remote resource protocol is involved.

The map write is the publication point. Readers never borrow through the map;
they clone an `Arc` while holding the read lock and then execute independently.
That is the ownership property the in-flight reader test is intended to prove.

## Test result

Targeted validation:

```text
cargo test -p capability-graph --all-features
16 passed; 0 failed
```

The E02 cases are:

- `child_inherits_parent_capability`
- `child_override_does_not_mutate_parent`
- `sibling_scope_isolation`
- `replacement_changes_new_reads`
- `in_flight_reader_survives_replacement`
- `failed_replacement_keeps_old_capability`
- `child_teardown_disposes_owned_capabilities`
- `child_teardown_does_not_dispose_parent_capability`
- `replacement_disposes_old_capability_when_safe`

`in_flight_reader_survives_replacement` crosses a thread boundary, holds V1 in
the reader thread, publishes V2 on the main thread, and verifies that V1 is
disposed only after the reader releases it.

## Findings

1. A child override is naturally represented as a new local entry rather than
   a mutation of an inherited entry.
2. `Arc` solves the reader-validity problem without requiring unsafe code or an
   immediate invalidation protocol.
3. Transactional replacement is easier to reason about when construction has
   no access to the publication map and the commit is one write-locked swap.
4. Scope teardown can be local and idempotent: drain only the local map, then
   let handles decide when resources are quiescent enough to dispose.
5. The minimal root/child model is enough to test the core invariant. The
   experiment provides no evidence that four hard-coded levels are necessary.

## Rust ownership impact

Rust makes the lifetime contract visible. A returned `CapabilityHandle` is not
just metadata: it is an ownership claim on the runtime instance. The scope can
stop publishing V1 without making an existing handle dangling. Conversely,
there is no safe way to promise that an old value is disposed immediately while
readers still own it; disposal must wait for the last `Arc` reference.

This differs from the simplest Cordis mental model, where unloading a provider
also unloads dependent plugins. The Rust experiment keeps the old reader valid
instead of restarting it. That is appropriate for this kernel, but dependent
restart/quiescence policy remains a higher-level decision.

## What to keep, defer, and reject

- Keep: explicit scope ownership, parent fallback, local shadowing, stable
  reader handles, transactional publication, and idempotent teardown.
- Defer: async disposal, dependency-triggered dependent restart, versioned
  replacement conflicts, and scope event routing.
- Do not copy: TypeScript proxy lookup, ambient async context as authority,
  or a Cordis-compatible plugin loader.

## Open questions

- Should replacement require an expected version to prevent last-writer-wins
  races between concurrent constructors?
- How should a dependent capability react when its provider is replaced: keep a
  stable reader, restart, or receive an explicit transition event?
- Should `CapabilityInstance::dispose` become async, or should async resources
  be wrapped by a higher-level quiescence manager?
- Are named Runtime/Session/Task scope labels useful, or is a generic child
chain sufficient for graph-core?
