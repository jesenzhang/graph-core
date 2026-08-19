//! Adversarial evidence for the explicit reactive capability coordinator.

use capability_graph::{
    CapabilityContext, CapabilityDefinition, CapabilityFiber, CapabilityRegistry, CapabilityValue,
    FiberState, PluginDefinition, PluginFactory, PluginLoadContext, PluginRuntime,
    ReactiveCapabilityRuntime, ScopedEffect,
};
use kernis_core::Id;
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

#[tokio::test(flavor = "current_thread")]
async fn successful_reactivation_from_pending_clears_previous_cleanup_errors() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let provider = reactive
        .provide(definition("model"), |_| Ok(value("v1")))
        .expect("v1 publishes");
    let cleanup_calls = Arc::new(AtomicUsize::new(0));
    let cleanup_calls_for_factory = Arc::clone(&cleanup_calls);
    let consumer_runtime = plugin(
        "pending-reactivation-consumer",
        definition("pending-reactivation-consumer").depends_on(id("model")),
        factory(move |context| {
            let cleanup_calls = Arc::clone(&cleanup_calls_for_factory);
            async move {
                context
                    .effect(ScopedEffect::sync(
                        "pending-reactivation-cleanup",
                        move || {
                            if cleanup_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                                Err("withdrawal cleanup failed".to_owned())
                            } else {
                                Ok(())
                            }
                        },
                    ))
                    .map_err(|error| error.to_string())?;
                Ok(value("consumer"))
            }
        }),
    );
    register(reactive.registry(), consumer_runtime);
    let consumer = reactive
        .instantiate(&id("pending-reactivation-consumer"), String::new())
        .expect("consumer instantiates");
    active(&consumer).await;
    let provider_generation = provider.generation();
    drop(provider);

    let withdrawal_report = reactive
        .withdraw_and_reconcile(&id("model"), provider_generation)
        .await
        .expect("withdrawal reports cleanup failure");
    assert_eq!(consumer.state(), FiberState::Pending);
    assert_eq!(withdrawal_report.cleanup_errors.len(), 1);
    assert!(!withdrawal_report.is_success());
    assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);

    let v2 = reactive
        .provide(definition("model"), |_| Ok(value("v2")))
        .expect("v2 publishes");
    let report = reactive.reconcile().await;
    assert_eq!(consumer.state(), FiberState::Active);
    assert!(report.errors.is_empty());
    assert!(report.cleanup_errors.is_empty());
    assert!(report.is_success());
    assert_eq!(
        consumer
            .dependency_binding()
            .await
            .expect("consumer binding")
            .entry_id(&id("model")),
        Some(v2.entry_id())
    );
    assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);

    let stable = reactive.reconcile().await;
    assert!(stable.driven.is_empty());
    assert!(stable.cleanup_errors.is_empty());

    drop(v2);
}

#[tokio::test(flavor = "current_thread")]
async fn replacement_reaches_transitive_fixed_point_independent_of_fiber_id_order() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let a = reactive
        .provide(definition("a"), |_| Ok(value("a-v1")))
        .expect("a publishes");

    // Allocate C before B so the first pass examines C while it still sees
    // B-v1. B then reloads from A-v2 and the same reconcile call must make a
    // second deterministic pass for C.
    let c_runtime = plugin(
        "chain-c",
        definition("c").depends_on(id("b")),
        factory(|_| async { Ok(value("c")) }),
    );
    let b_runtime = plugin(
        "chain-b",
        definition("b").depends_on(id("a")),
        factory(|context| async move {
            let a = context
                .dependencies()
                .get(&id("a"))
                .expect("a dependency")
                .downcast_ref::<String>()
                .expect("a value")
                .clone();
            Ok(value(&format!("b-from-{a}")))
        }),
    );
    register(reactive.registry(), c_runtime);
    register(reactive.registry(), b_runtime);
    let c = reactive
        .instantiate(&id("chain-c"), String::new())
        .expect("C instantiates first");
    let b = reactive
        .instantiate(&id("chain-b"), String::new())
        .expect("B instantiates second");
    active(&b).await;
    active(&c).await;
    let b_v1 = b.handle().await.expect("B-v1 handle");

    let (a_v2, report) = reactive
        .replace_and_reconcile(definition("a"), a.generation(), |_| Ok(value("a-v2")))
        .await
        .expect("A-v2 publishes and reconciliation reaches a fixpoint");

    assert!(report.is_success());
    assert_eq!(report.driven, vec![b.id(), c.id()]);
    let b_v2 = b.handle().await.expect("B-v2 handle");
    let c_v2 = c.handle().await.expect("C-v2 handle");
    assert_eq!(
        b.dependency_binding()
            .await
            .expect("B binding")
            .entry_id(&id("a")),
        Some(a_v2.entry_id())
    );
    assert_eq!(
        c.dependency_binding()
            .await
            .expect("C binding")
            .entry_id(&id("b")),
        Some(b_v2.entry_id())
    );
    assert_ne!(b_v1.entry_id(), b_v2.entry_id());
    assert_eq!(c_v2.downcast_ref::<String>().map(String::as_str), Some("c"));

    let already_stable = reactive.reconcile().await;
    assert!(already_stable.driven.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn successful_replacement_clears_previous_cleanup_errors() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let provider = reactive
        .provide(definition("model"), |_| Ok(value("v1")))
        .expect("v1 publishes");
    let cleanup_calls = Arc::new(AtomicUsize::new(0));
    let cleanup_calls_for_factory = Arc::clone(&cleanup_calls);
    let consumer_runtime = plugin(
        "cleanup-consumer",
        definition("cleanup-consumer").depends_on(id("model")),
        factory(move |context| {
            let cleanup_calls = Arc::clone(&cleanup_calls_for_factory);
            async move {
                context
                    .effect(ScopedEffect::sync("replacement-cleanup", move || {
                        if cleanup_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            Err("first cleanup failed".to_owned())
                        } else {
                            Ok(())
                        }
                    }))
                    .map_err(|error| error.to_string())?;
                Ok(value("consumer"))
            }
        }),
    );
    register(reactive.registry(), consumer_runtime);
    let consumer = reactive
        .instantiate(&id("cleanup-consumer"), String::new())
        .expect("consumer instantiates");
    active(&consumer).await;

    let (v2, first_report) = reactive
        .replace_and_reconcile(definition("model"), provider.generation(), |_| {
            Ok(value("v2"))
        })
        .await
        .expect("v2 publishes and reconciles");
    assert!(!first_report.is_success());
    assert_eq!(first_report.cleanup_errors.len(), 1);
    assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);

    let (v3, second_report) = reactive
        .replace_and_reconcile(definition("model"), v2.generation(), |_| Ok(value("v3")))
        .await
        .expect("v3 publishes and reconciles");
    assert!(second_report.is_success());
    assert!(second_report.cleanup_errors.is_empty());
    assert_eq!(second_report.driven, vec![consumer.id()]);
    assert_eq!(cleanup_calls.load(Ordering::SeqCst), 2);
    assert_eq!(consumer.state(), FiberState::Active);
    assert_eq!(
        consumer
            .dependency_binding()
            .await
            .expect("consumer binding")
            .entry_id(&id("model")),
        Some(v3.entry_id())
    );

    drop(provider);
    drop(v2);
    drop(v3);
}

#[tokio::test(flavor = "current_thread")]
async fn one_reconcile_reaches_fixpoint_for_reverse_ordered_diamond() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let a = reactive
        .provide(definition("diamond-a"), |_| Ok(value("a-v1")))
        .expect("A publishes");

    let d_runtime = plugin(
        "diamond-d",
        definition("diamond-d")
            .depends_on(id("diamond-b"))
            .depends_on(id("diamond-c")),
        factory(|_| async { Ok(value("d")) }),
    );
    let c_runtime = plugin(
        "diamond-c-plugin",
        definition("diamond-c").depends_on(id("diamond-a")),
        factory(|_| async { Ok(value("c")) }),
    );
    let b_runtime = plugin(
        "diamond-b-plugin",
        definition("diamond-b").depends_on(id("diamond-a")),
        factory(|_| async { Ok(value("b")) }),
    );
    register(reactive.registry(), d_runtime);
    register(reactive.registry(), c_runtime);
    register(reactive.registry(), b_runtime);
    let d = reactive
        .instantiate(&id("diamond-d"), String::new())
        .expect("D instantiates first");
    let c = reactive
        .instantiate(&id("diamond-c-plugin"), String::new())
        .expect("C instantiates second");
    let b = reactive
        .instantiate(&id("diamond-b-plugin"), String::new())
        .expect("B instantiates third");
    active(&b).await;
    active(&c).await;
    active(&d).await;
    let b_v1 = b.handle().await.expect("B-v1 handle");
    let c_v1 = c.handle().await.expect("C-v1 handle");

    let a_v2 = reactive
        .replace(definition("diamond-a"), a.generation(), |_| {
            Ok(value("a-v2"))
        })
        .expect("A-v2 publishes");
    let report = reactive.reconcile().await;

    assert!(report.is_success());
    assert!(report.driven.contains(&b.id()));
    assert!(report.driven.contains(&c.id()));
    assert!(report.driven.contains(&d.id()));
    let b_v2 = b.handle().await.expect("B-v2 handle");
    let c_v2 = c.handle().await.expect("C-v2 handle");
    let d_v2 = d.handle().await.expect("D-v2 handle");
    assert_eq!(
        b.dependency_binding()
            .await
            .expect("B binding")
            .entry_id(&id("diamond-a")),
        Some(a_v2.entry_id())
    );
    assert_eq!(
        c.dependency_binding()
            .await
            .expect("C binding")
            .entry_id(&id("diamond-a")),
        Some(a_v2.entry_id())
    );
    assert_eq!(
        d.dependency_binding()
            .await
            .expect("D binding")
            .entry_id(&id("diamond-b")),
        Some(b_v2.entry_id())
    );
    assert_eq!(
        d.dependency_binding()
            .await
            .expect("D binding")
            .entry_id(&id("diamond-c")),
        Some(c_v2.entry_id())
    );
    assert_ne!(b_v1.entry_id(), b_v2.entry_id());
    assert_ne!(c_v1.entry_id(), c_v2.entry_id());
    assert!(reactive.reconcile().await.driven.is_empty());
    drop(d_v2);
}

#[tokio::test(flavor = "current_thread")]
async fn fiber_provider_withdrawal_skips_only_its_exact_detached_publication() {
    let root = CapabilityContext::root();
    let reactive = ReactiveCapabilityRuntime::new(root);
    let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let provider_events = Arc::clone(&events);
    let provider_cleanup_events = Arc::clone(&events);
    let provider_runtime = plugin(
        "fiber-provider",
        definition("provider"),
        factory(move |context| {
            let provider_events = Arc::clone(&provider_events);
            let provider_cleanup_events = Arc::clone(&provider_cleanup_events);
            async move {
                context
                    .effect(ScopedEffect::sync("provider-effect", move || {
                        provider_events
                            .lock()
                            .expect("events lock")
                            .push("provider-effect");
                        Ok(())
                    }))
                    .map_err(|error| error.to_string())?;
                Ok(CapabilityValue::new("provider-v1".to_owned(), move |_| {
                    provider_cleanup_events
                        .lock()
                        .expect("events lock")
                        .push("provider-value");
                }))
            }
        }),
    );
    let consumer_events = Arc::clone(&events);
    let consumer_runtime = plugin(
        "fiber-consumer",
        definition("consumer").depends_on(id("provider")),
        factory(move |context| {
            let consumer_events = Arc::clone(&consumer_events);
            async move {
                let committed_provider = context
                    .dependencies()
                    .get(&id("provider"))
                    .expect("committed provider")
                    .clone();
                context
                    .effect(ScopedEffect::sync("consumer-effect", move || {
                        assert_eq!(
                            committed_provider
                                .downcast_ref::<String>()
                                .map(String::as_str),
                            Some("provider-v1")
                        );
                        consumer_events
                            .lock()
                            .expect("events lock")
                            .push("consumer");
                        Ok(())
                    }))
                    .map_err(|error| error.to_string())?;
                Ok(value("consumer"))
            }
        }),
    );
    register(reactive.registry(), provider_runtime);
    register(reactive.registry(), consumer_runtime);
    let provider = reactive
        .instantiate(&id("fiber-provider"), String::new())
        .expect("provider fiber instantiates");
    let consumer = reactive
        .instantiate(&id("fiber-consumer"), String::new())
        .expect("consumer fiber instantiates");
    active(&provider).await;
    active(&consumer).await;
    let provider_generation = provider
        .handle()
        .await
        .expect("provider publication")
        .generation();

    let report = reactive
        .withdraw_and_reconcile(&id("provider"), provider_generation)
        .await
        .expect("provider withdrawal");

    assert!(report.is_success());
    assert!(report.errors.is_empty());
    assert!(report.cleanup_errors.is_empty());
    assert_eq!(report.driven, vec![consumer.id(), provider.id()]);
    assert_eq!(provider.state(), FiberState::Pending);
    assert_eq!(consumer.state(), FiberState::Pending);
    assert_eq!(
        events.lock().expect("events lock").as_slice(),
        &["consumer", "provider-effect", "provider-value"]
    );
}
