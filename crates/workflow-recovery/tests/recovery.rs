//! Semantic tests for the E04 crash/recovery boundary.

use graph_core::Id;
use std::collections::{BTreeMap, BTreeSet};
use workflow_graph::{Task, WorkflowGraph, WorkflowMutation};
use workflow_recovery::{
    AttemptId, DispatchRecord, DurableJournal, EffectIntent, EffectSemantics, JournalError,
    KnownEffectOutcome, OperationId, OutcomeRecord, RecoveredEffectState, RecoveryAction,
    classify_recovery,
};

fn id(value: &str) -> Id {
    Id::new(value).expect("test id is valid")
}

fn operation(value: &str) -> OperationId {
    OperationId::new(value).expect("test operation id is valid")
}

fn attempt(value: &str) -> AttemptId {
    AttemptId::new(value).expect("test attempt id is valid")
}

fn intent(operation_id: &OperationId, semantics: EffectSemantics) -> EffectIntent {
    EffectIntent {
        task_id: id("send-contract"),
        operation_id: operation_id.clone(),
        semantics,
    }
}

fn workflow_with_task() -> WorkflowGraph {
    let mut workflow = WorkflowGraph::default();
    workflow
        .apply_batch(
            workflow.revision(),
            [WorkflowMutation::AddTask {
                task: Task {
                    id: id("send-contract"),
                    label: "Send contract".to_owned(),
                },
            }],
        )
        .expect("test workflow is valid");
    workflow
}

fn workflow_with_prerequisite() -> WorkflowGraph {
    let mut workflow = WorkflowGraph::default();
    workflow
        .apply_batch(
            workflow.revision(),
            [
                WorkflowMutation::AddTask {
                    task: Task {
                        id: id("plan"),
                        label: "Plan".to_owned(),
                    },
                },
                WorkflowMutation::AddTask {
                    task: Task {
                        id: id("send-contract"),
                        label: "Send contract".to_owned(),
                    },
                },
                WorkflowMutation::AddDependency {
                    task_id: id("send-contract"),
                    dependency_id: id("plan"),
                },
            ],
        )
        .expect("test workflow is valid");
    workflow
}

fn prepared_journal(operation_id: &OperationId, semantics: EffectSemantics) -> DurableJournal {
    let mut journal = DurableJournal::new();
    journal
        .persist_intent(intent(operation_id, semantics))
        .expect("intent is valid");
    journal
}

fn dispatch(journal: &mut DurableJournal, operation_id: &OperationId, attempt_id: &AttemptId) {
    journal
        .persist_dispatch(DispatchRecord {
            operation_id: operation_id.clone(),
            attempt_id: attempt_id.clone(),
        })
        .expect("dispatch is valid");
}

fn outcome(
    journal: &mut DurableJournal,
    operation_id: &OperationId,
    attempt_id: &AttemptId,
    value: KnownEffectOutcome,
) {
    journal
        .persist_outcome(OutcomeRecord {
            operation_id: operation_id.clone(),
            attempt_id: attempt_id.clone(),
            outcome: value,
        })
        .expect("outcome is valid");
}

#[derive(Default)]
struct SimulatedExternalWorld {
    idempotent_commits: BTreeSet<OperationId>,
    non_idempotent_commits: BTreeMap<OperationId, usize>,
}

impl SimulatedExternalWorld {
    fn invoke(
        &mut self,
        operation_id: &OperationId,
        semantics: EffectSemantics,
    ) -> KnownEffectOutcome {
        match semantics {
            EffectSemantics::Idempotent => {
                self.idempotent_commits.insert(operation_id.clone());
            }
            EffectSemantics::NonIdempotent => {
                *self
                    .non_idempotent_commits
                    .entry(operation_id.clone())
                    .or_default() += 1;
            }
        }
        KnownEffectOutcome::Succeeded
    }

    fn commit_count(&self, operation_id: &OperationId) -> usize {
        if self.idempotent_commits.contains(operation_id) {
            1
        } else {
            self.non_idempotent_commits
                .get(operation_id)
                .copied()
                .unwrap_or_default()
        }
    }
}

#[test]
fn prepared_effect_is_safe_to_execute() {
    let operation_id = operation("send-contract/contract-123/v1");
    let journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);

    assert_eq!(journal.state(&operation_id), RecoveredEffectState::Prepared);
    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");

    assert_eq!(decision.action, RecoveryAction::Execute);
}

#[test]
fn dispatched_effect_without_outcome_is_unknown() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);

    assert_eq!(
        journal.state(&operation_id),
        RecoveredEffectState::OutcomeUnknown
    );
}

#[test]
fn non_idempotent_unknown_requires_reconciliation() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);

    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");

    assert_eq!(decision.action, RecoveryAction::Reconcile);
}

#[test]
fn idempotent_unknown_allows_same_operation_retry() {
    let operation_id = operation("notify/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::Idempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);

    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");

    assert_eq!(decision.action, RecoveryAction::RetrySameOperation);
}

#[test]
fn known_success_completes_without_reexecution() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Succeeded,
    );

    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");

    assert_eq!(decision.action, RecoveryAction::CompleteWithoutReexecution);
}

#[test]
fn completed_workflow_requires_no_action() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Succeeded,
    );
    let mut workflow = workflow_with_task();
    workflow
        .complete(&id("send-contract"))
        .expect("task completes");

    let decision =
        classify_recovery(&workflow, &journal, &operation_id).expect("operation is known");

    assert_eq!(decision.action, RecoveryAction::NoAction);
}

#[test]
fn known_failure_is_not_outcome_unknown() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Failed,
    );

    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");

    assert_eq!(
        journal.state(&operation_id),
        RecoveredEffectState::OutcomeKnown(KnownEffectOutcome::Failed)
    );
    assert_eq!(decision.action, RecoveryAction::ObserveFailure);
}

#[test]
fn non_idempotent_external_commit_is_not_duplicated_after_crash() {
    let operation_id = operation("charge/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);

    let mut external = SimulatedExternalWorld::default();
    let actual_result = external.invoke(&operation_id, EffectSemantics::NonIdempotent);
    assert_eq!(actual_result, KnownEffectOutcome::Succeeded);

    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");
    assert_eq!(decision.action, RecoveryAction::Reconcile);
    assert_eq!(external.commit_count(&operation_id), 1);
}

#[test]
fn idempotent_retry_reuses_operation_identity() {
    let operation_id = operation("notify/contract-123/v1");
    let attempt_one = attempt("attempt-1");
    let attempt_two = attempt("attempt-2");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::Idempotent);
    dispatch(&mut journal, &operation_id, &attempt_one);

    let mut external = SimulatedExternalWorld::default();
    external.invoke(&operation_id, EffectSemantics::Idempotent);
    let decision = classify_recovery(&workflow_with_task(), &journal, &operation_id)
        .expect("operation is known");
    assert_eq!(decision.action, RecoveryAction::RetrySameOperation);

    dispatch(&mut journal, &operation_id, &attempt_two);
    let retry_result = external.invoke(&operation_id, EffectSemantics::Idempotent);
    outcome(&mut journal, &operation_id, &attempt_two, retry_result);

    assert_eq!(
        journal
            .latest_dispatch(&operation_id)
            .expect("dispatch")
            .operation_id,
        operation_id
    );
    assert_eq!(external.commit_count(&operation_id), 1);
}

#[test]
fn idempotent_retry_does_not_duplicate_external_commit() {
    let operation_id = operation("notify/contract-123/v1");
    let attempt_one = attempt("attempt-1");
    let attempt_two = attempt("attempt-2");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::Idempotent);
    dispatch(&mut journal, &operation_id, &attempt_one);
    let mut external = SimulatedExternalWorld::default();
    external.invoke(&operation_id, EffectSemantics::Idempotent);

    assert_eq!(
        classify_recovery(&workflow_with_task(), &journal, &operation_id)
            .expect("operation is known")
            .action,
        RecoveryAction::RetrySameOperation
    );
    dispatch(&mut journal, &operation_id, &attempt_two);
    let result = external.invoke(&operation_id, EffectSemantics::Idempotent);
    outcome(&mut journal, &operation_id, &attempt_two, result);

    assert_eq!(external.commit_count(&operation_id), 1);
    assert_eq!(
        classify_recovery(&workflow_with_task(), &journal, &operation_id)
            .expect("operation is known")
            .action,
        RecoveryAction::CompleteWithoutReexecution
    );
}

#[test]
fn crash_after_outcome_before_completion_does_not_reexecute_effect() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    let mut external = SimulatedExternalWorld::default();
    let result = external.invoke(&operation_id, EffectSemantics::NonIdempotent);
    outcome(&mut journal, &operation_id, &attempt_id, result);

    let mut workflow = workflow_with_task();
    let before_revision = workflow.revision();
    let decision =
        classify_recovery(&workflow, &journal, &operation_id).expect("operation is known");
    assert_eq!(decision.action, RecoveryAction::CompleteWithoutReexecution);
    workflow
        .complete(&id("send-contract"))
        .expect("task completes");

    assert_eq!(external.commit_count(&operation_id), 1);
    assert_eq!(workflow.revision(), before_revision);
}

#[test]
fn recovery_decision_is_deterministic_from_durable_facts() {
    let operation_id = operation("charge/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    let workflow = workflow_with_task();
    let clone = journal.clone();

    let first = classify_recovery(&workflow, &journal, &operation_id).expect("operation is known");
    let second = classify_recovery(&workflow, &clone, &operation_id).expect("operation is known");

    assert_eq!(first, second);
}

#[test]
fn repeated_recovery_without_new_facts_is_stable() {
    let operation_id = operation("charge/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    let workflow = workflow_with_task();

    let decisions = (0..3)
        .map(|_| classify_recovery(&workflow, &journal, &operation_id).expect("operation is known"))
        .collect::<Vec<_>>();

    assert!(decisions.iter().all(|decision| decision == &decisions[0]));
}

#[test]
fn dispatch_without_intent_is_rejected() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = DurableJournal::new();

    assert_eq!(
        journal
            .persist_dispatch(DispatchRecord {
                operation_id: operation_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .expect_err("dispatch needs an intent"),
        JournalError::DispatchWithoutIntent(operation_id)
    );
}

#[test]
fn outcome_without_dispatch_is_rejected() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);

    assert_eq!(
        journal
            .persist_outcome(OutcomeRecord {
                operation_id: operation_id.clone(),
                attempt_id: attempt_id.clone(),
                outcome: KnownEffectOutcome::Succeeded,
            })
            .expect_err("outcome needs a dispatch"),
        JournalError::OutcomeWithoutDispatch {
            operation_id,
            attempt_id,
        }
    );
}

#[test]
fn outcome_for_wrong_attempt_is_rejected() {
    let first_operation = operation("send-contract/contract-123/v1");
    let second_operation = operation("send-contract/contract-123/v2");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&first_operation, EffectSemantics::NonIdempotent);
    journal
        .persist_intent(intent(&second_operation, EffectSemantics::NonIdempotent))
        .expect("second intent is valid");
    dispatch(&mut journal, &second_operation, &attempt_id);

    assert_eq!(
        journal
            .persist_outcome(OutcomeRecord {
                operation_id: first_operation.clone(),
                attempt_id: attempt_id.clone(),
                outcome: KnownEffectOutcome::Succeeded,
            })
            .expect_err("attempt belongs to another operation"),
        JournalError::AttemptMismatch {
            operation_id: first_operation,
            attempt_id,
            dispatched_for: second_operation,
        }
    );
}

#[test]
fn conflicting_outcome_is_rejected() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Succeeded,
    );

    assert_eq!(
        journal
            .persist_outcome(OutcomeRecord {
                operation_id: operation_id.clone(),
                attempt_id: attempt_id.clone(),
                outcome: KnownEffectOutcome::Failed,
            })
            .expect_err("outcomes cannot contradict"),
        JournalError::ConflictingOutcome {
            operation_id,
            attempt_id,
            existing: KnownEffectOutcome::Succeeded,
            attempted: KnownEffectOutcome::Failed,
        }
    );
}

#[test]
fn effect_completion_does_not_advance_topology_revision() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Succeeded,
    );
    let mut workflow = workflow_with_task();
    let before_revision = workflow.topology_revision();
    assert_eq!(
        classify_recovery(&workflow, &journal, &operation_id)
            .expect("operation is known")
            .action,
        RecoveryAction::CompleteWithoutReexecution
    );

    workflow
        .complete(&id("send-contract"))
        .expect("task completes");

    assert_eq!(workflow.topology_revision(), before_revision);
}

#[test]
fn recovery_completion_preserves_existing_completed_facts() {
    let operation_id = operation("send-contract/contract-123/v1");
    let attempt_id = attempt("attempt-1");
    let mut journal = prepared_journal(&operation_id, EffectSemantics::NonIdempotent);
    dispatch(&mut journal, &operation_id, &attempt_id);
    outcome(
        &mut journal,
        &operation_id,
        &attempt_id,
        KnownEffectOutcome::Succeeded,
    );
    let mut workflow = workflow_with_prerequisite();
    workflow.complete(&id("plan")).expect("plan completes");
    let before_revision = workflow.topology_revision();

    workflow
        .complete(&id("send-contract"))
        .expect("effect completes");

    assert_eq!(workflow.topology_revision(), before_revision);
    assert_eq!(
        workflow.completed_tasks(),
        vec![&id("plan"), &id("send-contract")]
    );
    assert_eq!(workflow.completion_log().len(), 2);
}
