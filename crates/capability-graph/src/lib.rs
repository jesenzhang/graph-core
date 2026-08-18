//! Capability dependency resolution and ownership-safe runtime experiments.

mod definition;
mod resolver;
mod runtime;
mod semantic;

pub use definition::{
    Capability, CapabilityDefinition, CapabilityId, Dependency, EntryId, Generation,
};
pub use resolver::{
    CapabilityGraph, CapabilityGraphError, ResolvedCapabilityGraph, ValidatedCapabilityDefinition,
};
pub use runtime::{CapabilityHandle, CapabilityValue, ResolvedDependencies, Scope, ScopeError};
pub use semantic::{
    CapabilityContext, CapabilityFiber, CapabilityRegistry, ConfigValidator, DependencyEpoch,
    DependencyPin, EffectError, EffectScope, EffectStack, FiberError, FiberId, FiberState,
    PluginConfig, PluginDefinition, PluginFactory, PluginFuture, PluginLoadContext, PluginRuntime,
    RegistryError, ScopedEffect,
};
