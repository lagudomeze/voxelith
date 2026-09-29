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

## Q9 🟨 `DamageRequest` 的流向与表现一致性（R50、R55）

**问题**：R55 让表现层监听 `DamageRequest`。但 L1 公式会拦截/修改它（闪避 → 伤害为 0；暴击 → 翻倍）。表现层监听**原始请求**时，会为被闪避的攻击播放命中特效。
**候选**：
- A. 表现层监听 `DamageRequest`（原规则），接受"闪避也播特效"
- B. L1 在结算后补发 `DamageResolved`（含最终数值、是否命中、是否暴击），表现层监听它
- C. 表现层同时监听两者，用 `DamageResolved` 覆盖表现

**建议 B 或 C**，但这是对 R55 的修订，需要你拍板。

---

## Q10 🟩 Buff / Regeneration 框架缺失（R52）

**问题**：R52 提到"自然恢复作为 Buff"，但全套规则没有定义 Buff 系统的组件/事件/归属层。
**建议**：L1 新增 `behaviors/buffs` 领域（`Buff` 组件 + 周期系统 → 发 `ModifyHealthMessage`），先确认归属。

---

## Q11 🟨 时间表示不统一

**问题**：R50 流程与现有代码用 `at: f32`（虚拟秒）。Bevy 惯用 `Time` / `Duration`。
**建议**：统一为 `Duration` 或 `f64` 秒，`f32` 会在长局内累积精度问题。

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
| `axiom` | `atoms::health` | `HealthPlugin` |
| `axiom` | `atoms::<生命周期领域>`（Q1） | `LifetimePlugin` |
| `axiom` | `behaviors::movement` | `MovementPlugin` |
| `axiom` | `behaviors::combat`（含 `formula`） | `CombatPlugin` |
| `axiom` | `behaviors::targeting` | 无独立 Plugin（并入 `CombatPlugin`） |
| `axiom` | `behaviors::skills` | `SkillsPlugin` |
| `axiom` | `behaviors::world`（Q6） | `WorldPlugin` |
| `prime` | `presentation` | `PresentationPlugin` |
| `prime` | `voxel_render` | `VoxelRenderPlugin` |
| `prime` | `interaction` | `InteractionPlugin`（或并入 `PresentationPlugin`） |
| `prime` | `spawn` | 无独立 Plugin（工厂函数） |
| `prime` | `input` / `ai` | `InputPlugin` / `AiPlugin`（或并入 `PresentationPlugin`） |

---

## Q15 🟩 守卫脚本的实现形式

**问题**：R90–R93 是命令清单，没有规定是否脚本化、是否接 CI。
**现状**：已落地 `scripts/arch-guard.ps1`（Windows 优先，因为当前开发环境是 Windows）。是否还需要 `.sh` 版本 / GitHub Actions，待确认。

---

## Q17 🟨 `behaviors::combat` 的定位（R41 vs R46、R102）

**问题**：机制被拆成 `modifiers` / `attributes` / `damage` / `resistance` / `rolls` / `damage_pipeline` / `status`
七个 L1 领域后，[combat.md](combat.md) 与 Q14 里的 `behaviors::combat` 还剩什么职责？
如果它 `build()` 里只有 `add_plugins((..))`，按 R46/R102 就是空壳 Plugin，应删掉。

**候选**：
- A. **保留为"配置 + 装配器"**：持有 `CombatConfig`（抵抗上限、判定系数、RNG 种子、取整与保底规则），
  满足 R41.1（独立配置）→ 不是空壳。**建议此项。**
- B. 删掉 `combat`，各域 Plugin 直接进 L2 的元组。调参常量散落各域。

**影响**：调参集中度、`main.rs`/`VoxelithPlugin` 的 `add_plugins` 列表形态。
详见 [combat-mechanics.md](combat-mechanics.md) §4.9、§9。

---

## Q19 🟨 属性最终值的回写模式（R13、R56）

**问题**：属性最终值缓存（`cached`）住在 L0 组件 `Attributes` 里，但聚合修饰符需要多组件 → 计算只能在 L1。
L1 又不能直接写 L0（R13）。

**候选**：
- A. **L1 算完发 `AttributeFinalMessage`，L0 的 `apply_attribute_final` 唯一回写缓存**。多一次值拷贝，换来 I2/I5 成立。**建议此项。**
- B. L1 直接写 `Attributes::cached`（破坏 R13，数值变化不可追踪）。
- C. 最终值不放组件，改由 L1 侧 `FinalAttributes` 组件承载（L0 就不再持有最终值，读方全部依赖 L1 组件）。

**影响**：读取路径（`AsRef` 指向谁）、L0/L1 边界表述、`Attributes` 的字段集合。
详见 [combat-mechanics.md](combat-mechanics.md) §4.1。

---

## Q20 🟨 状态实例的载体（R63、R112）

**问题**：状态需要"独立生命周期 + 净化 + 免疫 + 叠加/刷新 + 宿主销毁自动清理"。

**候选**：
- A. **一状态一实体（`StatusInstance` + `ChildOf(宿主)`）**：净化=despawn、宿主销毁由层级语义带走、
  监听器可作用域化绑定到实例实体。**建议此项。**
- B. 宿主上一个 `StatusBucket(Vec<StatusInstance>)` 组件：少建实体，但净化/清理/计时都要手写，
  且无法用实体级 observer。

**影响**：状态的查询形态、清理路径、表现层如何订阅单个状态。
详见 [combat-mechanics.md](combat-mechanics.md) §4.8。

---

## Q22 🟩 两个命名确认（R20、R111）

**问题**：
1. 三个域（属性/资源/伤害类型/状态）共用的定义宏放哪？`axiom/src/defs.rs` 的 `defs` 不是领域名，
   与 R20 的"按功能领域命名"存在张力（但它也不是 R21 禁名）。
2. 概率判定层（命中/闪避/暴击掷骰）的领域名。

**候选**：
- 宏模块：A. `axiom/src/defs.rs`（**建议**）；B. `axiom/src/taxonomy.rs`；C. 每个域各写一份 `macro_rules!`（重复）。
- 判定层：A. `behaviors::rolls`（**建议**）；B. `behaviors::resolution`；C. `behaviors::chance`。

**影响**：模块名改动成本随时间上升（R111），建议尽早拍板。
详见 [combat-mechanics.md](combat-mechanics.md) §3.1、§4.6。

---

## 已确认（推翻或细化原规则）

| 原规则 | 结论 | 确认日期 |
|---|---|---|
| R29（事件命名） | **细化**：后缀即机制——`Message` 用 `...Message`（请求型用 `...Request`），observer 事件用 `...Event`。原 R29 的"名词短语/过去式"仍然适用。已回写 [naming.md](naming.md) §2。 | 已确认 |
| Q16（事件后缀歧义） | **关闭**：采用"Message 结尾 = Message，Event 结尾 = Event"。代码已重命名 `ModifyHealthMessage`；判定规则见 [bevy-events.md](bevy-events.md) §2，命名表见 [naming.md](naming.md) §2。 | 已确认 |
| Q13（测试策略） | **部分落地**：`cargo test --workspace` 已纳入架构守卫；首个样板见 [`crates/voxelith-axiom/tests/health.rs`](../crates/voxelith-axiom/tests/health.rs)（同时固化 Message/Event 用法）。L1 公式的表驱动测试待战斗模块落地时补。 | 已确认 |
| Q18（资源池 vs `Health`） | **采纳 C（推迟）**：暂不引入 `atoms::resource::ResourcePools`，保留 `Health`；法力/耐力出现真实需求时再按 [combat-mechanics.md](combat-mechanics.md) §4.3 落地，并一并处理 Q2（`Health` 字段私有化）。 | 已确认 |
| Q21（时钟与时间表示） | **采纳 B（放宽 R5）**：`voxelith-axiom` 允许依赖 `bevy_time`，用 `Time`/`Duration`；不引入 `BattleClock`。已回写 [architecture.md](architecture.md)、[anti-patterns.md](anti-patterns.md)、`axiom/Cargo.toml`、`axiom/src/lib.rs`。**注意：Q7 的 `bevy_tasks` 未获放宽**，异步与网格化仍留在 L2。 | 已确认 |

