//! Capability dependency and scope ownership experiments.

use graph_core::Id;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Stable identifier for a capability.
pub type CapabilityId = Id;

/// Capability metadata kept for compatibility with the initial baseline API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    /// Stable capability identifier.
    pub id: CapabilityId,
    /// Human-readable capability kind such as `model` or `service`.
    pub kind: String,
}

impl Capability {
    /// Creates a capability with no declared dependencies.
    #[must_use]
    pub fn new(id: CapabilityId, kind: impl Into<String>) -> Self {
        Self {
            id,
            kind: kind.into(),
        }
    }
}

/// A single declared capability dependency.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Dependency {
    /// Identifier of the capability being required.
    pub id: CapabilityId,
}

impl Dependency {
    /// Creates a dependency on `id`.
    #[must_use]
    pub fn new(id: CapabilityId) -> Self {
        Self { id }
    }
}

/// Capability metadata plus the dependencies required to construct it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDefinition {
    /// Stable capability identifier.
    pub id: CapabilityId,
    /// Human-readable capability kind such as `model` or `service`.
    pub kind: String,
    /// Required capabilities. A sorted set makes equivalent definitions stable.
    pub dependencies: BTreeSet<Dependency>,
}

impl CapabilityDefinition {
    /// Creates a capability definition with no dependencies.
    #[must_use]
    pub fn new(id: CapabilityId, kind: impl Into<String>) -> Self {
        Self {
            id,
            kind: kind.into(),
            dependencies: BTreeSet::new(),
        }
    }

    /// Returns this definition with one additional dependency.
    #[must_use]
    pub fn depends_on(mut self, dependency: CapabilityId) -> Self {
        self.dependencies.insert(Dependency::new(dependency));
        self
    }

    /// Adds a dependency to this definition.
    pub fn add_dependency(&mut self, dependency: CapabilityId) {
        self.dependencies.insert(Dependency::new(dependency));
    }
}

impl From<Capability> for CapabilityDefinition {
    fn from(capability: Capability) -> Self {
        Self::new(capability.id, capability.kind)
    }
}

/// In-memory capability dependency model.
#[derive(Clone, Debug, Default)]
pub struct CapabilityGraph {
    definitions: BTreeMap<CapabilityId, CapabilityDefinition>,
}

impl CapabilityGraph {
    /// Inserts or replaces a capability definition.
    ///
    /// Dependencies are checked by [`Self::resolve`], which allows callers to
    /// assemble definitions in any order. The returned value is the previous
    /// definition, if the identifier was already present.
    pub fn insert(
        &mut self,
        capability: impl Into<CapabilityDefinition>,
    ) -> Option<CapabilityDefinition> {
        let capability = capability.into();
        self.definitions.insert(capability.id.clone(), capability)
    }

    /// Declares that `consumer` requires `dependency`.
    ///
    /// # Errors
    ///
    /// Returns an error when either the consumer or dependency is not
    /// registered. Definitions built with [`CapabilityDefinition::depends_on`]
    /// are instead validated when [`Self::resolve`] is called.
    pub fn require(
        &mut self,
        consumer: &CapabilityId,
        dependency: &CapabilityId,
    ) -> Result<(), CapabilityGraphError> {
        if !self.definitions.contains_key(consumer) {
            return Err(CapabilityGraphError::UnknownCapability(consumer.clone()));
        }
        if !self.definitions.contains_key(dependency) {
            return Err(CapabilityGraphError::MissingDependency {
                capability: consumer.clone(),
                dependency: dependency.clone(),
            });
        }
        self.definitions
            .get_mut(consumer)
            .expect("consumer was checked above")
            .add_dependency(dependency.clone());
        Ok(())
    }

    /// Returns direct requirements in deterministic identifier order.
    #[must_use]
    pub fn requirements(&self, capability: &CapabilityId) -> Vec<&CapabilityId> {
        self.definitions
            .get(capability)
            .map_or_else(Vec::new, |definition| {
                definition
                    .dependencies
                    .iter()
                    .map(|dependency| &dependency.id)
                    .collect()
            })
    }

    /// Resolves all capabilities in dependency-first construction order.
    ///
    /// The traversal visits identifiers and dependencies in sorted order, so
    /// insertion order does not affect the result.
    pub fn resolve(&self) -> Result<ResolvedCapabilityGraph, CapabilityGraphError> {
        let mut states = BTreeMap::new();
        let mut active_path = Vec::new();
        let mut construction_order = Vec::with_capacity(self.definitions.len());

        for id in self.definitions.keys() {
            visit(
                &self.definitions,
                id,
                &mut states,
                &mut active_path,
                &mut construction_order,
            )?;
        }

        Ok(ResolvedCapabilityGraph { construction_order })
    }

    /// Returns the number of registered capabilities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    /// Returns whether the graph contains no capabilities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisitState {
    Active,
    Done,
}

fn visit(
    definitions: &BTreeMap<CapabilityId, CapabilityDefinition>,
    id: &CapabilityId,
    states: &mut BTreeMap<CapabilityId, VisitState>,
    active_path: &mut Vec<CapabilityId>,
    construction_order: &mut Vec<CapabilityId>,
) -> Result<(), CapabilityGraphError> {
    match states.get(id) {
        Some(VisitState::Done) => return Ok(()),
        Some(VisitState::Active) => {
            let start = active_path
                .iter()
                .position(|active_id| active_id == id)
                .expect("active capability must be on the active path");
            let mut path = active_path[start..].to_vec();
            path.push(id.clone());
            return Err(CapabilityGraphError::Cycle { path });
        }
        None => {}
    }

    let definition = definitions
        .get(id)
        .expect("visit is only called with a registered capability");
    states.insert(id.clone(), VisitState::Active);
    active_path.push(id.clone());

    for dependency in &definition.dependencies {
        if !definitions.contains_key(&dependency.id) {
            return Err(CapabilityGraphError::MissingDependency {
                capability: id.clone(),
                dependency: dependency.id.clone(),
            });
        }
        visit(
            definitions,
            &dependency.id,
            states,
            active_path,
            construction_order,
        )?;
    }

    active_path.pop();
    states.insert(id.clone(), VisitState::Done);
    construction_order.push(id.clone());
    Ok(())
}

/// A validated capability graph with deterministic lifecycle order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCapabilityGraph {
    construction_order: Vec<CapabilityId>,
}

impl ResolvedCapabilityGraph {
    /// Returns capability identifiers in dependency-first construction order.
    #[must_use]
    pub fn construction_order(&self) -> &[CapabilityId] {
        &self.construction_order
    }

    /// Returns capability identifiers in reverse construction order for teardown.
    #[must_use]
    pub fn teardown_order(&self) -> Vec<CapabilityId> {
        self.construction_order.iter().rev().cloned().collect()
    }
}

/// Errors produced while building or resolving a capability graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityGraphError {
    /// A graph operation referenced a capability that is not registered.
    UnknownCapability(CapabilityId),
    /// A capability refers to a dependency that is not registered.
    MissingDependency {
        /// Capability whose construction cannot proceed.
        capability: CapabilityId,
        /// Missing required capability.
        dependency: CapabilityId,
    },
    /// A dependency cycle, including the repeated starting node.
    Cycle {
        /// Cycle path such as `A -> B -> C -> A`.
        path: Vec<CapabilityId>,
    },
}

impl fmt::Display for CapabilityGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCapability(id) => write!(f, "unknown capability: {id}"),
            Self::MissingDependency {
                capability,
                dependency,
            } => write!(
                f,
                "capability {capability} requires missing dependency {dependency}"
            ),
            Self::Cycle { path } => write!(f, "capability dependency cycle: {}", join_ids(path)),
        }
    }
}

impl std::error::Error for CapabilityGraphError {}

/// A type-erased runtime capability resource.
pub trait CapabilityInstance: Send + Sync + 'static {
    /// Releases the resource owned by the instance.
    fn dispose(&self);

    /// Exposes the concrete value for typed inspection in experiments.
    fn as_any(&self) -> &dyn Any;
}

struct InstanceSlot {
    instance: Box<dyn CapabilityInstance>,
}

impl Drop for InstanceSlot {
    fn drop(&mut self) {
        self.instance.dispose();
    }
}

struct CapabilityEntry {
    definition: Arc<CapabilityDefinition>,
    instance: Arc<InstanceSlot>,
}

impl CapabilityEntry {
    fn handle(&self) -> CapabilityHandle {
        CapabilityHandle {
            definition: Arc::clone(&self.definition),
            instance: Arc::clone(&self.instance),
        }
    }
}

struct ScopeState {
    parent: Option<Scope>,
    entries: RwLock<BTreeMap<CapabilityId, CapabilityEntry>>,
    closed: AtomicBool,
}

/// A hierarchical scope that owns local capability instances.
///
/// Lookup walks from the current scope toward its parent. A child can replace
/// a capability locally without changing the parent's entry, and each handle
/// keeps its instance alive until the last reader releases it.
#[derive(Clone)]
pub struct Scope {
    state: Arc<ScopeState>,
}

impl Scope {
    /// Creates an empty root scope.
    #[must_use]
    pub fn root() -> Self {
        Self {
            state: Arc::new(ScopeState {
                parent: None,
                entries: RwLock::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// Creates an empty child scope inheriting from this scope.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            state: Arc::new(ScopeState {
                parent: Some(self.clone()),
                entries: RwLock::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// Looks up the nearest capability visible from this scope.
    #[must_use]
    pub fn get(&self, id: &CapabilityId) -> Option<CapabilityHandle> {
        if self.state.closed.load(Ordering::Acquire) {
            return None;
        }

        if let Some(entry) = self
            .state
            .entries
            .read()
            .expect("scope lock poisoned")
            .get(id)
        {
            return Some(entry.handle());
        }

        self.state.parent.as_ref()?.get(id)
    }

    /// Provides or replaces a local capability using a transactional factory.
    ///
    /// Dependencies are validated before the factory runs. The new resource
    /// is then constructed and atomically published under the local id. If
    /// construction fails, the previous local value remains installed.
    pub fn provide<F>(
        &self,
        definition: CapabilityDefinition,
        construct: F,
    ) -> Result<CapabilityHandle, ScopeError>
    where
        F: FnOnce() -> Result<Box<dyn CapabilityInstance>, String>,
    {
        self.validate_dependencies(&definition)?;
        let instance = construct().map_err(|reason| ScopeError::ConstructionFailed {
            capability: definition.id.clone(),
            reason,
        })?;
        let definition = Arc::new(definition);
        let instance = Arc::new(InstanceSlot { instance });

        let mut entries = self.state.entries.write().expect("scope lock poisoned");
        if self.state.closed.load(Ordering::Acquire) {
            drop(entries);
            drop(instance);
            return Err(ScopeError::Closed);
        }

        let id = definition.id.clone();
        let handle = CapabilityHandle {
            definition: Arc::clone(&definition),
            instance: Arc::clone(&instance),
        };
        let old = entries.insert(
            id,
            CapabilityEntry {
                definition,
                instance,
            },
        );
        drop(entries);
        drop(old);
        Ok(handle)
    }

    /// Replaces or adds a local capability with the same transactional
    /// semantics as [`Self::provide`].
    pub fn replace<F>(
        &self,
        definition: CapabilityDefinition,
        construct: F,
    ) -> Result<CapabilityHandle, ScopeError>
    where
        F: FnOnce() -> Result<Box<dyn CapabilityInstance>, String>,
    {
        self.provide(definition, construct)
    }

    /// Tears down this scope's locally owned capabilities.
    ///
    /// Parent capabilities are not touched. The operation is idempotent. A
    /// capability instance with in-flight readers is disposed when the last
    /// reader releases its [`CapabilityHandle`].
    pub fn teardown(&self) {
        if self.state.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let entries =
            std::mem::take(&mut *self.state.entries.write().expect("scope lock poisoned"));
        drop(entries);
    }

    fn validate_dependencies(&self, definition: &CapabilityDefinition) -> Result<(), ScopeError> {
        if self.state.closed.load(Ordering::Acquire) {
            return Err(ScopeError::Closed);
        }
        for dependency in &definition.dependencies {
            if self.get(&dependency.id).is_none() {
                return Err(ScopeError::MissingDependency {
                    capability: definition.id.clone(),
                    dependency: dependency.id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// A reader-owned reference to a capability instance.
#[derive(Clone)]
pub struct CapabilityHandle {
    definition: Arc<CapabilityDefinition>,
    instance: Arc<InstanceSlot>,
}

impl fmt::Debug for CapabilityHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityHandle")
            .field("id", &self.id())
            .field("kind", &self.kind())
            .finish()
    }
}

impl CapabilityHandle {
    /// Returns the capability identifier.
    #[must_use]
    pub fn id(&self) -> &CapabilityId {
        &self.definition.id
    }

    /// Returns the capability kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.definition.kind
    }

    /// Returns the type-erased runtime instance.
    #[must_use]
    pub fn instance(&self) -> &dyn CapabilityInstance {
        self.instance.instance.as_ref()
    }

    /// Returns the concrete instance when it has type `T`.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.instance().as_any().downcast_ref()
    }
}

/// Errors produced while publishing a scoped capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeError {
    /// A required capability is not visible from the scope being changed.
    MissingDependency {
        /// Capability whose construction cannot proceed.
        capability: CapabilityId,
        /// Missing required capability.
        dependency: CapabilityId,
    },
    /// The factory rejected construction of the replacement.
    ConstructionFailed {
        /// Capability that failed to construct.
        capability: CapabilityId,
        /// Factory-provided failure description.
        reason: String,
    },
    /// The scope has already been torn down.
    Closed,
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDependency {
                capability,
                dependency,
            } => write!(
                f,
                "capability {capability} requires missing dependency {dependency}"
            ),
            Self::ConstructionFailed { capability, reason } => {
                write!(f, "failed to construct capability {capability}: {reason}")
            }
            Self::Closed => f.write_str("scope is already closed"),
        }
    }
}

impl std::error::Error for ScopeError {}

fn join_ids(ids: &[CapabilityId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn id(value: &str) -> CapabilityId {
        Id::new(value).expect("test id is valid")
    }

    fn capability(value: &str) -> Capability {
        Capability::new(id(value), "service")
    }

    fn definition(value: &str) -> CapabilityDefinition {
        CapabilityDefinition::new(id(value), "service")
    }

    fn labels(ids: &[CapabilityId]) -> Vec<&str> {
        ids.iter().map(Id::as_str).collect()
    }

    #[test]
    fn resolve_simple_dependency() {
        let mut graph = CapabilityGraph::default();
        graph.insert(capability("a"));
        graph.insert(capability("b"));
        graph.require(&id("a"), &id("b")).expect("edge is valid");

        let resolved = graph.resolve().expect("graph is valid");
        assert_eq!(labels(resolved.construction_order()), vec!["b", "a"]);
    }

    #[test]
    fn resolve_multi_level_dependency() {
        let mut graph = CapabilityGraph::default();
        graph.insert(definition("a").depends_on(id("b")));
        graph.insert(definition("b").depends_on(id("c")));
        graph.insert(definition("c"));

        let resolved = graph.resolve().expect("graph is valid");
        assert_eq!(labels(resolved.construction_order()), vec!["c", "b", "a"]);
        assert_eq!(labels(&resolved.teardown_order()), vec!["a", "b", "c"]);
    }

    #[test]
    fn resolution_is_deterministic() {
        let mut forward = CapabilityGraph::default();
        forward.insert(definition("a").depends_on(id("b")).depends_on(id("c")));
        forward.insert(definition("b").depends_on(id("d")));
        forward.insert(definition("c").depends_on(id("d")));
        forward.insert(definition("d"));

        let mut reverse = CapabilityGraph::default();
        reverse.insert(definition("d"));
        reverse.insert(definition("c").depends_on(id("d")));
        reverse.insert(definition("b").depends_on(id("d")));
        reverse.insert(definition("a").depends_on(id("c")).depends_on(id("b")));

        assert_eq!(
            forward.resolve().expect("graph is valid"),
            reverse.resolve().expect("graph is valid")
        );
    }

    #[test]
    fn missing_dependency_is_rejected() {
        let mut graph = CapabilityGraph::default();
        graph.insert(definition("a").depends_on(id("missing")));

        assert_eq!(
            graph.resolve(),
            Err(CapabilityGraphError::MissingDependency {
                capability: id("a"),
                dependency: id("missing"),
            })
        );
    }

    #[test]
    fn cycle_is_rejected() {
        let mut graph = CapabilityGraph::default();
        graph.insert(definition("a").depends_on(id("b")));
        graph.insert(definition("b").depends_on(id("c")));
        graph.insert(definition("c").depends_on(id("a")));

        assert!(matches!(
            graph.resolve(),
            Err(CapabilityGraphError::Cycle { .. })
        ));
    }

    #[test]
    fn cycle_error_contains_path() {
        let mut graph = CapabilityGraph::default();
        graph.insert(definition("a").depends_on(id("b")));
        graph.insert(definition("b").depends_on(id("c")));
        graph.insert(definition("c").depends_on(id("a")));

        let error = graph.resolve().expect_err("cycle must be rejected");
        assert_eq!(
            error,
            CapabilityGraphError::Cycle {
                path: vec![id("a"), id("b"), id("c"), id("a")],
            }
        );
        assert_eq!(
            error.to_string(),
            "capability dependency cycle: a -> b -> c -> a"
        );
    }

    #[test]
    fn teardown_order_is_reverse_resolution_order() {
        let mut graph = CapabilityGraph::default();
        graph.insert(definition("a").depends_on(id("b")));
        graph.insert(definition("b").depends_on(id("c")));
        graph.insert(definition("c"));

        let resolved = graph.resolve().expect("graph is valid");
        assert_eq!(labels(&resolved.teardown_order()), vec!["a", "b", "c"]);
    }

    #[derive(Debug)]
    struct TestInstance {
        label: &'static str,
        disposed: Arc<AtomicUsize>,
    }

    impl CapabilityInstance for TestInstance {
        fn dispose(&self) {
            self.disposed.fetch_add(1, Ordering::SeqCst);
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn instance(label: &'static str, disposed: &Arc<AtomicUsize>) -> Box<dyn CapabilityInstance> {
        Box::new(TestInstance {
            label,
            disposed: Arc::clone(disposed),
        })
    }

    fn label(handle: &CapabilityHandle) -> &'static str {
        handle
            .downcast_ref::<TestInstance>()
            .expect("test instance type")
            .label
    }

    #[test]
    fn child_inherits_parent_capability() {
        let root = Scope::root();
        let disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), || Ok(instance("openai", &disposed)))
            .expect("root capability constructs");
        let child = root.child();

        assert_eq!(
            label(&child.get(&id("model")).expect("inherited model")),
            "openai"
        );
    }

    #[test]
    fn child_override_does_not_mutate_parent() {
        let root = Scope::root();
        let root_disposed = Arc::new(AtomicUsize::new(0));
        let child_disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), || {
            Ok(instance("openai", &root_disposed))
        })
        .expect("root capability constructs");
        let child = root.child();
        child
            .provide(definition("model"), || {
                Ok(instance("deepseek", &child_disposed))
            })
            .expect("child override constructs");

        assert_eq!(
            label(&root.get(&id("model")).expect("root model")),
            "openai"
        );
        assert_eq!(
            label(&child.get(&id("model")).expect("child model")),
            "deepseek"
        );
        assert_eq!(root_disposed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sibling_scope_isolation() {
        let root = Scope::root();
        let root_disposed = Arc::new(AtomicUsize::new(0));
        let first_disposed = Arc::new(AtomicUsize::new(0));
        let second_disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), || {
            Ok(instance("openai", &root_disposed))
        })
        .expect("root capability constructs");
        let first = root.child();
        let second = root.child();
        first
            .provide(definition("model"), || {
                Ok(instance("deepseek", &first_disposed))
            })
            .expect("first override constructs");
        second
            .provide(definition("model"), || {
                Ok(instance("anthropic", &second_disposed))
            })
            .expect("second override constructs");

        assert_eq!(
            label(&first.get(&id("model")).expect("first model")),
            "deepseek"
        );
        assert_eq!(
            label(&second.get(&id("model")).expect("second model")),
            "anthropic"
        );
        assert_eq!(
            label(&root.get(&id("model")).expect("root model")),
            "openai"
        );
        assert_eq!(root_disposed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn replacement_changes_new_reads() {
        let scope = Scope::root();
        let first_disposed = Arc::new(AtomicUsize::new(0));
        let second_disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), || Ok(instance("v1", &first_disposed)))
            .expect("v1 constructs");
        scope
            .replace(definition("model"), || Ok(instance("v2", &second_disposed)))
            .expect("v2 constructs");

        assert_eq!(
            label(&scope.get(&id("model")).expect("current model")),
            "v2"
        );
        assert_eq!(first_disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn in_flight_reader_survives_replacement() {
        let scope = Scope::root();
        let first_disposed = Arc::new(AtomicUsize::new(0));
        let second_disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), || Ok(instance("v1", &first_disposed)))
            .expect("v1 constructs");
        let reader = scope.get(&id("model")).expect("reader gets v1");
        let barrier = Arc::new(Barrier::new(2));
        let reader_barrier = Arc::clone(&barrier);
        let reader_thread = std::thread::spawn(move || {
            assert_eq!(label(&reader), "v1");
            reader_barrier.wait();
            assert_eq!(label(&reader), "v1");
            reader
        });
        barrier.wait();

        scope
            .replace(definition("model"), || Ok(instance("v2", &second_disposed)))
            .expect("v2 constructs");
        assert_eq!(
            label(&scope.get(&id("model")).expect("new reader gets v2")),
            "v2"
        );
        assert_eq!(first_disposed.load(Ordering::SeqCst), 0);

        drop(reader_thread.join().expect("reader thread completes"));
        assert_eq!(first_disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_replacement_keeps_old_capability() {
        let scope = Scope::root();
        let disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), || Ok(instance("v1", &disposed)))
            .expect("v1 constructs");

        let error = scope
            .replace(definition("model"), || {
                Err::<Box<dyn CapabilityInstance>, _>("constructor failed".to_owned())
            })
            .expect_err("replacement must fail");

        assert_eq!(
            error,
            ScopeError::ConstructionFailed {
                capability: id("model"),
                reason: "constructor failed".to_owned(),
            }
        );
        assert_eq!(
            label(&scope.get(&id("model")).expect("old model remains")),
            "v1"
        );
        assert_eq!(disposed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn child_teardown_disposes_owned_capabilities() {
        let root = Scope::root();
        let child = root.child();
        let disposed = Arc::new(AtomicUsize::new(0));
        let child_value = child
            .provide(definition("model"), || Ok(instance("child", &disposed)))
            .expect("child capability constructs");
        drop(child_value);

        child.teardown();

        assert_eq!(disposed.load(Ordering::SeqCst), 1);
        assert!(child.get(&id("model")).is_none());
    }

    #[test]
    fn child_teardown_does_not_dispose_parent_capability() {
        let root = Scope::root();
        let child = root.child();
        let disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), || Ok(instance("parent", &disposed)))
            .expect("parent capability constructs");

        child.teardown();

        assert_eq!(disposed.load(Ordering::SeqCst), 0);
        assert_eq!(
            label(&root.get(&id("model")).expect("parent remains")),
            "parent"
        );
    }

    #[test]
    fn replacement_disposes_old_capability_when_safe() {
        let scope = Scope::root();
        let old_disposed = Arc::new(AtomicUsize::new(0));
        let reader = scope
            .provide(definition("model"), || Ok(instance("old", &old_disposed)))
            .expect("old capability constructs");
        let in_flight = scope.get(&id("model")).expect("reader gets old");

        scope
            .replace(definition("model"), || {
                Ok(instance("new", &Arc::new(AtomicUsize::new(0))))
            })
            .expect("new capability constructs");
        assert_eq!(old_disposed.load(Ordering::SeqCst), 0);

        drop(reader);
        assert_eq!(old_disposed.load(Ordering::SeqCst), 0);
        drop(in_flight);
        assert_eq!(old_disposed.load(Ordering::SeqCst), 1);
    }
}
