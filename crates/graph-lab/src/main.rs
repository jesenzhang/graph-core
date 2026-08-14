//! Executable laboratory for small cross-structure experiments.

use capability_graph::{Capability, CapabilityDefinition, CapabilityGraph, CapabilityValue, Scope};
use execution_stream::{Sequence, StreamItem};
use graph_core::Id;
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};

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

    drop(service);
    drop(model_v1);
    runtime.teardown();
    Ok(())
}
