//! Capability dependency resolution and ownership-safe runtime experiments.

use graph_core::Id;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

/// Stable identifier for a capability.
pub type CapabilityId = Id;

/// Capability metadata kept for compatibility with the initial baseline API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    /// Stable capability identifier.
    pub id: CapabilityId,
    /// Human-readable capability kind such as model or service.
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
    /// Creates a dependency on id.
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
    /// Human-readable capability kind such as model or service.
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

/// Monotonically increasing version of a published local capability entry.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Generation(u64);

impl Generation {
    /// Generation used to represent an absent entry.
    pub const ZERO: Self = Self(0);

    /// Generation assigned to the first published entry.
    pub const FIRST: Self = Self(1);

    /// Returns the numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Opaque identity of one exact runtime entry.
///
/// A capability identifier names a slot. An entry identity names one
/// publication in that slot and therefore remains distinct across ABA-style
/// replacement sequences.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryId(u64);

impl EntryId {
    /// Returns the opaque numeric identity for diagnostics and tests.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
    /// Dependencies are checked by resolve, which allows callers to assemble
    /// definitions in any order. The returned value is the previous
    /// definition, if the identifier was already present.
    pub fn insert(
        &mut self,
        capability: impl Into<CapabilityDefinition>,
    ) -> Option<CapabilityDefinition> {
        let capability = capability.into();
        self.definitions.insert(capability.id.clone(), capability)
    }

    /// Declares that consumer requires dependency.
    ///
    /// # Errors
    ///
    /// Returns an error when either the consumer or dependency is not
    /// registered. Definitions built with depends_on are instead validated
    /// when resolve is called.
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

    /// Validates one candidate against this graph's current definitions.
    ///
    /// This is the runtime admission boundary. Callers receive a proof object
    /// only after the same deterministic resolver used by E01 accepts the
    /// candidate topology.
    pub fn validate_candidate(
        &self,
        candidate: CapabilityDefinition,
    ) -> Result<ValidatedCapabilityDefinition, CapabilityGraphError> {
        let mut graph = self.clone();
        graph.insert(candidate.clone());
        let resolution = graph.resolve()?;
        Ok(ValidatedCapabilityDefinition {
            definition: candidate,
            resolution,
        })
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

    fn from_definitions(definitions: impl IntoIterator<Item = CapabilityDefinition>) -> Self {
        let mut graph = Self::default();
        for definition in definitions {
            graph.insert(definition);
        }
        graph
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

    /// Returns capability identifiers in reverse construction order for
    /// teardown.
    #[must_use]
    pub fn teardown_order(&self) -> Vec<CapabilityId> {
        self.construction_order.iter().rev().cloned().collect()
    }
}

/// A candidate definition that has crossed the graph admission boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCapabilityDefinition {
    definition: CapabilityDefinition,
    resolution: ResolvedCapabilityGraph,
}

impl ValidatedCapabilityDefinition {
    /// Returns the definition accepted by the resolver.
    #[must_use]
    pub fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    /// Returns the deterministic topology proof produced by the resolver.
    #[must_use]
    pub fn resolution(&self) -> &ResolvedCapabilityGraph {
        &self.resolution
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
        /// Cycle path such as A -> B -> C -> A.
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

type Cleanup = Box<dyn FnOnce(Box<dyn Any + Send + Sync>) + Send>;

/// A reader-visible capability value with runtime-owned cleanup.
///
/// Readers can inspect or downcast the value, but this type deliberately has
/// no public disposal operation. The runtime-owned instance slot is the only
/// object that owns the value, and dropping that slot invokes the cleanup
/// callback at most once.
pub struct CapabilityValue {
    value: Option<Box<dyn Any + Send + Sync>>,
    cleanup: Mutex<Option<Cleanup>>,
}

impl CapabilityValue {
    /// Wraps a value whose normal Rust Drop implementation is its cleanup.
    #[must_use]
    pub fn from_value<T>(value: T) -> Self
    where
        T: Any + Send + Sync,
    {
        Self {
            value: Some(Box::new(value)),
            cleanup: Mutex::new(None),
        }
    }

    /// Wraps a value with a runtime-owned, exactly-once cleanup callback.
    #[must_use]
    pub fn new<T, F>(value: T, cleanup: F) -> Self
    where
        T: Any + Send + Sync,
        F: FnOnce(T) + Send + 'static,
    {
        let cleanup = Box::new(move |value: Box<dyn Any + Send + Sync>| {
            let value = value
                .downcast::<T>()
                .expect("capability cleanup type must match its value");
            cleanup(*value);
        });
        Self {
            value: Some(Box::new(value)),
            cleanup: Mutex::new(Some(cleanup)),
        }
    }

    /// Returns the value as a concrete type when it has type T.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value.as_ref()?.downcast_ref()
    }
}

impl Drop for CapabilityValue {
    fn drop(&mut self) {
        let cleanup = self
            .cleanup
            .get_mut()
            .expect("capability cleanup lock must not be poisoned")
            .take();
        if let Some(cleanup) = cleanup {
            if let Some(value) = self.value.take() {
                cleanup(value);
            }
        }
    }
}

/// Exact dependency handles resolved for one construction attempt.
#[derive(Clone, Debug, Default)]
pub struct ResolvedDependencies {
    handles: BTreeMap<CapabilityId, CapabilityHandle>,
}

impl ResolvedDependencies {
    /// Looks up the exact published handle resolved for dependency id.
    #[must_use]
    pub fn get(&self, id: &CapabilityId) -> Option<&CapabilityHandle> {
        self.handles.get(id)
    }

    /// Returns the exact dependency handles in deterministic identifier order.
    pub fn iter(&self) -> impl Iterator<Item = (&CapabilityId, &CapabilityHandle)> {
        self.handles.iter()
    }

    /// Returns the number of resolved dependencies.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Returns whether no dependencies were resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

struct InstanceSlot {
    value: CapabilityValue,
    dependencies: ResolvedDependencies,
}

struct CapabilityEntry {
    definition: Arc<CapabilityDefinition>,
    instance: Arc<InstanceSlot>,
    generation: Generation,
    entry_id: EntryId,
}

impl CapabilityEntry {
    fn handle(&self) -> CapabilityHandle {
        CapabilityHandle {
            definition: Arc::clone(&self.definition),
            instance: Arc::clone(&self.instance),
            generation: self.generation,
            entry_id: self.entry_id,
        }
    }
}

/// A reader-owned reference to one exact capability entry.
#[derive(Clone)]
pub struct CapabilityHandle {
    definition: Arc<CapabilityDefinition>,
    instance: Arc<InstanceSlot>,
    generation: Generation,
    entry_id: EntryId,
}

impl fmt::Debug for CapabilityHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityHandle")
            .field("id", &self.id())
            .field("kind", &self.kind())
            .field("generation", &self.generation())
            .field("entry_id", &self.entry_id())
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

    /// Returns the exact publication generation.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns the exact runtime entry identity.
    #[must_use]
    pub fn entry_id(&self) -> EntryId {
        self.entry_id
    }

    /// Returns the reader-visible capability value.
    ///
    /// The returned value has no disposal method. Its lifetime is secured by
    /// this handle, while cleanup remains owned by the runtime slot.
    #[must_use]
    pub fn instance(&self) -> &CapabilityValue {
        &self.instance.value
    }

    /// Returns the exact dependency snapshot retained by this entry.
    #[must_use]
    pub fn dependencies(&self) -> &ResolvedDependencies {
        &self.instance.dependencies
    }

    /// Returns the concrete value when it has type T.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.instance().downcast_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lifecycle {
    Open = 0,
    Closing = 1,
    Closed = 2,
}

impl Lifecycle {
    fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Open,
            1 => Self::Closing,
            _ => Self::Closed,
        }
    }
}

struct ScopeState {
    parent: Option<Scope>,
    entries: RwLock<BTreeMap<CapabilityId, CapabilityEntry>>,
    lifecycle: AtomicU8,
    topology: Arc<RwLock<()>>,
    children: Mutex<Vec<Weak<ScopeState>>>,
}

/// A hierarchical scope that owns local capability instances.
///
/// Lookup walks from the current scope toward its parent. A child can publish
/// a local capability without changing the parent's entry. Handles keep exact
/// instances alive until the last reader releases them.
#[derive(Clone)]
pub struct Scope {
    state: Arc<ScopeState>,
}

static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(1);

fn next_entry_id() -> EntryId {
    EntryId(NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed))
}

impl Scope {
    /// Creates an empty root scope.
    #[must_use]
    pub fn root() -> Self {
        let topology = Arc::new(RwLock::new(()));
        Self {
            state: Arc::new(ScopeState {
                parent: None,
                entries: RwLock::new(BTreeMap::new()),
                lifecycle: AtomicU8::new(Lifecycle::Open as u8),
                topology,
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Creates an empty child scope inheriting from this scope.
    ///
    /// If the parent is already closing or closed, the child is created
    /// closed and cannot publish new entries.
    #[must_use]
    pub fn child(&self) -> Self {
        let child = Self {
            state: Arc::new(ScopeState {
                parent: Some(self.clone()),
                entries: RwLock::new(BTreeMap::new()),
                lifecycle: AtomicU8::new(Lifecycle::Open as u8),
                topology: Arc::clone(&self.state.topology),
                children: Mutex::new(Vec::new()),
            }),
        };
        let _topology = self
            .state
            .topology
            .write()
            .expect("scope topology lock poisoned");
        if !self.is_open() {
            child
                .state
                .lifecycle
                .store(Lifecycle::Closed as u8, Ordering::Release);
        }
        self.state
            .children
            .lock()
            .expect("scope child lock poisoned")
            .push(Arc::downgrade(&child.state));
        child
    }

    /// Looks up the nearest capability visible from this scope.
    ///
    /// New lookups are rejected after the scope begins teardown. Handles
    /// already returned before that boundary remain valid.
    #[must_use]
    pub fn get(&self, id: &CapabilityId) -> Option<CapabilityHandle> {
        if !self.is_open() {
            return None;
        }

        let local = self
            .state
            .entries
            .read()
            .expect("scope entry lock poisoned")
            .get(id)
            .map(CapabilityEntry::handle);
        if local.is_some() {
            return local;
        }

        self.state.parent.as_ref()?.get(id)
    }

    /// Returns the generation of the nearest visible entry.
    #[must_use]
    pub fn generation(&self, id: &CapabilityId) -> Option<Generation> {
        self.get(id).map(|handle| handle.generation())
    }

    /// Validates a candidate against the current visible graph.
    ///
    /// The returned proof is a snapshot for inspection. Publication performs
    /// the same validation again while holding the topology admission lock.
    pub fn validate_definition(
        &self,
        definition: CapabilityDefinition,
    ) -> Result<ValidatedCapabilityDefinition, ScopeError> {
        let _topology = self
            .state
            .topology
            .read()
            .expect("scope topology lock poisoned");
        if !self.is_open() {
            return Err(ScopeError::Closed);
        }
        self.visible_graph()
            .validate_candidate(definition)
            .map_err(Into::into)
    }

    /// Provides a new local capability using a transactional factory.
    ///
    /// The factory runs only after graph admission and receives exact handles
    /// for the visible dependencies. A local entry that already exists must
    /// be changed through replace with its expected generation.
    pub fn provide<F>(
        &self,
        definition: CapabilityDefinition,
        construct: F,
    ) -> Result<CapabilityHandle, ScopeError>
    where
        F: FnOnce(&ResolvedDependencies) -> Result<CapabilityValue, String>,
    {
        self.publish(definition, PublishExpectation::New, construct)
    }

    /// Replaces a local capability only when expected_generation is current.
    ///
    /// Construction may run concurrently with another replacement, but the
    /// final publication compares the generation again under the topology
    /// lock. A stale constructor therefore returns ReplacementConflict and
    /// cannot change the current entry.
    pub fn replace<F>(
        &self,
        definition: CapabilityDefinition,
        expected_generation: Generation,
        construct: F,
    ) -> Result<CapabilityHandle, ScopeError>
    where
        F: FnOnce(&ResolvedDependencies) -> Result<CapabilityValue, String>,
    {
        self.publish(
            definition,
            PublishExpectation::Replace(expected_generation),
            construct,
        )
    }

    fn publish<F>(
        &self,
        definition: CapabilityDefinition,
        expectation: PublishExpectation,
        construct: F,
    ) -> Result<CapabilityHandle, ScopeError>
    where
        F: FnOnce(&ResolvedDependencies) -> Result<CapabilityValue, String>,
    {
        let dependencies = {
            let _topology = self
                .state
                .topology
                .read()
                .expect("scope topology lock poisoned");
            self.check_expectation(&definition.id, expectation)?;
            let _validated = self
                .visible_graph()
                .validate_candidate(definition.clone())
                .map_err(ScopeError::from)?;
            self.resolve_dependencies(&definition)?
        };

        let value = construct(&dependencies).map_err(|reason| ScopeError::ConstructionFailed {
            capability: definition.id.clone(),
            reason,
        })?;
        let instance = Arc::new(InstanceSlot {
            value,
            dependencies,
        });

        let (old, handle) = {
            let _topology = self
                .state
                .topology
                .write()
                .expect("scope topology lock poisoned");

            self.validate_descendant_admission(&definition)?;
            let mut entries = self
                .state
                .entries
                .write()
                .expect("scope entry lock poisoned");
            self.check_expectation_locked(&definition.id, expectation, &entries)?;
            self.visible_graph_with_entries(&entries)
                .validate_candidate(definition.clone())
                .map_err(ScopeError::from)?;

            let generation = match expectation {
                PublishExpectation::New => Generation::FIRST,
                PublishExpectation::Replace(_) => entries
                    .get(&definition.id)
                    .expect("replacement entry was checked above")
                    .generation
                    .next(),
            };
            let entry = CapabilityEntry {
                definition: Arc::new(definition),
                instance: Arc::clone(&instance),
                generation,
                entry_id: next_entry_id(),
            };
            let handle = entry.handle();
            let old = entries.insert(entry.definition.id.clone(), entry);
            (old, handle)
        };

        drop(old);
        Ok(handle)
    }

    fn check_expectation(
        &self,
        capability: &CapabilityId,
        expectation: PublishExpectation,
    ) -> Result<(), ScopeError> {
        let entries = self
            .state
            .entries
            .read()
            .expect("scope entry lock poisoned");
        self.check_expectation_locked(capability, expectation, &entries)
    }

    fn check_expectation_locked(
        &self,
        capability: &CapabilityId,
        expectation: PublishExpectation,
        entries: &BTreeMap<CapabilityId, CapabilityEntry>,
    ) -> Result<(), ScopeError> {
        match Lifecycle::from_raw(self.state.lifecycle.load(Ordering::Acquire)) {
            Lifecycle::Open => {}
            Lifecycle::Closing | Lifecycle::Closed => return Err(ScopeError::Closed),
        }
        match expectation {
            PublishExpectation::New => {
                if let Some(entry) = entries.get(capability) {
                    return Err(ScopeError::AlreadyProvided {
                        capability: capability.clone(),
                        generation: entry.generation,
                    });
                }
            }
            PublishExpectation::Replace(expected) => {
                let Some(entry) = entries.get(capability) else {
                    return Err(ScopeError::NoLocalEntry {
                        capability: capability.clone(),
                    });
                };
                if entry.generation != expected {
                    return Err(ScopeError::ReplacementConflict {
                        capability: capability.clone(),
                        expected,
                        actual: entry.generation,
                    });
                }
            }
        }
        Ok(())
    }

    fn resolve_dependencies(
        &self,
        definition: &CapabilityDefinition,
    ) -> Result<ResolvedDependencies, ScopeError> {
        let mut handles = BTreeMap::new();
        for dependency in &definition.dependencies {
            let handle = self
                .get(&dependency.id)
                .ok_or_else(|| ScopeError::MissingDependency {
                    capability: definition.id.clone(),
                    dependency: dependency.id.clone(),
                })?;
            handles.insert(dependency.id.clone(), handle);
        }
        Ok(ResolvedDependencies { handles })
    }

    fn is_open(&self) -> bool {
        Lifecycle::from_raw(self.state.lifecycle.load(Ordering::Acquire)) == Lifecycle::Open
    }

    fn has_local(&self, id: &CapabilityId) -> bool {
        self.state
            .entries
            .read()
            .expect("scope entry lock poisoned")
            .contains_key(id)
    }

    fn visible_definitions(&self) -> BTreeMap<CapabilityId, CapabilityDefinition> {
        if !self.is_open() {
            return BTreeMap::new();
        }
        let mut definitions = self
            .state
            .parent
            .as_ref()
            .map_or_else(BTreeMap::new, Scope::visible_definitions);
        let entries = self
            .state
            .entries
            .read()
            .expect("scope entry lock poisoned");
        for (id, entry) in entries.iter() {
            definitions.insert(id.clone(), (*entry.definition).clone());
        }
        definitions
    }

    fn visible_graph(&self) -> CapabilityGraph {
        CapabilityGraph::from_definitions(self.visible_definitions().into_values())
    }

    fn visible_graph_with_entries(
        &self,
        entries: &BTreeMap<CapabilityId, CapabilityEntry>,
    ) -> CapabilityGraph {
        let mut definitions = self
            .state
            .parent
            .as_ref()
            .map_or_else(BTreeMap::new, Scope::visible_definitions);
        for (id, entry) in entries {
            definitions.insert(id.clone(), (*entry.definition).clone());
        }
        CapabilityGraph::from_definitions(definitions.into_values())
    }

    fn live_descendants(&self) -> Vec<Scope> {
        let children = self
            .state
            .children
            .lock()
            .expect("scope child lock poisoned")
            .iter()
            .filter_map(Weak::upgrade)
            .map(|state| Scope { state })
            .collect::<Vec<_>>();
        let mut descendants = Vec::new();
        for child in children {
            descendants.push(child.clone());
            descendants.extend(child.live_descendants());
        }
        descendants
    }

    fn candidate_affects(&self, descendant: &Scope, capability: &CapabilityId) -> bool {
        let mut current = descendant.clone();
        loop {
            if Arc::ptr_eq(&current.state, &self.state) {
                return true;
            }
            if !current.is_open() || current.has_local(capability) {
                return false;
            }
            let Some(parent) = current.state.parent.as_ref() else {
                return false;
            };
            current = parent.clone();
        }
    }

    fn validate_descendant_admission(
        &self,
        definition: &CapabilityDefinition,
    ) -> Result<(), ScopeError> {
        for descendant in self.live_descendants() {
            if !descendant.is_open() || !self.candidate_affects(&descendant, &definition.id) {
                continue;
            }
            descendant
                .visible_graph()
                .validate_candidate(definition.clone())
                .map_err(ScopeError::from)?;
        }
        Ok(())
    }

    /// Stops new lookups and mutations, then releases local ownership in
    /// dependency-aware teardown order.
    ///
    /// Existing handles are not invalidated. A dependent entry retains the
    /// exact dependency handles it was constructed with, so those dependencies
    /// remain alive until the dependent handle is released.
    pub fn teardown(&self) {
        let (mut entries, order) = {
            let _topology = self
                .state
                .topology
                .write()
                .expect("scope topology lock poisoned");
            if self
                .state
                .lifecycle
                .compare_exchange(
                    Lifecycle::Open as u8,
                    Lifecycle::Closing as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return;
            }
            let entries = std::mem::take(
                &mut *self
                    .state
                    .entries
                    .write()
                    .expect("scope entry lock poisoned"),
            );
            let order = runtime_teardown_order(&entries);
            self.state
                .lifecycle
                .store(Lifecycle::Closed as u8, Ordering::Release);
            (entries, order)
        };

        for id in order {
            if let Some(entry) = entries.remove(&id) {
                drop(entry);
            }
        }
        for (_, entry) in entries {
            drop(entry);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublishExpectation {
    New,
    Replace(Generation),
}

fn runtime_teardown_order(entries: &BTreeMap<CapabilityId, CapabilityEntry>) -> Vec<CapabilityId> {
    let graph = runtime_graph(entries);
    match graph.resolve() {
        Ok(resolved) => resolved.teardown_order(),
        Err(_) => entries.keys().rev().cloned().collect(),
    }
}

fn runtime_graph(entries: &BTreeMap<CapabilityId, CapabilityEntry>) -> CapabilityGraph {
    let mut graph = CapabilityGraph::default();
    let mut visited = BTreeSet::new();
    for entry in entries.values() {
        graph.insert((*entry.definition).clone());
        visited.insert(entry.entry_id);
    }
    for entry in entries.values() {
        for (_, dependency) in entry.instance.dependencies.iter() {
            insert_snapshot(&mut graph, dependency, &mut visited);
        }
    }
    graph
}

fn insert_snapshot(
    graph: &mut CapabilityGraph,
    handle: &CapabilityHandle,
    visited: &mut BTreeSet<EntryId>,
) {
    if !visited.insert(handle.entry_id) {
        return;
    }
    graph
        .definitions
        .entry(handle.id().clone())
        .or_insert_with(|| (*handle.definition).clone());
    for (_, dependency) in handle.dependencies().iter() {
        insert_snapshot(graph, dependency, visited);
    }
}

/// Errors produced while publishing a scoped capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeError {
    /// A graph invariant rejected the runtime candidate.
    GraphAdmission(CapabilityGraphError),
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
    /// A local entry already exists and must be changed with replace.
    AlreadyProvided {
        /// Capability whose local slot is occupied.
        capability: CapabilityId,
        /// Current generation in the local slot.
        generation: Generation,
    },
    /// Replacement was requested for a capability without a local entry.
    NoLocalEntry {
        /// Capability that has no local entry in this scope.
        capability: CapabilityId,
    },
    /// The expected generation no longer names the current local entry.
    ReplacementConflict {
        /// Capability whose replacement lost the race.
        capability: CapabilityId,
        /// Generation observed by the stale replacement.
        expected: Generation,
        /// Generation currently published.
        actual: Generation,
    },
    /// The scope has begun or completed teardown.
    Closed,
}

impl From<CapabilityGraphError> for ScopeError {
    fn from(error: CapabilityGraphError) -> Self {
        match error {
            CapabilityGraphError::MissingDependency {
                capability,
                dependency,
            } => Self::MissingDependency {
                capability,
                dependency,
            },
            other => Self::GraphAdmission(other),
        }
    }
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GraphAdmission(error) => write!(f, "runtime graph admission rejected: {error}"),
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
            Self::AlreadyProvided {
                capability,
                generation,
            } => write!(
                f,
                "capability {capability} already has local generation {generation}"
            ),
            Self::NoLocalEntry { capability } => {
                write!(f, "capability {capability} has no local entry to replace")
            }
            Self::ReplacementConflict {
                capability,
                expected,
                actual,
            } => write!(
                f,
                "replacement conflict for {capability}: expected generation {expected}, actual {actual}"
            ),
            Self::Closed => f.write_str("scope is already closing or closed"),
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
    use std::thread;

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

    fn value(label: &str, disposed: &Arc<AtomicUsize>) -> CapabilityValue {
        let disposed = Arc::clone(disposed);
        CapabilityValue::new(label.to_owned(), move |_label: String| {
            disposed.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn label(handle: &CapabilityHandle) -> &str {
        handle
            .downcast_ref::<String>()
            .expect("test value type")
            .as_str()
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

    #[test]
    fn child_inherits_parent_capability() {
        let root = Scope::root();
        let disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), |_| Ok(value("openai", &disposed)))
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
        root.provide(definition("model"), |_| Ok(value("openai", &root_disposed)))
            .expect("root capability constructs");
        let child = root.child();
        child
            .provide(definition("model"), |_| {
                Ok(value("deepseek", &child_disposed))
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
        root.provide(definition("model"), |_| Ok(value("openai", &root_disposed)))
            .expect("root capability constructs");
        let first = root.child();
        let second = root.child();
        first
            .provide(definition("model"), |_| {
                Ok(value("deepseek", &first_disposed))
            })
            .expect("first override constructs");
        second
            .provide(definition("model"), |_| {
                Ok(value("anthropic", &second_disposed))
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
    fn replacement_changes_new_reads_and_advances_generation() {
        let scope = Scope::root();
        let first_disposed = Arc::new(AtomicUsize::new(0));
        let second_disposed = Arc::new(AtomicUsize::new(0));
        let first = scope
            .provide(definition("model"), |_| Ok(value("v1", &first_disposed)))
            .expect("v1 constructs");
        let generation = first.generation();
        assert_eq!(generation, Generation::FIRST);
        scope
            .replace(definition("model"), generation, |_| {
                Ok(value("v2", &second_disposed))
            })
            .expect("v2 constructs");

        let current = scope.get(&id("model")).expect("current model");
        assert_eq!(label(&current), "v2");
        assert_eq!(current.generation(), generation.next());
        drop(first);
        assert_eq!(first_disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn in_flight_reader_survives_replacement() {
        let scope = Scope::root();
        let first_disposed = Arc::new(AtomicUsize::new(0));
        let second_disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), |_| Ok(value("v1", &first_disposed)))
            .expect("v1 constructs");
        let reader = scope.get(&id("model")).expect("reader gets v1");
        let generation = reader.generation();
        let barrier = Arc::new(Barrier::new(2));
        let reader_barrier = Arc::clone(&barrier);
        let reader_thread = thread::spawn(move || {
            assert_eq!(label(&reader), "v1");
            reader_barrier.wait();
            assert_eq!(label(&reader), "v1");
            reader
        });
        barrier.wait();

        scope
            .replace(definition("model"), generation, |_| {
                Ok(value("v2", &second_disposed))
            })
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
        let old = scope
            .provide(definition("model"), |_| Ok(value("v1", &disposed)))
            .expect("v1 constructs");

        let error = scope
            .replace(definition("model"), old.generation(), |_| {
                Err::<CapabilityValue, _>("constructor failed".to_owned())
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
    fn consumer_cannot_dispose_live_instance() {
        let scope = Scope::root();
        let handle = scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("live".to_owned()))
            })
            .expect("model constructs");

        let value = handle.instance();
        assert_eq!(
            value.downcast_ref::<String>().map(String::as_str),
            Some("live")
        );
        // The reader-visible type has no dispose method. Cleanup is exercised
        // through the runtime-owned slot in the exactly-once tests below.
    }

    #[test]
    fn self_dependency_is_rejected_at_runtime_admission() {
        let scope = Scope::root();
        let constructed = Arc::new(AtomicUsize::new(0));
        let constructed_for_factory = Arc::clone(&constructed);
        let error = scope
            .provide(definition("a").depends_on(id("a")), move |_| {
                constructed_for_factory.fetch_add(1, Ordering::SeqCst);
                Ok(CapabilityValue::from_value("invalid".to_owned()))
            })
            .expect_err("self dependency must be rejected");

        assert!(matches!(
            error,
            ScopeError::GraphAdmission(CapabilityGraphError::Cycle { .. })
        ));
        assert_eq!(constructed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn runtime_cycle_is_rejected() {
        let scope = Scope::root();
        let a = scope
            .provide(definition("a"), |_| {
                Ok(CapabilityValue::from_value("a".to_owned()))
            })
            .expect("a constructs");
        scope
            .provide(definition("b").depends_on(id("a")), |_| {
                Ok(CapabilityValue::from_value("b".to_owned()))
            })
            .expect("b constructs");

        let error = scope
            .replace(definition("a").depends_on(id("b")), a.generation(), |_| {
                Ok(CapabilityValue::from_value("new-a".to_owned()))
            })
            .expect_err("runtime cycle must be rejected");
        assert!(matches!(
            error,
            ScopeError::GraphAdmission(CapabilityGraphError::Cycle { .. })
        ));
        assert_eq!(label(&scope.get(&id("a")).expect("old a remains")), "a");
    }

    #[test]
    fn candidate_replacement_that_introduces_cycle_is_rejected() {
        let scope = Scope::root();
        let a = scope
            .provide(definition("a"), |_| {
                Ok(CapabilityValue::from_value("a".to_owned()))
            })
            .expect("a constructs");
        scope
            .provide(definition("b").depends_on(id("a")), |_| {
                Ok(CapabilityValue::from_value("b".to_owned()))
            })
            .expect("b constructs");
        let constructed = Arc::new(AtomicUsize::new(0));
        let constructed_for_factory = Arc::clone(&constructed);

        let error = scope
            .replace(
                definition("a").depends_on(id("b")),
                a.generation(),
                move |_| {
                    constructed_for_factory.fetch_add(1, Ordering::SeqCst);
                    Ok(CapabilityValue::from_value("invalid".to_owned()))
                },
            )
            .expect_err("candidate cycle must be rejected");
        assert!(matches!(
            error,
            ScopeError::GraphAdmission(CapabilityGraphError::Cycle { .. })
        ));
        assert_eq!(constructed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_dependency_is_rejected_before_construction() {
        let scope = Scope::root();
        let constructed = Arc::new(AtomicUsize::new(0));
        let constructed_for_factory = Arc::clone(&constructed);
        let error = scope
            .provide(definition("a").depends_on(id("missing")), move |_| {
                constructed_for_factory.fetch_add(1, Ordering::SeqCst);
                Ok(CapabilityValue::from_value("invalid".to_owned()))
            })
            .expect_err("missing dependency must be rejected");

        assert_eq!(
            error,
            ScopeError::MissingDependency {
                capability: id("a"),
                dependency: id("missing"),
            }
        );
        assert_eq!(constructed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn constructor_receives_resolved_dependency_snapshot() {
        let scope = Scope::root();
        scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("model-v1".to_owned()))
            })
            .expect("model constructs");
        let service = scope
            .provide(
                definition("service").depends_on(id("model")),
                |dependencies| {
                    let model = dependencies
                        .get(&id("model"))
                        .expect("model dependency")
                        .downcast_ref::<String>()
                        .expect("model value")
                        .clone();
                    Ok(CapabilityValue::from_value(model))
                },
            )
            .expect("service constructs");

        assert_eq!(label(&service), "model-v1");
        assert_eq!(
            label(service.dependencies().get(&id("model")).unwrap()),
            "model-v1"
        );
        assert_eq!(
            service
                .dependencies()
                .get(&id("model"))
                .unwrap()
                .generation(),
            Generation::FIRST
        );
    }

    #[test]
    fn dependency_snapshot_survives_provider_replacement() {
        let scope = Scope::root();
        scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("model-v1".to_owned()))
            })
            .expect("model v1 constructs");
        let service = scope
            .provide(
                definition("service").depends_on(id("model")),
                |dependencies| {
                    let model = dependencies
                        .get(&id("model"))
                        .expect("model dependency")
                        .downcast_ref::<String>()
                        .expect("model value")
                        .clone();
                    Ok(CapabilityValue::from_value(model))
                },
            )
            .expect("service constructs");
        let model_generation = scope.generation(&id("model")).unwrap();

        scope
            .replace(definition("model"), model_generation, |_| {
                Ok(CapabilityValue::from_value("model-v2".to_owned()))
            })
            .expect("model v2 constructs");

        assert_eq!(label(&service), "model-v1");
        assert_eq!(
            label(service.dependencies().get(&id("model")).unwrap()),
            "model-v1"
        );
        assert_eq!(
            label(&scope.get(&id("model")).expect("new model")),
            "model-v2"
        );
    }

    #[test]
    fn published_entry_pins_exact_dependency_generation() {
        let scope = Scope::root();
        let model_disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), |_| {
                Ok(value("model-v1", &model_disposed))
            })
            .expect("model v1 constructs");
        let service = scope
            .provide(definition("service").depends_on(id("model")), |_| {
                Ok(CapabilityValue::from_value("service".to_owned()))
            })
            .expect("service constructs");
        let old_model = service.dependencies().get(&id("model")).unwrap().clone();
        let model_generation = old_model.generation();

        scope
            .replace(definition("model"), model_generation, |_| {
                Ok(CapabilityValue::from_value("model-v2".to_owned()))
            })
            .expect("model v2 constructs");
        assert_eq!(model_disposed.load(Ordering::SeqCst), 0);

        drop(service);
        scope.teardown();
        drop(old_model);
        assert_eq!(model_disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_conflicting_replacement_does_not_change_current_entry() {
        let scope = Scope::root();
        let first = scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("v1".to_owned()))
            })
            .expect("v1 constructs");
        let current = scope
            .replace(definition("model"), first.generation(), |_| {
                Ok(CapabilityValue::from_value("v2".to_owned()))
            })
            .expect("v2 constructs");
        let error = scope
            .replace(definition("model"), first.generation(), |_| {
                Ok(CapabilityValue::from_value("stale".to_owned()))
            })
            .expect_err("stale replacement must fail");

        assert_eq!(
            error,
            ScopeError::ReplacementConflict {
                capability: id("model"),
                expected: first.generation(),
                actual: current.generation(),
            }
        );
        assert_eq!(
            label(&scope.get(&id("model")).expect("current remains")),
            "v2"
        );
    }

    #[test]
    fn stale_reader_cannot_remove_new_entry_identity() {
        let scope = Scope::root();
        let old = scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("v1".to_owned()))
            })
            .expect("v1 constructs");
        let v2 = scope
            .replace(definition("model"), old.generation(), |_| {
                Ok(CapabilityValue::from_value("v2".to_owned()))
            })
            .expect("v2 constructs");
        let v3 = scope
            .replace(definition("model"), v2.generation(), |_| {
                Ok(CapabilityValue::from_value("v3".to_owned()))
            })
            .expect("v3 constructs");

        assert_ne!(old.entry_id(), v3.entry_id());
        drop(old);
        assert_eq!(label(&scope.get(&id("model")).expect("v3 remains")), "v3");
        assert_eq!(scope.generation(&id("model")), Some(v3.generation()));
    }

    #[test]
    fn teardown_respects_dependency_order() {
        let scope = Scope::root();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let track = |label: &str, events: &Arc<Mutex<Vec<String>>>| {
            let events = Arc::clone(events);
            CapabilityValue::new(label.to_owned(), move |label: String| {
                events.lock().expect("event lock").push(label);
            })
        };
        scope
            .provide(definition("c"), |_| Ok(track("c", &events)))
            .expect("c constructs");
        scope
            .provide(definition("b").depends_on(id("c")), |_| {
                Ok(track("b", &events))
            })
            .expect("b constructs");
        scope
            .provide(definition("a").depends_on(id("b")), |_| {
                Ok(track("a", &events))
            })
            .expect("a constructs");

        scope.teardown();
        assert_eq!(
            *events.lock().expect("event lock"),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn teardown_rejects_new_mutations() {
        let scope = Scope::root();
        scope.teardown();

        let provide = scope.provide(definition("a"), |_| {
            Ok(CapabilityValue::from_value("a".to_owned()))
        });
        assert!(matches!(provide, Err(ScopeError::Closed)));
    }

    #[test]
    fn cleanup_occurs_exactly_once() {
        let scope = Scope::root();
        let disposed = Arc::new(AtomicUsize::new(0));
        let handle = scope
            .provide(definition("a"), |_| Ok(value("a", &disposed)))
            .expect("a constructs");

        scope.teardown();
        scope.teardown();
        assert_eq!(disposed.load(Ordering::SeqCst), 0);
        drop(handle);
        assert_eq!(disposed.load(Ordering::SeqCst), 1);
        scope.teardown();
        assert_eq!(disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn teardown_is_idempotent() {
        let scope = Scope::root();
        let disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("a"), |_| Ok(value("a", &disposed)))
            .expect("a constructs");

        scope.teardown();
        scope.teardown();
        assert_eq!(disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn in_flight_dependent_pins_dependency_until_release() {
        let scope = Scope::root();
        let model_disposed = Arc::new(AtomicUsize::new(0));
        scope
            .provide(definition("model"), |_| Ok(value("model", &model_disposed)))
            .expect("model constructs");
        let service = scope
            .provide(definition("service").depends_on(id("model")), |_| {
                Ok(CapabilityValue::from_value("service".to_owned()))
            })
            .expect("service constructs");

        scope.teardown();
        assert_eq!(model_disposed.load(Ordering::SeqCst), 0);
        drop(service);
        assert_eq!(model_disposed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn parent_scope_remains_alive_after_child_teardown() {
        let root = Scope::root();
        let parent_disposed = Arc::new(AtomicUsize::new(0));
        root.provide(definition("model"), |_| {
            Ok(value("parent", &parent_disposed))
        })
        .expect("parent model constructs");
        let child = root.child();
        child
            .provide(definition("service").depends_on(id("model")), |_| {
                Ok(CapabilityValue::from_value("child-service".to_owned()))
            })
            .expect("child service constructs");

        child.teardown();
        assert!(child.get(&id("service")).is_none());
        assert_eq!(
            label(&root.get(&id("model")).expect("parent remains")),
            "parent"
        );
        assert_eq!(parent_disposed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn concurrent_stale_replacement_returns_conflict() {
        let scope = Arc::new(Scope::root());
        let initial = scope
            .provide(definition("model"), |_| {
                Ok(CapabilityValue::from_value("v1".to_owned()))
            })
            .expect("v1 constructs");
        let generation = initial.generation();
        let ready = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_scope = Arc::clone(&scope);
        let worker_ready = Arc::clone(&ready);
        let worker_release = Arc::clone(&release);
        let worker = thread::spawn(move || {
            worker_scope.replace(definition("model"), generation, move |_| {
                worker_ready.wait();
                worker_release.wait();
                Ok(CapabilityValue::from_value("worker".to_owned()))
            })
        });

        ready.wait();
        let winner = scope
            .replace(definition("model"), generation, |_| {
                Ok(CapabilityValue::from_value("winner".to_owned()))
            })
            .expect("main thread wins");
        release.wait();
        let loser = worker.join().expect("worker joins");

        assert!(matches!(
            loser,
            Err(ScopeError::ReplacementConflict {
                capability,
                expected,
                actual,
            }) if capability == id("model")
                && expected == generation
                && actual == winner.generation()
        ));
        assert_eq!(
            label(&scope.get(&id("model")).expect("winner remains")),
            "winner"
        );
    }
}
