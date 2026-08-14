# E02R — Capability Runtime Integrity

Date: 2026-08-14
Status: Complete

## Research Question

Can the E01 dependency graph and the E02 scoped reader model become one
runtime integrity boundary, so that static dependency invariants constrain
instance construction, replacement, and teardown?

The experiment remains synchronous and in-memory. It does not add agents,
tools, MCP, workflows, persistence, dynamic loading, or an async runtime.

## Problems found in E02

E02 proved parent fallback, local shadowing, transactional construction, and
reader lifetime, but left five semantic gaps:

1. CapabilityHandle::instance exposed the same trait that owned dispose.
2. Dependency checks verified only visible identifiers and discarded handles.
3. Scope publication did not call the E01 resolver, so runtime state could
   bypass cycle rejection.
4. Replacement was last-writer-wins and had no stale-owner identity.
5. Teardown drained a map rather than consuming dependency order and had no
   explicit lifecycle state.

The source and historical result are
[lib.rs](../../../crates/capability-graph/src/lib.rs) and
[E02-scoped-replacement.md](E02-scoped-replacement.md).

## Cleanup Authority

The public reader surface now returns CapabilityValue. It supports typed
inspection through downcast_ref, but has no public dispose operation.
CapabilityValue owns an optional cleanup callback behind a private Mutex
slot. InstanceSlot is runtime-private and is held by Arc from both the
published entry and reader handles.

Dropping the last InstanceSlot-owned value takes the callback out of its
Option and invokes it once. A reader can drop or clone its handle, but cannot
invoke cleanup directly. Replacement, scope teardown, and construction
conflicts all release the same Arc ownership path, so they cannot introduce a
second cleanup call.

Result: PASS.

## Exact Entry Identity

Every publication receives both:

- a monotonic per-capability Generation used for replacement conflicts;
- an opaque process-local EntryId used to distinguish exact runtime entries.

CapabilityHandle records both values. Old handles therefore remain readers of
their original entry across A(v1) -> A(v2) -> A(v3); dropping the v1 handle
does not address the v3 map entry. The scope map is changed only by the
runtime publication path, never by a reader handle.

Result: PASS.

## Dependency Snapshot

The constructor shape is now:

    FnOnce(&ResolvedDependencies) -> Result<CapabilityValue, String>

ResolvedDependencies is a sorted map from capability identity to the exact
CapabilityHandle visible at the start of the construction attempt. The
constructor receives those handles directly, and the published InstanceSlot
retains the same snapshot.

This gives the ownership relation:

    published A -> dependency snapshot -> B(v1)

If B(v1) is replaced by B(v2) while A is being constructed or while A is
running, A keeps B(v1). New lookup resolves B(v2). The final publication
revalidates the candidate topology, but it does not silently rebind the
constructor's snapshot.

Result: PASS.

## Runtime Admission

CapabilityGraph::validate_candidate is the formal admission seam. It clones
the current graph, replaces the candidate definition, and calls the same
deterministic DFS resolver used by E01. Scope does not implement a second
cycle algorithm.

Scope::provide and Scope::replace perform admission before construction and
again at the final publication point. The final check combines current
visible parent definitions, current local entries, and the candidate. It
rejects:

- self-dependency;
- cycles introduced by a replacement;
- missing dependencies before the constructor runs.

Scope trees share a topology admission lock. A parent candidate is also
checked against open descendants for which the candidate remains visible, so
local shadowing does not accidentally turn a child view into an invalid
topology.

Result: PASS for the synchronous in-process scope model.

## Replacement Generation

The first local publication is Generation 1. replace requires the caller to
pass the expected current generation. A successful replacement increments the
generation. The expected-generation comparison is repeated while the final
publication lock is held.

A stale attempt returns the structured error:

    ReplacementConflict { capability, expected, actual }

The candidate value is not inserted when the comparison fails. Its private
Arc slot is dropped after the lock is released, leaving the current entry
unchanged.

Result: PASS.

## Quiescence / Teardown

Scope lifecycle is:

    Open -> Closing -> Closed

Open allows lookup, provide, and replace. Closing and Closed reject new
lookups and mutations. Handles already returned before Closing remain valid.
teardown is idempotent: only the Open-to-Closing transition drains the local
map.

Teardown builds a runtime graph from the published definitions and the exact
dependency snapshots retained by the entries. It consumes the resolver's
reverse construction order, so A depends on B depends on C is released as
A, B, C. The topology lock is released before user cleanup runs.

The protocol is deliberately ownership-based rather than scheduler-based:

1. teardown stops new runtime operations;
2. published ownership is removed in dependency order;
3. a dependent slot still owns its dependency handles;
4. in-flight readers retain the dependent slot;
5. the final handle releases the last pinned dependency and cleanup runs once.

No async drain, task cancellation, or external reference counter is needed
for this synchronous model.

Result: PASS for synchronous handle quiescence.

## Concurrency Test

concurrent_stale_replacement_returns_conflict uses two real threads and two
standard-library Barrier values:

1. T1 starts a replacement from generation N and pauses inside its factory.
2. T2 publishes a replacement from generation N.
3. T1 resumes and attempts its final commit.
4. T1 receives ReplacementConflict and the T2 entry remains visible.

The test exercises the final locked compare-and-publish boundary rather than
simulating concurrency with sequential calls.

Result: PASS.

## Rust Ownership Findings

Rust's Arc model makes the useful dependency invariant direct: if a published
entry owns dependency handles, the dependency cannot be destroyed while the
dependent entry or any dependent reader remains alive. This is simpler and
more local than inventing a separate dependency drain scheduler.

The cleanup boundary is also clearer when the public value is not the owner.
CapabilityValue can expose inspection without exposing a destructor authority;
the runtime slot owns the only cleanup callback and its Option state provides
the exactly-once guard.

The implementation uses only standard-library Arc, Mutex, RwLock, atomics,
BTreeMap, BTreeSet, Barrier, and thread primitives. The relevant contracts
are documented by the [Rust Arc documentation](https://doc.rust-lang.org/std/sync/struct.Arc.html),
[Rust Mutex documentation](https://doc.rust-lang.org/std/sync/struct.Mutex.html),
[Rust RwLock documentation](https://doc.rust-lang.org/std/sync/struct.RwLock.html),
and [Rust Drop documentation](https://doc.rust-lang.org/std/ops/trait.Drop.html).

## Rejected Alternatives

| Alternative | Decision | Reason |
|---|---|---|
| Keep dispose on the public instance trait | Rejected | A reader could kill a resource still owned by other readers. |
| Re-resolve dependencies inside the constructor | Rejected | Provider replacement would silently drift the construction snapshot. |
| Validate only dependency identifier existence | Rejected | Runtime publication could bypass E01 cycle invariants. |
| Last-writer-wins replacement | Rejected | Concurrent constructors could overwrite newer state. |
| Remove by capability identifier only | Rejected | ABA replacement makes stale ownership ambiguous. |
| Drop the entry map in iteration order | Rejected | Map order is not dependency teardown order. |
| Add Tokio, async-trait, futures, or a swap crate | Rejected | The required proof is synchronous and standard-library ownership is sufficient. |
| Add a custom dependency drain state machine | Deferred | Arc dependency snapshots already express the v0 lifetime invariant. |

## Remaining Limitations

- Cleanup is synchronous. Async disposal and cancellation are intentionally
  outside E02R.
- A public validation proof is a snapshot and can become stale; publication
  therefore revalidates under the topology lock.
- The topology lock serializes in-process publication across a scope tree.
  Distributed coordination is not modeled.
- Dependent capabilities remain bound to their old dependency snapshot after a
  provider replacement. Automatic dependent restart or transition events need
  a separate experiment.
- Scope teardown does not wait for an async task; it relies on reader and
  dependency handle ownership to keep synchronous resources valid.
- Generation overflow and process restart persistence are not modeled.

These limitations are explicit boundaries, not hidden runtime behavior.

## Decision

E02R: PASS.

Capability Kernel v0 is supported as YES, BUT NARROWER. The evidence supports
the following kernel surface:

- capability identity;
- dependency declaration;
- deterministic resolution;
- cycle rejection;
- scoped visibility;
- exact runtime entry identity;
- reader-owned handles;
- dependency snapshots;
- transactional publication;
- generation conflict detection;
- ownership-safe cleanup;
- dependency-aware teardown.

The kernel explicitly does not include Agent, LLM, Tool, MCP, Workflow,
Persistence, Distributed coordination, Plugin loader, Dynamic module loading,
Config language, Event bus, HMR watcher, or Async runtime.

## Verification

Targeted capability-graph validation during this experiment:

    30 passed; 0 failed

The required E02R semantic tests are in
[crates/capability-graph/src/lib.rs](../../../crates/capability-graph/src/lib.rs),
including cleanup authority by API shape, runtime cycle admission, exact
dependency generations, replacement conflicts, dependency-aware teardown,
parent/child lifetime, and the two-thread race.

The graph-lab demonstration binds a service to model-v1, replaces the model
with model-v2, and shows that the old service remains bound to v1 while new
lookup sees v2.

## Sources

- [E01 capability resolution](E01-capability-resolution.md)
- [E02 scoped replacement](E02-scoped-replacement.md)
- [Capability kernel decision](../CAPABILITY_KERNEL_DECISION.md)
- [Cordis and DeepSeek Harness capability research](../CORDIS-CAPABILITY-RESEARCH.md)
- [Rust Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html)
- [Rust Drop](https://doc.rust-lang.org/std/ops/trait.Drop.html)
