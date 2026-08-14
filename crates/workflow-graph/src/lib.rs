//! Versioned workflow topology and immutable execution-fact experiments.

use graph_core::{Id, Revision};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Immutable task definition in the workflow topology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    /// Stable logical task identity.
    pub id: Id,
    /// Human-readable task description.
    pub label: String,
}

/// The mutable definition/topology portion of a workflow.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkflowTopology {
    tasks: BTreeMap<Id, Task>,
    /// Incoming dependencies keyed by task. Each dependency must complete
    /// before the keyed task can complete.
    dependencies: BTreeMap<Id, BTreeSet<Id>>,
}

impl WorkflowTopology {
    /// Returns the task with `id`, if it exists.
    #[must_use]
    pub fn task(&self, id: &Id) -> Option<&Task> {
        self.tasks.get(id)
    }

    /// Returns all tasks in deterministic identifier order.
    #[must_use]
    pub fn tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    /// Returns all task identities in deterministic order.
    #[must_use]
    pub fn task_ids(&self) -> Vec<&Id> {
        self.tasks.keys().collect()
    }

    /// Returns the direct prerequisites of `task` in deterministic order.
    #[must_use]
    pub fn dependencies(&self, task: &Id) -> Vec<&Id> {
        self.dependencies.get(task).into_iter().flatten().collect()
    }

    /// Returns every `(task, dependency)` edge in deterministic order.
    #[must_use]
    pub fn edges(&self) -> Vec<(Id, Id)> {
        self.dependencies
            .iter()
            .flat_map(|(task, dependencies)| {
                dependencies
                    .iter()
                    .map(|dependency| (task.clone(), dependency.clone()))
            })
            .collect()
    }

    fn contains_task(&self, id: &Id) -> bool {
        self.tasks.contains_key(id)
    }

    fn cycle_path(&self) -> Option<Vec<Id>> {
        let mut marks = BTreeMap::new();
        let mut stack = Vec::new();

        for id in self.tasks.keys() {
            if !marks.contains_key(id) {
                if let Some(path) = self.visit(id, &mut marks, &mut stack) {
                    return Some(path);
                }
            }
        }

        None
    }

    fn visit(
        &self,
        id: &Id,
        marks: &mut BTreeMap<Id, VisitState>,
        stack: &mut Vec<Id>,
    ) -> Option<Vec<Id>> {
        marks.insert(id.clone(), VisitState::Active);
        stack.push(id.clone());

        for dependency in self.dependencies.get(id).into_iter().flatten() {
            match marks.get(dependency).copied() {
                None => {
                    if let Some(path) = self.visit(dependency, marks, stack) {
                        return Some(path);
                    }
                }
                Some(VisitState::Active) => {
                    let start = stack
                        .iter()
                        .position(|candidate| candidate == dependency)
                        .expect("active dependency must be on the DFS stack");
                    let mut path = stack[start..].to_vec();
                    path.push(dependency.clone());
                    return Some(path);
                }
                Some(VisitState::Complete) => {}
            }
        }

        stack.pop();
        marks.insert(id.clone(), VisitState::Complete);
        None
    }
}

/// Execution facts kept separately from the workflow topology.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionFacts {
    completed: BTreeSet<Id>,
    completion_log: Vec<CompletionRecord>,
}

impl ExecutionFacts {
    /// Returns completed task identities in deterministic order.
    #[must_use]
    pub fn completed_tasks(&self) -> Vec<&Id> {
        self.completed.iter().collect()
    }

    /// Returns whether `task` is an immutable completed fact.
    #[must_use]
    pub fn is_completed(&self, task: &Id) -> bool {
        self.completed.contains(task)
    }

    /// Returns the independent typed completion fact log in execution order.
    #[must_use]
    pub fn completion_log(&self) -> &[CompletionRecord] {
        &self.completion_log
    }
}

/// A typed topology operation proposed by a planner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowMutation {
    /// Adds a task with a new immutable logical identity.
    AddTask {
        /// Task definition to add.
        task: Task,
    },
    /// Adds an incoming dependency to an existing future task.
    AddDependency {
        /// Task that will wait for `dependency_id`.
        task_id: Id,
        /// Existing task that must complete first.
        dependency_id: Id,
    },
}

/// An ordered, typed, atomic set of topology operations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MutationBatch {
    mutations: Vec<WorkflowMutation>,
}

impl MutationBatch {
    /// Creates an empty mutation batch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mutations: Vec::new(),
        }
    }

    /// Creates a batch from an ordered mutation iterator.
    #[must_use]
    pub fn from_mutations(mutations: impl IntoIterator<Item = WorkflowMutation>) -> Self {
        Self {
            mutations: mutations.into_iter().collect(),
        }
    }

    /// Appends one typed operation to this batch.
    pub fn push(&mut self, mutation: WorkflowMutation) {
        self.mutations.push(mutation);
    }

    /// Returns the number of operations in this batch.
    #[must_use]
    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    /// Returns whether this batch contains no operations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    /// Returns the ordered operations without exposing mutable graph state.
    #[must_use]
    pub fn as_slice(&self) -> &[WorkflowMutation] {
        &self.mutations
    }
}

impl From<Vec<WorkflowMutation>> for MutationBatch {
    fn from(mutations: Vec<WorkflowMutation>) -> Self {
        Self { mutations }
    }
}

impl<const N: usize> From<[WorkflowMutation; N]> for MutationBatch {
    fn from(mutations: [WorkflowMutation; N]) -> Self {
        Self::from_mutations(mutations)
    }
}

impl From<&MutationBatch> for MutationBatch {
    fn from(batch: &MutationBatch) -> Self {
        batch.clone()
    }
}

impl From<&[WorkflowMutation]> for MutationBatch {
    fn from(mutations: &[WorkflowMutation]) -> Self {
        Self::from_mutations(mutations.iter().cloned())
    }
}

impl FromIterator<WorkflowMutation> for MutationBatch {
    fn from_iter<T: IntoIterator<Item = WorkflowMutation>>(iter: T) -> Self {
        Self::from_mutations(iter)
    }
}

/// The durable-in-shape record of one successful topology transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowMutationRecord {
    /// Topology revision read by the planner before applying the batch.
    pub base_revision: Revision,
    /// Topology revision after the complete batch was committed.
    pub resulting_revision: Revision,
    /// Ordered typed operations that formed this transition.
    pub mutations: Vec<WorkflowMutation>,
}

/// A typed execution fact for replaying completed tasks separately from topology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRecord {
    /// Identity of the task whose completion was recorded.
    pub task_id: Id,
}

/// Versioned workflow state containing separate topology and execution facts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkflowGraph {
    topology: WorkflowTopology,
    facts: ExecutionFacts,
    topology_revision: Revision,
    mutation_log: Vec<WorkflowMutationRecord>,
}

/// Domain-oriented alias for the aggregate represented by [`WorkflowGraph`].
pub type WorkflowState = WorkflowGraph;

impl WorkflowGraph {
    /// Returns the immutable workflow topology view.
    #[must_use]
    pub const fn topology(&self) -> &WorkflowTopology {
        &self.topology
    }

    /// Returns the immutable execution-fact view.
    #[must_use]
    pub const fn facts(&self) -> &ExecutionFacts {
        &self.facts
    }

    /// Returns all tasks in deterministic identifier order.
    #[must_use]
    pub fn tasks(&self) -> Vec<&Task> {
        self.topology.tasks()
    }

    /// Returns the task with `id`, if it exists.
    #[must_use]
    pub fn task(&self, id: &Id) -> Option<&Task> {
        self.topology.task(id)
    }

    /// Returns the direct prerequisites of `task` in deterministic order.
    #[must_use]
    pub fn dependencies(&self, task: &Id) -> Vec<&Id> {
        self.topology.dependencies(task)
    }

    /// Returns all topology edges in deterministic order.
    #[must_use]
    pub fn edges(&self) -> Vec<(Id, Id)> {
        self.topology.edges()
    }

    /// Returns completed task identities in deterministic order.
    #[must_use]
    pub fn completed_tasks(&self) -> Vec<&Id> {
        self.facts.completed_tasks()
    }

    /// Returns whether `task` has been recorded as completed.
    #[must_use]
    pub fn is_completed(&self, task: &Id) -> bool {
        self.facts.is_completed(task)
    }

    /// Returns the independent execution fact log.
    #[must_use]
    pub fn completion_log(&self) -> &[CompletionRecord] {
        self.facts.completion_log()
    }

    /// Returns successful topology transitions in commit order.
    #[must_use]
    pub fn mutation_log(&self) -> &[WorkflowMutationRecord] {
        &self.mutation_log
    }

    /// Returns the current topology revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.topology_revision
    }

    /// Returns the current topology revision with an explicit domain name.
    #[must_use]
    pub const fn topology_revision(&self) -> Revision {
        self.topology_revision
    }

    /// Records a task completion without changing the topology revision.
    ///
    /// Every direct prerequisite must already be completed. The returned
    /// completion record can be replayed independently from topology records.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowGraphError::UnknownTask`] for an absent task,
    /// [`WorkflowGraphError::TaskAlreadyCompleted`] for a duplicate fact, or
    /// [`WorkflowGraphError::IncompletePrerequisite`] for an unfinished
    /// prerequisite.
    pub fn complete(&mut self, task_id: &Id) -> Result<CompletionRecord, WorkflowGraphError> {
        if !self.topology.contains_task(task_id) {
            return Err(WorkflowGraphError::UnknownTask(task_id.clone()));
        }
        if self.facts.is_completed(task_id) {
            return Err(WorkflowGraphError::TaskAlreadyCompleted(task_id.clone()));
        }

        for dependency in self.topology.dependencies(task_id) {
            if !self.facts.is_completed(dependency) {
                return Err(WorkflowGraphError::IncompletePrerequisite {
                    task: task_id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }

        let record = CompletionRecord {
            task_id: task_id.clone(),
        };
        self.facts.completed.insert(task_id.clone());
        self.facts.completion_log.push(record.clone());
        Ok(record)
    }

    /// Alias for [`Self::complete`] using execution-fact terminology.
    pub fn record_completed(
        &mut self,
        task_id: &Id,
    ) -> Result<CompletionRecord, WorkflowGraphError> {
        self.complete(task_id)
    }

    /// Returns tasks that are not completed and whose prerequisites are all
    /// completed, in deterministic identifier order.
    #[must_use]
    pub fn ready_tasks(&self) -> Vec<Id> {
        self.topology
            .tasks
            .keys()
            .filter(|task_id| !self.facts.is_completed(task_id))
            .filter(|task_id| {
                self.topology
                    .dependencies
                    .get(*task_id)
                    .is_none_or(|dependencies| {
                        dependencies
                            .iter()
                            .all(|dependency| self.facts.is_completed(dependency))
                    })
            })
            .cloned()
            .collect()
    }

    /// Applies a planner batch atomically at `expected_revision`.
    ///
    /// The batch is prepared against a clone, validated as a DAG, and only
    /// then committed. A successful batch creates one mutation record and
    /// advances the topology revision exactly once. Execution facts are not
    /// changed by topology mutations.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowGraphError::RevisionConflict`] when the planner's
    /// snapshot is stale. Other structured errors reject the candidate before
    /// any part of the batch is committed.
    pub fn apply_batch<B>(
        &mut self,
        expected_revision: Revision,
        batch: B,
    ) -> Result<WorkflowMutationRecord, WorkflowGraphError>
    where
        B: Into<MutationBatch>,
    {
        if expected_revision != self.topology_revision {
            return Err(WorkflowGraphError::RevisionConflict {
                expected: expected_revision,
                actual: self.topology_revision,
            });
        }

        let batch = batch.into();
        if batch.is_empty() {
            return Err(WorkflowGraphError::EmptyMutationBatch);
        }

        let mut candidate = self.clone();
        for mutation in batch.as_slice() {
            candidate.apply_mutation(mutation)?;
        }

        if let Some(path) = candidate.topology.cycle_path() {
            return Err(WorkflowGraphError::Cycle { path });
        }

        let resulting_revision = self.topology_revision.next();
        let record = WorkflowMutationRecord {
            base_revision: self.topology_revision,
            resulting_revision,
            mutations: batch.as_slice().to_vec(),
        };
        candidate.topology_revision = resulting_revision;
        candidate.mutation_log.push(record.clone());
        *self = candidate;
        Ok(record)
    }

    /// Replays topology mutation records from an empty workflow.
    ///
    /// Completion facts are intentionally not inferred from topology records;
    /// use [`Self::replay_with_facts`] when scheduler view must include them.
    pub fn replay(records: &[WorkflowMutationRecord]) -> Result<Self, WorkflowGraphError> {
        let mut graph = Self::default();
        for record in records {
            if record.base_revision != graph.topology_revision {
                return Err(WorkflowGraphError::RevisionConflict {
                    expected: record.base_revision,
                    actual: graph.topology_revision,
                });
            }

            let expected_result = graph.topology_revision.next();
            if record.resulting_revision != expected_result {
                return Err(WorkflowGraphError::ReplayRevisionMismatch {
                    expected: expected_result,
                    actual: record.resulting_revision,
                });
            }

            graph.apply_batch(record.base_revision, record.mutations.clone())?;
        }
        Ok(graph)
    }

    /// Replays topology records and then applies an independent completion log.
    pub fn replay_with_facts(
        records: &[WorkflowMutationRecord],
        completions: &[CompletionRecord],
    ) -> Result<Self, WorkflowGraphError> {
        let mut graph = Self::replay(records)?;
        for completion in completions {
            graph.complete(&completion.task_id)?;
        }
        Ok(graph)
    }

    fn apply_mutation(&mut self, mutation: &WorkflowMutation) -> Result<(), WorkflowGraphError> {
        match mutation {
            WorkflowMutation::AddTask { task } => {
                if self.topology.contains_task(&task.id) {
                    return Err(WorkflowGraphError::DuplicateTask(task.id.clone()));
                }
                self.topology.tasks.insert(task.id.clone(), task.clone());
                Ok(())
            }
            WorkflowMutation::AddDependency {
                task_id,
                dependency_id,
            } => {
                if !self.topology.contains_task(task_id) {
                    return Err(WorkflowGraphError::UnknownTask(task_id.clone()));
                }
                if !self.topology.contains_task(dependency_id) {
                    return Err(WorkflowGraphError::UnknownTask(dependency_id.clone()));
                }
                if self.facts.is_completed(task_id) {
                    return Err(WorkflowGraphError::CompletedTaskMutation(task_id.clone()));
                }

                let dependencies = self
                    .topology
                    .dependencies
                    .entry(task_id.clone())
                    .or_default();
                if !dependencies.insert(dependency_id.clone()) {
                    return Err(WorkflowGraphError::DuplicateDependency {
                        task: task_id.clone(),
                        dependency: dependency_id.clone(),
                    });
                }

                if let Some(path) = self.topology.cycle_path() {
                    let dependencies = self
                        .topology
                        .dependencies
                        .get_mut(task_id)
                        .expect("inserted dependency owner must exist");
                    dependencies.remove(dependency_id);
                    if dependencies.is_empty() {
                        self.topology.dependencies.remove(task_id);
                    }
                    return Err(WorkflowGraphError::Cycle { path });
                }

                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisitState {
    Active,
    Complete,
}

/// Structured reasons for rejecting a workflow mutation or fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowGraphError {
    /// A mutation or fact referenced an absent task.
    UnknownTask(Id),
    /// A task identity already exists and cannot be rewritten.
    DuplicateTask(Id),
    /// An identical dependency edge already exists.
    DuplicateDependency {
        /// Task that already has the edge.
        task: Id,
        /// Duplicate prerequisite.
        dependency: Id,
    },
    /// A dependency would make the workflow cyclic.
    Cycle {
        /// Deterministic cycle path, including the repeated start identity.
        path: Vec<Id>,
    },
    /// A topology operation attempted to change a completed task's history.
    CompletedTaskMutation(Id),
    /// A task was marked complete more than once.
    TaskAlreadyCompleted(Id),
    /// A task was completed before one of its prerequisites.
    IncompletePrerequisite {
        /// Task being completed.
        task: Id,
        /// Prerequisite that is not complete.
        dependency: Id,
    },
    /// A planner submitted a batch against an old topology revision.
    RevisionConflict {
        /// Revision observed by the planner.
        expected: Revision,
        /// Revision currently held by the workflow.
        actual: Revision,
    },
    /// A batch contained no topology transition.
    EmptyMutationBatch,
    /// A replay record's resulting revision was not the next revision.
    ReplayRevisionMismatch {
        /// Revision required by the replay sequence.
        expected: Revision,
        /// Revision stored in the record.
        actual: Revision,
    },
}

impl fmt::Display for WorkflowGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTask(id) => write!(f, "unknown task: {id}"),
            Self::DuplicateTask(id) => write!(f, "duplicate task identity: {id}"),
            Self::DuplicateDependency { task, dependency } => write!(
                f,
                "duplicate dependency: {task} already depends on {dependency}"
            ),
            Self::Cycle { path } => {
                let path = path
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "workflow dependency cycle: {path}")
            }
            Self::CompletedTaskMutation(id) => write!(
                f,
                "completed task cannot have its definition or prerequisites changed: {id}"
            ),
            Self::TaskAlreadyCompleted(id) => write!(f, "task is already completed: {id}"),
            Self::IncompletePrerequisite { task, dependency } => write!(
                f,
                "cannot complete {task}; prerequisite is incomplete: {dependency}"
            ),
            Self::RevisionConflict { expected, actual } => write!(
                f,
                "workflow topology revision conflict: expected {}, actual {}",
                expected.get(),
                actual.get()
            ),
            Self::EmptyMutationBatch => f.write_str("workflow mutation batch is empty"),
            Self::ReplayRevisionMismatch { expected, actual } => write!(
                f,
                "workflow replay revision mismatch: expected {}, record has {}",
                expected.get(),
                actual.get()
            ),
        }
    }
}

impl std::error::Error for WorkflowGraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("test id is valid")
    }

    fn task(value: &str) -> Task {
        Task {
            id: id(value),
            label: value.to_owned(),
        }
    }

    fn add_task(value: &str) -> WorkflowMutation {
        WorkflowMutation::AddTask { task: task(value) }
    }

    fn add_dependency(task_id: &str, dependency_id: &str) -> WorkflowMutation {
        WorkflowMutation::AddDependency {
            task_id: id(task_id),
            dependency_id: id(dependency_id),
        }
    }

    fn apply(graph: &mut WorkflowGraph, mutations: Vec<WorkflowMutation>) {
        graph
            .apply_batch(graph.revision(), mutations)
            .expect("test mutation is valid");
    }

    fn plan_research_execute() -> (WorkflowGraph, Id, Id, Id) {
        let plan = id("plan");
        let research = id("research");
        let execute = id("execute");
        let mut graph = WorkflowGraph::default();
        apply(
            &mut graph,
            vec![
                add_task("plan"),
                add_task("research"),
                add_task("execute"),
                add_dependency("research", "plan"),
                add_dependency("execute", "research"),
            ],
        );
        (graph, plan, research, execute)
    }

    #[test]
    fn successful_mutation_advances_revision() {
        let mut graph = WorkflowGraph::default();
        let record = graph
            .apply_batch(Revision::ZERO, [add_task("plan")])
            .expect("task addition is valid");

        assert_eq!(record.base_revision, Revision::ZERO);
        assert_eq!(record.resulting_revision, Revision::ZERO.next());
        assert_eq!(graph.revision(), Revision::ZERO.next());
    }

    #[test]
    fn failed_mutation_does_not_advance_revision() {
        let mut graph = WorkflowGraph::default();
        apply(&mut graph, vec![add_task("plan")]);
        let before = graph.clone();

        let error = graph
            .apply_batch(graph.revision(), [add_dependency("execute", "plan")])
            .expect_err("unknown task must be rejected");

        assert_eq!(error, WorkflowGraphError::UnknownTask(id("execute")));
        assert_eq!(graph, before);
    }

    #[test]
    fn successful_batch_advances_revision_once() {
        let mut graph = WorkflowGraph::default();
        let record = graph
            .apply_batch(Revision::ZERO, [add_task("a"), add_task("b")])
            .expect("batch is valid");

        assert_eq!(record.mutations.len(), 2);
        assert_eq!(record.resulting_revision.get(), 1);
        assert_eq!(graph.revision().get(), 1);
    }

    #[test]
    fn dependency_cycle_is_rejected() {
        let mut graph = WorkflowGraph::default();
        apply(&mut graph, vec![add_task("a"), add_task("b")]);
        apply(&mut graph, vec![add_dependency("b", "a")]);
        let revision = graph.revision();

        let error = graph
            .apply_batch(revision, [add_dependency("a", "b")])
            .expect_err("cycle must be rejected");

        assert!(matches!(error, WorkflowGraphError::Cycle { .. }));
        assert_eq!(graph.revision(), revision);
        assert_eq!(graph.dependencies(&id("a")), Vec::<&Id>::new());
    }

    #[test]
    fn cycle_rejection_is_atomic() {
        let mut graph = WorkflowGraph::default();
        apply(
            &mut graph,
            vec![add_task("a"), add_task("b"), add_dependency("b", "a")],
        );
        let before = graph.clone();

        let error = graph
            .apply_batch(
                graph.revision(),
                [
                    add_task("c"),
                    add_dependency("a", "c"),
                    add_dependency("c", "b"),
                ],
            )
            .expect_err("candidate cycle must reject the full batch");

        assert!(matches!(error, WorkflowGraphError::Cycle { .. }));
        assert_eq!(graph, before);
    }

    #[test]
    fn cannot_complete_task_before_prerequisites() {
        let (mut graph, plan, research, _) = plan_research_execute();
        let revision = graph.revision();

        let error = graph
            .complete(&research)
            .expect_err("research needs plan first");

        assert_eq!(
            error,
            WorkflowGraphError::IncompletePrerequisite {
                task: research,
                dependency: plan,
            }
        );
        assert_eq!(graph.revision(), revision);
        assert!(graph.completed_tasks().is_empty());
    }

    #[test]
    fn completed_task_identity_cannot_be_rewritten() {
        let (mut graph, plan, _, _) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        let before = graph.clone();

        let error = graph
            .apply_batch(
                graph.revision(),
                [WorkflowMutation::AddTask {
                    task: Task {
                        id: plan.clone(),
                        label: "rewritten".to_owned(),
                    },
                }],
            )
            .expect_err("task identity is immutable");

        assert_eq!(error, WorkflowGraphError::DuplicateTask(plan));
        assert_eq!(graph, before);
    }

    #[test]
    fn completed_task_prerequisites_cannot_be_rewritten() {
        let (mut graph, plan, research, _) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        graph
            .complete(&research)
            .expect("research follows completed plan");
        let before = graph.clone();

        let error = graph
            .apply_batch(
                graph.revision(),
                [add_task("extra"), add_dependency("research", "extra")],
            )
            .expect_err("completed prerequisite set is frozen");

        assert_eq!(error, WorkflowGraphError::CompletedTaskMutation(research));
        assert_eq!(graph, before);
    }

    #[test]
    fn completed_fact_does_not_advance_topology_revision() {
        let (mut graph, plan, _, _) = plan_research_execute();
        let revision = graph.revision();

        graph.complete(&plan).expect("plan has no prerequisite");

        assert_eq!(graph.revision(), revision);
        assert!(graph.is_completed(&plan));
    }

    #[test]
    fn planner_can_append_future_branch() {
        let (mut graph, plan, research, execute) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        graph.complete(&research).expect("research follows plan");
        let before = graph.revision();

        graph
            .apply_batch(
                before,
                [
                    add_task("validate"),
                    add_task("review"),
                    add_dependency("validate", "research"),
                    add_dependency("review", "research"),
                    add_dependency("execute", "validate"),
                    add_dependency("execute", "review"),
                ],
            )
            .expect("future branch is valid");

        assert_eq!(graph.revision(), before.next());
        assert!(graph.is_completed(&plan));
        assert!(graph.is_completed(&research));
        assert!(!graph.is_completed(&execute));
        assert_eq!(graph.ready_tasks(), vec![id("review"), id("validate")]);
    }

    #[test]
    fn mutation_batch_is_atomic() {
        let (mut graph, plan, research, _) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        graph.complete(&research).expect("research follows plan");
        let before = graph.clone();

        let error = graph
            .apply_batch(
                graph.revision(),
                [add_task("validate"), add_dependency("missing", "research")],
            )
            .expect_err("invalid operation rejects the batch");

        assert_eq!(error, WorkflowGraphError::UnknownTask(id("missing")));
        assert_eq!(graph, before);
    }

    #[test]
    fn invalid_operation_rolls_back_entire_batch() {
        let mut graph = WorkflowGraph::default();
        apply(&mut graph, vec![add_task("a"), add_task("b")]);
        let revision = graph.revision();

        let error = graph
            .apply_batch(
                revision,
                [add_dependency("b", "a"), add_dependency("b", "a")],
            )
            .expect_err("duplicate operation rejects the batch");

        assert_eq!(
            error,
            WorkflowGraphError::DuplicateDependency {
                task: id("b"),
                dependency: id("a"),
            }
        );
        assert_eq!(graph.revision(), revision);
        assert!(graph.edges().is_empty());
    }

    #[test]
    fn stale_planner_revision_is_rejected() {
        let mut graph = WorkflowGraph::default();
        apply(&mut graph, vec![add_task("a")]);
        let planner_revision = graph.revision();
        apply(&mut graph, vec![add_task("b")]);
        let before = graph.clone();

        let error = graph
            .apply_batch(planner_revision, [add_task("c")])
            .expect_err("stale planner must not overwrite newer topology");

        assert_eq!(
            error,
            WorkflowGraphError::RevisionConflict {
                expected: planner_revision,
                actual: before.revision(),
            }
        );
        assert_eq!(graph, before);
    }

    #[test]
    fn replay_reconstructs_same_topology() {
        let (mut graph, plan, research, _) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        graph.complete(&research).expect("research follows plan");
        apply(&mut graph, vec![add_task("validate"), add_task("review")]);

        let replayed = WorkflowGraph::replay(graph.mutation_log()).expect("records replay");

        assert_eq!(replayed.topology(), graph.topology());
        assert_eq!(replayed.edges(), graph.edges());
        assert!(replayed.completed_tasks().is_empty());
    }

    #[test]
    fn replay_reconstructs_same_revision() {
        let (mut graph, _, _, _) = plan_research_execute();
        apply(&mut graph, vec![add_task("validate")]);

        let replayed = WorkflowGraph::replay(graph.mutation_log()).expect("records replay");

        assert_eq!(replayed.revision(), graph.revision());
    }

    #[test]
    fn replay_reconstructs_same_scheduler_view() {
        let (mut graph, plan, research, execute) = plan_research_execute();
        graph.complete(&plan).expect("plan has no prerequisite");
        graph.complete(&research).expect("research follows plan");
        apply(
            &mut graph,
            vec![
                add_task("validate"),
                add_task("review"),
                add_dependency("validate", "research"),
                add_dependency("review", "research"),
                add_dependency("execute", "validate"),
                add_dependency("execute", "review"),
            ],
        );

        let replayed =
            WorkflowGraph::replay_with_facts(graph.mutation_log(), graph.completion_log())
                .expect("topology and facts replay");

        assert_eq!(replayed.topology(), graph.topology());
        assert_eq!(replayed.facts(), graph.facts());
        assert_eq!(replayed.revision(), graph.revision());
        assert_eq!(replayed.ready_tasks(), graph.ready_tasks());
        assert!(!replayed.ready_tasks().contains(&execute));
    }

    #[test]
    fn ready_tasks_are_deterministic() {
        let mut graph = WorkflowGraph::default();
        apply(
            &mut graph,
            vec![
                add_task("c"),
                add_task("a"),
                add_task("b"),
                add_dependency("c", "a"),
            ],
        );
        graph.complete(&id("a")).expect("a has no prerequisite");

        assert_eq!(graph.ready_tasks(), vec![id("b"), id("c")]);
    }

    #[test]
    fn completed_tasks_are_not_ready() {
        let mut graph = WorkflowGraph::default();
        apply(&mut graph, vec![add_task("a")]);
        graph.complete(&id("a")).expect("a has no prerequisite");

        assert!(graph.ready_tasks().is_empty());
    }

    #[test]
    fn task_is_ready_only_after_all_prerequisites_complete() {
        let mut graph = WorkflowGraph::default();
        apply(
            &mut graph,
            vec![
                add_task("a"),
                add_task("b"),
                add_task("c"),
                add_dependency("c", "a"),
                add_dependency("c", "b"),
            ],
        );
        graph.complete(&id("a")).expect("a has no prerequisite");
        assert!(!graph.ready_tasks().contains(&id("c")));

        graph.complete(&id("b")).expect("b has no prerequisite");
        assert_eq!(graph.ready_tasks(), vec![id("c")]);
    }

    #[test]
    fn insertion_order_does_not_change_topology_or_ready_view() {
        let mut first = WorkflowGraph::default();
        apply(
            &mut first,
            vec![
                add_task("a"),
                add_task("b"),
                add_task("c"),
                add_dependency("c", "a"),
                add_dependency("c", "b"),
            ],
        );
        first.complete(&id("a")).expect("a has no prerequisite");
        first.complete(&id("b")).expect("b has no prerequisite");

        let mut second = WorkflowGraph::default();
        apply(
            &mut second,
            vec![
                add_task("c"),
                add_task("b"),
                add_task("a"),
                add_dependency("c", "b"),
                add_dependency("c", "a"),
            ],
        );
        second.complete(&id("a")).expect("a has no prerequisite");
        second.complete(&id("b")).expect("b has no prerequisite");

        assert_eq!(first.topology(), second.topology());
        assert_eq!(first.edges(), second.edges());
        assert_eq!(first.ready_tasks(), second.ready_tasks());
    }
}
