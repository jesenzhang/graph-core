# Cordis Capability Research

English | [中文](CORDIS-CAPABILITY-RESEARCH.zh.md)

Snapshot taken on 2026-08-14 from the local checkouts you provided:

- `F:\Workspace\cordis` @ `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4` (`main...origin/main`, clean)
- `F:\Workspace\deepseek-harness` @ `47f943859bef60e4160492346772ded9b24f765a` (`master...origin/master`, clean)

Remote comparison:

- `cordiverse/cordis` `HEAD` matched the local checkout exactly.
- `deepseek-ai/deepseek-harness` `HEAD` matched the local checkout exactly.
- No working-tree differences were present in either repo, so there is no local-vs-remote drift to record.

Scope: current source only. This memo treats the live repository state as authoritative and does not assume older docs are still correct.

Primary source links:

- Cordis repo: <https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4>
- DeepSeek Harness repo: <https://github.com/deepseek-ai/deepseek-harness/tree/47f943859bef60e4160492346772ded9b24f765a>

## Fast Read

- **A, must have in Rust graph-core**: explicit ownership, idempotent disposal, staged publication, dependency declaration, scope-local registration, and replacement that cannot be confused with identity.
- **B, likely useful later**: hot reload / HMR, scope hierarchies, scope-aware registries, and event dispatch modes beyond simple pub-sub.
- **C, Cordis-specific and not a literal Rust target**: proxy-based property injection, TypeScript declaration merging, AsyncLocalStorage initiator attribution, and Cordis effect-generator ergonomics.
- **D, needs experiment**: whether graph-core should support parent-linked scope admission, one-scope-per-context, reload semantics, and the exact shape of teardown composition.

## A. Rust graph-core 必须有

### 1) Ownership must be explicit and separable from ambient context

Cordis keeps the live runtime object separate from the registration owner:

- `Context` is just the proxy-backed shell for services/fibers; actual resolution happens through the reflect layer, not by storing ownership in random fields. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/context.ts#L1-L67>
- `RegistryService.delete()` tears down every fiber tied to the runtime, and `Fiber.dispose()` is the quiescence boundary that waits for in-flight work. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L162-L170>, <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L275-L458>
- DeepSeek Harness makes the same split explicit for agents: `AgentHandle` is the consumer capability, while the bare registry entry cannot tear the agent down. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L41-L45>

Rust implication:

- keep the owned handle separate from the indexed/live record;
- make disposal idempotent;
- treat the registry entry as lookup state, not as authority to destroy;
- return a dedicated teardown token from creation.

### 2) Dependency declaration should happen before composition, not via hidden lookups

Cordis expresses dependency requirements directly on the plugin and resolves them when the plugin mounts:

- `Plugin.Base` carries `inject`, `provide`, `intercept`, and `Config`. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L63-L100>
- `Inject.resolve()` normalizes array/object/prototype-chained declarations into one dependency map. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L42-L60>
- DeepSeek Harness elevates `inject` to the primary dependency declaration in its primer and tutorial. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-primer.md#L9-L13>, <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-tutorial/index.md#L40-L46>

Rust implication:

- declare required capabilities at registration time;
- reject composition before runtime work starts;
- keep dependency discovery static enough to validate in tests and during load.

### 3) Scope must be a first-class owned view, not just a tag

DeepSeek Harness’s `dsh-scope` is the clearest source here:

- `createScope(ctx, key)` mints a tagged Cordis context and returns both the scoped context and the exact disposer. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L123-L146>
- `Scope.ctx` is the registration context; registrations through it are owned by that scope and inherit the minting plugin’s dependency API. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L104-L111>, <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L129-L145>
- `scopeTarget(base, key)` is routing-only: it preserves the base filter and admits listeners by scope key/ancestor chain. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L158-L184>
- `scopeChainOf()` and `bindScopeParent()` establish explicit parent links, and the parent relation is cycle-checked. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L49-L101>

Rust implication:

- model a scope as a real owned context/view with a disposal boundary;
- keep routing keys separate from the subject object;
- if you need child/parent scope behavior, make it explicit and cycle-checked.

### 4) Replacement must be staged, identity-checked, and rollback-safe

Cordis replacement semantics are built around exact entry identity:

- `Fiber._reload()` and `_unload()` move between loading/unloading states and wait for all disposables before clearing state. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L399-L458>
- `ReflectService.provide()` rejects double registration, stores the exact impl under a scope key, and notifies dependents on change. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts#L175-L227>
- `AgentRegistry.enter()` in DeepSeek Harness inserts an unpublished entry first, then `announce()` publishes it, and stale detach capability cannot delete a later same-id replacement. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L474-L575>
- The agent factory path says concurrent same-id creation may prepare in parallel, but only one exact entry can publish; losers roll back their private scope/session/driver. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L41-L45>

Rust implication:

- do not replace an instance by mutating identity in place;
- stage the new instance, validate it, then atomically publish it;
- make teardown keyed to the exact entry object/version, not to a string id alone;
- losers in a race must roll back their private resources completely.

### 5) Lifecycle must be ordered, quiescent, and idempotent

Cordis and DeepSeek Harness both treat disposal as a lifecycle boundary, not a simple drop:

- Cordis `effect()` collects sync/async/generator disposables, maintains child effect metadata, and suppresses unhandled rejection from cleanup paths. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L275-L339>
- `Fiber._unload()` waits for all disposables and can re-enter reload if the epoch changes mid-flight. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L437-L458>
- DeepSeek Harness says the agent loop owns one agent for its lifetime, drains the loop, unregisters the agent, removes session state, and then unwinds the scoped world. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L45-L51>, <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/README.md#L62-L70>

Rust implication:

- teardown should be a quiescence boundary, not a best-effort destructor;
- if teardown can race with new work, define whether the new work is rejected, latched, or replayed;
- expose idempotent disposal so callers can compose ownership safely.

## B. 以后可能需要

### 1) Hot reload / HMR semantics

Cordis supports reload-like behavior inside `Fiber._reload()`, and DeepSeek Harness uses that model for plugin hot reload and exact adapter-default rematerialization:

- Cordis reload/unload loop: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L399-L458>
- DeepSeek Harness tutorial chapter on composition and HMR: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-tutorial/06-composition-and-hmr.md>
- Agent-loop README on adapter-default rematerialization across steps/resume: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/README.md#L68-L70>

For Rust, this is useful later if graph-core needs live reconfiguration, but it is not a Phase 1 requirement.

### 2) Scope-aware registries and layered visibility

DeepSeek Harness’s `ScopedLayers` and scope-aware registries are useful if graph-core later needs per-scope overlays:

- `ScopedLayers` owns a global layer plus lazy exact-scope layers; `peek()` is chain-blind, `merge()` walks the parent chain. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/store.ts#L159-L241>
- Scope README: the registration context determines both visibility and ownership. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/README.md#L25-L31>

This is a likely fit for configurable overlays, but only if graph-core truly needs per-scope shadowing.

### 3) Event dispatch modes beyond plain pub-sub

Cordis supports `emit`, `serial`, `waterfall`, `parallel`, and `bail`, and DeepSeek Harness builds its agent-facing policy on top of those modes:

- Cordis events service: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/events.ts#L14-L178>
- DeepSeek Harness primer: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-primer.md#L15-L34>
- Agent dispatch helpers: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/dispatch.ts#L54-L175>

These are useful if graph-core wants policy hooks, retries, or around-middleware. They are not required if the Rust side only needs simple event broadcasting.

### 4) Durable session / agent replay behavior

DeepSeek Harness’s agent loop has a lot of replay-specific logic: `request/header`, `request/context`, turn boundaries, `assistant/chunk` stream anchoring, and cancellation replay. Source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/src/index.ts#L332-L495>

That is likely relevant later if graph-core needs resumable agent runs, but it is beyond a first-phase capability substrate.

## C. Cordis 特有、不应照搬

### 1) Proxy-based property injection is a TS ergonomics choice, not a Rust requirement

Cordis resolves unknown properties through a proxy trap and `ReflectService.handler`, so `ctx.foo` can act like service lookup. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts#L61-L124>

That is convenient in TypeScript, but in Rust it would hide too much behind magic. Prefer explicit accessors or typed handles.

### 2) TypeScript declaration merging is not a runtime concept

DeepSeek Harness relies heavily on `declare module ...` to extend `Context`, event maps, and lookup maps. Sources:

- <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L26-L49>
- <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L23-L37>

Rust should model these as explicit traits/registries, not as ambient global merges.

### 3) AsyncLocalStorage initiator scope is process-local attribution, not authority

DeepSeek Harness’s initiator scope uses `AsyncLocalStorage` to remember the current initiating agent across async work, and the README explicitly says ambient presence is neither liveness proof nor authorization. Sources:

- Agent service source: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L1-L17>, <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L619-L703>
- Agent README: <https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L26-L35>

For Rust, this is a traceability aid, not a security primitive.

### 4) Cordis effect-generator composition is an implementation convenience

Cordis `effect()` accepts sync values, async values, iterables, async iterables, and generator-style teardown chains. Source: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L229-L339>

The idea to keep is ordered teardown. The exact generator/iterable syntax should not be copied literally into Rust.

## D. 暂时无法判断 / 需实验

These are the places where current source is suggestive, but graph-core should not lock in a design without a Rust experiment:

1. Should a Rust scope support only one nearest tag per context, like `dsh-scope`, or allow multiple memberships?
2. Should ancestor scopes receive descendant events automatically, or should the routing rule be flatter and explicit?
3. Should reload/replacement preserve a stable identity object, or should every replacement produce a new versioned identity?
4. Should teardown be a single quiescence future, or a composable chain of owned cleanup tokens?
5. Should graph-core preserve Cordis-style `inject`/`waterfall`/`serial` distinctions, or collapse them into a smaller event API first?
6. Should replacement be allowed to start before publication, as DeepSeek Harness does for agents, or only after publication with a visible rollback path?

## Rust Implications, Condensed

If graph-core borrows only the useful parts, the Rust shape should be:

- a registry that stores exact live entries;
- an owned handle for each compositional unit;
- explicit dependency declarations at mount time;
- a scope object that owns a registration context and a quiescent disposer;
- identity-safe replacement with rollback for losers;
- clear separation between durable lineage, runtime ownership, and ambient attribution.

What it should not be:

- a proxy-heavy dynamic lookup layer;
- a hidden ambient authority system;
- a direct TypeScript port of Cordis effect/proxy/declaration-merging mechanics.
