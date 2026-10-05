# 待确认问题（OPEN QUESTIONS）

> 用途：117 条规则是**讨论结论**，落地时仍存在歧义、冲突或版本差异。这里记录所有尚未拍板的问题。
> 约定：AI 遇到架构歧义**不要猜**，先在此追加条目并询问；确认后把结论写进对应文档并从本文件删除
> （或移到底部的"已确认"表）。
>
> **新增规则的正规流程**（[README.md](README.md) §状态说明）：先在本文件记录 → 确认 →
> 落到对应文档**并分配编号**。所以本文件里的条目**都还没有 `R<n>` 编号**，
> 不要拿它当规则引用。

图例：🟥 阻塞编码 · 🟨 影响设计 · 🟩 只需确认

---

## Q28 🟨 移动与"动作"是什么关系（**当前最需要拍板的一条**）

**问题**：角色一次只能执行一个动作（[combat-design.md](combat-design.md) §7 原则 2）。
那"走一格"算不算那个动作？

**三个候选**：

| | 规则 | 后果 |
|---|---|---|
| A | 移动占行动槽（走一格 = 一个 `Action`） | 原则自动成立；打断顺带管住移动。代价：挪一步 = 少打一下 |
| B | 移动 = Idle 时的默认行为 | 能边跑边打；但反制/打断管不到移动 |
| C | **移动独立成通道，但受动作阶段门控** | 见下 |

**倾向 C**：由时间轴**自己**回答，不额外发明规则：

```text
前摇 [t0, t0+W)      移动 = 取消施法（"假动作 / feint"的空间）
释放点 t0+W          已结算，不可撤回
后摇 [t0+W, t0+W+R)  技能已生效，后摇的语义是【冷却】不是【定身】→ 应当可移动
```

即 **前摇锁、后摇放**（FFXIV 的滑步施法 / slidecast）。支持 C 的一条硬理由：
**威胁系统本身就要求前摇是一段独立、可被外人观测、还能被插进来的时间**
（"敌方进入前摇 → 开反制窗口"）。若移动与施法挤在同一个槽里，"前摇"就没有独立身份，
反制窗口也就没有明确起止。

**若采纳 C，原则 2 的措辞要收窄**：从"一次只执行一个动作"改成
"**一次只执行一个技能动作**；移动是独立通道，受动作阶段门控"。

**同时要落地**：L2 需要读"这个角色自己的动作阶段"，而现在只有全局的 `CombatPhase`，
`ActorState` 又只是个 `statuses: Vec<Entity>` 的空壳（**名字与内容不符**）。
阶段状态得有个真正的家 —— 与三段时间轴（前摇 / 释放点 / 后摇）是同一件事，建议一起做。

**待你选 A / B / C。**

---

## Q29 🟨 打断的有效窗口：只对前摇？

**问题**：现在 `cancel_actions` 是直接 `despawn` 行动实体。**今天没问题** —— 因为行动实体
在释放点就被销毁了，运行时**根本没有"后摇实体"**（后摇目前只是 `Skill.duration` 之后的
概念，冷却走独立的 `Cooldowns`）。

**但一旦按你上一份小抄把后摇并进同一个 action 实体**（"后摇即冷却"），
打断就能销毁后摇 → **等于可以取消别人的冷却**。

**倾向**：把"打断只对前摇有效"写成**规则**，而不是留给实现自己注意。
它同时让 `DispelAction` / `Interrupt` 有了明确的有效窗口。

---

## Q30 🟨 施法请求与路径的交互

**问题**：逻辑位置是 `IVec2`，**不能停在格子中间**。那"走到一半按了技能"怎么办？

**倾向**：**施法请求 = 清空 `path`，但当前这一格走完**。

好处：位置永远是合法格子；玩家有约 `1/speed` 秒（0.25s 量级）的反应窗，不会觉得按键被吞；
方向上也和小抄"输入只负责塞 path，走不走是 step_motion 的事"一致 —— 只是反过来，
动作请求会让 `path` **在当前格结束后**停止。

**待确认**：是否允许"打断当前格"（走到一半折返）。默认**不允许**。

---

## Q31 🟨 空间模型落地的三个细节

**背景**：格子坐标 = 体素块（**已拍板**，见下方"已确认"表）。由此产生三个具体问题：

1. **地面高度**。体素是 3D 的，格子移动是 2D 的。地面高度**不是常数**，
   所以"格子 → 世界坐标"的纯函数**不能返回 `y = 0`**（角色会陷进地板或浮空），
   必须从地形查。**待确认**：现在只支持单层（`GridPosition` 就是 `(x, z)`，
   高度做成派生值），还是直接支持多层？
   **倾向**：只支持单层，但高度**从世界查出来**而不是写死 —— 以后加高低差只改查表函数。
2. **`IVec2` 现在在 `axiom` 里拿不到**。已核实：`axiom` 的直接依赖恰好是白名单 5 个 +
   登记表 3 个，**依赖树里没有 `bevy_math`、没有 `glam`**，源码里也一个 `Vec2`/`IVec2` 都没有。
   若采纳"格子 = 体素块"，**倾向直接复用 `atoms::world::VoxelPos`**（去掉 Y 的视图），
   既不引新依赖，也不造平行坐标类型 —— `R107`（渲染层不许自己发明世界坐标）自动满足。
3. **多层地形下的可站点**（桥 / 洞）：同一个 `(x, z)` 会有两个可站点。现在**不支持**。

---

## Q4 🟩 Bevy 0.19 事件 API 更名（R34）

**问题**：R34 写的是 `.add_event::<T>()`。Bevy 0.19.1 中缓冲事件为 `Message`，
注册是 `.add_message::<M>()`，读取是 `MessageReader`。

**建议**：把 R34 的措辞更新为"谁定义谁注册（`.add_message::<M>()`）"，规则意图不变。
已在 [events-and-plugins.md](events-and-plugins.md)、[bevy-events.md](bevy-events.md) 说明，
**待你确认是否回写规则原文**（这是本文件里唯一一条"只差回写"的 🟩）。

---

## Q11 🟨 时间表示：`f32` 秒 vs `f64` 秒 vs `Duration`

**现状**：现行实现统一用 **`f32` 秒**（`Action::duration` / `ActiveStatus::remaining`），
来源是 `Res<Time>::delta_secs()`。

**移动设计带来了新情况**：`Motion` 用**绝对时间戳**（`started_at` / `ends_at`），
而 Bevy 的虚拟时钟只能给 `elapsed_secs_f64()`（没有 `f32` 版本）。
于是会出现"时钟是 `f64`、动作时长是 `f32`"的混用。

**候选**：
- A. 全面改 `f64`（与时钟一致，长局精度也好）
- B. 保持 `f32`，只在取 `now` 时从 `f64` 转一次（转换点唯一，好审计）
- C. 内部 `Duration`，外围转

**倾向 B**：`Time<Virtual>` 是唯一权威这个前提下，转换点只有一个，成本最低。
**待你确认**；Q29/Q30 落地前需要先定这条，否则时间戳的类型会分裂。

---

## Q14 🟩 模块 → crate / Plugin 映射（表已同步到当前代码）

**问题**：规则给了模块名，但没有"哪个模块属于哪个 crate、各自 Plugin 是否独立"的最终映射表。
下表是**当前实现**（已按最近几轮的重构同步；`world` 已收进 `atoms`，实例组件已进 L0）。

| crate | 模块 | Plugin |
|---|---|---|
| `axiom` | `atoms::actor`（池 / 属性 / 冷却 / 状态槽 / 能量 / 三条正交轴） | `ActorPlugin` |
| `axiom` | `atoms::vocabulary`（词汇 ID / `NameTable` / `UnknownName`） | 无 |
| `axiom` | `atoms::action` / `atoms::status` / `atoms::decision`（零依赖的实例组件与关系） | 无 |
| `axiom` | `atoms::world`（体素世界数据 + 纯计算，**零系统**） | `WorldPlugin` |
| `axiom` | `behaviors::combat`（**装配层**：独占注册所有跨域系统顺序） | `CombatPlugin` |
| `axiom` | `behaviors::action`（推进 / 结算 / 释放校验） | `ActionPlugin` |
| `axiom` | `behaviors::effect`（唯一世界突变原语） | 无（纯函数 + `SystemParam`） |
| `axiom` | `behaviors::contest`（对抗判定 + RNG，**唯一随机点**） | `ContestPlugin` |
| `axiom` | `behaviors::status`（定义 / 生命周期 / 派生修饰符） | `StatusPlugin` |
| `axiom` | `behaviors::phase`（相位 / 派生数据 / 战斗日志） | `PhasePlugin` |
| `axiom` | `behaviors::monster`（AI 决策） | `MonsterPlugin`（只持配置；系统由装配层注册） |
| `axiom` | `behaviors::time_scale`（相位 → 时间倍率） | `TimeControlPlugin` |
| `axiom` | `behaviors::content`（描述结构 + 加载管线，**不读文件**） | 无（L2 调用 `load_all`） |
| `prime` | `content`（读 RON + 注入 + 生成实体） | `ContentPlugin` |
| `prime` | `presentation`（HUD 等） | `PresentationPlugin` |
| `prime` | `voxel_render` | `VoxelRenderPlugin` |
| `prime` | `actor_render`（精灵 / 动画 / **演示用巡逻**） | `ActorRenderPlugin` |
| `prime` | `voxelith`（顶层装配） | `VoxelithPlugin` |

**约定**：**只在一个地方注册系统**。子域 Plugin 只负责"资源 + 消息"，
跨域顺序统一由 `CombatPlugin` 写（否则同一个系统会在一帧里跑两遍，`Commands` 被应用两次）。
见 [combat-design.md](combat-design.md) §7。

**待确认**：`prime` 侧的 `interaction` / `input` 是独立模块还是并入 `PresentationPlugin`。

---

## Q15 🟩 守卫脚本的实现形式

**问题**：R90–R93 是命令清单，没有规定是否脚本化、是否接 CI。
**现状**：已落地 `scripts/arch-guard.ps1`（Windows 优先，因为当前开发环境是 Windows）。
是否还需要 `.sh` 版本 / GitHub Actions，待确认。

---

## Q27 🟨 池见底没有"归零"通知（原 `DeathEvent` 的空位）

**问题**：旧设计有 `atoms::health` + `DeathEvent`（血量归零的即时通知）。资源池化之后，
`Effect::ModifyResource` 只是"把某个池的 `current` 改掉"——**引擎不知道"hp 见底 = 死亡"**，
因为"哪个池是命、见底了意味着什么"是**内容**语义。

**候选**：
- A. 内容语义留给内容层：`hp` 见底时由技能效果链自己的 `Effect::Conditional` 处理
- B. L0 加一条通用规则：`PoolTemplate` 加 `depleted_means_dead: bool`，见底即发 `DeathEvent`
- C. L1 提供一个 `Effect::Despawn`，由内容显式描述"什么时候死"

**倾向 B + C 并行**：B 管"普遍规则"（绝大多数池都不是命，所以默认关），C 管"特殊脚本"。
需要你拍板；在此之前，资源池见底**只是数值为 0**，不会触发任何事件。

---

## 已确认（推翻或细化原规则）

| 原规则 | 结论 | 确认日期 |
|---|---|---|
| **Q32**（空间模型：格子 vs 连续） | **格子**，且**格子 = 体素块**。推论：可走性 / 地形减速 / 视线遮挡全部直接查 `atoms::world`（`get_voxel`），**不需要第二张地图**，且这些是 L0 数据、由 L1 的规则直接查、L2 完全不参与 —— `R107` 自动满足。尺度上战场（约 26 格）**正好落在一个区块内**（`CHUNK_SIZE = 32`）。未定细节见 Q31。 | 已确认（用户拍板） |
| **Q33**（时间源） | **用 Bevy 的 `Time<Virtual>`，不自己造时钟**。但**冻结的唯一权威是 `CombatPhase`**（[behaviors::time_scale](../crates/voxelith-axiom/src/behaviors/time_scale.rs) 的 `drive_virtual_time` 按相位设倍率，测试 `delta_and_phase_never_disagree` 钉住"delta 与相位绝不说两套话"）。移动只**读** `Res<Time<Virtual>>`，**不许**再自己调 `pause()` / `unpause()` / `set_relative_speed()` —— 两个冻结源会互相打架。 | 已确认 |
| **Q34**（位置 = 时间的纯函数） | **采纳**：位置每帧从 `f(now)` **重算，不是累加**。理由不只是优雅：反制窗口会把虚拟时间倍率设为 0，累加式推进在"冻结→恢复"边界会留一帧漂移，纯函数不会。<br>**这条同时指出了现有代码的同一缺陷**：`behaviors::action` 的 `tick_actions` 是 `action.elapsed += delta`。**不做第二个时间轴** —— 采纳移动设计时一并统一为绝对时间戳（否则前摇与位移在倍率变化时会按不同误差漂移）。 | 已确认 |
| **Q35**（移动的分层） | 按 [layers.md](layers.md) 与 `R10` / `R107` 定死：<br>`GridPosition` / `Motion` / `Speed` → **L0**（零依赖组件）<br>`position_at`（格子 → 世界坐标的**纯函数**）→ **L0**（和 `VoxelPos::to_world` 同类；`R107` 禁止渲染层自己发明坐标）<br>`step_motion`（读 `Time<Virtual>` + 多组件）→ **L1**<br>`update_transform`（写 `Transform`）→ **L2**（`R10` 禁止 `Transform` 进 L0/L1） | 已确认 |
| R29（事件命名） | **细化**：后缀即机制——`Message` 用 `...Message`（请求型用 `...Request`），observer 事件用 `...Event`。原 R29 的"名词短语/过去式"仍然适用。已回写 [naming.md](naming.md) §2。 | 已确认 |
| Q16（事件后缀歧义） | **关闭**：采用"Message 结尾 = Message，Event 结尾 = Event"。判定规则见 [bevy-events.md](bevy-events.md) §2，命名表见 [naming.md](naming.md) §2。 | 已确认 |
| Q13（测试策略） | **已落地**：`cargo test --workspace` 纳入架构守卫（当前 295 个测试）。样板见 [`crates/voxelith-axiom/tests/combat_flow.rs`](../crates/voxelith-axiom/tests/combat_flow.rs)（用真实 `App` 钉住"系统顺序 + 命令应用时机"）与 [`crates/voxelith-prime/tests/content_pipeline.rs`](../crates/voxelith-prime/tests/content_pipeline.rs)（内容真能加载）。 | 已确认 |
| Q18（资源池 vs `Health`） | **改判为"采纳 B"**：`Health` 已删除，改为 `atoms::actor::Resources`（`HashMap<ResourceId, Pool>` + `PoolTemplate` 来自 `vocabulary.ron`）。原先"推迟"的理由（法力/耐力需求未到）已被半即时战斗的整体替换吞掉，一并解决了 Q2（字段私有化）。 | 已确认（改判） |
| Q21（时钟与时间表示） | **采纳 B（放宽 R5）**：`voxelith-axiom` 允许依赖 `bevy_time`，用 `Time`/`Duration`；不引入 `BattleClock`。已回写 [architecture.md](architecture.md)、[anti-patterns.md](anti-patterns.md)、`axiom/Cargo.toml`、`axiom/src/lib.rs`。**注意：Q7 的 `bevy_tasks` 未获放宽**，异步与网格化仍留在 L2。 | 已确认 |
| Q17（`behaviors::combat` 定位） | **采纳 A 并加强**：保留为"配置 + **装配器**"，持有 `CombatConfig`，并**独占注册所有跨域系统顺序**；子域 Plugin 退化为"资源 + 消息"（R41.1，非空壳）。 | 已确认（加强） |
| Q19（属性最终值回写模式） | **改成"状态派生修饰符"**：`Stats` 只持有 `base` / `modifiers` / `effective`，`refresh()` 负责聚合；**修饰符由状态每帧整批重建成**（`apply_status_modifiers`），所以到期不需要清理消息。旧 `StatBaseChangedMessage` / `StatFinalMessage` / `atoms::stats` 已删除。 | 已确认（已修订） |
| Q20（状态实例载体） | **改成 `AttachedTo` 关系**：状态是独立实体 + `AttachedTo(宿主)`，宿主侧 `ActorState.statuses` 是**同帧可见的快照**（`Commands` 延迟应用，关系组件下一帧才生效）。净化 = `Effect::RemoveStatus` / `PurgeStatusMessage`。 | 已确认（已修订） |
| Q22（命名：宏模块 / 判定层） | 判定层收进 `behaviors::contest`（`Formula` + `resolve_contest` + `CombatRng`，**唯一随机点**）；共享宏模块 `defs` 与 `behaviors::rolls` 均已删除。 | 已确认（已修订） |
| Q23（R5 白名单范围） | **采纳 A（登记制）**：`bevy_*` 严格白名单；非 bevy 工具 crate 需先在 [architecture.md](architecture.md) 的登记表登记，守卫按表检查。`derive_more` 收窄为 `display` + `error`。 | 已确认 |
| Q24（stats 取代 attribute） | **采纳 A 并继续演进**：旧 `atoms::attribute` / `behaviors::attributes` / `tests/attribute.rs` 已删除，属性落在 `atoms::actor::Stats`；**成长链（`behaviors::progression` / 经验 / 加点）已删除**，等级与成长留到有真实需求再设计。 | 已确认（已修订） |
| Q25（`u32` 与百分比修饰符取整） | **改判为"全程 `f32`"**：数值全程 `f32`（含 RON 里的数值），**不取整**。旧方案里的 `Rounding` / 整数管线随伤害管线一起删除。 | 已确认（改判） |
| Q26（`bevy_state` / `serde`） | **采纳 A**：`bevy_state` 进 R5 白名单，`serde` 进登记表（只用 `derive`，不读文件）。见 [architecture.md](architecture.md) 登记表。 | 已确认 |
| Q12（`prime` 纯 bin 还是 bin + lib） | **bin + lib**：`crates/voxelith-prime/src/lib.rs` 已存在，`main.rs` 只做装配与 `run()`。集成测试（`content_pipeline` / `voxel_edit` / `terrain` 等）直接引用 lib。 | 已确认（已落地） |
| Q6（体素数据放哪个 crate） | **采纳 A**：数据与 `get/set_voxel` 在 `axiom`；"方块种类表 + 生成参数"以 `.ron` 形式由 L2 读入后交给 L1 解析。现在已经更进一步 —— `world` 整域收进了 `atoms::world`（它是零系统的纯 L0）。 | 已确认（已落地） |
| Q8（区块生命周期） | **内存态 + LRU**：`WorldConfig.max_loaded_chunks`（`0` = 不限制，战场规模下默认全驻留），`load_around` 负责加载；磁盘持久化在 L2 的 [`prime/save.rs`](../crates/voxelith-prime/src/save.rs)。 | 已确认（已落地） |

### 已删除的过期条目（留痕，别再翻）

| 原编号 | 为什么删 |
|---|---|
| Q1 | 讨论的 `Lifetime` 模块**从未存在过**，半即时战斗的整体替换后也没有它的位置 |
| Q2 | 讨论对象 `atoms/health.rs` **已删除**；该问题早在 Q18 那一行就被一并解决（**这就是本文件腐烂的典型：底部说解决了，顶部还挂着 🟥**） |
| Q3 | `DeathEvent` **已不存在**，其问题由 Q27 承接 |
| Q5 | "L0 位置表达"由 **Q32 / Q35** 正面回答（格子 = 体素；`position_at` 在 L0） |
| Q7 | 由 Q21 那行定案：**`bevy_tasks` 未获放宽**，异步与网格化留在 L2 |
| Q9 / Q10 | 早已标为"已由半即时战斗消解"，本次正式移入本表 |
