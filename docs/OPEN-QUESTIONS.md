# 待确认问题（OPEN QUESTIONS）

> 用途：117 条规则是**讨论结论**，落地时仍存在歧义、冲突或版本差异。这里记录所有尚未拍板的问题。
> 约定：AI 遇到架构歧义**不要猜**，先在此追加条目并询问；确认后把结论写进对应文档并从本文件删除（或标记"已确认"）。

图例：🟥 阻塞编码 · 🟨 影响设计 · 🟩 只需确认

---

## Q1 🟨 `Lifetime` 的归宿领域名（R61、R63 vs R21）

**冲突**：R61 说 `Lifetime` 应放 `core` 或 `utils`；R21 禁止 `utils` 作为模块名，`core` 与 Rust 内置 `core` crate 同名。

**候选**：
- A. `axiom/src/lifetime/`（按功能领域命名，完全符合 R20/R21）
- B. `axiom/src/lifecycle/`（R61 已提到 `lifecycle` 模块名）
- C. `axiom/src/core/`

**现状**：文档按"通用生命周期领域"表述，未定名。**建议 A 或 B。**

---

## Q2 🟥 `Health` 字段公开性（R56、R16、R100）

**问题**：当前 `atoms/health.rs` 是 `pub struct Health { pub current: i32, pub max: i32 }`。字段 `pub` 意味着 L2 可以直接写 `health.current = 9999`，违反 R56"唯一写入口"。

**候选**：
- A. 字段私有 + 只读 getter（`current()` / `max()`），写只能通过事件 → **符合 R56，建议此项**
- B. 保持 `pub`，靠 review 约束

---

## Q3 🟨 `DeathEvent` 的定义归属（R33 vs R47）

**问题**：R33 说事件定义在"发出它的模块"。`DeathEvent` 由血量归零触发，看起来属于 `health`；但 R47 要求 `health` 不认识伤害来源/上下文。
**现状**：`DeathEvent` 目前定义在 `atom/health.rs`，且带 `killer: Option<Entity>`（击杀者概念偏 L1），doc 注释还引用了不存在的 `DamageEvent`（本次已修）。

**候选**：
- A. `DeathEvent` 留在 `health`，但只含 `entity` + `at`；击杀者信息由 L1 关联
- B. `DeathEvent` 定义在 L1（`behaviors/combat`），`health` 只发一个内部"归零"信号

---

## Q4 🟩 Bevy 0.19 事件 API 更名（R34）

**问题**：R34 写的是 `.add_event::<T>()`。Bevy 0.19.1 中缓冲事件为 `Message`，注册是 `.add_message::<M>()`，读取是 `MessageReader`。

**建议**：把 R34 的措辞更新为"谁定义谁注册（`.add_message::<M>()`）"，规则意图不变。已在 [events-and-plugins.md](events-and-plugins.md) 说明，待你确认是否回写规则原文。

---

## Q5 🟩 L0 位置表达（R10 vs R81）

**问题**：L0 禁用 `Transform`（R10），但位置又被表述为来自 `movement`（R81）。
**建议**：L0/L1 使用自有数据组件（如 `Position`）承载位置，L2 用同步系统映射到 `Transform`。**需确认。**

---

## Q6 🟨 体素数据放在 `axiom` 还是 `prime`（R2、R3、R74）

**问题**：`world`（体素数据）是纯数据，按分层应属 L0/L1 → `axiom`；但体素世界的"游戏内容"属性（方块种类、世界生成规则）有 L2 味道。

**候选**：
- A. `world` 数据与 `get/set_voxel` 放 `axiom`；方块**种类表与生成参数**放 `prime`
- B. 全部放 `prime`，仅把 `ChunkPos` 等类型放 `axiom`

---

## Q7 🟨 网格化任务的依赖（R72）

**问题**：`AsyncComputeTaskPool` 属于 `bevy_tasks`。如果 `world` 在 `axiom`，而网格化在 `prime`，任务池使用方是 L2，没问题；若想让 `axiom` 也调度异步，需要加 `bevy_tasks` 依赖。
**建议**：异步与网格化都留在 L2（`voxel_render`），`axiom` 不引入 `bevy_tasks`。**需确认是否放宽 R5 的依赖白名单。**

---

## Q8 🟨 区块生命周期未定义

**问题**：规则未定义区块的加载/卸载/流式策略（视距、缓存上限、持久化到磁盘 vs 内存）。
**建议**：先做内存态 + 视距半径参数，磁盘持久化留待后续。

---

## Q9 ✅ 已由半即时战斗设计消解（原：`DamageRequest` 的流向与表现一致性，R50、R55）

**原问题**：R55 让表现层监听 `DamageRequest`，但公式会改写它（闪避 → 0、暴击 → 翻倍），
于是"表现层看到的是未结算的请求"。

**结论**：**问题本身不存在了**。现行设计里没有"请求 + 拦截链"这回事：
一次攻击 = `CastRequest`（谁放什么）→ `Contest` 一次算出结果（`Verdict { outcome, power }`）→
`Effect` 执行 → 文案进 `CombatLog`。
表现层读的是**结算之后的产物**（`CombatLog` / 相位 / 可用技能 / 行动进度），
所以"闪避也播命中特效"这个歧义自然消失。
见 [combat-design.md](combat-design.md) §3.3、§4。

---

## Q10 ✅ 已由"状态"承接（原：Buff / Regeneration 框架缺失，R52）

**结论**：自然恢复有两条现成通道，不需要新的 `buffs` 领域：
- **池级**：`Pool.regen` 每帧由 `ActorPlugin::regenerate_resources` 恢复（L0，纯数值）；
- **内容级**：一条带 `on_tick` 的 `StatusDef`（例如"再生"）即可，走 `Effect::ModifyResource`。
不需要新组件、新事件、新层。内容加进 `statuses.ron` 即可。

---

## Q11 🟨 时间表示不统一

**问题**：R50 流程与现有代码用 `at: f32`（虚拟秒）。Bevy 惯用 `Time` / `Duration`。
**现状**：现行实现统一用 **`f32` 秒**（`Action::duration` / `Action::elapsed` / `ActiveStatus::remaining`），
来源是 `Res<Time>::delta_secs()`（已经是 `f32`）。
**待定**：长局（数小时）下 `f32` 秒的精度是否够；不够的话，内部换成 `f64` 或 `Duration`，
只在外围转换一次。

---

## Q12 🟩 `voxelith-prime` 是纯 bin 还是 bin + lib

**问题**：`prime` 目前只有 `main.rs`。L2 逻辑（表现系统、插件）需要可测试、可被集成测试引用。
**建议**：加 `src/lib.rs`，`main.rs` 只做 `App::new().add_plugins(...).run()`。

---

## Q14 🟩 业务模块目录与 Plugin 映射未定

**问题**：规则给了 `combat` / `targeting` / `skills` / `world` / `voxel_render` / `interaction` / `presentation` / `spawn` / `movement` 等模块名，但没有"哪个模块属于哪个 crate、各自 Plugin 是否独立"的最终映射表。

**当前建议映射**：

| crate | 模块 | Plugin |
|---|---|---|
| `axiom` | `atoms::actor`（池 / 属性 / 冷却 / 状态槽 / 能量） | `ActorPlugin` |
| `axiom` | `behaviors::combat`（**装配层**：注册所有跨域系统顺序） | `CombatPlugin` |
| `axiom` | `behaviors::action`（技能实例 / 行动槽 / 释放校验） | `ActionPlugin` |
| `axiom` | `behaviors::effect`（唯一世界突变原语） | 无独立 Plugin（纯函数 + `SystemParam`） |
| `axiom` | `behaviors::contest`（对抗判定 + RNG） | `ContestPlugin` |
| `axiom` | `behaviors::status`（定义 / 实例 / 生命周期 / 派生修饰符） | `StatusPlugin` |
| `axiom` | `behaviors::phase`（相位 / 派生数据 / 战斗日志） | `PhasePlugin` |
| `axiom` | `behaviors::monster`（AI 决策） | `MonsterPlugin`（只持配置；系统由装配层注册） |
| `axiom` | `behaviors::time_scale`（相位 → 时间倍率） | `TimeControlPlugin` |
| `axiom` | `behaviors::content`（描述结构 + 加载管线，**不读文件**） | 无（L2 调用 `load_all`） |
| `prime` | `content`（读 RON + 注入 + 生成实体） | `ContentPlugin` |
| `prime` | `presentation` | `PresentationPlugin` |
| `prime` | `voxel_render`（待做） | `VoxelRenderPlugin` |
| `prime` | `interaction` / `input`（待做） | 或并入 `PresentationPlugin` |
| `prime` | `spawn` | 无独立 Plugin（工厂函数） |
| `prime` | `voxelith`（顶层装配） | `VoxelithPlugin` |

**约定**：**只在一个地方注册系统**。子域 Plugin 只负责"资源 + 消息"，
跨域顺序统一由 `CombatPlugin` 写（否则同一个系统会在一帧里跑两遍，`Commands` 被应用两次）。
见 [combat-design.md](combat-design.md) §7。

---

## Q15 🟩 守卫脚本的实现形式

**问题**：R90–R93 是命令清单，没有规定是否脚本化、是否接 CI。
**现状**：已落地 `scripts/arch-guard.ps1`（Windows 优先，因为当前开发环境是 Windows）。是否还需要 `.sh` 版本 / GitHub Actions，待确认。

---

## Q26 🟨 `bevy_state` 与 `serde` 进 `axiom`（R5）

**问题**：半即时战斗（[combat-design.md](combat-design.md)）需要两样东西：`CombatPhase` 状态机与
RON 描述结构的 `Deserialize`。前者在 `bevy_state`，后者在 `serde`——两者都不在 R5 的 bevy 白名单里。

**候选**：
- A. 把 `bevy_state` 加进白名单、把 `serde` 加进登记表（**采纳**）
- B. 手写状态机（`Resource` + `NextState` 自管）与手写 RON 解析

**结论（已采纳 A）**：`bevy_state` 与 `bevy_ecs` / `bevy_app` / `bevy_reflect` 同级——它只依赖这几个，
不引入渲染；`serde` 走登记制，且 `axiom` **只用 `derive`，自己不读文件**（R101 精神：
读文件与反序列化都在 L2 的 `voxelith-prime`）。守卫脚本第 5 项已同步白名单与登记表。

---

## Q27 🟨 池见底没有"归零"通知（原 `DeathEvent` 的空位）

**问题**：旧设计有 `atoms::health` + `DeathEvent`（血量归零的即时通知）。资源池化之后，
`Effect::ModifyResource` 只是"把某个池的 `current` 改掉"——**引擎不知道"hp 见底 = 死亡"**，
因为"哪个池是命、见底了意味着什么"是**内容**语义。

**候选**：
- A. 内容语义留给内容层：`hp` 见底时由技能效果链自己的 `Effect::Conditional` 处理
      （例如"若目标 hp <= 0 则 despawn / 播死亡"）
- B. L0 加一条通用规则：`PoolTemplate.start_full` 同级的 `depleted_means_dead: bool`，
      见底即发 `DeathEvent`（`EntityEvent`）
- C. L1 提供一个 `Effect::Despawn`，由内容显式描述"什么时候死"

**倾向 B + C 并行**：B 管"普遍规则"（绝大多数池都不是命，所以默认关），C 管"特殊脚本"。
需要你拍板选哪个；在此之前，资源池见底**只是数值为 0**，不会触发任何事件。

---

## 已确认（推翻或细化原规则）

| 原规则 | 结论 | 确认日期 |
|---|---|---|
| R29（事件命名） | **细化**：后缀即机制——`Message` 用 `...Message`（请求型用 `...Request`），observer 事件用 `...Event`。原 R29 的"名词短语/过去式"仍然适用。已回写 [naming.md](naming.md) §2。 | 已确认 |
| Q16（事件后缀歧义） | **关闭**：采用"Message 结尾 = Message，Event 结尾 = Event"。判定规则见 [bevy-events.md](bevy-events.md) §2，命名表见 [naming.md](naming.md) §2。 | 已确认 |
| Q13（测试策略） | **已落地**：`cargo test --workspace` 纳入架构守卫（当前 76 个测试）。样板见 [`crates/voxelith-axiom/tests/combat_flow.rs`](../crates/voxelith-axiom/tests/combat_flow.rs)（用真实 `App` 钉住"系统顺序 + 命令应用时机"）与 [`crates/voxelith-prime/tests/content_pipeline.rs`](../crates/voxelith-prime/tests/content_pipeline.rs)（内容真能加载）。 | 已确认 |
| Q18（资源池 vs `Health`） | **改判为"采纳 B"**：`Health` 已删除，改为 `atoms::actor::Resources`（`HashMap<ResourceId, Pool>` + `PoolTemplate` 来自 `vocabulary.ron`）。原先"推迟"的理由（法力/耐力需求未到）已被半即时战斗的整体替换吞掉，一并解决了 Q2（字段私有化）。 | 已确认（改判） |
| Q21（时钟与时间表示） | **采纳 B（放宽 R5）**：`voxelith-axiom` 允许依赖 `bevy_time`，用 `Time`/`Duration`；不引入 `BattleClock`。已回写 [architecture.md](architecture.md)、[anti-patterns.md](anti-patterns.md)、`axiom/Cargo.toml`、`axiom/src/lib.rs`。**注意：Q7 的 `bevy_tasks` 未获放宽**，异步与网格化仍留在 L2。 | 已确认 |
| Q17（`behaviors::combat` 定位） | **采纳 A 并加强**：保留为"配置 + **装配器**"，持有 `CombatConfig`，并**独占注册所有跨域系统顺序**；子域 Plugin 退化为"资源 + 消息"（R41.1，非空壳）。 | 已确认（加强） |
| Q19（属性最终值回写模式） | **改成"状态派生修饰符"**：`Stats` 只持有 `base` / `modifiers` / `effective`，`refresh()` 负责聚合；**修饰符由状态每帧整批重建成**（`apply_status_modifiers`），所以到期不需要清理消息。旧 `StatBaseChangedMessage` / `StatFinalMessage` / `atoms::stats` 已删除。 | 已确认（已修订） |
| Q20（状态实例载体） | **改成 `AttachedTo` 关系**：状态是独立实体 + `AttachedTo(宿主)`，宿主侧 `ActorState.statuses` 是**同帧可见的快照**（`Commands` 延迟应用，关系组件下一帧才生效）。净化 = `Effect::RemoveStatus` / `PurgeStatusMessage`。 | 已确认（已修订） |
| Q22（命名：宏模块 / 判定层） | 判定层收进 `behaviors::contest`（`Formula` + `resolve_contest` + `CombatRng`，**唯一随机点**）；共享宏模块 `defs` 与 `behaviors::rolls` 均已删除。 | 已确认（已修订） |
| Q23（R5 白名单范围） | **采纳 A（登记制）**：`bevy_*` 严格白名单；非 bevy 工具 crate 需先在 [architecture.md](architecture.md) 的登记表登记，守卫按表检查。`derive_more` 收窄为 `display` + `error`。 | 已确认 |
| Q24（stats 取代 attribute） | **采纳 A 并继续演进**：旧 `atoms::attribute` / `behaviors::attributes` / `tests/attribute.rs` 已删除，属性落在 `atoms::actor::Stats`；**成长链（`behaviors::progression` / 经验 / 加点）已删除**，等级与成长留到有真实需求再设计。 | 已确认（已修订） |
| Q25（`u32` 与百分比修饰符取整） | **改判为"全程 `f32`"**：数值全程 `f32`（含 RON 里的数值），**不取整**。旧方案里的 `Rounding` / 整数管线随伤害管线一起删除。 | 已确认（改判） |
| Q26（`bevy_state` / `serde`） | **采纳 A**：`bevy_state` 进 R5 白名单，`serde` 进登记表（只用 `derive`，不读文件）。见上文 Q26。 | 已确认 |
| 半即时战斗重构 | **整体替换**：删除旧伤害管线 / 抵抗 / 判定 / 旧属性账本，落地 [combat-design.md](combat-design.md)（Skill / Action / Status / Effect / Contest / CombatPhase）。 | 已确认 |

