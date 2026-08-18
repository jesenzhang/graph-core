//! In-memory durable fact model for the E04 experiment.

use crate::DurableRunState;
use crate::model::{
    AttemptId, DispatchRecord, EffectIntent, KnownEffectOutcome, OperationId, OutcomeRecord,
    RecoveredEffectState,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Structured invariant that rejects an impossible durable fact sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalInvariant {
    /// A reserved extension point for future journal-only invariants.
    Reserved,
}

/// Error returned when a durable journal mutation would create malformed facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalError {
    /// A query or outcome referenced an operation without a durable intent.
    UnknownOperation(OperationId),
    /// An operation intent can be persisted only once.
    DuplicateIntent(OperationId),
    /// A dispatch was attempted without its intent.
    DispatchWithoutIntent(OperationId),
    /// A task already owns a different logical external operation.
    TaskOperationConflict {
        /// Workflow task that already has an effect owner.
        task_id: graph_core::Id,
        /// Operation currently owning the task.
        existing_operation: OperationId,
        /// Operation that attempted to claim the task.
        attempted_operation: OperationId,
    },
    /// An attempt identity was already used.
    DuplicateAttempt {
        /// Operation attempting to reuse the attempt.
        operation_id: OperationId,
        /// Reused attempt identity.
        attempt_id: AttemptId,
    },
    /// An outcome was recorded without a matching dispatch.
    OutcomeWithoutDispatch {
        /// Operation whose outcome was supplied.
        operation_id: OperationId,
        /// Attempt whose dispatch is missing.
        attempt_id: AttemptId,
    },
    /// The attempt exists, but belongs to another operation.
    AttemptMismatch {
        /// Operation named by the outcome.
        operation_id: OperationId,
        /// Attempt named by the outcome.
        attempt_id: AttemptId,
        /// Operation that actually owns the attempt.
        dispatched_for: OperationId,
    },
    /// The same attempt was assigned two different outcomes.
    ConflictingOutcome {
        /// Operation whose outcome conflicts.
        operation_id: OperationId,
        /// Attempt whose outcome conflicts.
        attempt_id: AttemptId,
        /// Previously recorded result.
        existing: KnownEffectOutcome,
        /// New contradictory result.
        attempted: KnownEffectOutcome,
    },
    /// A durable mutation violated a journal invariant.
    InvariantViolation(JournalInvariant),
    /// Workflow completion and effect facts disagree.
    WorkflowStateMismatch {
        /// Operation associated with the mismatch.
        operation_id: OperationId,
        /// Task associated with the mismatch.
        task_id: graph_core::Id,
        /// Effect state observed at the boundary.
        state: RecoveredEffectState,
    },
}

impl fmt::Display for JournalInvariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reserved => f.write_str("reserved journal invariant"),
        }
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOperation(operation_id) => {
                write!(f, "unknown operation: {operation_id}")
            }
            Self::DuplicateIntent(operation_id) => {
                write!(f, "duplicate effect intent: {operation_id}")
            }
            Self::DispatchWithoutIntent(operation_id) => {
                write!(f, "dispatch without effect intent: {operation_id}")
            }
            Self::TaskOperationConflict {
                task_id,
                existing_operation,
                attempted_operation,
            } => write!(
                f,
                "task {task_id} already belongs to operation {existing_operation}; \
                 cannot assign {attempted_operation}"
            ),
            Self::DuplicateAttempt {
                operation_id,
                attempt_id,
            } => write!(
                f,
                "duplicate attempt {attempt_id} for operation {operation_id}"
            ),
            Self::OutcomeWithoutDispatch {
                operation_id,
                attempt_id,
            } => write!(
                f,
                "outcome without dispatch: operation {operation_id}, attempt {attempt_id}"
            ),
            Self::AttemptMismatch {
                operation_id,
                attempt_id,
                dispatched_for,
            } => write!(
                f,
                "attempt {attempt_id} belongs to {dispatched_for}, not {operation_id}"
            ),
            Self::ConflictingOutcome {
                operation_id,
                attempt_id,
                existing,
                attempted,
            } => write!(
                f,
                "conflicting outcome for operation {operation_id}, attempt {attempt_id}: "
            )
            .and_then(|_| write!(f, "{existing:?} versus {attempted:?}")),
            Self::InvariantViolation(invariant) => {
                write!(f, "journal invariant violation: {invariant}")
            }
            Self::WorkflowStateMismatch {
                operation_id,
                task_id,
                state,
            } => write!(
                f,
                "workflow state mismatch for operation {operation_id}, task {task_id}: {state:?}"
            ),
        }
    }
}

impl std::error::Error for JournalError {}

/// Deterministic in-memory representation of durable local recovery facts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DurableJournal {
    intents: BTreeMap<OperationId, EffectIntent>,
    task_operations: BTreeMap<graph_core::Id, OperationId>,
    dispatches: BTreeMap<AttemptId, DispatchRecord>,
    operation_attempts: BTreeMap<OperationId, Vec<AttemptId>>,
    outcomes: BTreeMap<AttemptId, OutcomeRecord>,
}

impl DurableJournal {
    /// Creates an empty durable journal.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            intents: BTreeMap::new(),
            task_operations: BTreeMap::new(),
            dispatches: BTreeMap::new(),
            operation_attempts: BTreeMap::new(),
            outcomes: BTreeMap::new(),
        }
    }

    /// Reconstructs the compatibility journal view from detached durable
    /// state.  The store remains the persistence authority; this value is a
    /// process-local query view used by existing recovery callers.
    pub fn from_durable_state(state: &DurableRunState) -> Result<Self, JournalError> {
        let mut journal = Self::new();
        for intent in state.intents() {
            journal.persist_intent(intent.clone())?;
        }
        for dispatch in state.dispatch_history() {
            journal.persist_dispatch(dispatch.clone())?;
        }
        for outcome in state.outcome_history_all() {
            journal.persist_outcome(outcome.clone())?;
        }
        Ok(journal)
    }

    /// Persists one effect intent before any dispatch is allowed.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::DuplicateIntent`] when the operation already
    /// exists, or [`JournalError::TaskOperationConflict`] when the task is
    /// already owned by another operation.
    pub fn persist_intent(&mut self, intent: EffectIntent) -> Result<(), JournalError> {
        if self.intents.contains_key(&intent.operation_id) {
            return Err(JournalError::DuplicateIntent(intent.operation_id));
        }
        if let Some(existing_operation) = self.task_operations.get(&intent.task_id) {
            return Err(JournalError::TaskOperationConflict {
                task_id: intent.task_id,
                existing_operation: existing_operation.clone(),
                attempted_operation: intent.operation_id,
            });
        }
        self.task_operations
            .insert(intent.task_id.clone(), intent.operation_id.clone());
        self.intents.insert(intent.operation_id.clone(), intent);
        Ok(())
    }

    /// Persists the dispatch boundary for one transport attempt.
    ///
    /// # Errors
    ///
    /// Rejects missing intents, reused attempt identities, and dispatches after
    /// a known outcome.
    pub fn persist_dispatch(&mut self, dispatch: DispatchRecord) -> Result<(), JournalError> {
        if !self.intents.contains_key(&dispatch.operation_id) {
            return Err(JournalError::DispatchWithoutIntent(dispatch.operation_id));
        }
        if self.dispatches.contains_key(&dispatch.attempt_id) {
            return Err(JournalError::DuplicateAttempt {
                operation_id: dispatch.operation_id,
                attempt_id: dispatch.attempt_id,
            });
        }
        self.operation_attempts
            .entry(dispatch.operation_id.clone())
            .or_default()
            .push(dispatch.attempt_id.clone());
        self.dispatches
            .insert(dispatch.attempt_id.clone(), dispatch);
        Ok(())
    }

    /// Persists a known external outcome for a dispatched attempt.
    ///
    /// # Errors
    ///
    /// Rejects missing intents, missing dispatches, attempt mismatches, and
    /// contradictory outcomes.
    pub fn persist_outcome(&mut self, outcome: OutcomeRecord) -> Result<(), JournalError> {
        if !self.intents.contains_key(&outcome.operation_id) {
            return Err(JournalError::UnknownOperation(outcome.operation_id));
        }
        let Some(dispatch) = self.dispatches.get(&outcome.attempt_id) else {
            return Err(JournalError::OutcomeWithoutDispatch {
                operation_id: outcome.operation_id,
                attempt_id: outcome.attempt_id,
            });
        };
        if dispatch.operation_id != outcome.operation_id {
            return Err(JournalError::AttemptMismatch {
                operation_id: outcome.operation_id,
                attempt_id: outcome.attempt_id,
                dispatched_for: dispatch.operation_id.clone(),
            });
        }

        if let Some(existing) = self.outcomes.get(&outcome.attempt_id) {
            if existing.outcome == outcome.outcome {
                return Ok(());
            }
            return Err(JournalError::ConflictingOutcome {
                operation_id: outcome.operation_id,
                attempt_id: outcome.attempt_id,
                existing: existing.outcome,
                attempted: outcome.outcome,
            });
        }
        self.outcomes.insert(outcome.attempt_id.clone(), outcome);
        Ok(())
    }

    /// Returns the intent for an operation.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::UnknownOperation`] when no intent exists.
    pub fn intent(&self, operation_id: &OperationId) -> Result<&EffectIntent, JournalError> {
        self.intents
            .get(operation_id)
            .ok_or_else(|| JournalError::UnknownOperation(operation_id.clone()))
    }

    /// Returns the logical operation owned by a workflow task, if any.
    ///
    /// The mapping is derived from the durable intent admission table. It is
    /// exposed so coordinators do not need to keep a second task/effect map.
    #[must_use]
    pub fn operation_for_task(&self, task_id: &graph_core::Id) -> Option<&OperationId> {
        self.task_operations.get(task_id)
    }

    /// Returns the most recently persisted dispatch for an operation.
    #[must_use]
    pub fn latest_dispatch(&self, operation_id: &OperationId) -> Option<&DispatchRecord> {
        self.operation_attempts
            .get(operation_id)
            .and_then(|attempts| attempts.last())
            .and_then(|attempt_id| self.dispatches.get(attempt_id))
    }

    /// Returns the known outcome for the most recent dispatch, if it has been
    /// checkpointed.
    #[must_use]
    pub fn known_outcome(&self, operation_id: &OperationId) -> Option<&OutcomeRecord> {
        self.operation_attempts
            .get(operation_id)
            .and_then(|attempts| attempts.last())
            .and_then(|attempt_id| self.outcomes.get(attempt_id))
    }

    /// Derives local recovery knowledge from durable facts only.
    #[must_use]
    pub fn state(&self, operation_id: &OperationId) -> RecoveredEffectState {
        if !self.intents.contains_key(operation_id) {
            return RecoveredEffectState::NotPrepared;
        }
        if let Some(outcome) = self.known_outcome(operation_id) {
            return RecoveredEffectState::OutcomeKnown(outcome.outcome);
        }
        if self.latest_dispatch(operation_id).is_some() {
            RecoveredEffectState::OutcomeUnknown
        } else {
            RecoveredEffectState::Prepared
        }
    }

    /// Returns all operation identities in deterministic order.
    #[must_use]
    pub fn operation_ids(&self) -> BTreeSet<OperationId> {
        self.intents.keys().cloned().collect()
    }
}
