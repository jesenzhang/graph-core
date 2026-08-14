//! Task-level workflow DAG experiments.

use graph_core::{Id, Revision};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Immutable task definition for the baseline DAG model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    /// Stable task identifier.
    pub id: Id,
    /// Human-readable task description.
    pub label: String,
}

/// Versioned in-memory workflow topology.
#[derive(Clone, Debug, Default)]
pub struct WorkflowGraph {
    tasks: BTreeMap<Id, Task>,
    dependencies: BTreeMap<Id, BTreeSet<Id>>,
    revision: Revision,
}

impl WorkflowGraph {
    /// Adds or replaces a task and advances the topology revision.
    pub fn upsert_task(&mut self, task: Task) {
        self.tasks.insert(task.id.clone(), task);
        self.revision = self.revision.next();
    }

    /// Declares that `task` cannot run until `dependency` is complete.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowGraphError::UnknownTask`] if either endpoint is absent.
    pub fn depends_on(&mut self, task: &Id, dependency: &Id) -> Result<(), WorkflowGraphError> {
        if !self.tasks.contains_key(task) {
            return Err(WorkflowGraphError::UnknownTask(task.clone()));
        }
        if !self.tasks.contains_key(dependency) {
            return Err(WorkflowGraphError::UnknownTask(dependency.clone()));
        }
        let inserted = self
            .dependencies
            .entry(task.clone())
            .or_default()
            .insert(dependency.clone());
        if inserted {
            self.revision = self.revision.next();
        }
        Ok(())
    }

    /// Current topology revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Direct task dependencies in deterministic order.
    #[must_use]
    pub fn dependencies(&self, task: &Id) -> Vec<&Id> {
        self.dependencies
            .get(task)
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Errors produced while editing a workflow topology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowGraphError {
    /// An edge referenced a task that is not registered.
    UnknownTask(Id),
}

impl fmt::Display for WorkflowGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTask(id) => write!(f, "unknown task: {id}"),
        }
    }
}

impl std::error::Error for WorkflowGraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        Task {
            id: Id::new(id).expect("test id is valid"),
            label: id.to_owned(),
        }
    }

    #[test]
    fn topology_changes_advance_revision() {
        let mut graph = WorkflowGraph::default();
        let plan = task("plan");
        let execute = task("execute");

        graph.upsert_task(plan.clone());
        let after_first_task = graph.revision();
        graph.upsert_task(execute.clone());
        graph
            .depends_on(&execute.id, &plan.id)
            .expect("edge is valid");

        assert!(graph.revision() > after_first_task);
        assert_eq!(graph.dependencies(&execute.id), vec![&plan.id]);
    }
}
