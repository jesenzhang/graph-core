//! M2-C1 reactive lifecycle evidence through the Runtime-owned boundary.

use capability_graph::{
    CapabilityDefinition, CapabilityFiber, CapabilityValue, PluginDefinition, PluginFactory,
    PluginLoadContext, PluginRuntime, Scope,
};
use kernis_core::Id;
use runtime_core::{RunId, Runtime, StepResult, TaskConfig};
use std::future::Future;
use std::sync::Arc;
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn definition(value: &str) -> CapabilityDefinition {
    CapabilityDefinition::new(id(value), "service")
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

fn workflow() -> WorkflowGraph {
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

fn config(provider: &Id) -> TaskConfig {
    TaskConfig::new().require_capability(provider.clone())
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_owned_reactive_replacement_preserves_m1_attempt_pins() {
    let provider_id = id("shared-provider");
    let scope = Scope::root();
    let mut runtime = Runtime::start_run(
        RunId::new("m2c1-cross-boundary").expect("run id is valid"),
        workflow(),
        scope,
        [
            (id("attempt-a"), config(&provider_id)),
            (id("attempt-b"), config(&provider_id)),
        ],
    )
    .expect("runtime starts");
    let v1 = runtime
        .capability_runtime()
        .provide(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v1"),
            |_| Ok(value("v1")),
        )
        .expect("provider V1 publishes");

    let provider_id_for_factory = provider_id.clone();
    let dependent_runtime = PluginRuntime::new(PluginDefinition::new(
        id("shared-dependent"),
        definition("shared-dependent").depends_on(provider_id.clone()),
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
        .expect("dependent registers");
    let dependent: Arc<CapabilityFiber> = runtime
        .capability_runtime()
        .instantiate(&id("shared-dependent"), String::new())
        .expect("dependent instantiates");
    assert_eq!(
        dependent.start().await.expect("dependent activates"),
        capability_graph::FiberState::Active
    );

    assert!(matches!(
        runtime.step().expect("attempt A starts"),
        StepResult::Completed { ref task_id, .. } if task_id == &id("attempt-a")
    ));
    let attempt_a_pin = runtime.attempts()[0]
        .capability(&provider_id)
        .expect("attempt A pins provider")
        .handle()
        .clone();
    assert_eq!(
        attempt_a_pin.downcast_ref::<String>(),
        Some(&"v1".to_owned())
    );
    assert_eq!(
        runtime.attempts()[0]
            .capability(&provider_id)
            .expect("attempt A pin")
            .entry_id,
        v1.entry_id()
    );
    assert_eq!(
        runtime.attempts()[0]
            .capability(&provider_id)
            .expect("attempt A pin")
            .generation,
        v1.generation()
    );
    assert_eq!(
        runtime.attempts()[0]
            .capability(&provider_id)
            .expect("attempt A pin")
            .replay_identity
            .definition_identity(),
        "provider-v1"
    );

    let (v2, report) = runtime
        .capability_runtime()
        .replace_and_reconcile(
            CapabilityDefinition::new(provider_id.clone(), "provider")
                .with_replay_identity("provider-v2"),
            v1.generation(),
            |_| Ok(value("v2")),
        )
        .await
        .expect("provider V2 replaces V1");
    assert!(report.is_success());
    assert_eq!(
        dependent
            .handle()
            .await
            .expect("dependent V2 handle")
            .downcast_ref::<String>(),
        Some(&"v2".to_owned())
    );

    // The old attempt owns a cloned V1 handle and is not rewritten by the
    // reactive lifecycle transition.
    assert_eq!(
        attempt_a_pin.downcast_ref::<String>(),
        Some(&"v1".to_owned())
    );
    assert_ne!(v1.entry_id(), v2.entry_id());

    assert!(matches!(
        runtime.step().expect("attempt B starts"),
        StepResult::Completed { ref task_id, .. } if task_id == &id("attempt-b")
    ));
    let attempt_b_pin = runtime.attempts()[1]
        .capability(&provider_id)
        .expect("attempt B pins provider")
        .handle();
    assert_eq!(
        attempt_b_pin.downcast_ref::<String>(),
        Some(&"v2".to_owned())
    );
    assert_eq!(
        runtime.attempts()[1]
            .capability(&provider_id)
            .expect("attempt B pin")
            .entry_id,
        v2.entry_id()
    );
    assert_eq!(
        runtime.attempts()[1]
            .capability(&provider_id)
            .expect("attempt B pin")
            .generation,
        v2.generation()
    );
    assert_eq!(
        runtime.attempts()[1]
            .capability(&provider_id)
            .expect("attempt B pin")
            .replay_identity
            .definition_identity(),
        "provider-v2"
    );
}
