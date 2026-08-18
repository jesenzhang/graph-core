//! Capability definitions and exact runtime identity types.

use graph_core::Id;
use std::collections::BTreeSet;
use std::fmt;

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
    /// Stable provider/configuration identity used when reconstructing a
    /// capability in a new process.
    pub replay_identity: String,
    /// Required capabilities. A sorted set makes equivalent definitions stable.
    pub dependencies: BTreeSet<Dependency>,
}

impl CapabilityDefinition {
    /// Creates a capability definition with no dependencies.
    #[must_use]
    pub fn new(id: CapabilityId, kind: impl Into<String>) -> Self {
        let kind = kind.into();
        Self {
            id: id.clone(),
            replay_identity: format!("{}:{kind}", id),
            kind,
            dependencies: BTreeSet::new(),
        }
    }

    /// Returns this definition with an explicit stable replay identity.
    #[must_use]
    pub fn with_replay_identity(mut self, replay_identity: impl Into<String>) -> Self {
        self.replay_identity = replay_identity.into();
        self
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

    /// Largest representable generation.
    pub const MAX: Self = Self(u64::MAX);

    /// Returns the numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    #[must_use]
    pub const fn next(self) -> Self {
        self.checked_next().expect("capability generation overflow")
    }

    /// Returns the next generation, or None when the counter is exhausted.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Opaque process-local identity of one exact runtime entry.
///
/// A capability identifier names a slot. An entry identity names one
/// publication in that slot and therefore remains distinct across ABA-style
/// replacement sequences.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryId(u64);

impl EntryId {
    pub(crate) const fn from_raw(value: u64) -> Self {
        Self(value)
    }

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
