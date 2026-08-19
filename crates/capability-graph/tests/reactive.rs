//! Adversarial evidence for the explicit reactive capability coordinator.

use capability_graph::{
    CapabilityContext, CapabilityDefinition, CapabilityFiber, CapabilityRegistry, CapabilityValue,
    FiberState, PluginDefinition, PluginFactory, PluginLoadContext, PluginRuntime,
    ReactiveCapabilityRuntime, ScopedEffect,
};
use graph_core::Id;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

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

fn plugin(
    plugin_id: &str,
    capability: CapabilityDefinition,
    factory: PluginFactory,
) -> Arc<PluginRuntime> {
    PluginRuntime::new(PluginDefinition::new(id(plugin_id), capability, factory))
}

fn register(registry: &CapabilityRegistry, runtime: Arc<PluginRuntime>) {
    registry.register(runtime).expect("test plugin registers");
}

async fn active(fiber: &CapabilityFiber) {
    assert_eq!(
        fiber.start().await.expect("fiber reaches stable state"),
        FiberState::Active
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_and_active_fibers_react_to_publication_and_withdrawal() {
    let context = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(context.clone());
    let loads = Arc::new(AtomicUsize::new(0));
    let loads_for_factory = Arc::clone(&loads);
    let runtime = plugin(
        "consumer",
        definition("consumer").depends_on(id("model")),
        factory(move |_| {
            let loads = Arc::clone(&loads_for_factory);
            async move {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(value("consumer"))
            }
        }),
    );
    register(reactive.registry(), runtime);
    let fiber = reactive
        .instantiate(&id("consumer"), String::new())
        .expect("consumer instantiates");

    assert_eq!(
        fiber.start().await.expect("missing provider is pending"),
        FiberState::Pending
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let (provider, report) = reactive
        .provide_and_reconcile(definition("model"), |_| Ok(value("v1")))
        .await
        .expect("provider publishes");
    assert!(report.driven.contains(&fiber.id()));
    assert_eq!(fiber.state(), FiberState::Active);
    assert_eq!(loads.load(Ordering::SeqCst), 1);

    drop(provider);
    let report = reactive
        .withdraw_and_reconcile(&id("model"), capability_graph::Generation::FIRST)
        .await
        .expect("provider withdraws");
    assert!(report.provider_finalized);
    assert!(report.driven.contains(&fiber.id()));
    assert_eq!(fiber.state(), FiberState::Pending);
}

#[tokio::test(flavor = "current_thread")]
async fn replacement_reloads_only_effective_dependents_and_neutral_notifications_do_nothing() {
    let context = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(context);
    let model_loads = Arc::new(AtomicUsize::new(0));
    let other_loads = Arc::new(AtomicUsize::new(0));

    let model_loads_for_factory = Arc::clone(&model_loads);
    let model_runtime = plugin(
        "model-consumer",
        definition("model-consumer").depends_on(id("model")),
        factory(move |_| {
            let loads = Arc::clone(&model_loads_for_factory);
            async move {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(value("same"))
            }
        }),
    );
    let other_loads_for_factory = Arc::clone(&other_loads);
    let other_runtime = plugin(
        "other-consumer",
        definition("other-consumer").depends_on(id("other")),
        factory(move |_| {
            let loads = Arc::clone(&other_loads_for_factory);
            async move {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(value("other"))
            }
        }),
    );
    register(reactive.registry(), model_runtime);
    register(reactive.registry(), other_runtime);
    let model_fiber = reactive
        .instantiate(&id("model-consumer"), String::new())
        .expect("model consumer instantiates");
    let other_fiber = reactive
        .instantiate(&id("other-consumer"), String::new())
        .expect("other consumer instantiates");
    let model = reactive
        .provide(definition("model"), |_| Ok(value("same")))
        .expect("model publishes");
    let other = reactive
        .provide(definition("other"), |_| Ok(value("other")))
        .expect("other publishes");
    active(&model_fiber).await;
    active(&other_fiber).await;

    model_fiber.notify_dependency_change();
    let neutral = reactive.reconcile().await;
    assert!(neutral.driven.is_empty());
    assert_eq!(model_loads.load(Ordering::SeqCst), 1);

    let (replacement, report) = reactive
        .replace_and_reconcile(definition("model"), model.generation(), |_| {
            Ok(value("same"))
        })
        .await
        .expect("equal replacement publishes");
    assert_eq!(report.driven, vec![model_fiber.id()]);
    assert_eq!(model_loads.load(Ordering::SeqCst), 2);
    assert_eq!(other_loads.load(Ordering::SeqCst), 1);
    assert_ne!(model.entry_id(), replacement.entry_id());

    drop(model);
    drop(replacement);
    drop(other);
}

#[tokio::test(flavor = "current_thread")]
async fn isolated_and_unrelated_mutations_are_neutral() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root.clone());
    let loads = Arc::new(AtomicUsize::new(0));
    let loads_for_factory = Arc::clone(&loads);
    let runtime = plugin(
        "isolated-consumer",
        definition("isolated-consumer").depends_on(id("model")),
        factory(move |_| {
            let loads = Arc::clone(&loads_for_factory);
            async move {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(value("isolated"))
            }
        }),
    );
    register(reactive.registry(), runtime);
    let isolated = root.isolate(id("model"));
    let fiber = reactive
        .registry()
        .instantiate(&id("isolated-consumer"), isolated, String::new())
        .expect("isolated fiber instantiates");
    reactive.watch(&fiber).expect("isolated fiber watches");

    let unrelated = reactive
        .provide(definition("unrelated"), |_| Ok(value("unrelated")))
        .expect("unrelated provider publishes");
    let report = reactive.reconcile().await;
    assert!(report.driven.is_empty());
    assert_eq!(fiber.state(), FiberState::Pending);
    assert_eq!(loads.load(Ordering::SeqCst), 0);

    drop(unrelated);
}

#[tokio::test(flavor = "current_thread")]
async fn rapid_replacement_while_loading_converges_to_latest_provider() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root.clone());
    let model = reactive
        .provide(definition("model"), |_| Ok(value("v1")))
        .expect("v1 publishes");
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let loads = Arc::new(AtomicUsize::new(0));
    let started_for_factory = Arc::clone(&started);
    let release_for_factory = Arc::clone(&release);
    let loads_for_factory = Arc::clone(&loads);
    let runtime = plugin(
        "loading-consumer",
        definition("loading-consumer").depends_on(id("model")),
        factory(move |context| {
            let started = Arc::clone(&started_for_factory);
            let release = Arc::clone(&release_for_factory);
            let loads = Arc::clone(&loads_for_factory);
            async move {
                let attempt = loads.fetch_add(1, Ordering::SeqCst);
                let model = context
                    .dependencies()
                    .get(&id("model"))
                    .expect("model dependency")
                    .downcast_ref::<String>()
                    .expect("model value")
                    .clone();
                if attempt == 0 {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(value(&model))
            }
        }),
    );
    register(reactive.registry(), runtime);
    let fiber = reactive
        .instantiate(&id("loading-consumer"), String::new())
        .expect("loading consumer instantiates");
    let loading = {
        let fiber = Arc::clone(&fiber);
        tokio::spawn(async move { fiber.start().await })
    };
    started.notified().await;

    let v2 = reactive
        .replace(definition("model"), model.generation(), |_| Ok(value("v2")))
        .expect("v2 publishes");
    let v3 = reactive
        .replace(definition("model"), v2.generation(), |_| Ok(value("v3")))
        .expect("v3 publishes");
    release.notify_one();
    assert_eq!(
        loading
            .await
            .expect("loading task joins")
            .expect("latest load succeeds"),
        FiberState::Active
    );
    assert_eq!(
        fiber
            .handle()
            .await
            .expect("latest handle")
            .downcast_ref::<String>()
            .map(String::as_str),
        Some("v3")
    );
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    drop(model);
    drop(v2);
    drop(v3);
}

#[tokio::test(flavor = "current_thread")]
async fn withdrawal_drains_deep_dependents_before_provider_cleanup() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root.clone());
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let events_for_provider = Arc::clone(&events);
    let provider = reactive
        .provide(definition("a"), |_| {
            Ok(CapabilityValue::new("a".to_owned(), move |_| {
                events_for_provider
                    .lock()
                    .expect("events lock")
                    .push("a".to_owned());
            }))
        })
        .expect("a publishes");

    let b_context = root.child_context();
    let c_context = b_context.child_context();
    let b_events = Arc::clone(&events);
    let b_runtime = plugin(
        "b-plugin",
        definition("b").depends_on(id("a")),
        factory(move |context| {
            let events = Arc::clone(&b_events);
            async move {
                let committed_a = context
                    .dependencies()
                    .get(&id("a"))
                    .expect("committed a")
                    .clone();
                context
                    .effect(ScopedEffect::sync("b-teardown", move || {
                        assert_eq!(
                            committed_a.downcast_ref::<String>().map(String::as_str),
                            Some("a")
                        );
                        events.lock().expect("events lock").push("b".to_owned());
                        Ok(())
                    }))
                    .map_err(|error| error.to_string())?;
                Ok(value("b"))
            }
        }),
    );
    let c_events = Arc::clone(&events);
    let c_runtime = plugin(
        "c-plugin",
        definition("c").depends_on(id("b")),
        factory(move |context| {
            let events = Arc::clone(&c_events);
            async move {
                context
                    .effect(ScopedEffect::sync("c-teardown", move || {
                        events.lock().expect("events lock").push("c".to_owned());
                        Ok(())
                    }))
                    .map_err(|error| error.to_string())?;
                Ok(value("c"))
            }
        }),
    );
    register(reactive.registry(), b_runtime);
    register(reactive.registry(), c_runtime);
    let b = reactive
        .registry()
        .instantiate(&id("b-plugin"), b_context, String::new())
        .expect("b instantiates");
    let c = reactive
        .registry()
        .instantiate(&id("c-plugin"), c_context, String::new())
        .expect("c instantiates");
    reactive.watch(&b).expect("b watches");
    reactive.watch(&c).expect("c watches");
    active(&b).await;
    active(&c).await;
    drop(provider);

    let report = reactive
        .withdraw_and_reconcile(&id("a"), capability_graph::Generation::FIRST)
        .await
        .expect("a withdraws");
    assert_eq!(report.driven, vec![c.id(), b.id()]);
    assert!(report.provider_finalized);
    assert_eq!(
        events.lock().expect("events lock").as_slice(),
        &["c", "b", "a"]
    );
    assert_eq!(b.state(), FiberState::Pending);
    assert_eq!(c.state(), FiberState::Pending);

    let repeated = reactive
        .withdraw_and_reconcile(&id("a"), capability_graph::Generation::FIRST)
        .await
        .expect("repeated withdrawal is idempotent");
    assert!(repeated.provider_finalized);
    assert!(repeated.driven.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn withdrawal_collects_cleanup_failures_and_continues_siblings() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let provider = reactive
        .provide(definition("model"), |_| Ok(value("v1")))
        .expect("provider publishes");
    let cleaned = Arc::new(AtomicUsize::new(0));
    for (plugin_name, capability_name) in [("one", "one"), ("two", "two")] {
        let cleaned_for_factory = Arc::clone(&cleaned);
        let runtime = plugin(
            plugin_name,
            definition(capability_name).depends_on(id("model")),
            factory(move |context| {
                let cleaned = Arc::clone(&cleaned_for_factory);
                async move {
                    context
                        .effect(ScopedEffect::sync("failing-cleanup", move || {
                            cleaned.fetch_add(1, Ordering::SeqCst);
                            Err("cleanup failed".to_owned())
                        }))
                        .map_err(|error| error.to_string())?;
                    Ok(value(capability_name))
                }
            }),
        );
        register(reactive.registry(), runtime);
        let fiber = reactive
            .instantiate(&id(plugin_name), String::new())
            .expect("consumer instantiates");
        active(&fiber).await;
    }
    drop(provider);

    let report = reactive
        .withdraw_and_reconcile(&id("model"), capability_graph::Generation::FIRST)
        .await
        .expect("withdrawal finalizes despite cleanup failures");
    assert!(report.provider_finalized);
    assert_eq!(cleaned.load(Ordering::SeqCst), 2);
    assert_eq!(report.cleanup_errors.len(), 2);
}
