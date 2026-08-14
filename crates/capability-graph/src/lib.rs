//! Capability dependency resolution and ownership-safe runtime experiments.

mod definition;
mod resolver;
mod runtime;

pub use definition::{
    Capability, CapabilityDefinition, CapabilityId, Dependency, EntryId, Generation,
};
pub use resolver::{
    CapabilityGraph, CapabilityGraphError, ResolvedCapabilityGraph, ValidatedCapabilityDefinition,
};
pub use runtime::{CapabilityHandle, CapabilityValue, ResolvedDependencies, Scope, ScopeError};
