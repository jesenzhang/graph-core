# ADR-0002: Research before framework commitment

Status: Accepted as research baseline
Date: 2026-08-14

## Decision

The initialization workspace uses the Rust standard library only. No graph library, async runtime, serialization framework, plugin ABI, or persistence system is selected yet.

A dependency is introduced when an experiment demonstrates a concrete need and records the trade-off.

## Rationale

The project is evaluating architecture boundaries. Premature framework selection would make it difficult to distinguish domain requirements from framework-shaped requirements.
