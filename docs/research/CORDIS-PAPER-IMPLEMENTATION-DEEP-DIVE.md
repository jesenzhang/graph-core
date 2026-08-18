# Cordis Paper × Implementation Deep Dive

English | [中文](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.zh.md)

Date: 2026-08-18

## 0. Baseline and decision

This report is a follow-up to [`CORDIS-CAPABILITY-RESEARCH.md`](CORDIS-CAPABILITY-RESEARCH.md). That memo deliberately treated source as the only authority. This report adds the Cordis paper published on 2026-08-13 and reconciles the paper's formal model with current Cordis v4 source and graph-core's integrated M2-A capability runtime.

Frozen evidence:

- Paper: [`cordiverse/paper`](https://github.com/cordiverse/paper/tree/948a07b369c62adb3b12e102458be5c18dfb69b9), commit `948a07b369c62adb3b12e102458be5c18dfb69b9`, *A Programming Paradigm for Spatiotemporal Composability*.
- Cordis: [`cordiverse/cordis`](https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4), commit `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4`, `packages/core` version `4.0.0-rc.8`.
- graph-core: this pass started from `main@62f556d37a1f8c4c7c5cd26d9e21917abe17816a`; M2-A is integrated.

### Decision

The existing **YES, BUT NARROWER** decision remains correct, but the paper sharpens the boundary:

1. **Cordis is not fundamentally a dynamic graph or plugin container.** It is a context-mediated programming paradigm for spatiotemporal composability. The dependency graph is derived runtime structure, not the sole execution object.
2. **graph-core has already captured important invariants**: explicit capability identity, declared requirements, exact provider identity, scopes, fiber lifecycle, effect ownership, generation-aware replacement, and failure isolation.
3. **M2-A should not be read as a complete port of Cordis v4 spatial composability.** graph-core has `DependencyEpoch` and explicit `notify_dependency_change()`, but the inspected code does not connect `Scope::provide/replace/remove` to a Cordis-style automatic `ReflectService.notify()` chain for all affected fibers; provider withdrawal also lacks the complete committed-view + dependent-drain guard protocol.
4. **Cordis effect inverses are not a replacement for graph-core's durable effect model.** Paper Section 6.1 explicitly places irreversible external emissions beyond the recoverable system boundary. Intent / dispatch / outcome, idempotency and reconciliation must remain separate authorities.
5. **The highest-value next research target is reactive dependency lifecycle, not loader/HMR.** Provider identity changes should drive safe dependent exit, rebinding and restart without violating M1's frozen in-flight task capability pinning.

---

## 1. What the paper actually formalizes

### 1.1 Temporal composability

A revertible effect is conceptually:

```text
Γ -> Γ × inverse
```

An effect supplies an inverse at the point of application and the runtime tracks and composes inverses for component removal.

The important properties are stronger than an unload hook:

- inverse creation is local to the effect;
- composite recovery follows from atomic inverses;
- disposal is at-most-once;
- partial activation can recover only the effects that already completed;
- arbitrary withdrawal order across interleaved components needs an additional independence/commutativity discipline.

Therefore the paper does **not** claim that arbitrary side effects can always be rolled back. Global temporal composability depends on witnessed inverses, observational equivalence, and effect independence.

### 1.2 Spatial composability

A component declares a coeffect specification: what it requires from its environment. Every context change is classified against that specification as:

```text
activating | deactivating | neutral
```

Dependencies are therefore not injected once at construction. They are runtime relations that remain subject to re-resolution:

```text
provider appears      -> consumer may activate
provider identity changes -> old binding becomes stale and consumer reloads
provider disappears   -> consumer leaves before provider recovery completes
```

This reactive lifecycle is the main difference from a conventional DI container.

### 1.3 Unified Context

The paper unifies effect and coeffect state into a recursive context carrying:

```text
Context
├─ current state
├─ accumulated inverse
└─ dependency/coeffect state
```

Component/environment interactions mediated by that context can be attributed to one component/fiber lifecycle.

### 1.4 Components and fibers

A component is formalized as:

```text
Component = required dependencies (d)
          + provided keys (p)
          + witnessed effect function (e)
```

A fiber is one runtime instantiation. The realistic lifecycle is:

```text
INACTIVE
  -> RELOADING
  -> ACTIVE
  -> UNLOADING
  -> INACTIVE / FAILED
```

The calculus then adds withdrawal, effect iteration, asynchronous inertia, failure, parent/child instantiation and committed dependency views.

Cordis's primary form of dynamism is therefore **component lifecycle reacting to context changes**, not workflow scheduling over a mutable DAG.

---

## 2. Mapping the formal model to current Cordis v4

Paper Section 5 provides a theory-to-implementation correspondence. Current `8cc9e33` source follows it semantically, although paper pseudocode field names are not literal source field names.

### 2.1 `ctx.effect`: the runtime realization of revertible effects

`packages/core/src/fiber.ts` implements `Fiber.effect()` with:

- sync and async callbacks;
- iterable and async-iterable disposer production;
- fiber-local ownership;
- at-most-once disposal through the armed/epoch state;
- partial cleanup after setup failure;
- nested effect metadata.

`packages/core/tests/dispose.spec.ts` covers repeated disposal, nested/yielded LIFO recovery, partial asynchronous abort, and cleanup of already completed setup after failure.

The paper also states an important limitation: **the runtime does not verify that a disposer is a mathematically correct inverse**. Correctness is a component-author contract.

#### Implementation nuance: LIFO is local, not globally sequential

There are two cleanup layers in current Cordis:

1. yielded/nested disposers inside one `ctx.effect()` are composed in strict LIFO order;
2. `Fiber._unload()` obtains top-level disposables in reverse registration order but starts them through `Promise.all(...)`.

It is therefore too strong to say that all fiber-level cleanup completes in globally sequential LIFO order. A more precise reading is: **local composite effects are LIFO; top-level sibling cleanup may complete concurrently.** The paper's global treatment relies on independence where reordering is allowed.

graph-core's current `EffectStack::dispose_all()` is intentionally more conservative: it awaits each disposer sequentially in reverse order. That is a valid Rust adaptation, not something that must be changed to match Cordis implementation detail.

### 2.2 `inject` and provider identity

`Plugin.Base.inject` declares requirements and `ReflectService.provide()` binds a provider to the current fiber.

A Fiber maintains both current and committed resolution state:

- `_store`: providers resolvable from the current environment;
- `store`: the provider view committed for the current activation.

`_refresh()` builds the dependency epoch from provider fiber UIDs, not service value equality. Consequently:

- an in-place value update by the same provider does not count as provider replacement;
- a new provider with an equal value is still a different binding.

This matches the paper's target/committed-view rule: **dependency identity is provider identity, not value equality.**

### 2.3 Provider withdrawal: the most important remaining reference for graph-core

The disposer returned by `ReflectService.provide()` follows a significant sequence:

```text
1. remove provider from the globally resolvable store
2. notify affected consumers
3. await the consumers' fiber.await()
4. only then remove the provider's own committed binding
```

This means:

- future consumers immediately stop seeing the provider;
- committed consumers are driven into unload;
- consumer teardown can still use the binding it committed to;
- provider recovery waits for dependent teardown.

This is the implementation counterpart of the paper's withdrawal guard. It is a **live dependency handoff protocol**, not merely reverse-topological destruction.

### 2.4 Inertia

`Fiber._setEpoch()`, `_reload()` and `_unload()` serialize lifecycle transitions through `fiber.inertia`.

If dependencies change while asynchronous initialization is in flight, Cordis does not pretend the work can always be cancelled. The current transition lands, then the new target is observed and the fiber chains into unload/reload:

```text
transition starts
-> target changes in flight
-> transition lands
-> stale target is observed
-> unload/reload converges to the latest target
```

This is the paper's inertia property and is a more realistic model than assuming arbitrary futures can be synchronously aborted.

### 2.5 Isolation and interception

`Context.isolate()` derives a context with a new realm symbol for a key. `Context.intercept()` derives a metadata/configuration overlay, and `Service.resolveConfig()` merges intercept layers.

The paper classifies these as derived realizations rather than shared-state mutations requiring tracked inverses. graph-core's explicit child/isolate/intercept contexts retain the semantic core without copying JavaScript Proxy/prototype mechanics.

---

## 3. Loader and HMR: useful references above the kernel

### 3.1 Component Loader is desired-state reconciliation

`@cordisjs/plugin-loader` uses Entry/EntryTree as a declarative description of desired composition:

- entries describe component, config, inject, disabled and grouping state;
- entry changes update or recreate fibers;
- component self-update/self-disable can be written back to the entry;
- the tree owns create/remove/update and import behavior.

This is orchestration/configuration policy above the context/coeffect substrate. Keeping loader/config language outside graph-core's capability kernel remains correct.

### 3.2 HMR is a three-phase process-local reload transaction

The paper and current `packages/hmr/src/index.ts` align on:

1. **module classification** — propagate accepted/declined state through the import graph; framework externals force full restart;
2. **stale-entry detection** — reload only component entries whose dependency tree reaches an accepted module;
3. **transactional reload** — back up and invalidate ESM/CJS caches, re-import stale entry modules, replace fibers, and restore caches/old fibers on failure.

The useful graph-core lesson is staged replacement:

```text
prepare new artifacts
-> validate all
-> commit publication
-> retire old
-> roll back private/prepared state on failure
```

Cordis HMR is not durable ACID: it is a best-effort process-local transaction over Node module caches and fiber lifecycle. Old-plugin disposal errors are logged and swap handling continues. It is reference material for future replacement coordination, not durability evidence.

---

## 4. System boundary: why Cordis is not durable execution

Paper Section 6.1 is especially important for graph-core.

### 4.1 Acquisition can often remain inside the context boundary

Examples include:

```text
open  -> close
malloc -> free
fork   -> kill
listener register -> unregister
```

The system owns an internal record and can exclusively remove it, so these operations can be represented as revertible process-local effects.

### 4.2 Emission usually crosses the boundary

Examples include:

```text
write bytes to an externally observed file
send a datagram
send an email
charge a payment
perform a non-idempotent remote mutation
```

Once another party can observe the output, an inverse cannot make the event never have happened. The paper offers two classes of treatment:

- **withholding/output commit** until the producing state is committed;
- **compensation**, which restores an application-defined coarser equivalence rather than exact prior state.

This complements graph-core's M1/M2-B authority split:

```text
Capability Runtime / Cordis-like effects
    owns process-local composition and acquired-resource lifetime

DurableJournal
    owns operation intent / dispatch / outcome / reconciliation truth
```

A `ScopedEffect` disposer must therefore never become authority for whether an external operation succeeded, failed, or can be retried.

---

## 5. Paper/Cordis vs graph-core

| Paper / Cordis mechanism | Cordis v4 | graph-core now | Assessment |
|---|---|---|---|
| Revertible atomic effect | `ctx.effect` + disposer | `ScopedEffect` / `EffectScope` / `EffectStack` | Core semantics captured |
| Composite LIFO recovery | strict for yielded/nested effect; sibling fiber cleanup may run concurrently | strict reverse sequential `EffectStack` | Conservative Rust choice; keep |
| Runtime proves inverse | **No** | No | Must remain a contract/test obligation |
| Reactive dependency declaration | `inject` | requirements/dependency definitions | Implemented |
| Provider-identity target | fiber UID | `Generation + EntryId` / `DependencyEpoch` | Implemented with more explicit identity |
| Committed dependency view | `fiber.store` | `ResolvedDependencies` + retained handles | Identity/lifetime core implemented |
| Automatic context-change notification | `ReflectService.notify()` -> `_refresh()` | explicit `notify_dependency_change()` | **Partial; no complete reactive propagation** |
| Provider withdrawal guard | hide from future resolution, drain committed dependents, then recover provider | scope teardown order + Arc snapshot lifetime; no inspected automatic dependent-drain chain on fiber withdrawal | **Important difference** |
| Async inertia | `fiber.inertia` and chained transitions | `AsyncMutex` serialization + stale epoch/token check | Rust adaptation implemented |
| Failure isolation | per-fiber FAILED | per-fiber `Failed` + cleanup errors | Implemented |
| Isolation | realm-derived context | explicit isolated child context | Semantic core implemented |
| Interception | metadata/config overlay | explicit intercept config | Semantic core implemented |
| Declarative loader | Entry/EntryTree | excluded | Correctly excluded |
| HMR | classify/detect/reload+rollback | excluded | Correctly excluded; future transaction reference |
| Effect independence / observational equivalence | formal preconditions | not modeled | Do not claim full Cordis metatheory |
| Durable external effects | outside recoverable boundary | separate DurableJournal authority | graph-core separation is stronger |

---

## 6. Two different kinds of pinning

Cordis's reactive fibers and graph-core M1 task-attempt pinning operate at different levels and should coexist.

### Component/fiber level

When provider identity changes, a component can become stale and should eventually unload/rebind/reload.

### Task-attempt level

M1 freezes an exact `CapabilityHandle` for a started task attempt. Replacement affects future lookups, not an already executing attempt.

The combined rule should be:

```text
provider V1 is replaced by V2

existing task attempt
    -> retains V1 handle until the attempt ends

component/fiber lifecycle
    -> observes target change
    -> stops admitting new work / unloads when safe
    -> rebinds to V2

future task attempt
    -> resolves V2
```

Future reactive-coeffect work must not mutate an already-started TaskAttempt's dependency set to imitate Cordis reload. Quiescence and admission belong at the provider/fiber boundary.

---

## 7. Gaps exposed by the paper

These do not retroactively invalidate M2-A. M2-A remains a bounded semantic port. They refine what has not yet been proven after adding the paper as evidence.

### P0: automatic reactive propagation is not closed

`CapabilityFiber::notify_dependency_change()` increments an epoch, but the inspected `Scope::provide/replace/remove` paths do not directly maintain provider-to-dependent-fiber observation or automatically notify every affected fiber.

A full Cordis-style spatial-composability experiment should prove:

- provider publish/replace/withdraw automatically identifies affected fibers;
- only fibers depending on the resolved binding are affected;
- isolated/sibling consumers are not spuriously reloaded;
- notification storms may coalesce without losing the final target;
- failed/disposed fibers are not accidentally revived.

### P0: provider withdrawal needs a dependent-drain protocol

`Scope::teardown()` already computes deterministic dependency-aware destruction, and retained exact `Arc` dependency snapshots keep old provider objects alive while readers exist.

That is strong lifetime safety, but Cordis makes an additional ordering claim:

- provider disappears from future resolution first;
- consumer runs its own teardown;
- consumer teardown retains its committed provider;
- provider recovery begins only after committed dependents quiesce.

The future test target is therefore not just “reverse-topological drop”; it is **withdraw visibility + committed access + dependent quiescence + provider recovery**.

### P1: effect independence is not a graph-core contract

The current Rust implementation uses strict sequential reverse cleanup and therefore avoids many independence questions. Keep that simple model unless measurements justify concurrent teardown.

If concurrency is later required, classify at least:

```text
ordered effects
independent effects
external/durable effects
```

and add an explicit commutativity/ownership contract for independent groups.

### P1: CapabilityId is not interface compatibility

`Generation + EntryId` solves provider-publication identity, but it does not solve independently built ecosystem problems:

- key collision;
- interface drift;
- behavioral contract version mismatch.

Paper Section 6.6 discusses namespacing, package/peer version constraints, and structural compatibility. A future Rust-oriented model may become:

```text
CapabilityId = namespace + logical name
CapabilityContract = typed trait / schema / optional version fingerprint
```

There is not yet evidence to expand the public API immediately; retain this as a plugin-ecosystem problem.

### P2: HMR/dynamic loading remains outside the kernel

The paper does not change this decision. It adds a useful design reference: separate prepare/import from publish/retire, and retain rollback material.

---

## 8. Rust-specific choices

### 8.1 Do not reproduce Proxy semantics

Paper Section 6.4 explicitly allows language-specific mediation and names Rust procedural macros as a way to generate typed dependency declarations/accessors.

graph-core's explicit handles and context APIs better match Rust ownership and authority. Consider derive/proc-macro ergonomics only if declaration boilerplate becomes measured friction.

### 8.2 Keep fail-fast cycle rejection

In the base reactive model a dependency cycle leaves components permanently inactive; the paper notes that this is predictable from declarations and can be reported when loading components.

graph-core's admission-time structured cycle error is a stronger and more appropriate kernel policy.

### 8.3 Prefer strict cleanup ordering until there is evidence for concurrency

`EffectStack`'s sequential reverse await is easy to reason about and verify. Cordis's concurrent sibling cleanup is an implementation optimization, not a Rust requirement.

---

## 9. Recommended next experiments

Do not start with loader/HMR. First run three bounded runtime experiments.

### Experiment A — Reactive provider replacement

Goal: prove that exact provider identity changes automatically drive dependent-fiber convergence.

Acceptance:

- after V1 -> V2, an old active consumer stops admitting new work;
- existing in-flight consumer handles remain valid;
- the consumer eventually re-enters Active against V2;
- isolated/sibling consumers do not reload;
- concurrent V1 -> V2 -> V3 converges only to V3 without stale publication.

### Experiment B — Withdrawal guard and dependent quiescence

Goal: reproduce the engineering protocol corresponding to the paper's provider-withdrawal ordering, rather than only reverse-topological drop.

Acceptance:

- provider withdrawal removes it from future lookup immediately;
- dependent teardown retains its committed provider handle;
- provider cleanup starts only after all committed dependents quiesce;
- dependent cleanup failure has a defined policy and cannot hang provider recovery indefinitely;
- parent/child scope order remains distinct from logical dependency order.

### Experiment C — Process-local inverse vs durable emission boundary

Goal: encode the system boundary in tests so the two meanings of “effect” cannot silently merge later.

Acceptance:

- local acquisitions can register `ScopedEffect` cleanup;
- only DurableJournal changes dispatched external-operation facts;
- capability teardown cannot overwrite a dispatched operation outcome;
- non-idempotent unknown outcomes still require reconciliation even when a disposer exists.

Only after A/B/C should graph-core decide whether it needs multi-capability atomic replacement, capability-interface versioning, declarative loading, or HMR/dynamic module loading.

---

## 10. Architectural impact

The paper cross-check does **not** require replacing the current architecture. It strengthens the authority split:

```text
Capability Graph / Context
    identity, visibility, dependency declarations

Reactive Capability Runtime
    fiber state, provider target, committed dependency view,
    effect ownership, quiescence, replacement convergence

Runtime Core
    task attempts, scheduling coordination, exact capability pinning

Durable Workflow / Journal
    operation intent, dispatch, outcome, retry/reconciliation truth

Execution Stream
    observations only
```

The layer most worth completing is the second: **reactive dependency lifecycle**. Do not grow the first into a universal Graph, and do not move durable effect truth from the fourth into `ScopedEffect`.

The most precise statement of Cordis's value to graph-core is now:

> **Cordis is not a Rust plugin-framework checklist to copy. It provides formal invariants for how dynamic component lifetimes, environmental dependencies, and locally revertible effects compose. graph-core should absorb those invariants while preserving stricter Rust identity, ownership, and durability authorities.**

## Primary source map

- Paper: <https://github.com/cordiverse/paper/tree/948a07b369c62adb3b12e102458be5c18dfb69b9>
- Cordis core context: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/context.ts>
- Cordis fiber/effects: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts>
- Cordis coeffect/provider resolution: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts>
- Cordis registry/inject: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts>
- Cordis loader: <https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/loader>
- Cordis HMR: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/hmr/src/index.ts>
- graph-core capability runtime: `crates/capability-graph/src/runtime.rs`
- graph-core Cordis semantic adaptation: `crates/capability-graph/src/semantic.rs`
- graph-core runtime authority contract: [`../runtime/M1-runtime-core.md`](../runtime/M1-runtime-core.md)
