# Capability kernel decision / Capability Kernel 阶段决策

[English](CAPABILITY_KERNEL_DECISION.md) | 中文

日期：2026-08-14
证据：E01、E02、E02R、E02R-F1，以及当前本地 Cordis/DeepSeek Harness 源码快照

## 决策

**YES, BUT NARROWER / 是，但范围要更窄**

值得继续做一个小型、类似 Cordis 的 Rust capability composition substrate。实验已经建立了可独立测试的语义：确定性依赖顺序、scope inheritance、local replacement、reader-owned lifetime 和事务式 publication。

这些证据不足以支持构建 Rust Cordis clone，也不足以把 Agent runtime、plugin loader、event system、configuration language 或 durable execution model 放入 `graph-core`。长期方向应是更窄的 kernel：

```text
capability identity
→ dependency validation and deterministic order
→ explicit scope visibility
→ owned runtime instances
→ atomic in-memory replacement
→ safe teardown boundary
```

详细实验记录：

- [`E01-capability-resolution.zh.md`](results/E01-capability-resolution.zh.md)
- [`E02-scoped-replacement.zh.md`](results/E02-scoped-replacement.zh.md)
- [`E02R-capability-runtime-integrity.md`](results/E02R-capability-runtime-integrity.md)
- [`CORDIS-CAPABILITY-RESEARCH.zh.md`](CORDIS-CAPABILITY-RESEARCH.zh.md)

## E02R 更新

E02R 支持现有的“是，但范围要更窄”决策，并补上 runtime integrity 边界：

- cleanup authority 归 runtime-owned slot，而不是 reader handle；
- constructor 接收并由已发布 entry 保留精确的 dependency snapshot；
- runtime admission 使用 E01 resolver 检查当前 definition 与 candidate；
- replacement 要求 expected generation，并保持精确的 entry identity；
- teardown 拒绝新操作，按 dependency order 执行，并依赖 `Arc` ownership 实现同步 quiescence。

v0 的正式边界仍然是同步、进程内的 capability kernel，不包含 Agent、LLM、Tool、MCP、Workflow、Persistence、distributed coordination、plugin loader、dynamic module loading、config language、event bus、HMR watcher 或 async runtime。

`Scope::teardown()` 是 Capability Kernel v0 正式的 hierarchy lifecycle boundary。`Scope` 是 cloneable handle，因此：

```text
drop(scope)
```

不等价于 child-first、dependency-aware teardown。v0 暂不引入 `ScopeOwner`、`ScopeHandle` split 或 Drop auto teardown；这些属于尚未证明的 production RAII owner semantics。

## 1. 什么属于 graph-core

### 应该属于 graph-core

- capability ID、kind 和显式 dependency declaration；
- 确定性 dependency resolution 和结构化 cycle error；
- 带 parent fallback 的 scope-local registration；
- owned capability instance handle；
- replacement publication 与 rollback-safe construction；
- 可以在未来支持更强 quiescence protocol 的同步 ownership/teardown primitive；
- 针对这些语义的不变量和测试。

这些是 capability composition 的语义不变量。最终 consumer 可以是 Agent、workflow runner、CLI 或其他 application，不影响它们的价值。

### 不应该属于 graph-core

- Agent loop、prompt assembly、model provider、tool、MCP 或 session policy；
- workflow scheduling、retry、persistence、event sourcing 或 execution stream；
- YAML/JSON configuration 和 schema evaluation；
- dynamic library/WASM loading 与 sandbox policy；
- network、database、distributed coordination 或 multi-process state；
- 与 workflow 和 execution-stream crate 共享的 universal `Graph` trait；
- Cordis 的 proxy/declaration-merging ergonomics。

这些 concern 可以消费 kernel，或放在独立 crate 中，但不是定义 capability ownership 所必需的部分。

## 2. Cordis 机制评估

| 机制 | 对 graph-core 的判断 | 原因 |
|---|---|---|
| Dependency injection | 保留，但收窄为显式 ID | E01 证明声明式 requirement 可以提供 fail-fast 验证和确定性构造。使用 handle/accessor，不使用 proxy property。 |
| Scope inheritance | 保留 | E02 证明 parent fallback 和 local shadowing 可以不修改 parent。暂不硬编码四层。 |
| Service lifecycle | 保留 | Owned instance 和 reverse teardown 是 composition 核心语义。异步 lifecycle 另做实验。 |
| Effect cleanup | 之后部分保留 | 保留 registration 有 owner/disposer 的思想；等 async/quiescence 需求明确后再做通用 effect stack。 |
| Hot replacement | 窄范围保留 | staged construction + `Arc` reader-safe 的内存 replacement 有用；HMR/file watching 不属于 kernel。 |
| Plugin loader | 排除 | 加载 code/config 是 deployment/composition tooling，不是 dependency graph 语义。 |
| Dynamic module loading | 暂时排除 | 在没有 E01/E02 证据前，它会引入 ABI、平台、安全和 rollback 成本。 |
| Service isolation | 保留语义核心 | child-local override 已提供 isolation；Cordis 的 proxy realm、label 和 loader integration 不必照搬。 |
| Typed event dispatch | 延后 | `emit`/`serial`/`parallel`/`waterfall` 对 runtime 有价值，但两个实验都不需要。 |
| Ambient context propagation | 不作为 authority | DeepSeek Harness 自己也将 ambient initiator state 视为 attribution，而不是 liveness 或 authorization。Rust API 应显式传递 ownership。 |

## 3. Rust 实现成本

| 模块 | 复杂度 | 证据 / 剩余成本 |
|---|---|---|
| Dependency graph | 低—中 | 已用标准库实现确定性 DFS。Versioned graph change 和更丰富 diagnostics 仍待处理。 |
| Scope model | 中 | Root/child lookup 和 shadowing 很小；multiple scope kind、routing 和并发 close 需要更多设计。 |
| Resource lifecycle | 中 | `Arc` handle ownership 和同步 disposal 已可行；异步 cleanup/quiescence 尚未解决。 |
| Hot replacement | 中—高 | 内存 publication 已实现；expected-version conflict、dependent restart 和 multi-capability rollback 仍待研究。 |
| Plugin registration | 中—高 | 可在 kernel 之上实现，但 plugin identity、unload order 和 registration effect 应单独实验。 |
| Configuration | 中—高 | Parsing/schema validation 属于 application/configuration concern，暂不选择格式。 |
| Dynamic loading | 高 | ABI、平台、安全、capability boundary 和 rollback 成本都很高，明确延后。 |
| Durability | 高 | Snapshot/version/replay 会改变 ownership model，需要 workflow-oriented experiment。 |
| Distributed runtime | 极高 | Coordination、lease、failure detection 和跨进程 ownership 超出范围。 |

## 4. 实验已经证明的内容

- 不引入第三方 graph crate，也可以让插入顺序不影响解析结果。
- 有意义的 cycle path 是小而重要的 error contract。
- Teardown order 应由 dependency order 推导，而不是由 map 或 registration 偶然决定。
- Child override 是 local publication，而不是 parent mutation。
- `Arc` 让 in-flight reader 保持旧 snapshot 有效，同时让新 lookup 看到 replacement。
- 当 publication 独立成 commit step 时，construction failure 可以保留旧 value 不变。
- Last-reader disposal 是真实的 ownership consequence，而不是测试技巧。

## E02R-F1 Scope hierarchy closure / Scope 层级收口

- 祖先 scope 拥有所有后代 scope 的生命周期。父级 teardown 会递归地、按最深层优先关闭仍然存活的后代；v0 不支持 detach、reparent 或 orphan。
- 已存在的 handle 通过精确的 `Arc` dependency snapshot 继续有效；dependent handle 会保持依赖直到自身和其他 reader 都释放。已关闭的后代拒绝 lookup、validate、provide 与 replace。
- teardown 规划把 resolver 失败视为内部不变量破坏，不再静默退回 map 顺序。逻辑 capability 顺序与精确 snapshot 的资源生命周期保持分离。
- Generation 与进程内 EntryId 的耗尽都经过 checked 处理；实现拆分为 definition/resolver 与 runtime 模块，并由精简的 `lib.rs` facade 对外导出。

## 5. 证据边界与下一步研究

E01、E02、E02R 和 E02R-F1 是同步、内存内实验。E02R 已经完成 expected-generation conflict 语义，包括结构化 `ReplacementConflict` 结果和真实双线程 stale replacement 测试；这不再是开放问题。

Capability Kernel 仍有以下边界：

- replacement 或 teardown 请求到来时，异步 resource 如何 drain 并达到 quiescence；
- provider 变化后 dependent restart / rebind policy；
- multi-capability transactional replacement；
- durability；
- distributed ownership；
- production RAII owner semantics。

这些问题的 invariants 明确之前，仍应避免引入 workflow scheduling 和 provider-specific code。
