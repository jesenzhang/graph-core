# E01 — Capability dependency resolution / Capability 依赖解析

[English](E01-capability-resolution.md) | 中文

日期：2026-08-14
状态：已完成

## 研究问题

一个小型 Rust capability graph 是否能够在不引入图算法第三方库、也不耦合运行时概念的前提下，提供确定性的依赖解析、有用的 cycle 诊断，以及明确的构造/销毁顺序？

本实验明确排除 provider、tool、agent、MCP、网络、配置、序列化和持久化，只研究 composition kernel 本身。

## Cordis 对应机制

当前本地 Cordis checkout（`8cc9e33`）通过插件的 `inject` 列表声明所需 service。插件在依赖 service 存在之前保持 pending；决定插件何时启动的是依赖声明，而不是配置文件中的排列顺序。DeepSeek Harness 的 Cordis primer 和 services tutorial 也记录了相同语义。相关源码快照和链接见 [`CORDIS-CAPABILITY-RESEARCH.zh.md`](../CORDIS-CAPABILITY-RESEARCH.zh.md)。

Rust 实验保留了有价值的不变量：依赖必须显式声明，并在构造前验证；但不实现 Cordis 的 pending fiber、proxy 属性查找或 plugin loader。

## 候选设计

| 关注点 | 候选方案 | 结果 |
|---|---|---|
| Definition 存储 | `HashMap`，每次使用时排序 | 否决：很容易不小心暴露迭代顺序。 |
| Definition 存储 | 以 `CapabilityId` 为键的 `BTreeMap` | 采用：根节点和直接访问都稳定。 |
| 依赖列表 | 无序 vector | 否决：除非每次遍历都重新排序。 |
| 依赖列表 | `BTreeSet<Dependency>` | 采用：等价 definition 具有相同遍历顺序，重复边自动消除。 |
| 解析算法 | Kahn queue | 可行，但 cycle 路径需要额外的 predecessor/path 记录。 |
| 解析算法 | 带 active path 的确定性 DFS | 采用：直接得到依赖优先顺序和 `A -> B -> C -> A` 诊断。 |
| 缺失依赖 | 构造时再忽略 | 否决：composition 必须在运行时工作开始前失败。 |

不需要第三方 crate。标准库集合直接表达了排序不变量；对于这个规模的 graph，直观的 DFS 比更通用的图算法更容易产生清晰诊断。

## 最终实验设计

`CapabilityDefinition` 包含：

- 稳定的 `CapabilityId`；
- 人类可读的 capability kind；
- 一个有序的 `Dependency` 集合。

`CapabilityGraph::resolve()` 按排序后的顺序遍历 capability identifier 和依赖。只有在某个依赖的全部依赖都访问完成后，该依赖才会被加入构造顺序。返回的 `ResolvedCapabilityGraph` 暴露：

- `construction_order()`：依赖优先；
- `teardown_order()`：构造顺序的严格逆序。

缺失依赖表示为 `CapabilityGraphError::MissingDependency { capability, dependency }`。cycle 表示为 `CapabilityGraphError::Cycle { path }`，path 包含重复出现的起始 identifier。

为了兼容初始 baseline，旧的 `Capability { id, kind }` 仍然可以传给 `CapabilityGraph::insert()`，并会被转换为无依赖 definition。需要声明依赖的新代码应使用 `CapabilityDefinition`。

## 实现方式

实现位于 [`crates/capability-graph/src/lib.rs`](../../../crates/capability-graph/src/lib.rs)，只使用：

- `BTreeMap<CapabilityId, CapabilityDefinition>` 保存节点；
- `BTreeSet<Dependency>` 保存直接依赖；
- `Active`/`Done` 两状态 DFS；
- active recursion path 生成结构化 cycle 错误。

`require()` 会立即验证两个 endpoint。通过 `depends_on()` 构造的 definition 可以按任意插入顺序加入 graph，并在 `resolve()` 时统一验证，因此 graph builder 不依赖插入顺序。

## 测试结果

定向验证：

```text
cargo test -p capability-graph --all-features
16 passed; 0 failed
```

E01 测试包括：

- `resolve_simple_dependency`
- `resolve_multi_level_dependency`
- `resolution_is_deterministic`
- `missing_dependency_is_rejected`
- `cycle_is_rejected`
- `cycle_error_contains_path`
- `teardown_order_is_reverse_resolution_order`

同一个 crate 中其余定向测试覆盖 E02。

## 发现

1. 当排序成为数据模型的一部分时，确定性解析成本很低，不需要第三方 graph crate。
2. DFS 能直接给出有意义的 cycle path，比单纯返回布尔 cycle 标记更有用。
3. 构造和销毁顺序是 graph 语义，而不是 runtime registry 的偶然属性，因此可以在构造任何资源前测试。
4. 可以先组装引用 provider 的 definition，再插入 provider definition；但不完整 graph 必须在 resolve 时拒绝，不能默默忽略缺失要求。

## 对 ownership 的影响

E01 不拥有 runtime instance，只返回一个不可变的顺序结果，供 scope/lifecycle 实验使用。将 graph 验证与资源 ownership 分离，可以避免 dependency graph 负责释放任意对象。

这与直接移植 Cordis 的思路不同：Cordis 把依赖可用性与 fiber 激活、unload/reload 状态结合在一起。Rust `graph-core` 可以保留依赖不变量，同时把激活、ownership 和并发交给 `Scope`。

## 应保留、延后和拒绝的内容

- 保留：显式依赖声明、fail-fast 验证、确定性顺序、结构化 cycle path。
- 延后：pending 激活、依赖触发的重新激活、event dispatch、配置 overlay 和异步生命周期状态机。
- 不照搬：proxy 风格的 `ctx.service` 查找、TypeScript declaration merging，以及放在 graph crate 内的 plugin loader。

## 未解决问题

- 后续 graph revision 是否应支持在 instance 活跃时替换 definition 的依赖集合？
- 当 runtime 构造需要 metadata 时，resolve 是否应同时返回 definition 而不只是 identifier？
- 异步资源依赖是否需要独立的 ready/active 状态，还是由 scope 层在不改变 graph 的情况下负责？
