//! Cross-boundary evidence that Capability Runtime lifecycle is not an M1
//! workflow, recovery, or execution-stream authority.

use capability_graph::{
    CapabilityContext, CapabilityDefinition, CapabilityValue, PluginDefinition, PluginFactory,
    PluginLoadContext, PluginRuntime,
};
use graph_core::Id;
use runtime_core::{RunId, Runtime};
use std::sync::Arc;
use workflow_graph::WorkflowGraph;

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn factory(
    function: impl Fn(PluginLoadContext) -> capability_graph::PluginFuture + Send + Sync + 'static,
) -> PluginFactory {
    Arc::new(function)
}

#[tokio::test(flavor = "current_thread")]
async fn capability_fiber_lifecycle_does_not_write_m1_authorities() {
    let mut runtime = Runtime::start_run(
        RunId::new("m2a-boundary").expect("run id is valid"),
        WorkflowGraph::default(),
        capability_graph::Scope::root(),
        [],
    )
    .expect("runtime starts");
    let plugin = PluginRuntime::new(PluginDefinition::new(
        id("m2a-plugin"),
        CapabilityDefinition::new(id("m2a-capability"), "test"),
        factory(|context| {
            Box::pin(async move { Ok(CapabilityValue::from_value(context.config().to_owned())) })
        }),
    ));
    runtime
        .capability_registry()
        .register(Arc::clone(&plugin))
        .expect("plugin registers");
    let fiber = runtime
        .capability_registry()
        .instantiate(
            &id("m2a-plugin"),
            CapabilityContext::from_scope(runtime.scope().child()),
            "runtime-only".to_owned(),
        )
        .expect("fiber instantiates");
    fiber.start().await.expect("fiber starts");
    assert_eq!(fiber.state(), capability_graph::FiberState::Active);
    assert_eq!(runtime.workflow().revision().get(), 0);
    assert!(runtime.workflow().completion_log().is_empty());
    assert!(runtime.journal().operation_ids().is_empty());
    assert!(runtime.drain_execution_events().is_empty());

    runtime
        .capability_registry()
        .remove(&id("m2a-plugin"))
        .await
        .expect("plugin removes");
    assert_eq!(fiber.state(), capability_graph::FiberState::Disposed);
    assert_eq!(runtime.workflow().revision().get(), 0);
    assert!(runtime.journal().operation_ids().is_empty());
    assert!(runtime.drain_execution_events().is_empty());
}
