//! Deterministic capability graph resolution and admission proofs.

use crate::definition::{CapabilityDefinition, CapabilityId};
use std::collections::BTreeMap;
use std::fmt;

/// In-memory capability dependency model.
#[derive(Clone, Debug, Default)]
pub struct CapabilityGraph {
    pub(crate) definitions: BTreeMap<CapabilityId, CapabilityDefinition>,
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

    pub(crate) fn from_definitions(
        definitions: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> Self {
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

fn join_ids(ids: &[CapabilityId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}
