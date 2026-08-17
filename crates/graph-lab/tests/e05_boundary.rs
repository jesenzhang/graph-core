//! Cross-structure evidence that stream transport is not workflow truth.

use execution_stream::{CoalescingBuffer, KeyedStreamItem, LossyBuffer, Sequence, StreamItem};
use graph_core::Id;
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};
use workflow_recovery::{
    DurableJournal, EffectIntent, EffectSemantics, OperationId, RecoveryAction, classify_recovery,
};

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn workflow_with_task() -> WorkflowGraph {
    let mut workflow = WorkflowGraph::default();
    workflow
        .apply_batch(
            workflow.revision(),
            [WorkflowMutation::AddTask {
                task: Task {
                    id: id("task"),
                    label: "Task".to_owned(),
                },
            }],
        )
        .expect("workflow setup is valid");
    workflow
}

#[test]
fn lossy_stream_does_not_change_workflow_completion_facts() {
    let mut workflow = workflow_with_task();
    workflow.complete(&id("task")).expect("task completes");
    let before = workflow.facts().clone();
    let stream_id = id("telemetry");
    let mut telemetry = LossyBuffer::new(1).expect("capacity is valid");

    telemetry.push(StreamItem {
        stream_id: stream_id.clone(),
        sequence: Sequence::FIRST,
        payload: "10%",
    });
    telemetry.push(StreamItem {
        stream_id,
        sequence: Sequence::FIRST.next(),
        payload: "20%",
    });

    assert_eq!(workflow.facts(), &before);
}

#[test]
fn coalesced_stream_does_not_change_topology_revision() {
    let workflow = workflow_with_task();
    let before_revision = workflow.topology_revision();
    let stream_id = id("progress");
    let mut progress = CoalescingBuffer::new(1).expect("capacity is valid");

    progress
        .try_push(KeyedStreamItem {
            key: "task",
            item: StreamItem {
                stream_id: stream_id.clone(),
                sequence: Sequence::FIRST,
                payload: 10_u32,
            },
        })
        .expect("first progress fits");
    progress
        .try_push(KeyedStreamItem {
            key: "task",
            item: StreamItem {
                stream_id,
                sequence: Sequence::FIRST.next(),
                payload: 20_u32,
            },
        })
        .expect("same key coalesces");

    assert_eq!(workflow.topology_revision(), before_revision);
}

#[test]
fn workflow_recovery_does_not_depend_on_execution_stream() {
    let workflow = workflow_with_task();
    let operation_id = OperationId::new("task/effect/v1").expect("operation id is valid");
    let mut journal = DurableJournal::new();
    journal
        .persist_intent(EffectIntent {
            task_id: id("task"),
            operation_id: operation_id.clone(),
            semantics: EffectSemantics::Idempotent,
        })
        .expect("intent is valid");

    let decision = classify_recovery(&workflow, &journal, &operation_id)
        .expect("recovery classification is independent");
    assert_eq!(decision.action, RecoveryAction::Execute);
}
