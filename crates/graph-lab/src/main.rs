//! Executable laboratory for small cross-structure experiments.

use capability_graph::{Capability, CapabilityDefinition, CapabilityGraph, CapabilityValue, Scope};
use execution_stream::{
    CoalescingBuffer, KeyedStreamItem, LosslessBuffer, LossyBuffer, PushError, Sequence,
    SequenceObservation, SequenceTracker, StreamItem,
};
use kernis_core::Id;
use runtime_core::{RunId, Runtime as CoreRuntime, StepResult, TaskConfig};
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};
use workflow_recovery::{
    AttemptId, DispatchRecord, DurableJournal, EffectIntent, EffectSemantics, KnownEffectOutcome,
    OperationId, RecoveryAction, classify_recovery,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_id = Id::new("model")?;
    let runtime_id = Id::new("runtime")?;
    let mut capabilities = CapabilityGraph::default();
    capabilities.insert(Capability {
        id: model_id.clone(),
        kind: "model".to_owned(),
    });
    capabilities.insert(Capability {
        id: runtime_id.clone(),
        kind: "runtime".to_owned(),
    });
    capabilities.require(&runtime_id, &model_id)?;
    let resolution = capabilities.resolve()?;
    let construction_order = resolution
        .construction_order()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ");

    let runtime = Scope::root();
    let model_v1 = runtime.provide(CapabilityDefinition::new(model_id.clone(), "model"), |_| {
        Ok(CapabilityValue::from_value("model-v1".to_owned()))
    })?;
    let model_generation = model_v1.generation();
    let service = runtime.provide(
        CapabilityDefinition::new(runtime_id.clone(), "runtime").depends_on(model_id.clone()),
        |dependencies| {
            let model = dependencies
                .get(&model_id)
                .expect("model dependency was admitted")
                .downcast_ref::<String>()
                .expect("model value is a string")
                .clone();
            Ok(CapabilityValue::from_value(format!(
                "service bound to {model}"
            )))
        },
    )?;
    runtime.replace(
        CapabilityDefinition::new(model_id.clone(), "model"),
        model_generation,
        |_| Ok(CapabilityValue::from_value("model-v2".to_owned())),
    )?;
    let old_service = service
        .downcast_ref::<String>()
        .expect("service value is a string");
    let new_model_handle = runtime.get(&model_id).expect("new model is published");
    let new_model = new_model_handle
        .downcast_ref::<String>()
        .expect("model value is a string");

    let plan = Task {
        id: Id::new("plan")?,
        label: "Plan work".to_owned(),
    };
    let research = Task {
        id: Id::new("research")?,
        label: "Research work".to_owned(),
    };
    let execute = Task {
        id: Id::new("execute")?,
        label: "Execute work".to_owned(),
    };
    let mut workflow = WorkflowGraph::default();
    workflow.apply_batch(
        workflow.revision(),
        [
            WorkflowMutation::AddTask { task: plan.clone() },
            WorkflowMutation::AddTask {
                task: research.clone(),
            },
            WorkflowMutation::AddTask {
                task: execute.clone(),
            },
            WorkflowMutation::AddDependency {
                task_id: research.id.clone(),
                dependency_id: plan.id.clone(),
            },
            WorkflowMutation::AddDependency {
                task_id: execute.id.clone(),
                dependency_id: research.id.clone(),
            },
        ],
    )?;
    workflow.complete(&plan.id)?;
    workflow.complete(&research.id)?;
    let before_planner_revision = workflow.revision();
    workflow.apply_batch(
        before_planner_revision,
        [
            WorkflowMutation::AddTask {
                task: Task {
                    id: Id::new("validate")?,
                    label: "Validate work".to_owned(),
                },
            },
            WorkflowMutation::AddTask {
                task: Task {
                    id: Id::new("review")?,
                    label: "Review work".to_owned(),
                },
            },
            WorkflowMutation::AddDependency {
                task_id: Id::new("validate")?,
                dependency_id: research.id.clone(),
            },
            WorkflowMutation::AddDependency {
                task_id: Id::new("review")?,
                dependency_id: research.id.clone(),
            },
            WorkflowMutation::AddDependency {
                task_id: execute.id.clone(),
                dependency_id: Id::new("validate")?,
            },
            WorkflowMutation::AddDependency {
                task_id: execute.id.clone(),
                dependency_id: Id::new("review")?,
            },
        ],
    )?;
    let ready = workflow
        .ready_tasks()
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let stream = StreamItem {
        stream_id: Id::new("runtime-events")?,
        sequence: Sequence::FIRST,
        payload: "baseline-ready",
    };

    println!(
        "capabilities={}, construction_order={}, workflow_revision={}, stream={}#{}:{}",
        capabilities.len(),
        construction_order,
        workflow.revision().get(),
        stream.stream_id,
        stream.sequence.get(),
        stream.payload
    );
    println!(
        "e02r: {} -> replacement -> new lookup {}",
        old_service, new_model
    );
    println!(
        "e03: topology_revision={} -> {}, completed=plan,research, ready={}",
        before_planner_revision.get(),
        workflow.revision().get(),
        ready
    );

    let effect_task = Id::new("send-contract")?;
    let operation_id = OperationId::new("send-contract/contract-123/v1")?;
    let attempt_id = AttemptId::new("attempt-1")?;
    let mut recovery_workflow = WorkflowGraph::default();
    recovery_workflow.apply_batch(
        recovery_workflow.revision(),
        [WorkflowMutation::AddTask {
            task: Task {
                id: effect_task,
                label: "Send contract".to_owned(),
            },
        }],
    )?;
    let mut journal = DurableJournal::new();
    journal.persist_intent(EffectIntent {
        task_id: Id::new("send-contract")?,
        operation_id: operation_id.clone(),
        semantics: EffectSemantics::NonIdempotent,
    })?;
    journal.persist_dispatch(DispatchRecord {
        operation_id: operation_id.clone(),
        attempt_id,
    })?;
    let external_commits = 1;
    let decision = classify_recovery(&recovery_workflow, &journal, &operation_id)?;
    assert_eq!(decision.action, RecoveryAction::Reconcile);
    println!(
        "e04: non-idempotent outcome unknown -> {}, external_commits={external_commits}",
        decision.action
    );

    let stream_id = Id::new("e05-events")?;
    let first = StreamItem {
        stream_id: stream_id.clone(),
        sequence: Sequence::FIRST,
        payload: 10_u32,
    };
    let second = StreamItem {
        stream_id: stream_id.clone(),
        sequence: first.sequence.next(),
        payload: 20_u32,
    };
    let mut lossless = LosslessBuffer::new(1)?;
    lossless
        .try_push(first.clone())
        .expect("lossless setup has capacity");
    let lossless_status = match lossless.try_push(second.clone()) {
        Err(PushError::Backpressure(item)) => {
            lossless.pop();
            lossless
                .try_push(item)
                .expect("lossless retry has capacity");
            "backpressured"
        }
        Ok(()) => "accepted",
    };

    let mut coalescing = CoalescingBuffer::new(1)?;
    coalescing
        .try_push(KeyedStreamItem {
            key: "progress",
            item: first.clone(),
        })
        .expect("coalescing setup has capacity");
    coalescing
        .try_push(KeyedStreamItem {
            key: "progress",
            item: second.clone(),
        })
        .expect("same key coalesces");
    let coalesced = coalescing.pop().expect("coalesced item");
    let mut coalescing_tracker = SequenceTracker::new(stream_id.clone());
    let coalesced_gap = matches!(
        coalescing_tracker.observe(&coalesced.item)?,
        SequenceObservation::Gap { .. }
    );

    let mut lossy = LossyBuffer::new(1)?;
    lossy.push(first);
    lossy.push(second);
    let telemetry = lossy.pop().expect("latest telemetry");
    let mut telemetry_tracker = SequenceTracker::new(stream_id);
    let telemetry_gap = matches!(
        telemetry_tracker.observe(&telemetry)?,
        SequenceObservation::Gap { .. }
    );
    println!(
        "e05: lossless={lossless_status}, coalesced_gap={coalesced_gap}, telemetry_gap={telemetry_gap}"
    );

    run_m1_smoke()?;

    drop(service);
    drop(model_v1);
    runtime.teardown();
    Ok(())
}

fn run_m1_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let provider_id = Id::new("smoke-provider")?;
    let smoke_scope = Scope::root();
    let provider_v1 = smoke_scope.provide(
        CapabilityDefinition::new(provider_id.clone(), "provider"),
        |_| Ok(CapabilityValue::from_value("provider-v1".to_owned())),
    )?;
    let provider_v1_entry = provider_v1.entry_id();

    let a = Task {
        id: Id::new("m1-a")?,
        label: "M1 A".to_owned(),
    };
    let b = Task {
        id: Id::new("m1-b")?,
        label: "M1 B".to_owned(),
    };
    let c = Task {
        id: Id::new("m1-c")?,
        label: "M1 C".to_owned(),
    };
    let mut workflow = WorkflowGraph::default();
    workflow.apply_batch(
        workflow.revision(),
        [
            WorkflowMutation::AddTask { task: a.clone() },
            WorkflowMutation::AddTask { task: b.clone() },
            WorkflowMutation::AddTask { task: c.clone() },
            WorkflowMutation::AddDependency {
                task_id: b.id.clone(),
                dependency_id: a.id.clone(),
            },
            WorkflowMutation::AddDependency {
                task_id: c.id.clone(),
                dependency_id: b.id.clone(),
            },
        ],
    )?;

    let operation_id = OperationId::new("m1-idempotent-effect")?;
    let mut runtime = CoreRuntime::start_run(
        RunId::new("m1-smoke")?,
        workflow,
        smoke_scope.clone(),
        [
            (
                a.id.clone(),
                TaskConfig::new()
                    .require_capability(provider_id.clone())
                    .with_effect(operation_id.clone(), EffectSemantics::Idempotent),
            ),
            (
                b.id.clone(),
                TaskConfig::new().require_capability(provider_id.clone()),
            ),
        ],
    )?;

    let first_attempt = match runtime.step()? {
        StepResult::EffectPending { attempt_id, .. } => attempt_id,
        result => return Err(format!("unexpected M1 first step: {result:?}").into()),
    };
    runtime.dispatch_effect(&operation_id)?;
    let recovery = runtime.recover(&operation_id)?;
    if recovery.action != RecoveryAction::RetrySameOperation {
        return Err(format!("unexpected M1 recovery action: {}", recovery.action).into());
    }

    let generation = smoke_scope
        .generation(&provider_id)
        .expect("smoke provider is visible");
    let provider_v2 = smoke_scope.replace(
        CapabilityDefinition::new(provider_id.clone(), "provider"),
        generation,
        |_| Ok(CapabilityValue::from_value("provider-v2".to_owned())),
    )?;
    let retry_attempt = runtime.dispatch_effect(&operation_id)?;
    runtime.record_effect_outcome(
        &operation_id,
        retry_attempt.clone(),
        KnownEffectOutcome::Succeeded,
    )?;
    runtime.recover(&operation_id)?;
    if first_attempt == retry_attempt {
        return Err("M1 retry must use a new attempt identity".into());
    }
    runtime.run_until_blocked()?;

    runtime.emit_progress(&a.id, 10)?;
    runtime.emit_progress(&a.id, 20)?;
    runtime.emit_telemetry("m1-first")?;
    runtime.emit_telemetry("m1-latest")?;
    let progress_count = runtime.drain_progress_events().len();
    let telemetry_count = runtime.drain_telemetry_events().len();
    if progress_count != 1 || telemetry_count != 1 {
        return Err("M1 stream policy did not coalesce/drop as expected".into());
    }

    let v1_attempts = runtime
        .attempts()
        .iter()
        .filter(|attempt| {
            attempt
                .capability(&provider_id)
                .is_some_and(|pin| pin.entry_id == provider_v1_entry)
        })
        .count();
    let v2_attempts = runtime
        .attempts()
        .iter()
        .filter(|attempt| {
            attempt
                .capability(&provider_id)
                .is_some_and(|pin| pin.entry_id == provider_v2.entry_id())
        })
        .count();
    let completed = runtime.workflow().completed_tasks().len() == 3;
    println!(
        "m1: run={} tasks={} attempts={} provider_v1_attempts={} provider_v2_attempts={} recovery=idempotent-retry-ok stream_loss=non_authoritative",
        if completed { "completed" } else { "blocked" },
        runtime.workflow().tasks().len(),
        runtime.attempts().len(),
        v1_attempts,
        v2_attempts,
    );
    Ok(())
}
