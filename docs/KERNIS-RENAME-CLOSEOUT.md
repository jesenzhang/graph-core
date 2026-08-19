# Kernis R1 Rename Closeout

Status: R1 candidate

## Identity

- Previous project name: `graph-core`
- New brand: `Kernis`
- Category: Rust Meta-Runtime Kernel
- Long-term target: composable Meta-Framework
- R1 base: `60066dcfb7d7038d64da441f3ee852893fbd9119`
- R1 branch: `chore/kernis-project-repositioning`

## Scope

R1 is a non-semantic project identity and documentation repositioning. The
README, roadmap, current project metadata, active architecture/status
documents, repository URL, and neutral primitives package now use the Kernis
identity. Historical research documents retain `graph-core` where it records
the name, baseline, or state that existed when the experiment was performed.

The neutral primitives package is the only crate package renamed: `graph-core`
became `kernis-core`. Domain crates such as `capability-graph`,
`workflow-graph`, `workflow-recovery`, `execution-stream`, `runtime-core`, and
`graph-lab` retain their descriptive names.

## Semantic freeze

R1 does not change capability lifecycle, reactive reconciliation,
`Runtime::step`, durable store semantics, workflow mutation, execution-stream
policies, recovery decisions, operation/attempt identity, provider withdrawal,
or any fixed-point algorithm. The rename is limited to project/package
identity, documentation, metadata, and the import path required by the
neutral primitives package rename.

## Repository

The GitHub repository is now:

<https://github.com/jesenzhang/kernis>

The local `origin` remote points to:

```text
https://github.com/jesenzhang/kernis.git
```
