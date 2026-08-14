# Cordis Capability Research / Cordis 能力机制研究

[English](CORDIS-CAPABILITY-RESEARCH.md) | 中文

## 研究快照

快照日期：2026-08-14，来源为本地 checkout：

- `F:\Workspace\cordis` @ `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4`（`main...origin/main`，clean）
- `F:\Workspace\deepseek-harness` @ `47f943859bef60e4160492346772ded9b24f765a`（`master...origin/master`，clean）

远端对比：

- `cordiverse/cordis` 的 HEAD 与本地 checkout 完全一致。
- `deepseek-ai/deepseek-harness` 的 HEAD 与本地 checkout 完全一致。
- 两个仓库都没有工作树差异，因此没有需要记录的 local-vs-remote drift。

范围：只研究当前源码。本文以 live repository state 为准，不假设旧文档仍然正确。

一手来源：

- Cordis：[commit 8cc9e33](https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4)
- DeepSeek Harness：[commit 47f9438](https://github.com/deepseek-ai/deepseek-harness/tree/47f943859bef60e4160492346772ded9b24f765a)

## 快速结论

- **A，Rust graph-core 必须有**：显式 ownership、幂等 disposal、staged publication、依赖声明、scope-local registration，以及不能混淆 identity 的 replacement。
- **B，之后可能需要**：hot reload/HMR、scope hierarchy、scope-aware registry，以及超越简单 pub-sub 的 event dispatch。
- **C，Cordis 特有、不应字面照搬**：基于 proxy 的 property injection、TypeScript declaration merging、AsyncLocalStorage initiator attribution，以及 Cordis effect-generator 的语法便利。
- **D，需要实验才能判断**：graph-core 是否需要 parent-linked scope admission、one-scope-per-context、reload 语义，以及 teardown composition 的准确形状。

## A. Rust graph-core 必须有

### 1. Ownership 必须显式，并与 ambient context 分离

Cordis 将 live runtime object 与 registration owner 分开：

- `Context` 是基于 proxy 的 service/fiber 外壳；实际解析由 reflect layer 完成，而不是由任意字段隐式保存 ownership。源码：[context.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/context.ts#L1-L67)
- `RegistryService.delete()` 会 teardown 与 runtime 关联的所有 fiber；`Fiber.dispose()` 是等待 in-flight work 的 quiescence boundary。源码：[registry.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L162-L170)、[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L275-L458)
- DeepSeek Harness 对 agent 也做了类似区分：`AgentHandle` 是 consumer capability，而裸 registry entry 不能负责 teardown agent。源码：[agent README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L41-L45)

对 Rust 的启示：

- 将 owned handle 与 indexed/live record 分开；
- disposal 必须幂等；
- registry entry 是 lookup state，不是 destroy authority；
- creation 应返回专门的 teardown token 或 ownership handle。

### 2. 依赖声明应发生在 composition 前，而不是通过隐藏 lookup 发现

Cordis 将 dependency requirement 直接写在 plugin 上，并在 mount 时解析：

- `Plugin.Base` 包含 `inject`、`provide`、`intercept` 和 `Config`。源码：[registry.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L63-L100)
- `Inject.resolve()` 将 array/object/prototype-chained 声明规范化为一个 dependency map。源码：[registry.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts#L42-L60)
- DeepSeek Harness 将 `inject` 作为主要依赖声明方式。来源：[Cordis primer](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-primer.md#L9-L13)、[services tutorial](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-tutorial/index.md#L40-L46)

对 Rust 的启示：

- 在 registration 时声明 required capability；
- 在 runtime work 开始前拒绝无效 composition；
- 保持依赖发现足够静态，以便在测试和 load 阶段验证。

### 3. Scope 必须是一等 owned view，而不只是一个 tag

DeepSeek Harness 的 `dsh-scope` 提供了最直接的证据：

- `createScope(ctx, key)` 创建带 tag 的 Cordis context，并返回 scoped context 与精确 disposer。来源：[scope/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L123-L146)
- `Scope.ctx` 是 registration context；通过它注册的内容由 scope 拥有，并继承 minting plugin 的 dependency API。来源：[scope/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L104-L111)
- `scopeTarget(base, key)` 只负责 routing：保留 base filter，并按 scope key/ancestor chain 接纳 listener。来源：[scope/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L158-L184)
- `scopeChainOf()` 与 `bindScopeParent()` 建立显式 parent link，并检查 parent relation cycle。来源：[scope/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L49-L101)

对 Rust 的启示：

- scope 应是有真实 ownership 和 disposal boundary 的 context/view；
- routing key 应与 subject object 分开；
- 如果需要 child/parent 行为，关系应显式且防 cycle。

### 4. Replacement 必须 staged、identity-safe，并支持 rollback

Cordis replacement 依赖精确 entry identity：

- `Fiber._reload()` 和 `_unload()` 在 loading/unloading 状态间切换，并在清理完成后更新状态。源码：[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L399-L458)
- `ReflectService.provide()` 拒绝 double registration，在 scope key 下保存 exact impl，并通知依赖者。源码：[reflect.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts#L175-L227)
- DeepSeek Harness 的 `AgentRegistry.enter()` 先创建未发布 entry，再 `announce()` 发布；旧 detach capability 不能删除后来同 id 的 replacement。源码：[agent/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L474-L575)
- agent factory 路径允许同 id creation 并行准备，但只有 exact entry 能 publish；失败者必须 rollback 自己的 private scope/session/driver。来源：[agent README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L41-L45)

对 Rust 的启示：

- 不要通过原地修改 identity 替换 instance；
- 先 stage、验证并构造，再 atomic publish；
- teardown 要绑定 exact entry/version，而不是只绑定 string id；
- race 中的 loser 必须完整 rollback 自己的 private resource。

### 5. Lifecycle 必须有序、quiescent、幂等

Cordis 与 DeepSeek Harness 都把 disposal 当作 lifecycle boundary，而不是简单 drop：

- Cordis `effect()` 收集 sync/async/generator disposer，维护 child effect metadata，并压制 cleanup path 的 unhandled rejection。源码：[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L275-L339)
- `Fiber._unload()` 等待所有 disposer，并能在 unload 期间 epoch 改变时重新进入 reload。源码：[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L437-L458)
- DeepSeek Harness 说明 agent loop 在 lifetime 内拥有一个 agent，先 drain loop、注销 agent、移除 session state，再 unwind scoped world。来源：[agent README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L45-L51)、[agent-loop README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/README.md#L62-L70)

对 Rust 的启示：

- teardown 应是 quiescence boundary，而不是 best-effort destructor；
- 如果 teardown 会与新 work 竞争，必须定义新 work 是被拒绝、锁存还是 replay；
- 暴露幂等 disposal，便于安全组合 ownership。

## B. 之后可能需要

### 1. Hot reload / HMR 语义

Cordis 在 `Fiber._reload()` 中支持 reload-like behavior，DeepSeek Harness 用相同模型完成 plugin hot reload 和 adapter-default rematerialization。来源：[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L399-L458)、[HMR tutorial](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-tutorial/06-composition-and-hmr.md)、[agent-loop README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/README.md#L68-L70)。

如果 graph-core 后续需要 live reconfiguration，这些语义会有用，但不是第一阶段要求。

### 2. Scope-aware registry 与 layered visibility

DeepSeek Harness 的 `ScopedLayers` 适合后续 per-scope overlay：global layer 与 lazy exact-scope layer 分开，`peek()` 不沿 chain，`merge()` 沿 parent chain。来源：[scope/store.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/store.ts#L159-L241)、[scope README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/README.md#L25-L31)。

这可能适合 configurable overlay，但只有真正需要 per-scope shadowing 时才应引入。

### 3. 超越普通 pub-sub 的 event dispatch mode

Cordis 支持 `emit`、`serial`、`waterfall`、`parallel` 和 `bail`；DeepSeek Harness 在 agent policy 上继续使用这些模式。来源：[events.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/events.ts#L14-L178)、[Cordis primer](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/docs/cordis-primer.md#L15-L34)、[agent dispatch](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/dispatch.ts#L54-L175)。

如果 graph-core 需要 policy hook、retry 或 around-middleware，这些模式可能有用；但 E01/E02 都不需要它们。

### 4. Durable session / agent replay

DeepSeek Harness 的 agent loop 包含大量 replay-specific logic，例如 `request/header`、`request/context`、turn boundary、`assistant/chunk` stream anchoring 和 cancellation replay。来源：[agent-loop/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent-loop/src/index.ts#L332-L495)。

如果 graph-core 未来支持可恢复 agent run，这些内容相关；但超出 capability substrate 第一阶段。

## C. Cordis 特有、不应照搬

### 1. Proxy-based property injection

Cordis 通过 proxy trap 和 `ReflectService.handler` 解析未知属性，所以 `ctx.foo` 可以像 service lookup 一样工作。来源：[reflect.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts#L61-L124)。

这适合 TypeScript ergonomics，但在 Rust 中会隐藏过多语义。应优先使用显式 accessor 或 typed handle。

### 2. TypeScript declaration merging

DeepSeek Harness 大量使用 `declare module ...` 扩展 Context、event map 和 lookup map。来源：[agent/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L26-L49)、[scope/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/scope/src/index.ts#L23-L37)。

Rust 应通过显式 trait/registry 表达，而不是 ambient global merge。

### 3. AsyncLocalStorage initiator scope

DeepSeek Harness 用 AsyncLocalStorage 在异步 work 中记住当前 initiating agent；其 README 明确指出 ambient presence 既不是 liveness proof，也不是 authorization。来源：[agent/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/src/index.ts#L1-L17)、[agent README](https://github.com/deepseek-ai/deepseek-harness/blob/47f943859bef60e4160492346772ded9b24f765a/packages/core/agent/README.md#L26-L35)。

Rust 中它最多是 traceability aid，不能作为 security primitive。

### 4. Cordis effect-generator composition

Cordis `effect()` 接受 sync value、async value、iterable、async iterable 和 generator-style teardown chain。来源：[fiber.ts](https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts#L229-L339)。

应保留 ordered teardown 的思想，不要照搬 generator/iterable 语法。

## D. 暂时无法判断 / 需要实验

当前源码提供了方向，但 graph-core 不应在 Rust 实验前锁定以下设计：

1. Rust scope 是只支持每个 context 一个 nearest tag，还是允许多个 membership？
2. ancestor scope 是否自动接收 descendant event，还是采用更扁平、显式的 routing rule？
3. reload/replacement 应保留 stable identity object，还是每次 replacement 都生成 versioned identity？
4. teardown 应是单一 quiescence future，还是可组合的 owned cleanup token chain？
5. graph-core 是否需要保留 Cordis 风格的 `inject`/`waterfall`/`serial` 区分，还是先收缩成更小的 event API？
6. replacement 应像 DeepSeek Harness 的 agent 一样在 publication 前并行 prepare，还是只在 visible rollback path 下启动？

## Rust 启示总结

如果 graph-core 只吸收有价值的部分，Rust 形状应当是：

- 保存 exact live entry 的 registry；
- 每个 composition unit 都有 owned handle；
- mount 时声明显式依赖；
- 拥有 registration context 和 quiescent disposer 的 scope object；
- 支持 rollback 的 identity-safe replacement；
- 明确区分 durable lineage、runtime ownership 和 ambient attribution。

它不应当是：

- 充满 proxy magic 的 dynamic lookup layer；
- 隐式的 ambient authority system；
- Cordis effect/proxy/declaration-merging mechanics 的 TypeScript 直译。
