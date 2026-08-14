# Contributing

Keep the baseline lightweight.

Before adding a dependency or shared abstraction, answer:

1. Which concrete experiment or implementation requires it?
2. Which crate owns the semantic invariant?
3. Does the change accidentally merge Capability Graph, Workflow Graph, and Execution Streams into one abstraction?
4. Can the decision be reversed cheaply if the experiment falsifies it?

Required checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```
