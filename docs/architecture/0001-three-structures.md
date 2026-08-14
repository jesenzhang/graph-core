# ADR-0001: Separate three runtime structures

Status: Accepted as research baseline
Date: 2026-08-14

## Context

Agent systems often describe plugin dependencies, task orchestration, event flow, and runtime topology with the word “graph”. Their semantics differ significantly in lifetime, mutation rate, recovery requirements, and ownership.

## Decision

`graph-core` starts with three independent structures:

- Capability Graph;
- Workflow Graph / DAG;
- Execution Streams.

There is no shared public `Graph` trait in the baseline.

## Consequences

Positive:

- invariants remain domain-specific;
- experiments can falsify the separation;
- stream backpressure is not distorted into graph mutation;
- dynamic capability composition can evolve independently from task scheduling.

Cost:

- some algorithms or identifiers may initially be duplicated;
- interoperability must be explicit.

Duplication is acceptable until repeated evidence proves a stable common abstraction.
