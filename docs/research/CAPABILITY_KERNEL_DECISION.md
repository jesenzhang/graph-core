# Capability kernel decision

English | [中文](CAPABILITY_KERNEL_DECISION.zh.md)

Date: 2026-08-14
Evidence: E01, E02, E02R, and the current local Cordis/DeepSeek Harness source snapshots

## Decision

**YES, BUT NARROWER**

A Cordis-like Rust kernel is worth pursuing as a small capability composition
substrate. The experiments establish useful, independently testable semantics
for deterministic dependency order, scope inheritance, local replacement,
reader-owned lifetime, and transactional publication.

The evidence does not justify building a Rust Cordis clone or putting an Agent
runtime, plugin loader, event system, configuration language, or durable
execution model into `graph-core`. The durable decision is a narrower kernel:

```text
capability identity
→ dependency validation and deterministic order
→ explicit scope visibility
→ owned runtime instances
→ atomic in-memory replacement
→ safe teardown boundary
```

The detailed experiment records are:

- [`E01-capability-resolution.md`](results/E01-capability-resolution.md)
- [`E02-scoped-replacement.md`](results/E02-scoped-replacement.md)
- [`CORDIS-CAPABILITY-RESEARCH.md`](CORDIS-CAPABILITY-RESEARCH.md)

## E02R update

E02R supports the existing YES, BUT NARROWER decision. It closes the runtime
integrity gap without expanding graph-core into a general runtime. The
follow-up record is [E02R capability runtime integrity](results/E02R-capability-runtime-integrity.md).

- cleanup authority is held by runtime-owned slots, not reader handles;
- constructors receive exact dependency snapshots that are retained by the
  published entry;
- runtime admission calls the E01 resolver for current definitions plus the
  candidate;
- replacements require an expected generation and preserve exact entry
  identity;
- teardown rejects new operations, follows dependency order, and relies on
  Arc ownership for synchronous quiescence.

The resulting v0 boundary is intentionally small:

    capability identity
    dependency declaration
    deterministic resolution
    cycle rejection
    scoped visibility
    exact runtime entry identity
    reader-owned handles
    dependency snapshots
    transactional publication
    generation conflict detection
    ownership-safe cleanup
    dependency-aware teardown

The following remain outside the kernel boundary:

    Agent, LLM, Tool, MCP, Workflow, Persistence
    Distributed coordination, Plugin loader, Dynamic module loading
    Config language, Event bus, HMR watcher, Async runtime

E02R is a synchronous in-process proof. Async disposal, dependent restart,
durability, and distributed ownership require independent evidence.

## 1. What belongs in graph-core

### Should belong

- capability IDs, kinds, and explicit dependency declarations;
- deterministic dependency resolution and structured cycle errors;
- scope-local registration with parent fallback;
- owned capability instance handles;
- replacement publication and rollback-safe construction;
- synchronous ownership/teardown primitives that can later support a stronger
  quiescence protocol;
- tests and invariants for these semantics.

These are semantic invariants of capability composition. They remain useful
whether the eventual consumer is an Agent, workflow runner, CLI, or another
application.

### Should not belong

- Agent loops, prompt assembly, model providers, tools, MCP, or session policy;
- workflow scheduling, retries, persistence, event sourcing, or execution
  streams;
- YAML/JSON configuration and schema evaluation;
- dynamic library/WASM loading and sandbox policy;
- network, database, distributed coordination, or multi-process state;
- a universal `Graph` trait shared with workflow and execution-stream crates;
- Cordis's proxy/declaration-merging ergonomics.

Those concerns may consume the kernel or live in separate crates, but they are
not required to define capability ownership.

## 2. Cordis mechanism assessment

| Mechanism | Judgment for graph-core | Reason |
|---|---|---|
| Dependency injection | Keep, in a narrower explicit-ID form | E01 shows that declared requirements enable fail-fast validation and deterministic construction. Use handles/accessors, not proxy properties. |
| Scope inheritance | Keep | E02 proves parent fallback and local shadowing without parent mutation. Keep the hierarchy generic; do not hard-code four levels yet. |
| Service lifecycle | Keep | Owned instances and reverse teardown are core composition semantics. Async lifecycle needs a later experiment. |
| Effect cleanup | Partially keep later | Keep the idea that registrations have owners and disposers. Defer a generic effect stack until async/quiescent requirements are measured. |
| Hot replacement | Keep narrowly | In-memory replacement with staged construction and `Arc`-safe readers is useful. HMR/file watching is outside the kernel. |
| Plugin loader | Exclude from graph-core | Loading code/config is deployment and composition tooling, not dependency-graph semantics. |
| Dynamic module loading | Exclude for now | It adds platform, ABI, safety, and rollback costs without evidence from E01/E02. |
| Service isolation | Keep the semantic core | Child-local overrides give isolation. Cordis's proxy realms, labels, and loader integration do not need to be copied. |
| Typed event dispatch | Defer | Cordis's `emit`/`serial`/`parallel`/`waterfall` modes are valuable for a runtime, but neither experiment requires them. |
| Ambient context propagation | Exclude as authority | DeepSeek Harness itself treats ambient initiator state as attribution, not liveness or authorization. Rust APIs should pass ownership explicitly. |

## 3. Rust implementation cost

| Module | Complexity | Evidence / remaining cost |
|---|---|---|
| Dependency graph | Low–Medium | Implemented with standard collections and deterministic DFS. Versioned graph changes and richer diagnostics remain. |
| Scope model | Medium | Root/child lookup and shadowing are small. Multiple scope kinds, routing, and concurrent close semantics need more design. |
| Resource lifecycle | Medium | `Arc` handle ownership and synchronous disposal work. Async cleanup and quiescence are not solved. |
| Hot replacement | Medium–High | Atomic in-memory publication works. Expected-version conflict handling, dependent restart, and rollback of multi-capability changes remain. |
| Plugin registration | Medium–High | A registry can be built above this kernel, but plugin identity, unload order, and registration effects need a separate experiment. |
| Configuration | Medium–High | Parsing and schema validation are application/configuration concerns; no format should be selected yet. |
| Dynamic loading | High | ABI, platform, safety, capability boundaries, and rollback dominate; explicitly deferred. |
| Durability | High | Snapshot/version/replay semantics would change the ownership model and need workflow-oriented experiments. |
| Distributed runtime | Very High | Coordination, leases, failure detection, and cross-process ownership are out of scope. |

## 4. What the experiments established

- Insertion order can be made irrelevant without a third-party graph crate.
- A meaningful cycle path is a small but important error contract.
- Teardown order should be derived from dependency order, not from map or
  registration accidents.
- A child override is a local publication, not a mutation of the parent.
- `Arc` gives in-flight readers a valid old snapshot while new lookups see the
  replacement.
- Construction failure can leave the old value untouched when publication is a
  separate commit step.
- Last-reader disposal is a real ownership consequence, not just a testing
  convenience.

### E02R-F1 scope hierarchy closure

- An ancestor owns the lifetime of every descendant scope. Tearing down an
  ancestor closes live descendants recursively, child-first; detached,
  reparented, and orphaned scopes are not part of the v0 lifecycle model.
- Existing handles remain valid through exact `Arc` dependency snapshots, and
  dependent handles keep their dependencies alive until the dependent is
  released. Closed descendants reject lookup and publication operations.
- Teardown planning is an internal invariant: resolver failure is surfaced as
  a panic rather than hidden by a map-order fallback. Logical capability
  ordering remains separate from exact snapshot resource lifetime.
- Generation and process-local entry identity allocation are checked for
  exhaustion. The implementation is split into definition/resolver and
  runtime modules behind a small `lib.rs` facade.

## 5. Evidence limits and next research

E01/E02 are synchronous in-memory experiments. They do not establish:

- how async resources drain while replacement or teardown is requested;
- whether dependents should restart when a provider changes;
- how concurrent replacements should resolve conflicts;
- how dependency graph revisions interact with already-created instances;
- whether named Runtime/Session/Task layers improve real applications;
- how durability, replay, or distributed ownership should work.

Recommended next experiments are a versioned replacement conflict test, an
async quiescence test, and a dependent-restart test. They should remain outside
workflow scheduling and provider-specific code until their invariants are clear.
