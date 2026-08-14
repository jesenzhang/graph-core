# Reference Index

Captured: 2026-08-14

This file records research targets only. Inclusion does not mean adoption.

| Reference | Role in research | What to inspect first |
|---|---|---|
| `https://github.com/deepseek-ai/deepseek-harness` | Production-oriented Agent harness reference | `README.md`, `docs/cordis-primer.md`, `docs/cordis-tutorial/`, `packages/core/agent-loop/`, `packages/core/agent/` |
| `https://github.com/cordiverse/cordis` | Capability composition/runtime reference | Context, Service, Scope/Fiber lifecycle, inject/provide/effect behavior, loader/reload semantics |
| `https://www.npmjs.com/package/cordis` | Upstream release tracking | current version, release cadence, package metadata |
| `https://github.com/hydro-dev/Hydro` | Mature Cordis application reference | plugin loader, service dependencies, runtime reload behavior |
| Goose ecosystem / predecessor `workflow_engine` | Prior plugin/workflow lessons | plugin boundaries, event/stream model, dynamic workflow assumptions |

## Reference discipline

For every mechanism copied into a Rust experiment, record:

1. source repository and commit;
2. exact behavior being reproduced;
3. whether the behavior is public contract or implementation detail;
4. Rust alternative considered;
5. test that demonstrates equivalence or intentional deviation.

Do not treat a TypeScript API shape as a requirement for the Rust API.
