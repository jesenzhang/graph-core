//! Pure recovery classification over workflow facts and the durable journal.

use crate::journal::{DurableJournal, JournalError};
use crate::model::{
    EffectSemantics, RecoveredEffectState, RecoveryAction, RecoveryDecision, RecoveryReason,
};
use workflow_graph::WorkflowGraph;

/// Classifies the next recovery action without executing an external effect.
///
/// The result is a deterministic function of the supplied workflow execution
/// facts and durable journal. It does not read process-local history, clocks,
/// streams, or scheduler state.
///
/// # Errors
///
/// Returns [`JournalError::UnknownOperation`] when the operation has no durable
/// intent. A journal that was built through its admission API cannot contain a
/// malformed dispatch/outcome sequence.
pub fn classify_recovery(
    workflow: &WorkflowGraph,
    journal: &DurableJournal,
    operation_id: &crate::model::OperationId,
) -> Result<RecoveryDecision, JournalError> {
    let intent = journal.intent(operation_id)?;
    let state = journal.state(operation_id);
    if workflow.task(&intent.task_id).is_none() {
        return Ok(invariant_decision(operation_id, intent, state));
    }
    let completed = workflow.is_completed(&intent.task_id);

    match state {
        RecoveredEffectState::Prepared => {
            if completed {
                return Ok(invariant_decision(operation_id, intent, state));
            }
            Ok(RecoveryDecision {
                action: RecoveryAction::Execute,
                reason: RecoveryReason::PreparedNotDispatched {
                    operation_id: operation_id.clone(),
                },
            })
        }
        RecoveredEffectState::OutcomeUnknown => {
            let attempt_id = journal
                .latest_dispatch(operation_id)
                .expect("unknown outcome requires a dispatch")
                .attempt_id
                .clone();
            if completed {
                return Ok(invariant_decision(operation_id, intent, state));
            }

            let (action, reason) = match intent.semantics {
                EffectSemantics::Idempotent => (
                    RecoveryAction::RetrySameOperation,
                    RecoveryReason::IdempotentOutcomeUnknown {
                        operation_id: operation_id.clone(),
                        attempt_id,
                    },
                ),
                EffectSemantics::NonIdempotent => (
                    RecoveryAction::Reconcile,
                    RecoveryReason::NonIdempotentOutcomeUnknown {
                        operation_id: operation_id.clone(),
                        attempt_id,
                    },
                ),
            };
            Ok(RecoveryDecision { action, reason })
        }
        RecoveredEffectState::OutcomeKnown(outcome) => {
            let attempt_id = journal
                .known_outcome(operation_id)
                .expect("known state requires an outcome")
                .attempt_id
                .clone();
            match (outcome, completed) {
                (crate::model::KnownEffectOutcome::Succeeded, true) => Ok(RecoveryDecision {
                    action: RecoveryAction::NoAction,
                    reason: RecoveryReason::WorkflowAlreadyCompleted {
                        operation_id: operation_id.clone(),
                        task_id: intent.task_id.clone(),
                    },
                }),
                (crate::model::KnownEffectOutcome::Succeeded, false) => Ok(RecoveryDecision {
                    action: RecoveryAction::CompleteWithoutReexecution,
                    reason: RecoveryReason::KnownSuccess {
                        operation_id: operation_id.clone(),
                        attempt_id,
                    },
                }),
                (crate::model::KnownEffectOutcome::Failed, true) => {
                    Ok(invariant_decision(operation_id, intent, state))
                }
                (crate::model::KnownEffectOutcome::Failed, false) => Ok(RecoveryDecision {
                    action: RecoveryAction::ObserveFailure,
                    reason: RecoveryReason::KnownFailure {
                        operation_id: operation_id.clone(),
                        attempt_id,
                    },
                }),
            }
        }
        RecoveredEffectState::NotPrepared => Ok(RecoveryDecision {
            action: RecoveryAction::InvariantViolation,
            reason: RecoveryReason::NotPrepared {
                operation_id: operation_id.clone(),
            },
        }),
    }
}

fn invariant_decision(
    operation_id: &crate::model::OperationId,
    intent: &crate::model::EffectIntent,
    state: RecoveredEffectState,
) -> RecoveryDecision {
    RecoveryDecision {
        action: RecoveryAction::InvariantViolation,
        reason: RecoveryReason::WorkflowStateMismatch {
            operation_id: operation_id.clone(),
            task_id: intent.task_id.clone(),
            state,
        },
    }
}
