//! Integration coverage for the deterministic Runtime Core invariants.

use capability_graph::{CapabilityDefinition, CapabilityValue, Scope};
use graph_core::Id;
use runtime_core::{Cancellation, RunId, Runtime, RuntimeError, StepResult, TaskConfig};
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};
use workflow_recovery::{
    EffectSemantics, KnownEffectOutcome, OperationId, RecoveredEffectState, RecoveryAction,
};

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn operation(value: &str) -> OperationId {
    OperationId::new(value).expect("test operation is valid")
}

fn task(value: &str) -> Task {
    Task {
        id: id(value),
        label: value.to_owned(),
    }
}

fn workflow(mutations: impl IntoIterator<Item = WorkflowMutation>) -> WorkflowGraph {
    let mut graph = WorkflowGraph::default();
    let revision = graph.revision();
    let mutations = mutations.into_iter().collect::<Vec<_>>();
    graph
        .apply_batch(revision, mutations)
        .expect("workflow setup is valid");
    graph
}

fn provider_scope(label: &str) -> (Scope, Id, u64) {
    let scope = Scope::root();
    let provider_id = id("provider");
    let handle = scope
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider"),
            |_| Ok(CapabilityValue::from_value(label.to_owned())),
        )
        .expect("provider is admitted");
    (scope, provider_id, handle.entry_id().get())
}

fn start(
    workflow: WorkflowGraph,
    scope: Scope,
    configs: impl IntoIterator<Item = (Id, TaskConfig)>,
) -> Runtime {
    Runtime::start_run(
        RunId::new("run-1").expect("run id is valid"),
        workflow,
        scope,
        configs,
    )
    .expect("runtime starts")
}

#[test]
fn normal_run_is_deterministic_and_effects_complete_once() {
    let a = task("a");
    let b = task("b");
    let workflow = workflow([
        WorkflowMutation::AddTask { task: a.clone() },
        WorkflowMutation::AddTask { task: b.clone() },
        WorkflowMutation::AddDependency {
            task_id: b.id.clone(),
            dependency_id: a.id.clone(),
        },
    ]);
    let op = operation("b-effect");
    let mut runtime = start(
        workflow,
        Scope::root(),
        [(
            b.id.clone(),
            TaskConfig::new().with_effect(op.clone(), EffectSemantics::Idempotent),
        )],
    );

    let first = runtime
        .run_until_blocked()
        .expect("first scheduler pass succeeds");
    assert!(matches!(first[0], StepResult::Completed { ref task_id, .. } if task_id == &a.id));
    let pending = match first.last().expect("effect is pending") {
        StepResult::EffectPending {
            task_id,
            attempt_id,
            operation_id,
        } => {
            assert_eq!(task_id, &b.id);
            assert_eq!(operation_id, &op);
            attempt_id.clone()
        }
        other => panic!("expected pending effect, got {other:?}"),
    };
    assert_eq!(
        runtime.dispatch_effect(&op).expect("dispatch is admitted"),
        pending
    );
    runtime
        .record_effect_outcome(&op, pending, KnownEffectOutcome::Succeeded)
        .expect("outcome is durable");
    assert_eq!(
        runtime
            .recover(&op)
            .expect("known success is recovered")
            .action,
        RecoveryAction::CompleteWithoutReexecution
    );

    assert_eq!(runtime.workflow().completion_log().len(), 2);
    assert_eq!(runtime.attempts().len(), 2);
    assert_eq!(runtime.step().expect("final idle step"), StepResult::Idle);
}

#[test]
fn replacement_does_not_change_an_in_flight_capability_pin() {
    let a = task("a");
    let b = task("b");
    let workflow = workflow([
        WorkflowMutation::AddTask { task: a.clone() },
        WorkflowMutation::AddTask { task: b.clone() },
        WorkflowMutation::AddDependency {
            task_id: b.id.clone(),
            dependency_id: a.id.clone(),
        },
    ]);
    let (scope, provider_id, first_entry_id) = provider_scope("provider-v1");
    let config = |op: &str| {
        TaskConfig::new()
            .require_capability(provider_id.clone())
            .with_effect(operation(op), EffectSemantics::Idempotent)
    };
    let mut runtime = start(
        workflow,
        scope.clone(),
        [
            (a.id.clone(), config("a-effect")),
            (b.id.clone(), config("b-effect")),
        ],
    );

    let first_attempt = match runtime.step().expect("a starts") {
        StepResult::EffectPending { attempt_id, .. } => attempt_id,
        other => panic!("expected a pending effect, got {other:?}"),
    };
    let first_pin = runtime.attempts()[0]
        .capability(&provider_id)
        .expect("a has provider pin");
    let first_pin_entry_id = first_pin.entry_id;
    assert_eq!(first_pin_entry_id.get(), first_entry_id);
    let first_operation = operation("a-effect");
    runtime
        .dispatch_effect(&first_operation)
        .expect("a dispatches");

    let current_generation = scope.generation(&provider_id).expect("provider is visible");
    let replacement = scope
        .replace(
            CapabilityDefinition::new(provider_id.clone(), "provider"),
            current_generation,
            |_| Ok(CapabilityValue::from_value("provider-v2".to_owned())),
        )
        .expect("replacement is admitted");
    assert_ne!(replacement.entry_id(), first_pin_entry_id);

    runtime
        .record_effect_outcome(
            &first_operation,
            first_attempt,
            KnownEffectOutcome::Succeeded,
        )
        .expect("a outcome is durable");
    runtime
        .recover(&first_operation)
        .expect("a completes from its pinned v1 outcome");
    let second_operation = operation("b-effect");
    match runtime.step().expect("b starts") {
        StepResult::EffectPending { .. } => {}
        other => panic!("expected b pending effect, got {other:?}"),
    }
    let second_pin = runtime.attempts()[1]
        .capability(&provider_id)
        .expect("b has provider pin");
    assert_eq!(second_pin.entry_id, replacement.entry_id());
    assert_eq!(
        second_pin.handle().downcast_ref::<String>(),
        Some(&"provider-v2".to_owned())
    );
    assert_ne!(second_pin.entry_id, first_pin_entry_id);
    assert_eq!(
        runtime.journal().state(&second_operation),
        RecoveredEffectState::Prepared
    );
}

#[test]
fn dynamic_mutation_is_read_again_by_the_scheduler() {
    let a = task("a");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [],
    );
    assert!(matches!(
        runtime.step().expect("a runs"),
        StepResult::Completed { .. }
    ));

    let c = task("c");
    let revision = runtime.workflow().revision();
    runtime
        .apply_workflow_mutation(
            revision,
            [
                WorkflowMutation::AddTask { task: c.clone() },
                WorkflowMutation::AddDependency {
                    task_id: c.id.clone(),
                    dependency_id: a.id.clone(),
                },
            ],
        )
        .expect("mutation uses the current revision");
    runtime
        .configure_task(c.id.clone(), TaskConfig::new())
        .expect("new task configuration is accepted");

    assert!(
        matches!(runtime.step().expect("c is discovered ready"), StepResult::Completed { ref task_id, .. } if task_id == &c.id)
    );
    assert_eq!(runtime.workflow().completed_tasks().len(), 2);
}

#[test]
fn idempotent_unknown_recovery_reuses_operation_with_a_new_attempt() {
    let a = task("a");
    let op = operation("idempotent-effect");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [(
            a.id.clone(),
            TaskConfig::new().with_effect(op.clone(), EffectSemantics::Idempotent),
        )],
    );
    let first = match runtime.step().expect("effect prepares") {
        StepResult::EffectPending { attempt_id, .. } => attempt_id,
        other => panic!("expected pending effect, got {other:?}"),
    };
    runtime.dispatch_effect(&op).expect("first dispatch");
    assert_eq!(
        runtime.recover(&op).expect("recovery classifies").action,
        RecoveryAction::RetrySameOperation
    );
    let retry = runtime.dispatch_effect(&op).expect("idempotent retry");
    assert_ne!(first, retry);
    assert_eq!(
        &runtime.journal().latest_dispatch(&op).unwrap().attempt_id,
        &retry
    );
    runtime
        .record_effect_outcome(&op, retry, KnownEffectOutcome::Succeeded)
        .expect("one logical operation outcome is durable");
    runtime
        .recover(&op)
        .expect("known success completes without reexecution");
    assert_eq!(runtime.workflow().completed_tasks().len(), 1);
}

#[test]
fn non_idempotent_unknown_recovery_requires_reconciliation() {
    let a = task("a");
    let op = operation("non-idempotent-effect");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [(
            a.id.clone(),
            TaskConfig::new().with_effect(op.clone(), EffectSemantics::NonIdempotent),
        )],
    );
    runtime.step().expect("effect prepares");
    runtime.dispatch_effect(&op).expect("one dispatch");
    assert_eq!(
        runtime.recover(&op).expect("recovery classifies").action,
        RecoveryAction::Reconcile
    );
    assert_eq!(
        runtime.dispatch_effect(&op),
        Err(RuntimeError::ReconciliationRequired(op.clone()))
    );
    assert_eq!(runtime.attempts().len(), 1);
    assert_eq!(
        runtime.journal().state(&op),
        RecoveredEffectState::OutcomeUnknown
    );
}

#[test]
fn stream_loss_does_not_change_workflow_or_recovery_truth() {
    let a = task("a");
    let op = operation("stream-effect");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [],
    );
    runtime
        .record_effect_intent(a.id.clone(), op.clone(), EffectSemantics::Idempotent)
        .expect("intent is durable");
    let before_workflow = runtime.workflow().clone();
    let before_recovery = runtime.recover(&op).expect("recovery is durable");

    runtime.emit_progress(&a.id, 10).expect("progress fits");
    runtime
        .emit_progress(&a.id, 20)
        .expect("same task coalesces");
    runtime.emit_telemetry("first").expect("telemetry fits");
    runtime
        .emit_telemetry("second")
        .expect("old telemetry is disposable");

    assert_eq!(runtime.drain_progress_events().len(), 1);
    assert_eq!(runtime.drain_telemetry_events().len(), 1);
    assert_eq!(runtime.workflow(), &before_workflow);
    assert_eq!(
        runtime.recover(&op).expect("recovery ignores streams"),
        before_recovery
    );
}

#[test]
fn cancellation_before_dispatch_prevents_start() {
    let a = task("a");
    let op = operation("cancel-before-dispatch");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [(
            a.id.clone(),
            TaskConfig::new().with_effect(op.clone(), EffectSemantics::Idempotent),
        )],
    );
    runtime
        .record_effect_intent(a.id.clone(), op.clone(), EffectSemantics::Idempotent)
        .expect("intent exists but is not dispatched");
    assert_eq!(
        runtime
            .cancel_task(&a.id)
            .expect("cancellation is accepted"),
        Cancellation::NotDispatched {
            task_id: a.id.clone()
        }
    );
    assert_eq!(
        runtime.step().expect("cancelled task is skipped"),
        StepResult::Idle
    );
    assert_eq!(runtime.attempts().len(), 0);
    assert_eq!(
        runtime.dispatch_effect(&op),
        Err(RuntimeError::CancelledBeforeDispatch(a.id))
    );
}

#[test]
fn cancellation_after_dispatch_preserves_unknown_effect_fact() {
    let a = task("a");
    let op = operation("cancel-after-dispatch");
    let mut runtime = start(
        workflow([WorkflowMutation::AddTask { task: a.clone() }]),
        Scope::root(),
        [(
            a.id.clone(),
            TaskConfig::new().with_effect(op.clone(), EffectSemantics::NonIdempotent),
        )],
    );
    runtime.step().expect("effect prepares");
    let attempt_id = runtime.dispatch_effect(&op).expect("effect dispatches");
    assert_eq!(
        runtime
            .cancel_task(&a.id)
            .expect("cancellation observes dispatch"),
        Cancellation::AlreadyDispatched {
            task_id: a.id.clone(),
            operation_id: op.clone(),
            attempt_id,
        }
    );
    assert_eq!(
        runtime.journal().state(&op),
        RecoveredEffectState::OutcomeUnknown
    );
    assert_eq!(
        runtime
            .recover(&op)
            .expect("recovery remains authoritative")
            .action,
        RecoveryAction::Reconcile
    );
    assert_eq!(runtime.workflow().completed_tasks().len(), 0);
}

#[test]
fn lossless_lifecycle_backpressure_retains_event_for_retry() {
    let mutations = (0..65)
        .map(|index| WorkflowMutation::AddTask {
            task: task(&format!("task-{index:02}")),
        })
        .collect::<Vec<_>>();
    let mut runtime = start(workflow(mutations), Scope::root(), []);

    for _ in 0..64 {
        assert!(matches!(
            runtime.step().expect("lifecycle events fit"),
            StepResult::Completed { .. }
        ));
    }

    let item = match runtime.step() {
        Err(RuntimeError::ExecutionBackpressure { item }) => item,
        other => panic!("expected retained lifecycle item, got {other:?}"),
    };
    assert_eq!(runtime.drain_execution_events().len(), 128);
    runtime
        .retry_execution_event(item)
        .expect("drained lossless stream accepts the original item");
    assert_eq!(runtime.drain_execution_events().len(), 1);
}
