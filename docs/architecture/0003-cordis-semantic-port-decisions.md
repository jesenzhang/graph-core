# ADR 0003: Cordis semantic port boundaries

Status: accepted for M2-A

## Decision

Port Cordis's capability-runtime behavior into explicit Rust structures:
contexts/scopes, plugin runtime identity, dependency requirements, fiber
epochs, owned effects, and quiescent disposal. Do not port JavaScript syntax
or dynamic object behavior.

The owning crate is `capability-graph`. `runtime-core` may coordinate it, but
does not become a second capability-lifecycle authority.

## Async runtime selection

M2-A uses Tokio directly in `capability-graph` because Fiber initialization and
effect disposal must await user-supplied futures while lifecycle transitions
remain serialized. The public boundary is still ordinary `Future` values and
explicit async methods; no Tokio task handle or Tokio-specific type crosses
the capability API. This keeps a future executor substitution localized to the
owning crate if a later runtime boundary requires it.

## Service / Reflect / Events decision

| Cordis surface | Decision | Rust form | Reason |
|---|---|---|---|
| `Service` | ADAPT | typed capability factory/evaluate callback | Service lifecycle and owned registration are useful; a universal class hierarchy is not required by Rust. |
| `Reflect` | ADAPT/REJECT | explicit `CapabilityContext::get`, `provide`, `override` | Preserve deterministic resolution and scoped interception; reject Proxy/property traps and runtime reflection. |
| `Events` | ADAPT/DEFER | typed lifecycle observers | Observer hooks are useful for tests and coordination; generic `Any` event buses and Cordis's dynamic waterfall API are outside M2-A. |
| proxy context | REJECT | direct methods and typed handles | Rust ownership and the type system provide the safer boundary. |
| declaration merging | REJECT | explicit traits and requirement values | Compile-time Rust APIs should not depend on ambient global extension. |
| `group` / `include` | DEFER | extension packages later | Configuration composition is not core capability lifecycle. |
| `loader` / `hmr` | DEFER | loading/watch layers later | Filesystem and hot-module policy are outside the process-local kernel. |

## Invariants preserved

- a resolved capability handle pins the exact capability generation and entry;
- replacement changes future resolution only;
- an in-flight M1 attempt remains on its exact old handle;
- dependency changes invalidate the current fiber epoch and are serialized
  through unload/reload;
- effect cleanup is owned by the fiber/context scope and is reverse ordered;
- a stale load or cleanup completion cannot publish into a newer epoch;
- workflow, durable recovery, and execution streams remain separate
  authorities.

## Rejected alternatives

- `Arc<Mutex<dyn Any>>` as a universal service locator;
- treating a Cordis `Fiber` as a durable workflow task;
- using an execution stream to recover capability state;
- serializing the complete process-local fiber object graph;
- adding a plugin ABI, WASM boundary, database, scheduler, or agent loop in
  M2-A.
