//! M2-C2 evidence for the Runtime-owned reactive capability boundary.

use capability_graph::{
    CapabilityDefinition, CapabilityFiber, CapabilityValue, FiberState, PluginDefinition,
    PluginFactory, PluginLoadContext, PluginRuntime, ReactiveRuntimeError, RegistryError, Scope,
};
use kernis_core::Id;
use runtime_core::{RunId, Runtime, StepResult, TaskConfig};
use std::future::Future;
use std::sync::Arc;
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};
use workflow_recovery::{EffectSemantics, OperationId};

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn operation(value: &str) -> OperationId {
    OperationId::new(value).expect("test operation is valid")
}

fn value(value: &str) -> CapabilityValue {
    CapabilityValue::from_value(value.to_owned())
}

fn factory<F, Fut>(function: F) -> PluginFactory
where
    F: Fn(PluginLoadContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<CapabilityValue, String>> + Send + 'static,
{
    Arc::new(move |context| Box::pin(function(context)))
}

fn definition(value: &str) -> CapabilityDefinition {
    CapabilityDefinition::new(id(value), "service")
}

fn single_task_workflow() -> WorkflowGraph {
    let mut workflow = WorkflowGraph::default();
    workflow
        .apply_batch(
            workflow.revision(),
            [WorkflowMutation::AddTask {
                task: Task {
                    id: id("task"),
                    label: "task".to_owned(),
                },
            }],
        )
        .expect("workflow is valid");
    workflow
}

fn dependent_workflow() -> WorkflowGraph {
    let mut workflow = WorkflowGraph::default();
    workflow
        .apply_batch(
            workflow.revision(),
            [
                WorkflowMutation::AddTask {
                    task: Task {
                        id: id("attempt-a"),
                        label: "attempt-a".to_owned(),
                    },
                },
                WorkflowMutation::AddTask {
                    task: Task {
                        id: id("attempt-b"),
                        label: "attempt-b".to_owned(),
                    },
                },
                WorkflowMutation::AddDependency {
                    task_id: id("attempt-b"),
                    dependency_id: id("attempt-a"),
                },
            ],
        )
        .expect("workflow is valid");
    workflow
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_registry_is_the_coordinator_registry() {
    let runtime = Runtime::start_run(
        RunId::new("m2c2-registry").expect("run id is valid"),
        single_task_workflow(),
        Scope::root(),
        [],
    )
    .expect("runtime starts");
    let plugin_id = id("runtime-owned-plugin");
    let plugin = PluginRuntime::new(PluginDefinition::new(
        plugin_id.clone(),
        definition("runtime-owned-plugin"),
        factory(|_| async { Ok(value("loaded")) }),
    ));

    runtime
        .capability_registry()
        .register(plugin)
        .expect("plugin registers through Runtime");
    assert!(std::ptr::eq(
        runtime.capability_registry(),
        runtime.capability_runtime().registry()
    ));

    let fiber: Arc<CapabilityFiber> = runtime
        .capability_runtime()
        .instantiate(&plugin_id, String::new())
        .expect("coordinator instantiates through the Runtime registry");
    assert_eq!(
        fiber.start().await.expect("plugin activates"),
        FiberState::Active
    );
}

#[tokio::test(flavor = "current_thread")]
async fn withdrawal_preserves_old_attempt_pin_and_new_attempt_reads_v2() {
    let provider_id = id("provider");
    let mut runtime = Runtime::start_run(
        RunId::new("m2c2-withdrawal").expect("run id is valid"),
        dependent_workflow(),
        Scope::root(),
        [
            (
                id("attempt-a"),
                TaskConfig::new().require_capability(provider_id.clone()),
            ),
            (
                id("attempt-b"),
                TaskConfig::new().require_capability(provider_id.clone()),
            ),
        ],
    )
    .expect("runtime starts");
    let provider_v1 = runtime
        .capability_runtime()
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v1"),
            |_| Ok(value("v1")),
        )
        .expect("provider V1 publishes");
    let v1_generation = provider_v1.generation();
    let v1_entry_id = provider_v1.entry_id();

    let dependent_id = id("withdrawal-dependent");
    let provider_id_for_factory = provider_id.clone();
    let dependent_runtime = PluginRuntime::new(PluginDefinition::new(
        dependent_id.clone(),
        definition("withdrawal-dependent").depends_on(provider_id.clone()),
        factory(move |context| {
            let provider_id = provider_id_for_factory.clone();
            async move {
                let provider = context
                    .dependencies()
                    .get(&provider_id)
                    .expect("provider dependency");
                Ok(value(
                    provider.downcast_ref::<String>().expect("provider value"),
                ))
            }
        }),
    ));
    runtime
        .capability_registry()
        .register(dependent_runtime)
        .expect("dependent registers through Runtime");
    let dependent = runtime
        .capability_runtime()
        .instantiate(&dependent_id, String::new())
        .expect("dependent instantiates through Runtime");
    assert_eq!(
        dependent.start().await.expect("dependent activates"),
        FiberState::Active
    );

    assert!(matches!(
        runtime.step().expect("attempt A is admitted"),
        StepResult::Completed { ref task_id, .. } if task_id == &id("attempt-a")
    ));
    let attempt_a_pin = runtime.attempts()[0]
        .capability(&provider_id)
        .expect("attempt A pins V1")
        .clone();

    drop(provider_v1);
    let withdrawal = runtime
        .capability_runtime()
        .withdraw_and_reconcile(&provider_id, v1_generation)
        .await
        .expect("provider V1 withdraws");
    assert!(withdrawal.is_success());
    assert!(withdrawal.provider_finalized);
    assert!(runtime.scope().get(&provider_id).is_none());
    assert_eq!(dependent.state(), FiberState::Pending);
    assert_eq!(attempt_a_pin.generation, v1_generation);
    assert_eq!(attempt_a_pin.entry_id, v1_entry_id);
    assert_eq!(
        attempt_a_pin.replay_identity.definition_identity(),
        "provider-v1"
    );
    assert_eq!(
        attempt_a_pin.handle().downcast_ref::<String>(),
        Some(&"v1".to_owned())
    );
    assert!(matches!(
        runtime.step(),
        Err(runtime_core::RuntimeError::MissingCapability { .. })
    ));

    let provider_v2 = runtime
        .capability_runtime()
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v2"),
            |_| Ok(value("v2")),
        )
        .expect("provider V2 publishes");
    let v2_entry_id = provider_v2.entry_id();
    let report = runtime.capability_runtime().reconcile().await;
    assert!(report.is_success());
    assert_eq!(dependent.state(), FiberState::Active);
    assert_eq!(
        dependent
            .dependency_binding()
            .await
            .expect("dependent binds V2")
            .pin(&provider_id)
            .expect("provider V2 binding")
            .entry_id(),
        v2_entry_id
    );
    assert_eq!(
        dependent
            .dependency_binding()
            .await
            .expect("dependent binds V2")
            .pin(&provider_id)
            .expect("provider V2 binding")
            .generation(),
        provider_v2.generation()
    );
    assert_eq!(
        dependent
            .handle()
            .await
            .expect("dependent has V2")
            .downcast_ref::<String>(),
        Some(&"v2".to_owned())
    );

    assert!(matches!(
        runtime.step().expect("attempt B is admitted"),
        StepResult::Completed { ref task_id, .. } if task_id == &id("attempt-b")
    ));
    let attempt_b_pin = runtime.attempts()[1]
        .capability(&provider_id)
        .expect("attempt B pins V2");
    assert_eq!(attempt_b_pin.entry_id, v2_entry_id);
    assert_eq!(attempt_b_pin.generation, provider_v2.generation());
    assert_eq!(
        attempt_b_pin.replay_identity.definition_identity(),
        "provider-v2"
    );
    assert_eq!(
        attempt_a_pin.handle().downcast_ref::<String>(),
        Some(&"v1".to_owned())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn reactive_replacement_does_not_mutate_durable_attempt_authority() {
    let provider_id = id("durable-provider");
    let operation_id = operation("durable-operation");
    let mut runtime = Runtime::start_run(
        RunId::new("m2c2-durable").expect("run id is valid"),
        single_task_workflow(),
        Scope::root(),
        [(
            id("task"),
            TaskConfig::new()
                .require_capability(provider_id.clone())
                .with_effect(operation_id.clone(), EffectSemantics::Idempotent),
        )],
    )
    .expect("runtime starts");
    let provider_v1 = runtime
        .capability_runtime()
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v1"),
            |_| Ok(value("v1")),
        )
        .expect("provider V1 publishes");
    let attempt_id = match runtime.step().expect("attempt admission succeeds") {
        StepResult::EffectPending { attempt_id, .. } => attempt_id,
        other => panic!("expected pending effect, got {other:?}"),
    };
    let before = runtime
        .durable_state()
        .expect("durable state loads before replacement");
    let before_attempt = before.attempt(&attempt_id).expect("attempt is durable");

    let (_, report) = runtime
        .capability_runtime()
        .replace_and_reconcile(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v2"),
            provider_v1.generation(),
            |_| Ok(value("v2")),
        )
        .await
        .expect("provider replacement succeeds");
    assert!(report.is_success());

    let after = runtime
        .durable_state()
        .expect("durable state loads after replacement");
    let after_attempt = after.attempt(&attempt_id).expect("attempt remains durable");
    assert_eq!(after_attempt.attempt_id, before_attempt.attempt_id);
    assert_eq!(after_attempt.operation_id, Some(operation_id.clone()));
    assert_eq!(after_attempt.capabilities, before_attempt.capabilities);
    assert_eq!(runtime.journal().latest_dispatch(&operation_id), None);
}

#[tokio::test(flavor = "current_thread")]
async fn restore_builds_a_fresh_process_local_reactive_runtime() {
    let provider_id = id("restore-provider");
    let plugin_id = id("old-process-plugin");
    let config =
        TaskConfig::new().require_capability_with_identity(provider_id.clone(), "provider-v1");
    let workflow = single_task_workflow();
    let mut runtime = Runtime::start_run(
        RunId::new("m2c2-restore").expect("run id is valid"),
        workflow.clone(),
        Scope::root(),
        [(id("task"), config.clone())],
    )
    .expect("runtime starts");
    runtime
        .capability_runtime()
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v1"),
            |_| Ok(value("v1")),
        )
        .expect("provider publishes");
    assert!(matches!(
        runtime.step().expect("attempt admits"),
        StepResult::Completed { .. }
    ));
    let old_plugin = PluginRuntime::new(PluginDefinition::new(
        plugin_id.clone(),
        definition("old-process-plugin"),
        factory(|_| async { Ok(value("old")) }),
    ));
    runtime
        .capability_registry()
        .register(old_plugin)
        .expect("old process registers plugin");
    let old_fiber = runtime
        .capability_runtime()
        .instantiate(&plugin_id, String::new())
        .expect("old process instantiates fiber");
    assert_eq!(
        old_fiber.start().await.expect("old fiber activates"),
        FiberState::Active
    );

    let restored_scope = Scope::root();
    restored_scope
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v1"),
            |_| Ok(value("v1")),
        )
        .expect("restored provider publishes");
    let restarted = Runtime::restore_run(
        RunId::new("m2c2-restore").expect("run id is valid"),
        workflow,
        restored_scope,
        [(id("task"), config)],
        runtime.store().clone(),
    )
    .expect("restore reconstructs Runtime");
    assert!(!std::ptr::eq(
        runtime.capability_runtime(),
        restarted.capability_runtime()
    ));
    assert!(!restarted.capability_registry().contains(&plugin_id));
    assert!(matches!(
        restarted
            .capability_runtime()
            .instantiate(&plugin_id, String::new()),
        Err(ReactiveRuntimeError::Registry(RegistryError::Unknown(id))) if id == plugin_id
    ));
    assert_eq!(old_fiber.state(), FiberState::Active);
}
