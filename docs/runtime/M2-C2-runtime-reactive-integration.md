# M2-C2 Runtime Reactive Integration

Status: implemented on `feat/m2-c2-runtime-reactive-boundary`; base
`589827af0156fa0d3f25f5bb6f4044f2be61b527`

M2-C2 composes the existing M1 task admission, M2-B durable recovery, and
M2-C1 reactive lifecycle under one Runtime Core boundary. It does not add a
new capability manager or persistence mechanism.

## Runtime ownership model

`Runtime` owns exactly one process-local `ReactiveCapabilityRuntime`. That
coordinator owns the capability `Scope`, `CapabilityContext`,
`CapabilityRegistry`, and watched capability fibers:

```text
Runtime
  +-- WorkflowGraph authority
  +-- DurableStore / DurableJournal compatibility view
  +-- ReactiveCapabilityRuntime
  |     +-- Scope
  |     +-- CapabilityContext
  |     +-- CapabilityRegistry
  |     +-- watched fibers
  +-- Execution Streams
```

The existing `Runtime::scope()`, `capability_context()`, and
`capability_registry()` accessors remain available as views over that
coordinator. `Runtime::capability_runtime()` is the explicit reactive entry
point. There is no parallel Runtime context or registry ownership.

## Synchronous step and explicit reconciliation

`Runtime::step()` remains synchronous, deterministic, and responsible for
workflow/task admission. It does not start arbitrary plugin reconciliation.
The boundary for a mutation that should affect future admission is:

```text
ReactiveCapabilityRuntime mutation
  -> explicit async reconcile()
  -> stable capability state
  -> synchronous Runtime::step()
  -> new TaskAttempt admission
```

Direct low-level `Scope` mutation remains available for scope-level semantics,
but it does not promise reactive dependent fixed-point convergence. Callers
using reactive semantics should use the Runtime-owned coordinator mutation
helpers and the explicit reconciliation boundary.

## Task admission and pinning

Admission captures the exact capability publication visible at the stable
capability boundary. A `TaskAttempt` retains the capability handle plus its
`Generation`, `EntryId`, and stable replay/config identity.

Therefore, if attempt A admits V1, the coordinator replaces V1 with V2, and
reconciliation reaches a fixed point:

```text
A -> exact V1 handle, Generation, EntryId, replay identity
B -> exact V2 handle, Generation, EntryId, replay identity
```

Reactive replacement never rewrites A, and durable recovery never persists a
live capability handle.

## Withdrawal semantics

`withdraw_and_reconcile()` first removes the exact provider publication from
future resolution. It then quiesces affected reactive dependents according to
M2-C1 before releasing the coordinator's retirement guard. Existing attempts
retain their cloned handles, so an attempt pinned to withdrawn V1 can still
read exact V1. A later V2 publication and explicit reconciliation can reactivate
eligible dependents; a subsequent task admission sees V2.

## Restart and reconstruction

`Runtime::restore_run()` reconstructs a fresh process-local
`ReactiveCapabilityRuntime` from the supplied `Scope` and capability
configuration. It does not restore old fibers, watched-fiber registrations,
registry caches, effect scopes, async tasks, or live handles. Durable attempt
admissions are replayed by resolving fresh handles and validating their stable
replay/config identities.

The in-memory M2-B store remains a deterministic adapter. A physical durable
storage adapter is not part of M2-C2.

## Durable versus process-local authority

The durable authority remains responsible for workflow/effect facts:

- `OperationId` and `AttemptId` lineage;
- latest-dispatch authority and late-outcome rejection;
- cancellation history and recovery classification;
- stable capability replay/config identity used for restart validation.

Reactive replacement, withdrawal, fiber lifecycle, capability handles,
`Generation`/`EntryId` process-local pins, and reconciliation reports are
process-local lifecycle state. Reactive mutation does not alter a persisted
attempt's operation identity, attempt identity, or replay identity.

## Known limitations

- Mutation/reconciliation ordering is serialized by the caller; no concurrent
  multi-writer reactive scheduler is introduced.
- There is no background watcher, filesystem watcher, dynamic plugin loader,
  HMR, provider SDK, distributed runtime, durable timer, or durable fiber
  serialization.
- `Runtime::step()` intentionally does not reconcile capabilities implicitly.
- The coordinator is in-memory and process-local; physical persistence remains
  a later adapter concern.
