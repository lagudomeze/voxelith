# 战斗机制 ECS 全景（属性 / 伤害类型 / 抵抗 / 伤害管线 / 扩展 / 状态）

> ## ⚠️ 已被取代（历史文档）
>
> 本文描述的**伤害管线 / 抵抗 / 判定层 / 属性账本**已在半即时战斗重构中**整体删除**。
> 现行设计见 [combat-design.md](combat-design.md)（唯一权威）：`Skill` / `Action` / `ActiveStatus` /
> `Effect` / `Contest` / `CombatPhase`，内容全部走 RON。
>
> 保留本文的原因：它记录的**取舍过程**（不变量 I1–I6、阶段排序、注册制依赖）仍然有效，
> 而且新设计里有几条正是从这里的教训来的（例如"组件自成一体"演化为"状态派生修饰符"）。
> 读代码时请以 [combat-design.md](combat-design.md) 为准。
>
> 对应关系的简表见 [combat-design.md §13 与旧方案的差异](combat-design.md#13-与旧方案的差异迁移记录)。

> **状态：全景骨架已落地**。组件、消息、配置 Resource、阶段排序、注册表与关键公式都已就位；
> 少数阶段体留了 `TODO(M5)` 接口（见 §8）。
>
> 规则依据：[layers.md](layers.md)（R8–R19、R112–R115）、[combat.md](combat.md)（R47–R63）、
> [bevy-events.md](bevy-events.md)（Message/Event 判定）、[naming.md](naming.md)（R28–R32）。

## 0. 一句话

**定义写在组件旁边，组件自成一体（收到变更消息自己刷新最终值），公式归组件内部，
判定单独一层，管线只算数，状态自成生命周期，所有可调参数走 Resource 注入。**

## 1. 六条不变量

| # | 不变量 | 保障手段 | 违反时的症状 |
|---|---|---|---|
| I1 | **改一处就够，漏一处会叫** | 枚举按下标开数组时（`DamageType` / `StatusId`）自带 `COUNT` / `ALL` / `index()` / `name()`；属性用显式字段 + `get` / `set` / `name` 的穷尽 `match` | 漏改 → 编译期报错（穷尽 `match`）或加载期报错（注册表覆盖检查）；属性的手写聚合漏改是静默错误，靠测试兜 |
| I2 | **组件自成一体**：数据拥有者算自己的最终值 | `Stat` / `Resistance` 收到变更消息后调用内部 `refresh()`；**没有**"脏了 / 算好了"的来回消息 | 出现 `*FinalMessage`、脏标记在模块间来回跑 → 消息噪音与顺序耦合 |
| I3 | **管线只读"最终值视图"** | `Stat` 与 `Resistance` 的 `Deref` 暴露 `cached`；管线不查修饰符 | 管线里出现 `Modifier` → 公式与来源耦合 |
| I4 | **随机只出现在判定层** | RNG 资源只在 `behaviors::rolls`（与状态判定）里被调用，种子可注入 | 管线里掷骰 → 结果不可复现、不可测 |
| I5 | **一个组件一个写入口** | 字段私有 + `pub(crate)` 改值方法 + 消息；变更与结果分开（指令 / 广播） | 两处都能写 → 双写打架 |
| I6 | **配置皆 Resource** | 每个域的 `XxxConfig` 都是 Resource；内容层加载后注入，`CombatConfig` 统一分发 | 参数散落常量 → 无法按关卡 / 难度调参 |

## 2. 分层落点总表

| 模块 | 层 | 拥有的组件 / 资源 | 系统 | 绝不做的 |
|---|---|---|---|---|
| `atoms::health` | L0 | `Health` | `apply_health_change`（R56） | 不认识 `DamageType`（R47、R98） |
| `atoms::modifiers` | L0 | `Modifier` / `ModifierOp` / `ModifierSet` / `ModifierCaps`（Resource）/ `Rounding` | 无（纯数据 + 纯算法） | 不认识"被修饰的是什么" |
| `atoms::stats` | L0 | **`Stat`**（`base` / `allocated` / `modifiers` / `cached`，自成一体）、`Level`、`LevelConfig`（Resource）、`StatConfig`（Resource） | 加点 / 发点数 / 洗点 / 修饰符增删与计时（**每条只查 `Stat`**，改完自己 `refresh`） | 不跨组件查询，不管成长编排 |
| `behaviors::progression` | L1 | `GainExperienceMessage` / `LevelUpMessage` | 经验 → 升级 → 发点数（`StatStage::Intake`） | 不直接改 `Stat` |
| `behaviors::damage` | L1 | `DamageType`、`DamageRequest`、`DamageResolvedMessage`、`DamageTags`、`DamageBehaviorRegistry`（Resource） | 按标签分派附加行为 | 不算减伤、不判状态 |
| `behaviors::resistance` | L1 | **`Resistance`**（`base` / `modifiers` / `cached`，自成一体）、`ResistanceCaps`（Resource） | 修饰符增删与计时（只查 `Resistance`）；纯函数 `mitigate()` | 不掷骰、不写血量 |
| `behaviors::rolls` | L1 | `CombatRng`（Resource）、`RollConfig`（Resource） | 规避 / 暴击判定（**唯一随机点**） | 不改数值、不减伤 |
| `behaviors::damage_pipeline` | L1 | `PipelineConfig`（Resource）、`DamageStage` | 四阶段 → `DamageResolvedMessage` → `ModifyHealthMessage` | 不掷骰、不管状态 |
| `behaviors::status` | L1 | `StatusId` / `StatusDef` / `StatusRegistry`（Resource）、`StatusInstance`、`StatusImmunity`、`StatusConfig`、`ContestParams` | 判定 / 施加 / 每回合结算 / 到期 / 净化 | 不塞进修饰符列表、不混进管线 |
| `behaviors::combat` | L1 | `CombatConfig`（Resource） | 分发配置 + 装配子域 Plugin | 不写具体内容（状态表、技能表） |
| L2 `content` / `presentation` | L2 | 具体状态定义、技能表、特效 | 监听 `DamageResolvedMessage` / `StatusResolvedMessage` | 不算伤害（R17） |

**放 L0 还是 L1**（沿用 [workflow.md](workflow.md) ①）：

- **公式需要的信息全在这一个组件内部** → L0（`Stat`、`Resistance` 的最终值都在 L0/L1 自己的组件里算，
  系统只查自己 → 满足 R8）。
- **需要两个以上组件同时在场才能算 / 跨领域编排** → L1（`progression` 的成长链、`damage_pipeline` 的伤害链）。
- 修饰符本身没有组件（纯数据 + 纯算法），下放 L0 供两处共用；"槽位"由各自的组件持有。

## 3. 定义与扩展方式

### 3.1 枚举：辅助项就写在枚举旁边

按下标开定宽数组的枚举（`DamageType` 的抵抗表列、`StatusId` 的免疫位图与注册表列）
自己在定义处带上辅助项，没有额外的宏、也没有 `defs.rs`：

```rust
// crates/voxelith-axiom/src/behaviors/damage.rs
pub enum DamageType { Physical, Fire, Frost, Arcane }

impl DamageType {
    pub const COUNT: usize = 4;
    pub const ALL: [Self; Self::COUNT] = [Physical, Fire, Frost, Arcane];
    pub const fn index(self) -> usize { /* match */ }
    pub const fn name(self) -> &'static str { /* match */ }
}
```

新增一种伤害类型 = 改这一个文件；漏改会在 `match` / 数组长度上**编译期报错**。

### 3.2 属性：显式字段 + 组件自成一体

```rust
// crates/voxelith-axiom/src/atoms/stats/stat.rs
pub struct StatBlock { pub strength: u32, pub dexterity: u32, /* ... */ }
pub struct Stat { base: StatBlock, allocated: StatBlock, modifiers: StatModifiers, cached: StatBlock, unspent: u32 }

let base = StatBlock { dexterity: 8, constitution: 12, ..StatBlock::new(10) }; // 直接写字段
let will = block.get(StatId::Willpower);                                       // 参数化访问
let label = StatId::Willpower.name();                                          // "willpower"
```

- 字段 `pub` 只为**方便读**；`set` / `add` / `append` / `reset` 都是 `pub(crate)`，也没有 `IndexMut`。
- **改值只能通过消息**；系统改完账本 / 修饰符后调用组件自己的 `refresh()`，最终值同帧就是新的。
- **代价（自觉接受）**：新增一个属性要同时改 `StatId`、`StatBlock` 字段、`StatModifiers` 槽位字段
  与 `refresh` 里的一行（`append` / `total` / `reset` 这类手写聚合漏改是静默错误，靠测试兜）。

### 3.3 注册表覆盖检查（加载期兜底）

```rust
StatusRegistry::builder().define(def).build()        -> Result<_, MissingStatusDefinition>
DamageBehaviorRegistry::builder().bind(..).build()   -> Result<_, MissingDamageBehavior>
```

漏定义 → `Err` → 内容层启动时 `expect` 掉，**不会静默跑起来**。

### 3.4 配置约定：全部走 Resource（I6）

- 每个域自带配置 Resource：`StatConfig`、`ModifierCaps`、`LevelConfig`、`ResistanceCaps`、
  `PipelineConfig`、`RollConfig`、`StatusConfig`、`ContestParams`。
- 内容层聚成 [`CombatConfig`](../crates/voxelith-axiom/src/behaviors/combat.rs) 注入一次，
  `CombatPlugin` 分发到各子域（子域的 `init_resource` 只在独立使用 / 测试时兜底）。
- **L0/L1 不读文件、不碰 `AssetServer`**（R101 精神）：加载与反序列化都在 L2，读完注入 Resource。

```rust
// L2（prime）：加载 / 构造配置 → 注入 → 装配
app.insert_resource(CombatConfig::new())   // 真实项目：从文件读出来再构造
   .add_plugins((HealthPlugin, StatPlugin, CombatPlugin));
```

## 4. 模块设计

### 4.1 属性（L0 `atoms::stats` + L1 `behaviors::progression`）

**`Stat` 一个组件装四样**：

| 字段 | 含义 | 谁改 |
|---|---|---|
| `base` | **初始值**（内容层生成时设定，运行期不变） | `Stat::from_base` |
| `allocated` | 已分配的点数（升级成长发的点数也在里面） | 加点 / 洗点消息 |
| `modifiers` | 每个属性一组修饰符槽位（装备 / 被动 / 状态派生） | 修饰符消息 |
| `cached` | **最终值视图**：`(base + allocated) 经修饰符聚合 → 取整` | 上述变更后由 `Stat::refresh` 刷新 |

**变更消息（指令）**：`AllocateStatRequest`、`GrantStatPointsMessage`、`RespecStatsMessage`、
`AddStatModifierMessage`、`RemoveStatModifiersMessage`。
**结果消息（广播）**：`StatAllocatedMessage`、`StatAllocationFailedMessage`（带结构化 `StatError`）。

每条系统都只查询 `Stat`：加点、发点数、洗点、加 / 删修饰符、临时修饰符计时（`Time` 驱动）。
**没有** `StatBaseChangedMessage` / `StatFinalMessage` 这类来回消息。

**成长在 L1**（`behaviors::progression`）：`GainExperienceMessage → Level::gain_exp(&LevelConfig) →
LevelUpMessage + GrantStatPointsMessage`。曲线通过 `LevelCurve` 注入（数据驱动、可换表、可测试桩）。

### 4.2 修饰符（L0 `atoms::modifiers`）：属性不知道修饰符从哪来

只有数据与纯函数（没有组件）：

```text
final = (base + Σflat) * (1 + clamp(Σpercent_add, min, max)) * Π(1 + percent_mul)
```

- `flat` 先结算；`percent_add` 合并后只乘一次（多个 +10% = +20%）；`percent_mul` 复利。
- 上限与取整口径由 `ModifierCaps` / `Rounding` 提供。
- **永久 vs 临时**：`Modifier.remaining` 为 `None` = 永久（装备 / 天赋），`Some(d)` = 临时（药水 /
  状态派生）；`Modifier::lasting(d)` 标记临时，组件的计时系统用 `Time` 推进寿命，到期即移除并 `refresh`。
- **按来源清理**：`ModifierSource` 就是施加者的 `Entity`，`remove_by_source` 一次清掉该来源在
  所有槽位上的修饰符（卸装备、被动失效、状态到期共用这条路）。
- 槽位由组件持有：`Stat` 里是 `StatModifiers`（按 `StatId` 分槽），`Resistance` 里是
  `ResistanceModifiers`（按伤害类型分 flat / percent，再加护甲 / 闪避）。

### 4.3 伤害类型（L1 `behaviors::damage`）

- `DamageType`（**R48**：定义在 L1，不进 `health`）+ `DamageTags`（`DOT` / `MELEE` / `CRIT` 位标志）。
- `DamageRequest { source, target, damage_type, base_amount, amount, tags, missed, crit }`：
  `amount` 是管线推进中的当前值，`missed` / `crit` 由判定层写入。
- **附加行为注册表**：每种伤害类型绑一组 `DamageBehaviorBinding { behavior, requires }`；
  `build()` 要求每个 `DamageType` 要么绑定行为、要么显式 `declare_no_behaviors` → 加载期报错。
- 分派只做"标签筛选"，`DamageBehaviorId → 具体请求` 的映射表由内容层提供（L1 不认识内容）。

**不做**：减伤（`resistance`）、状态判定（`status`）、血量（`health`）。

### 4.4 抵抗（L1 `behaviors::resistance`）

`Resistance` 也是一个自成整体的组件：`base`（内容层设定）+ `modifiers`（槽位）+ `cached`（视图）。

```rust
pub struct ResistValues { per_type: [ResistEntry; DamageType::COUNT], armor: f32, evasion: f32 }
let resistance: &Resistance = ...; resistance.per_type[DamageType::Fire.index()];   // Deref → 视图
```

- 两类减伤分家：
  - **确定性**（进管线）：`mitigate(amount, entry, armor, caps)` =
    `max((amount - flat - armor).max(0) * (1 - min(percent, max_percent)), amount * min_damage_ratio)`
    —— 上限防免疫，保底防"零伤害"。
  - **概率规避**（不进管线）：`evasion` 只被 `behaviors::rolls` 读。
- 修饰符消息：`AddResistanceModifierMessage`（按 `ResistSlot` 定位）、`RemoveResistanceModifiersMessage`
  （按来源整体清）；两条系统只查 `Resistance`，改完自己 `refresh`。
- **不参与状态判定**：状态走 §4.7 的独立豁免框架。

### 4.5 判定层（L1 `behaviors::rolls`）：唯一允许随机的地方

```rust
pub struct CombatRng(SplitMix64);   // 自带 PRNG，不引入 rand；种子来自 CombatConfig
pub struct RollConfig { base_evasion, max_evasion, base_crit_chance, crit_multiplier }
```

`roll_avoidance`（读目标 `evasion`）→ `roll_crit`，用 `MessageMutator<DamageRequest>` 就地写
`missed` / `crit`，两个系统 `.chain()` 固定顺序，并排在管线之前。同种子 = 同结果 → 整条链路可复现、可测。

### 4.6 伤害管线（L1 `behaviors::damage_pipeline`）：固定四阶段

```text
[判定层] → Base → AttackerBonus → DefenderMitigation → Finalize → DamageResolvedMessage
```

| 阶段 | 现状 |
|---|---|
| `stage_base` | 已实现：把 `amount` 从 `base_amount` 初始化（保证重入安全） |
| `stage_attacker_bonus` | 已实现暴击倍率；**TODO(M5)**：属性缩放 + 增伤规则表（规则顺序 = 注册顺序） |
| `stage_defender_mitigation` | 已实现：读 `Resistance` 视图 + `ResistanceCaps` 调 `mitigate` |
| `stage_finalize` | 已实现：按 `PipelineConfig::rounding` 取整 → 发 `DamageResolvedMessage` |

- **顺序固定、不允许插队**：扩展方式是"往阶段里加规则"，不是插新阶段。
- **确定性**：`missed` / `crit` 已由判定层写好，这里没有随机。
- 之后 `forward_resolved_damage` 发 `ModifyHealthMessage`（零伤害不发），
  再由 `behaviors::damage` 分派附加行为 → 状态判定**独立**发起。

### 4.7 状态（L1 `behaviors::status`）

**标识 / 定义 / 注册表与组件放在一起**（`status/mod.rs`），判定公式在 `contest.rs`，系统在 `systems.rs`：

```rust
pub enum StatusId { Burning, Frozen, Stunned }   // 自带 COUNT / ALL / index / name

pub struct StatusDef {
    id, contest: ContestKind, base_duration: Duration,
    max_stacks: u8, stacking: Stacking,     // Refresh / Stack / Unique
    behaviors: StatusBehaviors,             // on_apply / on_tick / on_expire: BehaviorId
}
StatusRegistry::builder().define(..).build()  // 每个 StatusId 都要有定义 → 加载期报错
```

**实例 = 独立子实体**（`StatusInstance` + `ChildOf(宿主)`）：

| 需求 | 落地方式 |
|---|---|
| 独立生命周期 | 实例自己藏 `remaining`；到期 despawn 并发 `StatusExpiredEvent`（`EntityEvent`） |
| 净化 | `PurgeStatusMessage` → despawn 实例（可只清指定状态） |
| 免疫 | `StatusImmunity` 位图组件，判定前查；命中 → `StatusResolvedMessage { applied: false, reason: Immune }` |
| 叠加 / 刷新 | 按 `Stacking` 改 `stacks` / 重置 `remaining`，`Stack` 受 `max_stacks` 限制 |
| 宿主销毁自动清理 | Bevy `ChildOf` 层级语义 |
| 每回合结算 | `tick_status_timers` 按 `StatusConfig::tick_interval` 发 `StatusTickMessage`（内容层跑 `on_tick`） |
| 不占修饰符列表 | 数值效果由 `on_apply` / `on_tick` 行为**派生** `AddStatModifierMessage` |

**统一概率框架**（物理 / 法术 / 精神共用一份公式，类型只决定"取哪对属性"）：

```rust
ContestKind::scores(attacker, defender)   // 物理: 力量 vs 体质；法术: 魔力 vs 意志；精神: 机敏 vs 意志
contest_chance(off, def, &ContestParams)  // 0.5 + slope*(off-def)/(off+def)，夹在 [min,max]
contest_duration(base, off, def, params)  // 用同一个强度比，倍数夹在 [1, max_duration_multiplier]
```

### 4.8 装配与内容（L1 `behaviors::combat`）

`CombatConfig` 聚合各子域配置（内容层注入一次），`CombatPlugin` 分发并装配
`(ProgressionPlugin, DamagePlugin, ResistancePlugin, RollsPlugin, DamagePipelinePlugin, StatusPlugin)`。
L2 只注册 `(HealthPlugin, StatPlugin, CombatPlugin)`。

内容层（L2，后续）：具体状态的 `StatusDef`、伤害类型的附加行为表、技能表、装备 / 被动数据。

## 5. 事件流（时序）

```text
L2 输入/AI ─► SkillCastMessage (behaviors::skills，待落地)
    ├─► DamageRequest ─► [rolls] 规避/暴击 ─► [pipeline 四阶段] ─► DamageResolvedMessage
    │                                                              ├─► ModifyHealthMessage ─► L0 health ─► DeathEvent ─► L2
    │                                                              └─► [damage 分派] ─► ApplyStatusRequest
    └─► ApplyStatusRequest ─► [status 判定] ─► StatusResolvedMessage ─► L2
                                      ├─► 实例增删改（独立子实体）
                                      └─► on_apply/on_tick ─► AddStatModifierMessage ─► Stat::refresh（同帧）

成长：GainExperienceMessage ─► [Intake] 升级 ─► LevelUpMessage + GrantStatPointsMessage
      ─► [Apply] Stat 记账 + 自己 refresh（同帧生效）

属性/抵抗的任何变更（加点、装备、药水到期）都只有一条消息，组件收到后自己刷新视图。
```

## 6. 依赖图与阶段排序

```text
atoms::stats ──► atoms::modifiers（纯算法）
behaviors::combat（配置 / 装配）
  ├─► status ──► rolls, atoms::stats(只读视图), atoms::modifiers(派生修饰符)
  ├─► damage_pipeline ──► damage, resistance, atoms::health(ModifyHealthMessage), atoms::modifiers(Rounding)
  ├─► rolls ──► damage, resistance(读 evasion)
  ├─► damage ──► （只 dep 自己的类型；分派只发消息）
  ├─► resistance ──► damage（按类型索引）+ atoms::modifiers
  └─► progression ──► atoms::stats
```

禁止：`damage → resistance`（伤害类型不认识减伤）、`atoms::* → behaviors::*`（R4/R92）、任何 `behaviors::* → L2`。

**阶段排序契约**（跨模块顺序不靠运气）：

| 阶段集合 | 顺序 | 归属 |
|---|---|---|
| `StatStage` | `Intake → Apply` | `atoms::stats` 定义；L1 成长插 `Intake`，L0 变更占 `Apply` |
| `DamageStage` | `Base → AttackerBonus → DefenderMitigation → Finalize` | `behaviors::damage_pipeline` 定义；判定层用 `.before(Base)` |

## 7. 扩展手册

| 新增 | 改动点 | 兜底 |
|---|---|---|
| 属性 | `atoms/stats/stat.rs`：`StatId` 变体 + `StatBlock` 字段 + `StatModifiers` 槽位 + `refresh` 一行 | `get` / `set` / `name` 的穷尽 `match` 编译期报错；聚合漏改靠测试 |
| 伤害类型 | `behaviors/damage.rs`：枚举 + `COUNT` / `ALL` / `index` / `name` | 抵抗表列宽与表现映射编译期报错 |
| 伤害附加行为 | 给该类型 `bind(...)`，或 `declare_no_behaviors(..)` | `build()` 覆盖检查 → 启动期报错 |
| 状态 | `behaviors/status/mod.rs`：枚举 + 辅助项 + 内容层注册 `StatusDef` | `StatusRegistry::build()` → 启动期报错 |
| 技能 | 内容数据表加一条 + 新效果类型加一个变体 | 穷尽 `match` 编译期报错（数据与执行分离） |
| 被动 / 装备效果 | 发 `AddStatModifierMessage` / `AddResistanceModifierMessage`（带 `ModifierSource`） | 按来源整体移除 = 卸下 / 失效 / 到期统一清理 |
| 调参 | 改对应 `XxxConfig`（或内容层从文件加载后注入） | 代码里不留魔法数 |

## 8. 落地状态

**已完成（有测试，共 73 条）**

- `atoms::modifiers`：固定顺序聚合 + 上限 + `Rounding` + 临时修饰符寿命（纯数据 + 纯算法）。
- `atoms::stats`：`StatId` / `StatBlock` / `StatModifiers` / **自成一体**的 `Stat`（五条变更系统 + 结果广播），
  `Level` / `LevelConfig` / `LevelCurve`。
- `behaviors::progression`：经验 → 升级 → 发点数（`Intake` 阶段，同帧可见）。
- `behaviors::damage`：伤害类型 + 标签 + 注册表（覆盖检查）+ 分派接口。
- `behaviors::resistance`：自成整体的 `Resistance` + `mitigate` 公式 + 上限 / 保底 + 临时修饰符。
- `behaviors::rolls`：SplitMix64 + 种子化 `CombatRng` + 规避 / 暴击判定。
- `behaviors::damage_pipeline`：四阶段 + `DamageResolvedMessage` + 转发 `ModifyHealthMessage`。
- `behaviors::status`：标识 / 定义 / 注册表 / 组件 / 免疫 / 叠加 / 净化 / 计时 / 判定（含 `contest_*` 公式）。
- `behaviors::combat`：`CombatConfig` 汇总 + 分发 + 装配。

**留了接口（`TODO(M5)`）**

- `stage_attacker_bonus` 的属性缩放与增伤规则表（需要技能 / 装备数据）。
- `dispatch_damage_behaviors` 的 `DamageBehaviorId → 请求消息` 映射（需要内容层行为表）。
- L2 的 `content`（状态定义、附加行为绑定、技能表）与 `presentation`（表现监听）。

**历史取舍**

| 曾有过 | 最终决定 |
|---|---|
| `voxelith_defs!` 宏 + 每个域一个 `defs.rs` | **删掉**：枚举的辅助项写在枚举旁边；定义并进组件文件，不再有 `defs.rs` |
| `Stat` 用 `[u32; COUNT]` 数组 + `Index` / `IndexMut` | **显式字段**：`stat.strength` 直接读，参数化才用 `StatId`；没有 `IndexMut` |
| L1 算最终值 → `StatFinalMessage` 回写 L0 | **组件自成一体**：`Stat` 持有修饰符槽位，收到变更消息自己 `refresh`，
  没有回写消息与脏标记 |

## 9. 已拍板 / 待确认

| 编号 | 问题 | 结论 |
|---|---|---|
| Q17 | `behaviors::combat` 定位 | ✅ 采纳 A：配置 + 装配（非空壳） |
| Q18 | 资源池（法力 / 耐力） | ✅ 采纳 C：推迟，保留 `Health` |
| Q19 | 属性最终值回写模式 | ✅ 采纳 A 后**再修订**：组件自成一体，内部 `refresh`（见 §8 历史取舍） |
| Q20 | 状态实例载体 | ✅ 采纳 A：独立子实体 + `ChildOf` |
| Q21 | 时钟与时间表示 | ✅ 采纳 B：`axiom` 允许 `bevy_time` |
| Q22 | 命名（宏模块 `defs`、判定层 `rolls`） | ✅ 判定层 `rolls` 落地；`defs` 模块**已删除** |
| Q23 | R5 白名单范围 | ✅ 采纳 A：bevy 家族严格白名单 + 非 bevy 依赖登记制 |
| Q24 | stats 取代 attribute | ✅ 采纳 A：迁移完成（`Stat` 保留） |
| Q25 | `u32` 与百分比修饰符取整口径 | ✅ 采纳 A：只在最后取整一次，`Rounding` 可配 |

## 10. 里程碑

| # | 内容 | 状态 |
|---|---|---|
| M1 | 定义工具（宏） | 已废弃（改为枚举自带辅助项） |
| M2 | `atoms::stats`（`Stat` 自成一体 + 变更系统 + 阶段契约） | ✅ |
| M3 | `atoms::modifiers`（聚合 + 上限 + 临时寿命） | ✅ |
| M4 | 资源池（法力 / 耐力） | 推迟（Q18） |
| M5 | 伤害链路（damage / resistance / rolls / pipeline） | ✅ 骨架 + 关键公式；规则表待内容层 |
| M6 | 状态（定义 / 判定 / 生命周期 / 免疫 / 净化 / 叠加） | ✅ 骨架 + 判定；`on_*` 行为表待内容层 |
| M7 | L2 内容与表现接线 | 待做（`content` + `presentation`） |

## 11. 自检清单

- [ ] 新增一项（属性 / 伤害类型 / 状态）是否只改一处？漏改会编译期叫吗？
- [ ] 最终值是不是由**拥有数据的组件自己**刷新的？有没有多余的"脏了 / 算好了"消息？
- [ ] 管线里有没有 `Modifier` / `StatusInstance` / RNG？（I3、I4）
- [ ] 每个组件的写入口是否唯一？外部只能读吗？
- [ ] 伤害类型是否仍然不认识减伤与状态判定？
- [ ] 状态的数值效果是否通过"派生的修饰符"而不是塞进修饰符列表？
- [ ] 新参数是否进了某个 `XxxConfig` Resource，而不是写成常量？
- [ ] 单文件是否 < 500 行（R26）？
