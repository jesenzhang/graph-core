# Reference Index

Captured: 2026-08-18

This file records research targets only. Inclusion does not mean adoption.

| Reference | Role in research | What to inspect first |
|---|---|---|
| `https://github.com/deepseek-ai/deepseek-harness` | Production-oriented Agent harness reference | `README.md`, `docs/cordis-primer.md`, `docs/cordis-tutorial/`, `packages/core/agent-loop/`, `packages/core/agent/` |
| `https://github.com/cordiverse/paper` | Formal Cordis / spatiotemporal-composability reference | Paper Sections 3–6: revertible effects, reactive coeffects, unified context, component/fiber calculus, implementation mapping, system boundary |
| `https://github.com/cordiverse/cordis` | Capability composition/runtime reference | Context, Service, Scope/Fiber lifecycle, inject/provide/effect behavior, loader/reload semantics |
| `https://www.npmjs.com/package/cordis` | Upstream release tracking | current version, release cadence, package metadata |
| `https://github.com/hydro-dev/Hydro` | Mature Cordis application reference | plugin loader, service dependencies, runtime reload behavior |
| Goose ecosystem / predecessor `workflow_engine` | Prior plugin/workflow lessons | plugin boundaries, event/stream model, dynamic workflow assumptions |

## Frozen Cordis paper cross-check

The 2026-08-18 paper/source reconciliation is recorded in:

- [`CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.md`](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.md)
- [`CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.zh.md`](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.zh.md)

Frozen sources for that pass:

- `cordiverse/paper` commit `948a07b369c62adb3b12e102458be5c18dfb69b9`;
- `cordiverse/cordis` commit `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4`, `packages/core` `4.0.0-rc.8`;
- graph-core baseline `62f556d37a1f8c4c7c5cd26d9e21917abe17816a`.

## Reference discipline

For every mechanism copied into a Rust experiment, record:

1. source repository and commit;
2. exact behavior being reproduced;
3. whether the behavior is public contract, formal assumption, or implementation detail;
4. Rust alternative considered;
5. test that demonstrates equivalence or intentional deviation.

Do not treat a TypeScript API shape as a requirement for the Rust API. Do not treat a theorem whose premises are not implemented or verified as a property of graph-core merely because Cordis proves it under those premises.
