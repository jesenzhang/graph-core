# Cordis Port Matrix

Status: M2-A Integrated

## M2-A closeout

- Code candidate: `2e148d816e4a1390f08c02114ea0904f1685e793`.
- Independent semantic review: PASS, 0 blockers.
- Candidate branch CI: run `32108287390`, PASS.
- Integrated main CI: run `32110400912`, PASS.
- Cordis baseline: `cordiverse/cordis` commit
  `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4`.
- Known follow-ups: M2-B0 durability reference mapping and durable-state
  boundary design; persistence implementation remains out of scope.

This matrix freezes the reference used for the semantic port. “PORT” means
the behavioral invariant is implemented in Rust with an explicit Rust API;
it does not mean the TypeScript API shape is copied.

## 2026-08-18 paper reconciliation

The original M2-A port was source-driven. The later Cordis paper,
*A Programming Paradigm for Spatiotemporal Composability*, was cross-checked
against the same upstream Cordis commit on 2026-08-18. The detailed record is
[`CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.md`](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.md)
([中文](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.zh.md)).

The paper does **not** invalidate the M2-A acceptance result, but it narrows
what M2-A proves:

1. `DependencyEpoch` plus exact `Generation`/`EntryId` is a strong Rust
   counterpart to Cordis's provider-identity target and committed dependency
   view.
2. M2-A exposes `CapabilityFiber::notify_dependency_change()`, but the
   inspected `Scope::provide/replace/remove` paths do not themselves implement
   Cordis's full automatic context-change notification chain. Full reactive
   coeffect propagation therefore remains a separate semantic target.
3. `Scope::teardown()` gives deterministic dependency-aware release and exact
   snapshot lifetime, but Cordis provider withdrawal additionally hides the
   provider from future resolution, drives committed consumers through their
   own teardown, keeps the old binding readable during that teardown, and
   waits for dependent quiescence before provider recovery. That protocol is
   not implied by reverse-topological drop alone.
4. Cordis's inverse model is process-local. The paper's system-boundary
   discussion explicitly separates recoverable acquisitions from external
   emissions. M1/M2 durable operation intent, dispatch, outcome, idempotency,
   and reconciliation remain separate and higher-priority authorities.
5. The paper's global temporal-composability results depend on witnessed
   inverses, observational equivalence, and independence assumptions that
   graph-core does not currently model. No full Cordis metatheory is claimed
   for M2-A.

One source-level correction is also frozen here: a single Cordis `ctx.effect`
composes yielded/nested disposers in strict LIFO order, but current
`Fiber._unload()` obtains top-level disposables in reverse order and starts
those sibling cleanups with `Promise.all(...)`. graph-core's `EffectStack`
intentionally uses stricter sequential reverse-order cleanup. That is a
conservative Rust adaptation, not an incompatibility.

## Frozen references

- Cordis paper: [`cordiverse/paper`](https://github.com/cordiverse/paper)
  at `948a07b369c62adb3b12e102458be5c18dfb69b9` (draft dated 2026-08-13).
- Cordis upstream: [`cordiverse/cordis`](https://github.com/cordiverse/cordis)
  at `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4` (`main`), package
  `packages/core` version `4.0.0-rc.8`.
- DeepSeek Harness: [`deepseek-ai/deepseek-harness`](https://github.com/deepseek-ai/deepseek-harness)
  at `99f6f02fecdb7dff40c3fbc9470f5907c29f74ca` (`master`). Its vendored
  `@deepseek-ai/cordis` package is version `4.0.1`; the vendoring manifest
  records upstream Cordis core `56b3d4f725681cf4556c1a8695a709cc3b6eed74`
  (`4.0.0-rc.7`) plus local lifecycle hardening.
- Directly inspected Cordis files: `packages/core/src/context.ts`,
  `registry.ts`, `fiber.ts`, `service.ts`, `reflect.ts`, `events.ts`, and
  `utils.ts`; related package directories `group`, `include`, `loader`, and
  `hmr` were checked for core dependencies and deferred extension behavior.
- Directly inspected Harness files: `vendor/README.md`,
  `vendor/cordis/package.json`, `packages/core/agent/package.json`,
  `packages/core/scope/package.json`, `packages/core/agent/README.md`, and
  `docs/cordis-primer.md`.

Harness is therefore not treated as an unmodified copy of current Cordis
main. Its practical baseline uses the vendored API and adds lifecycle,
transaction, and ownership hardening. The Rust port follows the shared
behavioral invariants and keeps M1 capability pinning as the higher-priority
compatibility rule.

## Summary matrix

| Cordis | graph-core Rust | Strategy | Status |
|---|---|---|---|
| Context | `CapabilityContext` / `Scope` | PORT | implemented |
| `extend` | `child_context` / child scope | PORT | implemented |
| `isolate` | scoped isolation boundary | PORT | implemented |
| `intercept` | typed local override map | PORT | implemented |
| `RegistryService` | `CapabilityRegistry` | PORT | implemented |
| plugin runtime | `PluginRuntime` | PORT | implemented |
| one runtime, many fibers | runtime metadata + `CapabilityFiber` set | PORT | implemented |
| Fiber | `CapabilityFiber` | PORT | implemented |
| `FiberState` | `FiberState` | PORT | implemented |
| `inject` | explicit `Requirement` list | PORT | implemented |
| provider-identity target | `DependencyEpoch` over `Generation`/`EntryId` | PORT | implemented |
| committed dependency view | `ResolvedDependencies` + retained handles | PORT | implemented for identity/lifetime |
| automatic context-change notification | explicit `notify_dependency_change()` | STUDY | partial; automatic propagation not proven |
| provider-withdrawal dependent drain | scope order + exact snapshot lifetime | STUDY | partial; live withdrawal protocol not proven |
| `effect` | `ScopedEffect` | PORT | implemented |
| `DisposableList` | `EffectStack` | PORT | implemented with stricter sequential LIFO |
| `restart` | `CapabilityFiber::restart` | PORT | implemented |
| `update` | `CapabilityFiber::update` | PORT | implemented |
| Service | typed capability factory/evaluate boundary | ADAPT | evaluated, no dynamic service base |
| Reflect | explicit context lookup and registration | ADAPT | proxy rejected |
| Events | lifecycle observer hooks | ADAPT | dynamic dispatch modes deferred |
| effect independence / observational equivalence | no direct kernel counterpart | STUDY | not modeled; no global-theorem claim |
| `group` | extension package | LATER/STUDY | deferred |
| `include` | extension package | LATER/STUDY | deferred |
| `loader` | plugin loading/config layer | LATER | deferred |
| `hmr` | hot reload/watch layer | LATER | deferred |

## PORT contracts

### Context, scope, and overrides

Reference: `context.ts` `Context` constructor, `extend()`, `isolate()`, and
`intercept()`, plus `reflect.ts` lookup. Cordis uses prototype/proxy lookup;
the invariant is nearest local resolution, parent fallback, isolation of a
named service key, and scoped configuration override. Rust stores `local`,
`parent`, an isolation boundary, and explicit intercept/config maps. A child
does not copy its parent. A lookup is deterministic: local override, then
parent fallback unless isolation hides that key. A handle already returned by
lookup keeps its exact entry alive across parent replacement.

Existing equivalent: `capability_graph::Scope` already proves root/child
fallback, local replacement, exact `Generation` and `EntryId`, and reader
ownership. M2-A adds the named context facade and explicit isolation/intercept
operations while retaining `Scope` as the ownership primitive.

Compatibility tests: parent fallback, child override, sibling isolation,
intercept precedence, and a pinned handle surviving parent replacement.

### Registry and plugin runtime

Reference: `registry.ts` `Plugin.Runtime`, `RegistryService.resolve()`,
`plugin()`, `delete()`, and `Inject.resolve()`. The callback/object identity
is the logical plugin identity; one runtime metadata record owns many fiber
instances; deleting a runtime removes it from lookup and disposes its fibers.
Invalid plugin/config must fail closed before publication.

Rust mapping: `PluginId` is an explicit stable identity, `PluginRuntime` owns
metadata and fibers, and `CapabilityRegistry` owns runtime lookup. A runtime
removal is exact and idempotent. Configuration is supplied through a typed
factory boundary; no `Any` service locator is introduced.

Compatibility tests: duplicate registration, one runtime with multiple
fibers, invalid registration, exact removal, and cleanup of every associated
fiber.

### Fiber, requirements, provider identity, and dependency epoch

Reference: `fiber.ts` `FiberState`, `_refresh()`, `_setEpoch()`, `_reload()`,
`_unload()`, `await()`, `restart()`, and `update()`. A fiber is pending until
all declared injections resolve, loads once, unloads when a required
implementation disappears or changes, and serializes unload/reload. Cordis
computes the target from provider fiber identity rather than service value;
a replacement provider is therefore different even when it exports an equal
value.

Rust mapping: `CapabilityFiber` is an explicit state machine. A
`DependencyEpoch` snapshots exact `Generation`/`EntryId` requirements and a
`PluginLoadContext` retains the exact `ResolvedDependencies`. Each load
attempt carries an epoch token; completion publishes only when the token and
exact dependencies are still current. This is the Rust counterpart to the
paper's target/committed-view identity rule.

Important limit: dependency-change signaling is currently explicit through
`notify_dependency_change()`. M2-A proves stale-publication rejection and
same-fiber convergence when driven, but it does not by itself prove that every
`Scope` mutation automatically discovers and drives all affected dependent
fibers.

Compatibility tests: every Stage 3 transition, dependency replacement while
loading/unloading, restart, update, failed initialization, stable-state await,
and final disposal.

### Effects and disposal

Reference: `fiber.ts` `effect()` and `utils.ts` `DisposableList`, cross-checked
against paper Section 5.1.1. Effects are owned by the fiber/context scope and
may register nested effects and asynchronous disposers. A single composite
`ctx.effect()` unwinds yielded/nested effects in reverse order and can recover
partial setup.

At the outer Fiber level, current Cordis reverses the top-level disposable
list before launching cleanup through `Promise.all(...)`; sibling disposer
completion is therefore not a globally sequential LIFO guarantee. The paper's
global reordering results are conditional on effect independence.

Rust mapping: `EffectStack` owns `ScopedEffect` values and intentionally awaits
each disposer sequentially in reverse registration order. Cleanup continues
after errors. `Drop` only releases Rust-owned memory; explicit disposal remains
the authority for process-local external resources.

Neither Cordis nor graph-core proves that a developer-supplied disposer is a
correct semantic inverse. That remains a component contract and must be
validated with effect-specific tests.

Compatibility tests: reverse order, nested ownership, partial setup failure,
async setup/cleanup, repeated disposal, cleanup error isolation, and stale
epoch publication.

### Provider withdrawal and committed dependencies

Paper Section 4.3.1 and Cordis `ReflectService.provide()` add a stronger rule
than dependency-aware destruction: when a provider leaves, it first stops
participating in future resolution, then notifies and drains consumers that
committed to it, while those consumers retain access to their committed
binding during teardown. Provider recovery runs only after those dependents
have quiesced.

graph-core currently has two related but distinct mechanisms:

- `Scope::teardown()` stops new lookup/mutation and releases local ownership in
  deterministic dependency order;
- each published entry retains exact dependency handles, so old provider
  instances remain alive while dependents/readers hold snapshots.

Those mechanisms provide strong identity/lifetime safety but do not alone
prove Cordis's live withdrawal protocol. Automatic reactive notification and
dependent quiescence are retained as follow-up semantics rather than silently
claimed as part of M2-A.

### System boundary and durability

The Cordis paper distinguishes recoverable acquisitions from emissions that
cross the system boundary. A resource handle/register/unregister lifetime can
fit a revertible effect; an externally observed send, payment, or other
non-idempotent mutation cannot be made as-if-never-happened by a disposer.
Withholding/output commit or application-level compensation is required.

This reinforces graph-core's existing authority split: `ScopedEffect` owns
process-local cleanup; `DurableJournal` owns operation intent, dispatch,
outcome and reconciliation truth. Capability teardown must never reinterpret
a dispatched external operation.

## Stage 1 decisions

1. Existing `capability-graph` graph validation and `Scope` ownership stay;
   they are extended with runtime modules, not replaced.
2. The crate evolves from “graph only” to “graph plus capability runtime”,
   while `CapabilityGraph` remains a topology/resolution type and lifecycle
   authority remains in runtime types.
3. No new crate is required for M2-A. The existing crate owns the capability
   invariants; `runtime-core` consumes its public handles.
4. Cordis proxy property lookup, prototype inheritance, declaration merging,
   ambient async context, dynamic loader, HMR, and generic JS event ergonomics
   are not ported.
5. Dependency direction remains one-way:

   `runtime-core -> capability-graph -> core`

   `workflow-graph`, `workflow-recovery`, and `execution-stream` remain peers;
   capability runtime state is not written into their authorities.

6. `Service`, `Reflect`, and `Events` are adapted where their ownership and
   lifecycle semantics are useful, and rejected or deferred where they only
   provide dynamic-language convenience. See the decision note for the
   detailed PORT/ADAPT/DEFER/REJECT record.
7. M1 task-attempt capability pinning remains authoritative: reactive provider
   replacement may converge component/fiber state for future work, but it must
   not rewrite exact capability handles already retained by an in-flight task
   attempt.
8. Loader/HMR remains above the kernel. Cordis's classify/detect/reload and
   rollback pattern is retained only as a future multi-capability replacement
   reference, not as durability evidence.
