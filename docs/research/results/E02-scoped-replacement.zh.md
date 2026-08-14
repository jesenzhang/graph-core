# E02 — Scoped capability replacement / Scope 内 capability 替换

[English](E02-scoped-replacement.md) | 中文

日期：2026-08-14
状态：已完成

## 研究问题

一个最小的 parent/child scope 模型，是否能够支持继承 capability、本地 override、in-flight reader 安全，以及事务式 replacement，同时不修改或释放 parent 拥有的 value？

本实验有意只使用 root scope 和任意 child scope，不假设 `Root -> Runtime -> Session -> Task` 一定是最终层级。验证的 primitive 是“child 可以继承并覆盖”；只有在证据需要时，才增加带名称的应用层级。

## Cordis 对应机制

当前本地 Cordis checkout（`8cc9e33`）通过 context 提供 service 查找，把 service 注册为可逆 effect，并在 fiber dispose 时卸载插件拥有的 effect。其 loader 还可以在 group 内隔离 service name，让 sibling group 看到不同 provider。DeepSeek Harness 也明确说明：scoped context 的 registration context 同时决定可见性和 ownership。

最相关的源码快照和直接链接见 [`CORDIS-CAPABILITY-RESEARCH.zh.md`](../CORDIS-CAPABILITY-RESEARCH.zh.md)，尤其是 scope ownership、replacement、lifecycle 和 service isolation 部分。

Rust 实验保留三点：

1. child lookup 找不到本地值时回退到 parent；
2. local provider shadow parent，但不修改 parent；
3. cleanup 属于发布 local provider 的 scope。

不实现 proxy context、plugin fiber、异步 effect generator 或 dynamic module loading。

## 候选设计

| 设计 | 问题 | 决策 |
|---|---|---|
| 原地修改一个共享 instance | reader 可能看到部分更新，也无法安全保留 V1。 | 否决。 |
| 先删除 V1，再构造并发布 V2 | constructor 失败会让 scope 为空或损坏。 | 否决。 |
| 把所有继承值复制到每个 child | teardown 和 ownership 不清晰，parent 更新也无法传播。 | 否决。 |
| `Arc` value + scope-local map + child fallback | 发布是本地的，reader 拥有稳定 snapshot，parent ownership 保持独立。 | 采用。 |
| 第三方 atomic-swap crate | 本次同步 `RwLock` 实验已足够；不在测量需要前锁定设计。 | 延后。 |

## 最终实验设计

`Scope` 通过 `RwLock` 持有本地 `BTreeMap<CapabilityId, CapabilityEntry>`，并保存可选的 parent `Scope`。`get()` 先查本地 map，再递归查 parent。

`CapabilityEntry` 持有：

- `Arc<CapabilityDefinition>`，表示已发布的 metadata；
- `Arc<InstanceSlot>`，表示 runtime resource。

`CapabilityHandle` 是 reader-owned 的两个 Arc 的 clone。`InstanceSlot` 持有一个 boxed `CapabilityInstance`；只有最后一个 scope/reader reference 释放时，`Drop` 才调用 `dispose()`。因此行为是：

```text
scope 持有 V1
reader 持有 V1
replacement 发布 V2，并释放 scope 对 V1 的引用
reader 继续使用 V1
reader 释放 V1 -> V1 被 dispose
```

replacement 流程为：

```text
检查 scope 未关闭
→ 通过当前 scope lookup 验证所有声明的依赖
→ 执行 constructor
→ 获取写锁，原子替换 local map entry
→ 在锁外释放 old entry
→ Arc lifetime 决定 old disposal 何时安全
```

construction error 发生在 map 写入前，所以失败的 replacement 会保留原 entry，并继续可用。child replacement 只写 child map；child teardown 永远不会移除 parent entry。

## 实现方式

实现位于 [`crates/capability-graph/src/lib.rs`](../../../crates/capability-graph/src/lib.rs)。公共 API 有意保持很小：

- `Scope::root()` 和 `Scope::child()`；
- `Scope::get()`；
- `Scope::provide()` / `Scope::replace()`；
- `Scope::teardown()`；
- `CapabilityInstance`、`CapabilityHandle` 和 `ScopeError`。

`CapabilityInstance` 是同步资源边界，提供显式 `dispose()` 和仅用于实验类型检查的 `as_any()`。没有 Tokio、async trait、dynamic library 或远程 resource protocol。

map write 是 publication point。reader 不会通过 map 长期借用；它只在读锁内 clone 一个 `Arc`，之后独立执行。这正是 in-flight reader 测试要证明的 ownership 属性。

## 测试结果

定向验证：

```text
cargo test -p capability-graph --all-features
16 passed; 0 failed
```

E02 测试包括：

- `child_inherits_parent_capability`
- `child_override_does_not_mutate_parent`
- `sibling_scope_isolation`
- `replacement_changes_new_reads`
- `in_flight_reader_survives_replacement`
- `failed_replacement_keeps_old_capability`
- `child_teardown_disposes_owned_capabilities`
- `child_teardown_does_not_dispose_parent_capability`
- `replacement_disposes_old_capability_when_safe`

`in_flight_reader_survives_replacement` 跨线程持有 V1，在主线程发布 V2，并验证 V1 只有在 reader 释放后才 dispose。

## 发现

1. child override 最自然的表示是新的 local entry，而不是修改 inherited entry。
2. `Arc` 在不使用 unsafe code、也不需要立即失效协议的情况下解决了 reader 有效性问题。
3. 当 constructor 无法访问 publication map、commit 是一次写锁 swap 时，事务式 replacement 更容易推理。
4. scope teardown 可以是本地且幂等的：只 drain local map，再让 handle 决定 resource 何时达到可 dispose 状态。
5. 最小 root/child 模型已经足以验证核心不变量；本实验没有证据表明必须硬编码四个层级。

## Rust ownership 的影响

Rust 让 lifetime contract 变得可见。返回的 `CapabilityHandle` 不只是 metadata，它是对 runtime instance 的 ownership claim。scope 可以停止发布 V1，而不会让已有 handle 变成悬空引用。反过来，在 reader 仍然拥有旧值时，也无法安全承诺“立即” dispose；dispose 必须等到最后一个 `Arc` 引用释放。

这与最简单的 Cordis mental model 不同：Cordis 卸载 provider 时通常也会卸载依赖它的 plugin。Rust 实验选择保留旧 reader，而不是强制重启它。这适合当前 kernel，但 dependent restart/quiescence policy 仍应由上层决定。

## 应保留、延后和拒绝的内容

- 保留：显式 scope ownership、parent fallback、local shadowing、稳定 reader handle、事务式 publication 和幂等 teardown。
- 延后：异步 dispose、依赖触发的 dependent restart、versioned replacement conflict 和 scope event routing。
- 不照搬：TypeScript proxy lookup、将 ambient async context 当作 authority，以及 Cordis-compatible plugin loader。

## 未解决问题

- replacement 是否需要 expected version，来阻止并发 constructor 的 last-writer-wins race？
- provider replacement 后 dependent 应如何响应：保留稳定 reader、重启，还是接收显式 transition event？
- `CapabilityInstance::dispose` 是否应变成 async，还是由更高层的 quiescence manager 包装异步 resource？
- named Runtime/Session/Task scope 是否有价值，还是 generic child chain 已经足够？
