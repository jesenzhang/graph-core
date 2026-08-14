//! Executable laboratory for small cross-structure experiments.

use capability_graph::{Capability, CapabilityGraph};
use execution_stream::{Sequence, StreamItem};
use graph_core::Id;
use workflow_graph::{Task, WorkflowGraph};

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

    let plan = Task {
        id: Id::new("plan")?,
        label: "Plan work".to_owned(),
    };
    let execute = Task {
        id: Id::new("execute")?,
        label: "Execute work".to_owned(),
    };
    let mut workflow = WorkflowGraph::default();
    workflow.upsert_task(plan.clone());
    workflow.upsert_task(execute.clone());
    workflow.depends_on(&execute.id, &plan.id)?;

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

    Ok(())
}
