# Research Questions

## Priority A — boundary validation

1. Which data types are genuinely shared across Capability Graph and Workflow Graph?
2. Does a shared graph algorithm crate reduce complexity, or merely erase semantic differences?
3. Can Execution Streams stay completely outside graph topology while still supporting audit/replay?
4. What is the smallest capability scope/lifecycle model that can reproduce the useful Cordis behaviors?
5. What exact workflow mutations are required by an Agent that replans during execution?

## Priority B — runtime semantics

6. Should workflow mutation be append-only with graph revisions?
7. How should dependency cycles be reported in capability composition?
8. Can capability replacement be staged and rolled back without affecting in-flight tasks?
9. Which events are domain facts versus transient runtime observations?
10. Where does cancellation live: workflow scheduler, execution stream, capability scope, or all three with explicit propagation?

## Priority C — production pressure tests

11. Crash after task side effect but before checkpoint.
12. Capability provider disappears during an in-flight task.
13. Agent adds a task whose dependency is already completed.
14. Agent invalidates a branch that has partially executed.
15. Slow UI consumer while model/tool streams continue at high rate.
16. Replay a run using a newer capability configuration.
