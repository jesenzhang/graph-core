# Cordis Port Matrix

Status: M2-A Stage 1 contract

This matrix freezes the reference used for the semantic port. “PORT” means
the behavioral invariant is implemented in Rust with an explicit Rust API;
it does not mean the TypeScript API shape is copied.

## Frozen references

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
| dependency epoch | `DependencyEpoch` | PORT | implemented |
| `effect` | `ScopedEffect` | PORT | implemented |
| `DisposableList` | `EffectStack` | PORT | implemented |
| `restart` | `CapabilityFiber::restart` | PORT | implemented |
| `update` | `CapabilityFiber::update` | PORT | implemented |
| Service | typed capability factory/evaluate boundary | ADAPT | evaluated, no dynamic service base |
| Reflect | explicit context lookup and registration | ADAPT | proxy rejected |
| Events | lifecycle observer hooks | ADAPT | dynamic dispatch modes deferred |
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

### Fiber, requirements, and dependency epoch

Reference: `fiber.ts` `FiberState`, `_refresh()`, `_setEpoch()`, `_reload()`,
`_unload()`, `await()`, `restart()`, and `update()`. A fiber is pending until
all declared injections resolve, loads once, unloads when a required
implementation disappears or changes, and serializes unload/reload. An epoch
change invalidates stale initialization; a final dispose cannot reload.

Rust mapping: `CapabilityFiber` is an explicit state machine. A
`DependencyEpoch` snapshots the exact `Generation`/`EntryId` requirements.
Each load attempt carries an epoch token; completion publishes only when the
token is still current. A transition queue coalesces changes but never creates
a replacement fiber merely to escape a race.

Compatibility tests: every Stage 3 transition, dependency replacement while
loading/unloading, restart, update, failed initialization, stable-state await,
and final disposal.

### Effects and disposal

Reference: `fiber.ts` `effect()` and `utils.ts` `DisposableList`. Effects are
owned by the fiber/context scope, may register nested effects and asynchronous
disposers, and unwind in reverse registration order. Cleanup failures are
observed independently so one failure does not suppress the rest.

Rust mapping: `EffectStack` owns `ScopedEffect` values. Synchronous tests use
owned disposer closures; async lifecycle uses explicit `Future` settlement
through the runtime's lifecycle executor. `Drop` only releases Rust-owned
memory; explicit disposal remains the authority for external resources.

Compatibility tests: reverse order, nested ownership, partial setup failure,
async setup/cleanup, repeated disposal, cleanup error isolation, and stale
epoch publication.

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
