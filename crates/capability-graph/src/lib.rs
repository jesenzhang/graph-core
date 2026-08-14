//! Capability/service dependency graph experiments.

use graph_core::Id;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A capability available to a runtime scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    /// Stable capability identifier.
    pub id: Id,
    /// Human-readable capability kind such as `model`, `tool`, or `service`.
    pub kind: String,
}

/// In-memory capability dependency model.
#[derive(Clone, Debug, Default)]
pub struct CapabilityGraph {
    nodes: BTreeMap<Id, Capability>,
    dependencies: BTreeMap<Id, BTreeSet<Id>>,
}

impl CapabilityGraph {
    /// Inserts or replaces a capability definition.
    pub fn insert(&mut self, capability: Capability) {
        self.nodes.insert(capability.id.clone(), capability);
    }

    /// Declares that `consumer` requires `dependency`.
    ///
    /// # Errors
    ///
    /// Returns [`CapabilityGraphError::UnknownCapability`] if either endpoint is absent.
    pub fn require(&mut self, consumer: &Id, dependency: &Id) -> Result<(), CapabilityGraphError> {
        if !self.nodes.contains_key(consumer) {
            return Err(CapabilityGraphError::UnknownCapability(consumer.clone()));
        }
        if !self.nodes.contains_key(dependency) {
            return Err(CapabilityGraphError::UnknownCapability(dependency.clone()));
        }
        self.dependencies
            .entry(consumer.clone())
            .or_default()
            .insert(dependency.clone());
        Ok(())
    }

    /// Returns direct requirements in deterministic identifier order.
    #[must_use]
    pub fn requirements(&self, capability: &Id) -> Vec<&Id> {
        self.dependencies
            .get(capability)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Returns the number of registered capabilities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether the graph contains no capabilities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Errors produced while building the capability graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityGraphError {
    /// A dependency edge referenced a capability that is not registered.
    UnknownCapability(Id),
}

impl fmt::Display for CapabilityGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCapability(id) => write!(f, "unknown capability: {id}"),
        }
    }
}

impl std::error::Error for CapabilityGraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(id: &str) -> Capability {
        Capability {
            id: Id::new(id).expect("test id is valid"),
            kind: "service".to_owned(),
        }
    }

    #[test]
    fn requirements_are_domain_owned() {
        let mut graph = CapabilityGraph::default();
        let runtime = capability("runtime");
        let model = capability("model");
        graph.insert(runtime.clone());
        graph.insert(model.clone());
        graph.require(&runtime.id, &model.id).expect("edge is valid");

        assert_eq!(graph.requirements(&runtime.id), vec![&model.id]);
    }
}
