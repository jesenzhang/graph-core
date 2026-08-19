# M2-C1 Reactive Coeffect Runtime

## Candidate

- Base: `c404ae73041d2855062a55c2b8d6efa91d81fb3f`
- Scope: durable replay-head repair plus reactive capability lifecycle
- Owner: `capability-graph`; `runtime-core` remains the durable/task coordinator

## Semantic mapping

| Behavior | Formal invariant | Cordis technique | Rust adaptation |
|---|---|---|---|
| Provider identity | A committed dependency is an exact binding, not a value comparison | Provider fiber identity is used as the target | `DependencyBinding` stores `CapabilityId + Generation + EntryId` |
| Transition races | A stale asynchronous transition cannot publish | Inertia serializes transitions and re-observes the target | `DependencyEpoch` carries a separate transition token; publication checks the current binding and force-reload state |
| Reactive propagation | Only fibers whose effective target changed are driven | Reflect notification refreshes affected fibers | `ReactiveCapabilityRuntime::reconcile()` compares each watched fiber's current resolution with its committed binding |
| Reconciliation boundary | A reconciliation call reaches a stable fixpoint | Inertia chains transitions until the latest target is observed | `reconcile()` repeats deterministic FiberId-ordered passes until no watched fiber is stale |
| Withdrawal | Dependents quiesce before provider recovery | Hide provider, unload consumers, await fibers, then recover provider | `Scope::withdraw()` returns a temporary provider guard; the coordinator drains affected fibers deepest-first before dropping it |
| Failure isolation | One cleanup failure does not prevent sibling cleanup | Fiber cleanup continues across disposers | `ReconcileReport` retains lifecycle and cleanup failures structurally |

`Scope` remains a low-level ownership/admission primitive. Reactive callers use
`ReactiveCapabilityRuntime` and an explicit async reconciliation boundary rather
than relying on a generic event bus or background task.

## Proven behaviors

The reactive integration tests cover:

- pending activation and active deactivation;
- exact affected-fiber replacement, including equal values from new entries;
- neutral notifications, unrelated mutations, and isolated contexts;
- convergence of a loading consumer through rapid `V1 -> V2 -> V3` replacement;
- one-call fixpoint convergence for reverse-ordered chain and diamond graphs;
- stale async publication rejection;
- committed old-provider access during dependent teardown;
- `C -> B -> A` withdrawal order and provider cleanup after dependents;
- cleanup-failure collection without skipping sibling dependents;
- provider-fiber withdrawal without double-removing its exact detached entry;
- idempotent repeated withdrawal.

The runtime-core cross-boundary test shares one `Scope` with the reactive
coordinator: attempt A retains V1, the dependent fiber converges to V2, and a
new attempt B resolves V2. No existing `TaskAttempt` pin is mutated.

The existing lifecycle tests continue to cover replacement during unloading,
stale load errors, disposal during loading, and same-fiber restart.

## Durable replay repair

An idempotent replay remains `replayed = true` and does not mutate durable
state. Its returned revision is now the store's current revision, not the
revision of the original commit. This keeps `Runtime::store_revision` as a
valid CAS head after replaying an old successful commit.

## Authority compatibility

- M1 task attempts still retain the exact `CapabilityHandle` captured at
  admission; reactive replacement only changes future resolution.
- Durable intent, dispatch, outcome, idempotency, and reconciliation remain in
  `DurableStore`/`DurableJournal`.
- Fiber state, handles, effects, and reactive registrations remain process
  local. Execution streams remain observations, not recovery authority.

## Known limitations

- Raw `Scope` mutations do not run background reconciliation. Callers claiming
  reactive semantics must use the coordinator's mutation methods and call the
  explicit async boundary.
- The coordinator is process-local and in-memory; no distributed watcher or
  durable fiber serialization is introduced.
- Cleanup failure policy is deterministic continuation plus structural
  reporting. Provider finalization still occurs after the affected fibers
  reach a stable state; application-specific retry or escalation is outside
  this milestone.
