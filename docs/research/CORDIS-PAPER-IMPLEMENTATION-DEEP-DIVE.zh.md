# Cordis 论文 × 实现深度调研

[English](CORDIS-PAPER-IMPLEMENTATION-DEEP-DIVE.md) | 中文

日期：2026-08-18

## 0. 研究基线与结论

本报告是对既有 [`CORDIS-CAPABILITY-RESEARCH.zh.md`](CORDIS-CAPABILITY-RESEARCH.zh.md) 的后续校正。旧报告刻意以源码为唯一权威；本报告加入 2026-08-13 发布的 Cordis 论文，并把论文的形式化模型、Cordis v4 当前实现和 graph-core 已集成的 M2-A 能力逐项对照。

冻结证据：

- Cordis 论文：[`cordiverse/paper`](https://github.com/cordiverse/paper/tree/948a07b369c62adb3b12e102458be5c18dfb69b9)，commit `948a07b369c62adb3b12e102458be5c18dfb69b9`，论文标题 *A Programming Paradigm for Spatiotemporal Composability*。
- Cordis：[`cordiverse/cordis`](https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4)，commit `8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4`，`packages/core` 版本 `4.0.0-rc.8`。
- graph-core：本次研究从 `main@62f556d37a1f8c4c7c5cd26d9e21917abe17816a` 开始；M2-A 已集成。

### 最终判断

既有 **YES, BUT NARROWER** 决策仍然成立，但论文让边界更清晰：

1. **Cordis 不是“动态图插件框架”本身，而是 Context-mediated 的时空可组合性范式。** 图是依赖关系的派生结果，不是需要预先构造后再调度的唯一执行对象。
2. **graph-core 已正确吸收了其中一部分核心不变量**：显式 capability identity、依赖声明、精确 provider identity、作用域、fiber 生命周期、effect ownership、replacement generation、失败隔离。
3. **M2-A 不能等同于“完整移植 Cordis v4 spatial composability”。** 当前 graph-core 有 `DependencyEpoch` 和显式 `notify_dependency_change()`，但在本次检查到的代码中，`Scope::provide/replace/remove` 与所有依赖 fiber 之间没有 Cordis `ReflectService.notify()` 那样的自动反应链；provider withdrawal 也没有完整实现论文的 committed-view + dependent-drain guard。
4. **Cordis 的 effect inverse 不能替代 graph-core 的 durable effect model。** 论文第 6.1 节明确把不可恢复的外部 emission 放在 system boundary 之外；graph-core 的 intent / dispatch / outcome、idempotency、reconciliation 仍应保持独立权威。
5. **下一步最值得研究的不是 loader/HMR，而是 reactive dependency lifecycle。** 即：provider 身份变化如何自动触发 dependent fiber 的安全退场、重绑定和重启，同时不破坏 M1 已冻结的 in-flight task capability pinning。

---

## 1. 论文真正解决的问题

论文把动态组合拆成两个正交维度。

### 1.1 Temporal composability：组件退出后能恢复自己的贡献

一个 effect 不再只是 `Γ -> Γ`，而是概念上的：

```text
Γ -> Γ × inverse
```

即 effect 在应用时同时给出一个 inverse；runtime 负责追踪这些 inverse，并在 component 被卸载时组合执行。

关键点不是“有一个 teardown hook”，而是：

- inverse 与创建 effect 的位置局部绑定；
- composite effect 的 inverse 由原子 inverse 自动组合；
- 同一个 effect 的重复 dispose 必须至多执行一次；
- 一次 activation 只要已经完成了前 N 个原子 effect，就应该能够只恢复这 N 个；
- 多组件 effect 交错时，任意顺序撤回需要额外的 independence / commutativity 条件。

这最后一点非常重要：论文的全局 temporal composability **不是无条件的“任意副作用都可以回滚”**。它依赖 effect witness、observational equivalence 和 independence discipline。

### 1.2 Spatial composability：依赖变化驱动组件生命周期

组件声明 coeffect specification，即自己需要哪些环境能力。共享 context 的变化会被按 specification 分类为：

```text
activating | deactivating | neutral
```

因此依赖不再是“初始化时注入一次”，而是持续成立的运行时关系：

```text
provider 出现      -> consumer 可激活
provider 身份变化  -> consumer 旧 binding 失效并重载
provider 消失      -> consumer 先退场，再允许 provider 真正回收
```

这才是 Cordis 相比普通 DI 容器更有价值的部分。

### 1.3 Unified Context：effect 与 coeffect 是同一个运行时介质的两面

论文把 effect context 与 coeffect context 合为递归 Context：

```text
Context
├─ current state
├─ accumulated inverse
└─ dependency/coeffect state
```

组件与环境之间的可组合交互都通过 Context 发生。这样 runtime 才能把“谁创建了什么 effect”和“谁依赖了哪个 provider”归属于同一个 component/fiber 生命周期。

### 1.4 Component 与 Fiber

论文中的 component 是三元组：

```text
Component = required dependencies (d)
          + provided keys (p)
          + witnessed effect function (e)
```

Fiber 是 component 的一次运行时实例。正式生命周期扩展为：

```text
INACTIVE
  -> RELOADING
  -> ACTIVE
  -> UNLOADING
  -> INACTIVE / FAILED
```

并进一步处理：

- withdrawal；
- effect iteration；
- async inertia；
- failure；
- parent/child instantiation；
- committed dependency view。

因此 Cordis 的“动态”首先是 **component lifecycle reacting to context changes**，而不是工作流 scheduler 动态修改 DAG。

---

## 2. 论文模型如何落到 Cordis v4 当前实现

论文第 5 章给出 theory-to-implementation correspondence。当前 `8cc9e33` 源码与其总体一致，但字段名和实现细节并不是论文伪代码的逐字复制。

### 2.1 `ctx.effect`: revertible effect 的真正落点

当前 `packages/core/src/fiber.ts` 的 `Fiber.effect()`：

- 接受 sync / async callback；
- 支持 iterable / async iterable 逐步产生 disposer；
- 每个 disposer 被归到当前 fiber；
- wrapper 有 armed/epoch 语义，重复 dispose 不会重复执行；
- setup 中途失败时会回收已经注册的部分；
- nested effect 元信息被保留。

测试 `packages/core/tests/dispose.spec.ts` 明确覆盖：

- repeated disposal；
- nested/yielded inverse 的 LIFO；
- async iterator 的 partial abort；
- setup error 后只清理已完成部分。

但论文也明确承认：**runtime 不验证 disposer 真的是 effect 的 inverse**。这是 component author 的 contract，而不是 Cordis 能动态证明的性质。

#### 一个实现层细节

论文把 component accumulator 描述成一个 LIFO inverse composition；当前源码有两层：

1. 一个 `ctx.effect()` 内部的 yielded / nested disposer 严格按 LIFO 串行组合；
2. `Fiber._unload()` 对 fiber 的顶层 `_disposables.clear()` 取得 reverse-order 列表后，通过 `Promise.all(...)` 启动清理。

因此不能把当前 Cordis 源码概括成“所有 fiber-level effect 都严格串行 LIFO 完成”。更准确地说：**局部 composite effect 保持 LIFO；顶层 sibling cleanup 可以并行完成。** 论文的 global guarantee 依赖 independence discipline 来解释这种可交换性。

这也说明 graph-core 当前 `EffectStack::dispose_all()` 的严格反序串行执行是一个更保守、更强排序的 Rust 选择，而不是必须逐字复制 Cordis 的实现。

### 2.2 `inject` + provider identity：reactive coeffect 的核心

`Plugin.Base.inject` 声明 required dependencies；`ReflectService.provide()` 把 provider 绑定到当前 fiber。

Fiber 内部同时维护两类 resolution：

- `_store`：当前环境下重新解析得到的 provider；
- `store`：当前 activation 已经 commit 的 provider view。

`_refresh()` 用 provider fiber UID 计算 dependency epoch；不是用 provider value 做比较。因此：

- 同一个 provider 原地改变 value，不被视为 provider replacement；
- 新 provider 即使提供完全相等的 value，也因为 UID 不同而被识别为新的 binding。

这与论文 `target`/`committed view` 的设计完全一致：**dependency identity 是 provider identity，不是 value equality。**

### 2.3 provider withdrawal：Cordis 最值得 graph-core 继续研究的部分

`ReflectService.provide()` 的 disposer 有一个非常关键的顺序：

```text
1. 从全局可解析 store 中删除 provider
2. notify 所有受影响 consumer
3. await consumers 的 fiber.await()
4. 最后才从 provider 自己的 committed store 中删除 binding
```

含义是：

- 新 consumer 立即看不到 provider；
- 已经 commit 到该 provider 的 consumer 被驱动进入 unload；
- consumer teardown 期间仍可以使用自己 committed 的旧 binding；
- provider 的最终资源回收等待 dependent teardown 完成。

这正是论文 withdrawal guard 的工程实现。它不是普通的“reverse topological drop”，而是一个 **live dependency handoff protocol**。

### 2.4 inertia：不是取消 transition，而是让它落地后再纠偏

当前 `Fiber._setEpoch()`、`_reload()`、`_unload()` 用 `fiber.inertia` 串行化 lifecycle transition。

如果依赖在 async load 过程中变化，Cordis 不假装已经启动的 async work 可以瞬间取消。旧 transition 先完成，再根据新的 epoch 链入 unload/reload。这就是论文的 inertia：

```text
transition started
-> target changes while in flight
-> current transition lands
-> runtime observes stale target
-> unload/reload to converge
```

这比“遇到 provider change 就直接 abort future”更符合真实异步资源初始化。

### 2.5 isolation / interception

`Context.isolate()` 通过新的 realm symbol 派生 context，使同一个逻辑 key 在不同 context 解析到不同 provider。

`Context.intercept()` 派生 metadata/config overlay；`Service.resolveConfig()` 沿 context 原型链收集并合并 interception。

这两者在论文里都是 **derived realization**：不是修改共享 dependency table 再生成 inverse，而是创建新的 context view；context 被丢弃时，隔离/拦截自然消失。

Rust 不需要 Proxy/原型链才能保留这个语义，graph-core 现有显式 `CapabilityContext` child/isolate/intercept 是合理 adaptation。

---

## 3. Loader 与 HMR：值得借鉴，但不属于 capability kernel

### 3.1 Component Loader 是 desired-state reconciliation 层

`@cordisjs/plugin-loader` 的 Entry / EntryTree 把配置树当成声明式 desired composition：

- entry 描述 component、config、inject、disabled、group；
- config 变化驱动 fiber update/re-init；
- component 自己修改 config/disable 也会写回 entry；
- tree 管理 create/remove/update 和 import。

这层解决的是 **orchestration/configuration**，不是 Context/coeffect 的最低层语义。

因此 graph-core 继续把 loader/config language 排除在 capability kernel 外是正确的。

### 3.2 HMR 是三阶段 reload transaction

论文与当前 `packages/hmr/src/index.ts` 都展示了三层结构：

1. **module classification**：changed files 向 import graph 传播 accepted/declined；框架 external 变化触发 full restart；
2. **stale-entry detection**：只选择 dependency tree 命中 changed module 的 component entry；
3. **transactional reload**：备份并清除 ESM/CJS cache，先 re-import，再替换 fibers；失败则恢复 cache 并重建旧 fibers。

这对 graph-core 未来 multi-capability replacement 有启发：

```text
prepare new artifacts
-> validate all
-> commit publication
-> retire old
-> rollback private/prepared state on failure
```

但 Cordis HMR 不是 durable ACID transaction：它是单进程 Node module cache + fiber lifecycle 上的 best-effort transactional swap；旧 plugin dispose 的错误会记录并继续。它不能直接成为 durable execution 证据。

---

## 4. System Boundary：为什么 Cordis 不能代替 durable execution

论文第 6.1 节是本次调研对 graph-core 最重要的新证据之一。

它把外部操作分为两阶段：

### 4.1 Acquisition 通常可以落在 Context 内

例如：

```text
open  -> close
malloc -> free
fork   -> kill
listener register -> unregister
```

系统拥有一个内部 record，并且能独占地撤销这个 record，因此可以建模为 revertible effect。

### 4.2 Emission 通常已经越过 Context 边界

例如：

```text
write bytes to external file
send datagram
send email
charge payment
invoke non-idempotent external mutation
```

数据一旦被外部观察者接收，`inverse` 不能让世界“从未发生过”。论文给出的办法只有：

- **withholding/output commit**：直到状态确定持久化后才让 emission 越界；
- **compensation**：执行业务定义的补偿动作，但恢复的是更粗的业务等价，而不是原始状态恒等。

这与 graph-core 已有 M1 / M2-B durable authority 恰好互补：

```text
Capability Runtime / Cordis-like effects
    owns process-local composition and acquired-resource lifetime

DurableJournal
    owns operation intent / dispatch / outcome / reconciliation truth
```

因此必须保持以下规则：

> `ScopedEffect` / disposer 不能成为外部 effect 已成功、失败或可以重试的权威。

支付、消息发送、远程 mutation 等仍应走 `OperationId`、`AttemptId`、idempotency 与 reconciliation 语义。

---

## 5. Cordis 论文与 graph-core 当前实现逐项对照

| 论文 / Cordis 机制 | Cordis v4 实现 | graph-core 当前状态 | 判断 |
|---|---|---|---|
| Revertible atomic effect | `ctx.effect` + disposer | `ScopedEffect` / `EffectScope` / `EffectStack` | 已吸收核心语义 |
| LIFO composite recovery | yielded/nested effect 严格 LIFO；fiber sibling cleanup 可并行 | `EffectStack` 严格反序串行 | Rust 选择更保守；保留 |
| Runtime verifies inverse | **不验证** | 不验证 disposer correctness | 必须作为 contract/test obligation |
| Reactive coeffect declaration | `inject` | `Requirement` / dependency definitions | 已实现 |
| Provider identity target | fiber UID | `Generation + EntryId` / `DependencyEpoch` | 已实现，而且 identity 更显式 |
| Committed dependency view | `fiber.store` committed snapshot | `ResolvedDependencies` + retained handles | 已实现 identity/lifetime 部分 |
| Context-change automatic notification | `ReflectService.notify()` 自动 `_refresh()` | `notify_dependency_change()` 是显式入口 | **部分实现；不是完整 reactive propagation** |
| Provider withdrawal guard | provider 先退出 shared resolution，再 drain dependents，最后 self cleanup | scope teardown 有 dependency order + Arc snapshot lifetime；fiber unload 未见自动 dependent-drain chain | **重要差异，值得下一阶段验证** |
| Async inertia | `fiber.inertia`, reload/unload chaining | `AsyncMutex` serialized transition + stale epoch/token check | 已做 Rust adaptation |
| Failure isolation | per-fiber FAILED | per-fiber `Failed` + cleanup errors | 已实现 |
| Isolation | realm-based derived context | explicit isolated child context | 已实现语义核心 |
| Interception | prototype metadata overlay | explicit intercept config | 已实现语义核心 |
| Declarative loader | Entry/EntryTree | 排除 | 正确排除 |
| HMR | classify/detect/reload+rollback | 排除 | 正确排除；保留为 future transaction reference |
| Effect independence / observational equivalence | 形式化前提 | 未建模 | 不应宣称全局 Cordis metatheory |
| Durable external effects | 明确在 system boundary 外 | DurableJournal 单独拥有 | graph-core 分层更合适 |

---

## 6. 一个必须澄清的“两种 pinning”

Cordis 的 reactive fiber 和 graph-core M1 的 task attempt pinning 看起来容易冲突，其实处于不同层级。

### Component/Fiber 层

一个 component 如果依赖的 provider identity 变化，下一次稳定状态应该重新绑定；必要时 unload/reload。

### Task Attempt 层

M1 已冻结：一个已经开始执行的 task attempt 持有 exact `CapabilityHandle`；provider replacement 只改变未来 lookup，不改变该 attempt 已经拿到的能力。

两者应该同时成立：

```text
provider V1 replaced by V2

existing task attempt
    -> keeps V1 handle until attempt ends

component/fiber lifecycle
    -> notices provider target changed
    -> stops admitting new work / unloads when safe
    -> rebinds to V2

future task attempt
    -> resolves V2
```

因此未来实现 reactive coeffect 时，**不能通过修改一个已经启动的 TaskAttempt 的 dependencies 来模拟 Cordis reload**。应把 quiescence / admission boundary 放在 fiber/provider 层。

---

## 7. 论文暴露出的当前 graph-core 缺口

这些不是对 M2-A 集成结果的追溯性否定；M2-A 的 bounded port 仍然成立。它们是论文新增证据后更精确的 scope boundary。

### P0：自动 reactive propagation 尚未闭环

当前 `CapabilityFiber::notify_dependency_change()` 只增加 epoch；本次检查到的 `Scope::provide/replace/remove` 没有直接维护“provider -> dependent fibers”观察关系，也没有自动调用所有相关 fiber。

如果目标是完整 Cordis-style spatial composability，需要证明：

- provider publish / replace / withdraw 会自动找到受影响的 fibers；
- 只影响实际依赖该 binding 的 fiber；
- sibling/isolated realm 不被误触发；
- 通知风暴可以合并，但不能漏掉最终 target；
- failed/disposed fiber 不会被错误复活。

### P0：provider withdrawal 的 dependent-drain protocol

graph-core `Scope::teardown()` 已经有 deterministic dependency-aware drop order，并且 exact dependency `Arc` snapshot 能让旧 provider 实例活到最后一个 reader 消失。

这保证了 memory/lifetime safety，但和 Cordis 的语义仍有差异：

- Cordis 先让 provider 从未来 resolution 中消失；
- 让 consumer 自己执行 teardown；
- consumer teardown 仍可读取 committed provider；
- provider 等 consumer 到稳定退场后再执行自身回收。

因此后续要验证的不是简单“逆拓扑 drop”，而是 **withdraw visibility + committed access + dependent quiescence + provider recovery** 四件事的顺序。

### P1：effect independence 没有成为 graph-core contract

当前 Rust 选择串行 reverse cleanup，因此暂时绕开了很多 independence 问题，这是合理的。

不要为了“像 Cordis”而引入并发 teardown。只有当性能数据证明 cleanup 并行值得时，才需要显式分类：

```text
ordered effects
independent effects
external/durable effects
```

并为 independent group 建立 commutativity/ownership contract。

### P1：CapabilityId 还不等于 interface compatibility

`Generation + EntryId` 很好地解决“是不是同一个 provider publication”，但不能解决独立组件生态中的：

- key collision；
- interface drift；
- behavioral contract version mismatch。

论文第 6.6 节给出 namespacing、peer dependency/semver、structural compatibility 三条路线。

Rust 对 graph-core 更自然的后续方向可能是：

```text
CapabilityId = namespace + logical name
CapabilityContract = typed trait / schema / optional version fingerprint
```

但现在没有证据需要立即扩大公共 API；先把它记录为插件生态阶段的问题。

### P2：HMR / dynamic loading 仍应在 kernel 外

论文没有改变现有决策。它只提供了未来 replacement coordinator 的好参考：prepare/import 与 publish/retire 分离，并保留 rollback material。

---

## 8. Rust 实现上的取舍

### 8.1 不复制 Proxy

论文第 6.4 节明确承认语言实现可以不同，并特别指出 Rust procedural macro 可以生成 typed declaration/accessor。

当前 graph-core 的显式 typed handle/context 比模拟 JS Proxy 更符合 Rust：

- ownership 清楚；
- authority 不隐藏在 ambient lookup；
- compiler 能帮助约束生命周期；
- 调试时 identity 可直接打印。

只有在 consumer API 的声明样板成为真实成本时，才值得评估 derive/proc-macro ergonomics。

### 8.2 循环依赖继续 fail-fast

论文的基础 reactive 模型中，dependency cycle 会让相关 components 永远无法满足，因此停在 inactive；论文也指出 cycle 可以从 declarations 预先预测并报告。

graph-core 当前 resolver 在 admission 时直接给出 cycle error 是更适合基础内核的策略，应保持。

### 8.3 严格 cleanup order 暂时优于追求最大并发

graph-core 的 `EffectStack` 串行反向 `await` 每个 disposer，语义清楚、验证简单。Cordis 源码的 sibling cleanup 并发不应成为 Rust 必须复制的性能优化。

---

## 9. 推荐的后续验证顺序

不建议立即实现 loader/HMR。先做三个小而可证伪的 runtime experiments。

### Experiment A — Reactive provider replacement

目标：证明 provider exact identity 变化能够自动驱动 dependent fiber convergence。

验收：

- V1 -> V2 replacement 后，旧 active consumer 不再接受新工作；
- consumer 的旧 in-flight handles 仍然有效；
- consumer 最终使用 V2 重新进入 Active；
- isolated/sibling consumer 不误 reload；
- 并发连续 V1 -> V2 -> V3 最终只收敛到 V3，不发布 stale epoch。

### Experiment B — Withdrawal guard and dependent quiescence

目标：复现论文 Theorem 63 对应的工程协议，而不是只做逆拓扑 drop。

验收：

- provider withdraw 后立即从 future lookup 消失；
- dependent teardown 期间仍能使用 committed provider handle；
- provider cleanup 在所有 committed dependents quiescent 后才开始；
- dependent cleanup failure 不造成 provider 永久悬挂，失败策略明确；
- parent/child scope 与 logical dependency 两种 order 不混淆。

### Experiment C — Process-local inverse vs durable emission boundary

目标：用代码测试固定 system boundary，防止未来把两个 effect 概念混在一起。

验收：

- local acquisition 可以注册 `ScopedEffect` 回收；
- dispatched external operation 的事实只能由 DurableJournal 改变；
- capability teardown 不覆盖已 dispatch 的 operation outcome；
- non-idempotent unknown outcome 仍进入 reconciliation，不因为 disposer 存在而 retry/pretend rollback。

完成 A/B/C 后，再决定是否需要：

- multi-capability atomic replacement；
- capability interface versioning；
- declarative loader；
- HMR / dynamic module loading。

---

## 10. 对 graph-core 架构决策的最终影响

这次论文交叉验证 **不要求推翻现有架构**，反而进一步支持当前 authority separation：

```text
Capability Graph / Context
    identity, visibility, dependency declarations

Reactive Capability Runtime
    fiber state, provider target, committed dependency view,
    effect ownership, quiescence, replacement convergence

Runtime Core
    task attempts, scheduling coordination, exact capability pinning

Durable Workflow / Journal
    operation intent, dispatch, outcome, retry/reconciliation truth

Execution Stream
    observations only
```

最值得补齐的是第二层的 **reactive dependency lifecycle**，而不是把第一层扩张成万能 Graph，也不是把第四层的 durable semantics 塞进 `ScopedEffect`。

Cordis 给 graph-core 的最大价值，现在可以更准确地表述为：

> **不是提供了一份“Rust 插件框架照抄清单”，而是提供了一组关于动态组件生命周期、环境依赖与可撤销局部 effect 如何组合的形式化不变量。graph-core 应吸收这些不变量，同时继续用自己的 identity、ownership 和 durability authority 把边界做得更严格。**

## Primary source map

- Paper: <https://github.com/cordiverse/paper/tree/948a07b369c62adb3b12e102458be5c18dfb69b9>
- Cordis core context: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/context.ts>
- Cordis fiber/effects: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/fiber.ts>
- Cordis coeffect/provider resolution: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/reflect.ts>
- Cordis registry/inject: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/core/src/registry.ts>
- Cordis loader: <https://github.com/cordiverse/cordis/tree/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/loader>
- Cordis HMR: <https://github.com/cordiverse/cordis/blob/8cc9e33fab69e2d0476d126baaf2acb24e6a6ab4/packages/hmr/src/index.ts>
- graph-core capability runtime: `crates/capability-graph/src/runtime.rs`
- graph-core Cordis semantic adaptation: `crates/capability-graph/src/semantic.rs`
- graph-core runtime authority contract: [`../runtime/M1-runtime-core.md`](../runtime/M1-runtime-core.md)
