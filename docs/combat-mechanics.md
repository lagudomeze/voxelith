# 战斗机制 ECS 设计（属性 / 伤害类型 / 抵抗 / 伤害管线 / 扩展 / 状态）

> **状态：设计稿（先设计后编码）。** 落地按 §10 里程碑推进；未拍板的点集中在 §9，并同步记在
> [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)（Q17–Q22）。
> 规则依据：[layers.md](layers.md)（R8–R19、R112–R115）、[combat.md](combat.md)（R47–R63）、
> [bevy-events.md](bevy-events.md)（Message/Event 判定）、[naming.md](naming.md)（R28–R32）。

## 0. 一句话

**定义在一处，公式归 L1，数值执行在 L0，判定单独一层，管线只算数，状态自成生命周期。**
凡是"需要多个组件同时存在才生效"的系统，一律放 **L1 `behaviors`**（R11）。

## 1. 五条不变量（贯穿全部模块）

| # | 不变量 | 保障手段 | 违反时的症状 |
|---|---|---|---|
| I1 | **定义一处**：属性/资源/伤害类型/状态各只有一个定义点 | `voxelith_defs!` 宏生成 `enum + COUNT + ALL + name + index` | 漏改一处 → 编译期报错（穷尽 `match`）或加载期报错（注册表覆盖检查） |
| I2 | **L1 算、L0 存**：公式在 L1，L0 只做"收下并写入" | L1 发 `*FinalMessage` / `Modify*Message`，L0 唯一写入口消费（R13、R56） | L1 直接写 L0 组件 → 数值变化不可追踪 |
| I3 | **管线只读"最终值视图"**：管线不认识修饰符、不认识状态 | 每个域把聚合结果写成视图组件（`Attributes.cached` / `Resistance.final` / `Offense.final`） | 管线里出现 `Modifier` / `StatusInstance` → 公式与来源耦合 |
| I4 | **随机只出现在判定层**：管线只有确定性的四阶段 | RNG 资源只在 `behaviors::rolls` 与状态判定中被调用，种子可注入 | 管线里 `if rng.roll()..` → 结果不可复现、不可测 |
| I5 | **一个组件一个写入口**：谁拥有数据，谁收口写入 | L0 组件由 `atoms::*` 的执行器写；L1 组件由本域的唯一写系统写 | 两处都能写 → 双写打架 |

## 2. 分层落点总表

| 模块 | 层 | 拥有什么 | 系统 | 绝不做的 |
|---|---|---|---|---|
| `atoms::attribute` | L0 | `AttributeId` 定义、`AttributeValues`、`Attributes` 组件（base/allocated/cached） | 只查 `Attributes` 的写入口 | 不认识修饰符、Buff、状态 |
| `atoms::resource`（**暂不落地**，Q18） | L0 | `ResourceId` 定义、`ResourcePools`（current/max） | 只查 `ResourcePools` 的写入口 | 不认识属性公式、状态 |
| `atoms::health` | L0 | `Health`（**保留**，Q18 决定资源池推迟） | `apply_health_change`（R56） | 不认识 `DamageType`（R47、R98） |
| `behaviors::modifiers` | L1 | `Modifier` / `ModifierOp` / `Stacking` + 纯聚合函数 | 无系统、无组件，只被各域调用 | 不认识"被修饰的是什么" |
| `behaviors::attributes` | L1 | `AttributeModifiers` 组件、脏消息 | 聚合 → `AttributeFinalMessage`；属性→资源上限映射 | 不写 `Attributes`（发消息，R13） |
| `behaviors::damage` | L1 | `DamageType` 定义、`DamageRequest`、附加行为注册表 | 分派附加行为（发消息） | 不算减伤、不做状态判定 |
| `behaviors::resistance` | L1 | `Resistance`（按伤害类型索引）、`ResistanceModifiers`、上限配置 | 聚合 → 视图；提供纯函数 `mitigate()` | 不掷骰、不写血量 |
| `behaviors::rolls` | L1 | `CombatRng`、命中/闪避/暴击判定 | `roll_avoidance`、`roll_crit` | 不改数值、不做减伤 |
| `behaviors::damage_pipeline` | L1 | 固定四阶段、`DamageResolvedMessage` | stage_base → attacker → defender → finalize | 不掷骰、不管状态、不管附加行为 |
| `behaviors::status` | L1 | `StatusId` 定义、`StatusDef` 注册表、状态实例、豁免判定 | 判定/施加/每回合结算/到期/净化/免疫/叠加 | 不塞进修饰符列表、不混进管线 |
| `behaviors::combat` | L1 | `CombatConfig`（caps/系数/种子）+ 装配 | 无业务系统 | 不写具体状态/技能内容 |
| L2 `content` / `presentation` | L2 | 具体状态定义与行为绑定、技能表、特效 | 监听 `DamageResolvedMessage` / `StatusResolvedMessage` | 不算伤害（R17） |

**怎么判断放 L0 还是 L1**（沿用 [workflow.md](workflow.md) ①）：

- 纯数据 + 系统只查自己 + **需要唯一写入口** → L0。
- 需要两个以上组件/资源同时在场才能算 → L1。
- 本设计里"状态实例"也放 L1：它的每个系统都要读宿主属性、免疫、修饰符，放 L0 只会产生一个"消费者全在 L1"的 L0 组件。

## 3. 定义工具：让"新增一项"只改一行

### 3.1 `voxelith_defs!`（宏，无 proc-macro、无新依赖）

```rust
// crates/voxelith-axiom/src/defs.rs
voxelith_defs! {
    /// 一级属性（唯一定义点）。
    pub enum AttributeId {
        Strength = "strength",
        Dexterity = "dexterity",
        Constitution = "constitution",
        Magic = "magic",
        Willpower = "willpower",
        Cunning = "cunning",
    }
}
```

宏展开为：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeId { Strength, Dexterity, /* ... */ }

impl AttributeId {
    pub const COUNT: usize = [$(stringify!($variant)),*].len(); // 数组字面量 .len() 是 const
    pub const ALL: [Self; Self::COUNT] = [$(Self::$variant),*];
    pub const fn index(self) -> usize { self as usize }        // 无字段枚举可 as
    pub const fn name(self) -> &'static str { match self { $(Self::$variant => $label),* } }
}
```

于是：**新增一个属性 = 在枚举里加一行**，`COUNT` / `ALL` / `name` / `index` 自动跟随；
任何别处的穷尽 `match AttributeId`（UI 分组、掉落表）立即**编译期报错** —— 这就是"遗漏定义时报错"的第一道。

### 3.2 数组按 `COUNT` 列宽（自动跟随）

```rust
#[derive(Clone, Copy)]
pub struct AttributeValues([f32; AttributeId::COUNT]);          // 加一项 → 自动扩列
impl core::ops::Index<AttributeId> for AttributeValues { /* by index() */ }

#[derive(Clone, Copy, Default)]
pub struct ResistEntry { flat: f32, percent: f32 }
pub struct Resistance { per_type: [ResistEntry; DamageType::COUNT], /* armor, evasion */ }
```

`Resistance` 的列宽跟着 `DamageType::COUNT` 走 —— **新增一种伤害类型，抵抗定义自动跟随**，
默认减伤 0，不会漏项。

### 3.3 注册表覆盖检查（加载期兜底）

数组能"自动跟随"，但"必须有内容"的项（每种伤害类型的附加行为、每个状态的 def）
用注册表 + `build()` 校验，**缺项 = 启动即失败**：

```rust
pub struct StatusRegistry { defs: [Option<StatusDef>; StatusId::COUNT] }

impl StatusRegistry {
    /// 漏定义 → Err，Plugin::build 里 expect → 启动期报错。
    pub fn build(defs: impl IntoIterator<Item = StatusDef>)
        -> Result<Self, MissingDefinition<StatusId>> { /* 收集缺失项 */ }
}
```

同一模式用于 `DamageBehaviorRegistry`（伤害类型 → 附加行为绑定）。

### 3.4 数据与执行分离

- 技能/状态/装备是**数据**（`SkillDef` / `StatusDef` / `Modifier` 列表），可配置、可序列化。
- 执行是**枚举或函数指针**（`SkillEffect` / `BehaviorId`），加一种效果 = 加一个变体 → 穷尽 `match` 编译期报错。
- L2 只提供数据，不写公式（R17）。

## 4. 模块设计

### 4.1 属性（L0 `atoms::attribute` + L1 `behaviors::attributes`）

**L0 数据组件**（字段私有，`AsRef<AttributeValues>` 暴露最终值）：

```rust
#[derive(Component, Default)]
pub struct Attributes {
    base: AttributeValues,       // 初始 + 升级 + 玩家分配
    allocated: AttributeValues,  // 洗点依据
    cached: AttributeValues,     // 最终值缓存（L1 算好回写）
    unspent: f32,
    revision: u64,               // 缓存版本号，避免每帧重算
}
```

**L0 唯一写入口**（每个都只查 `Attributes`）：

| 系统 | 消费 | 做什么 |
|---|---|---|
| `apply_attribute_allocation` | `AttributeAllocationMessage` | 加点/洗点/升级 → 改 `base` / `allocated` / `unspent` |
| `apply_attribute_final` | `AttributeFinalMessage` | **只把 L1 算好的值写进 `cached`**（+ `revision`） |

**L1 公式**（需要两个组件，所以必须在 L1）：

```rust
pub fn recompute_attribute_final(
    mut changed: MessageReader<AttributeModifiersChangedMessage>,
    actors: Query<(&Attributes, &AttributeModifiers)>,
    mut out: MessageWriter<AttributeFinalMessage>,
) { /* final = modifiers::evaluate(base, mods.iter()) */ }
```

- 触发时机是**脏消息**，不是每帧 → 满足"最终值可缓存，不必每帧重算"。
- L1 **不写** `Attributes`，只发 `AttributeFinalMessage`（R13）。
- 资源上限映射（`max_life = f(Constitution, ...)` → `SetResourceMaxMessage`）**随 §4.3 一并推迟**（Q18）。

### 4.2 修饰符（L1 `behaviors::modifiers`）：属性不知道修饰符从哪来

只放**纯数据 + 纯算法**，不认识"被修饰的是什么"，因此**没有中心 `ModifierTarget` 枚举**（避免枢纽耦合）：

```rust
pub enum ModifierOp { Flat, PercentAdd, PercentMul }
pub struct Modifier { pub op: ModifierOp, pub value: f32, pub source: ModifierSource, pub expires_at: Option<Duration> }

/// 固定顺序、确定性、无随机：flat → percent_add（带上限）→ percent_mul。
pub fn evaluate(base: f32, mods: impl Iterator<Item = Modifier>, caps: &ModifierCaps) -> f32 {
    // (base + Σflat) * (1 + clamp(Σpct_add)) * Π(1 + pct_mul)
}
```

各域**自治**：属性域有 `AttributeModifiers(Vec<Modifier>)` + `AttributeModifiersChangedMessage`；
抵抗域有 `ResistanceModifiers(Vec<Modifier>)` + 自己的脏消息。

> 取舍说明：中心 `ModifierTarget { Attribute(..), Resistance(..) }` 少写一点管道，但把"新增可修饰量"变成
> "改中心枚举 + 改所有消费者"。按本需求（低耦合优先），选**每域自带修饰符列表 + 共享聚合函数**。

### 4.3 资源（L0 `atoms::resource`）— **暂不落地（Q18 采纳 C）**

> 结论：现在只保留 `Health`，**不引入** `ResourcePools`；等法力/耐力出现真实需求时再按本节落地
> （届时一并处理 Q2：`Health` 字段私有化）。下面保留设计意图，供后续实现参考。

```rust
voxelith_defs! { pub enum ResourceId { Life = "life", Mana = "mana", Stamina = "stamina" } }

#[derive(Component, Default)]
pub struct ResourcePools([ResourcePool; ResourceId::COUNT]); // { current, max }
```

| 消息 | 系统（唯一写入口） | 语义 |
|---|---|---|
| `ModifyResourceMessage { entity, resource, amount }` | `apply_resource_change` | 负数=消耗，正数=恢复；夹取到 `0..=max` |
| `SetResourceMaxMessage { entity, resource, max }` | `apply_resource_max` | 上限只能由 L1 的属性映射写 |

- 归零 → 发 `ResourceDepletedEvent`（`EntityEvent`，target = 实体）。
- 当资源池真正落地时，建议把 `Health` 收编为 `ResourceId::Life`，并保留 `ModifyHealthMessage`
  作为语义化门面（两条消息 → 同一个写入口）。**当前决定：推迟，见 Q18。**

### 4.4 伤害类型（L1 `behaviors::damage`）

- `DamageType`（`voxelith_defs!`，R48：定义在 L1，不进 `health`）。
- `DamageRequest`（`Message`，请求型）携带标识与上下文，**不含减伤结果**。
- **附加行为分派**：`DamageBehaviorRegistry`，注册表按 `DamageType` 索引（列宽自动跟随），
  `build()` 校验"每种类型都有绑定"（可配置为必须/可选）。

```rust
pub fn dispatch_damage_behaviors(
    mut resolved: MessageReader<DamageResolvedMessage>,
    registry: Res<DamageBehaviorRegistry>,
    mut out: MessageWriter<ApplyStatusRequest>, // 只发"别的模块的消息"
) { /* 每种伤害类型 → 它自己的附加行为 */ }
```

约束：本模块**不算减伤、不判状态**，只做"标识 + 分派"；分派通过**事件（消息）触发**，
因此与抵抗算法零耦合。

### 4.5 抵抗（L1 `behaviors::resistance`）

```rust
#[derive(Component)]
pub struct Resistance {
    per_type: [ResistEntry; DamageType::COUNT], // 自动跟随伤害类型
    armor: f32,
    evasion: f32,                               // 概率规避，不进管线（I4）
}
pub struct ResistanceCaps { pub max_percent: f32, pub min_damage_ratio: f32 } // 防免疫
```

- `Resistance` 是**目标的防御数据，伤害结算时只读**；写入由本模块唯一写入口 `apply_resistance_final`
  （消费 `ResistanceFinalMessage`，由 `ResistanceModifiers` 聚合而来）。
- **确定性减伤**（进管线）：`after_flat = max(0, amount - flat)` →
  `after_pct = after_flat * (1 - min(percent, caps.max_percent))` → `max(结果, amount * min_damage_ratio)`。
- **概率规避**（不进管线）：`evasion` 只被 `behaviors::rolls` 读取。
- 抵抗**不参与状态判定**——状态判定走 §4.8 的独立豁免框架。
- 修饰符只改数据；管线只读视图（I3）。

### 4.6 判定层（L1 `behaviors::rolls`）：唯一允许随机的地方

```rust
#[derive(Resource)]
pub struct CombatRng(SplitMix64); // 自带十几行确定性 PRNG，不引入 rand/bevy_math 依赖

pub fn roll_avoidance(mut req: MessageMutator<DamageRequest>, mut rng: ResMut<CombatRng>) { /* 命中失败 → req.missed = true */ }
pub fn roll_crit(mut req: MessageMutator<DamageRequest>, mut rng: ResMut<CombatRng>) { /* req.crit = true */ }
```

- 顺序**显式 `.chain()`**：先规避、后暴击（[bevy-events.md](bevy-events.md) §5.2：`MessageMutator` 独占，可串）。
- 种子来自 `CombatConfig` → 同种子同输入 = 同结果，**可复现、可测**。
- 判定结果以 `missed` / `crit` 布尔进入管线；管线本身不再掷骰（I4）。

### 4.7 伤害管线（L1 `behaviors::damage_pipeline`）

**阶段固定、顺序固定、不许插队**：

```
DamageRequest
  ├─ stage_base           基础伤害（技能/武器数据解析，确定性）
  ├─ stage_attacker_bonus 攻击方加成（读 Attributes.cached + Offense 视图，纯确定）
  ├─ stage_defender_mitigation 防御方减伤（读 Resistance 视图 + caps，纯确定）
  └─ stage_finalize       上限/保底/取整（集中在一处）
        ▼
DamageResolvedMessage { source, target, damage_type, amount, missed, crit }   ← 唯一输出
        ▼
forward_resolved_damage → ModifyHealthMessage（L0 唯一写入口，R50/R56）
```

- 每阶段是**纯函数 + 薄系统**，只在**已注册的规则列表**上迭代（扩展 = 往槽位注册规则，
  不是插新阶段）；规则顺序 = 注册顺序，显式且可测。
- 被规避（`missed`）也算一次结算，发 `amount = 0` 的 `DamageResolvedMessage`，
  由表现层决定"闪避"演出（对应 Q9 建议 B/C）。
- **状态判定不在管线里**：管线之后，由 §4.4 的分派触发 `ApplyStatusRequest`（需求 4/6 的硬要求）。

### 4.8 状态（L1 `behaviors::status`）

**定义**（一处）+ **注册表**（加载期兜底）：

```rust
voxelith_defs! { pub enum StatusId { Burning = "burning", Frozen = "frozen", Stunned = "stunned" } }

pub struct StatusDef {
    pub id: StatusId,
    pub contest: ContestKind,        // Physical / Spell / Mental
    pub base_duration: Duration,
    pub max_stacks: u8,
    pub stacking: Stacking,          // Stack / Refresh / Unique
    pub behaviors: StatusBehaviors,  // on_apply / on_tick / on_expire: BehaviorId
}
```

**实例 = 独立子实体**（`StatusInstance` + `ChildOf(host)`）：

| 需求 | 落地方式 |
|---|---|
| 独立生命周期 | 实例实体自己计时；`StatusExpiredEvent`（`EntityEvent`）宣告结束 |
| 净化 | despawn 实例实体（或按 `PurgeFilter` 批量） |
| 免疫 | L1 判定系统读 `StatusImmunity` 组件；命中免疫 → `StatusResolvedMessage { applied: false }` |
| 叠加/刷新 | 判定系统查同 `StatusId` 实例，按 `stacking` 改 `stacks` / 重置 `timer` |
| 宿主销毁自动清理 | Bevy `ChildOf` 层级语义（并让监听器作用域化绑定到实例实体） |
| 状态不进修饰符列表 | 状态的数值效果由 `on_apply` / `on_tick` 行为**派生** `AddModifierMessage`，状态本身不占用修饰符槽位 |

**统一概率框架**（`behaviors::status::contest`，物理/法术/精神底层同一套逻辑）：

```rust
pub trait ContestProfile { fn offense(&self) -> f32; fn defense(&self) -> f32; }  // 只负责"取哪些数值"
pub fn contest_chance(offense: f32, defense: f32, p: &ContestParams) -> f32;      // 唯一概率公式，带上下限
pub fn contest_duration(base: Duration, offense: f32, defense: f32, p: &ContestParams) -> Duration;
```

- `ContestKind` 只决定"用哪对攻防强度"（策略映射），**判定算法只有一份**。
- 时长由强度差影响，且夹在 `[min, max]`，避免无限控。

**每回合结算**：状态计时用 `Time::delta()` 推进 `StatusTimer`（`Duration`），
按 `StatusTickInterval`（回合步长，L2 可配）触发 `StatusTickMessage` → 内容层注册的 `on_tick` 行为
（例如"每层每回合造成火焰伤害"→ 发 `DamageRequest`）；到期发 `StatusExpiredEvent`。

> **R5 已修订（Q21 采纳 B）**：`axiom` 允许依赖 `bevy_time`，用 `Time`/`Duration`，不再自建 `BattleClock`。
> 回合 = `StatusTickInterval` 的时间步长，测试里可用 `Time::advance_by` 精确推进，同样可复现。

### 4.9 装配与内容

- L1 `behaviors::combat`：`CombatConfig { rng_seed, resistance_caps, contest_params, min_damage_ratio, rounding }`
  + `CombatPlugin`（有配置 → 满足 R41.1，不是空壳）。
- L2 `content`：具体伤害类型的附加行为绑定、具体状态定义与行为、技能表、装备/被动数据。
- 被动/持续效果靠**监听事件**实现（不硬编码）；监听器**作用域化**绑定到实体，
  实体销毁即自动清理（Bevy 0.19 实体级 observer 的确切 API 名称以本地实测为准；
  不可用时退化为"`On<Despawn>` + 显式清理"）。

## 5. 事件流（时序）

```
L2 输入/AI ─► SkillCastMessage (behaviors::skills)
    ├─► DamageRequest ──► [rolls] 规避/暴击 ──► [pipeline 四阶段] ──► DamageResolvedMessage
    │                                                                  ├─► ModifyHealthMessage ─► L0 health ─► DeathEvent ─► L2 表现
    │                                                                  └─► [damage 分派] ─► ApplyStatusRequest
    └─► ApplyStatusRequest ──► [status 判定] ──► StatusResolvedMessage ─► L2 表现
                                      ├─► 实例增删改（本域唯一写入口）
                                      └─► AddModifierMessage ─► [域聚合] ─► *FinalMessage ─► L0 视图回写

每回合：L2 AdvanceTurnMessage ─► BattleClock ─► tick_status_timers ─► StatusTickMessage / StatusExpiredEvent
```

**伤害与状态的关系**：先出 `DamageResolvedMessage`（伤害结算完），再**独立**发起状态判定（需求 4/6）。

## 6. 模块依赖图（只允许向下）

```
behaviors::combat（配置/装配）
  ├─► behaviors::status ──► modifiers, atoms::attribute(只读视图), rolls
  ├─► behaviors::skills ──► damage, status
  ├─► behaviors::damage_pipeline ──► damage, resistance
  ├─► behaviors::rolls ──► damage
  ├─► behaviors::resistance ──► damage（仅为按类型索引）
  ├─► behaviors::damage ──► atoms::health（只用 ModifyHealthMessage）
  ├─► behaviors::attributes ──► modifiers, atoms::attribute（只读 base、只回写缓存消息）
  └─► behaviors::modifiers ──► （无内部依赖）
```

禁止：`damage → resistance`（伤害类型不认识减伤）、`damage → status`（分派只发消息）、
`atoms::* → behaviors::*`（依赖反向，R4/R92）、任何 `behaviors::* → L2`。

## 7. 扩展手册（新增内容要改哪几处）

| 新增 | 改动点 | 兜底报错 |
|---|---|---|
| 属性 | `atoms/attribute/defs.rs` 加一行 | `COUNT`/`ALL`/`name` 自动；别处穷尽 `match` 编译期报错 |
| 资源 | `atoms/resource/defs.rs` 加一行 + L1 上限映射规则加一条 | 同上 |
| 伤害类型 | `behaviors/damage/defs.rs` 加一行 | 抵抗数组自动扩列；表现层 `match` 编译期报错 |
| 伤害附加行为 | L2 内容注册表加一条 | 注册表 `build()` 覆盖检查 → 启动期报错 |
| 状态 | 定义宏加一行 + L2 注册一条 `StatusDef`（含三个行为） | `StatusRegistry::build()` → 启动期报错 |
| 技能 | L2 数据表加一条；新效果类型加一个 `SkillEffect` 变体 | 穷尽 `match` 编译期报错（数据与执行分离） |
| 被动/装备效果 | 注册 `Modifier` 数据 + 作用域化监听器 | 宿主销毁 → 监听自动清理 |

## 8. 现状差距（本仓库已有代码）

| 现状 | 问题 | 本设计的处理 |
|---|---|---|
| `atoms/attribute.rs` 把 Buff 重算写在 L0（查询 `Children` + `Buff`） | 违反 R8「L0 只查自己」、R11「多组件逻辑归 L1」 | 重算迁到 L1 `behaviors::attributes`；L0 只留数据 + `apply_attribute_final` |
| 同上文件 `dirty.write/read` 引用了未定义的 `AttributesDirty`，`recompute_final_attributes_system` 也无该参数 | **当前无法编译** | 落地 M2 时一并修掉（见 [TODO](../work/TODO.md)） |
| `Attributes` 用结构体字段逐个列出 | 加一个属性要改 6 处 | 换成 `voxelith_defs!` + `[f32; COUNT]` 数组 |
| `Health` 字段 `pub`（Q2）+ 无资源池概念 | L2 可直接写、法力/耐力无处安放 | Q18：收编为 `ResourceId::Life`，字段私有 + 只读访问器 |
| 时间用 `f32` / 混用 `Duration`（Q11） | 长局精度、口径不一 | 时长统一 `Duration`，节奏用 `BattleClock` 回合数 |

## 9. 待确认（细节见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)）

| 编号 | 问题 | 我的建议 |
|---|---|---|
| Q17 | `behaviors::combat` 是"配置 + 装配器"还是不存在（各域 Plugin 直接进 tuple） | 保留：它有 `CombatConfig`，满足 R41.1 |
| Q18 | `Health` 是否收编为 `ResourceId::Life`（影响 R56 与既有测试） | ✅ **已确认采纳 C（推迟）**：保留 `Health`，资源池等有需求再做 |
| Q19 | 属性最终值回写模式：L1 发 `AttributeFinalMessage`（推荐）还是允许 L1 直写缓存 | 发消息（守 R13） |
| Q20 | 状态实例载体：独立子实体（推荐）vs 宿主组件数组 | 子实体（清理/净化/作用域天然） |
| Q21 | 时钟与时间：`bevy_time` 的 `Time`/`Duration`（推荐）vs 自建 `BattleClock` | ✅ **已确认采纳 B**：R5 放宽，用 `bevy_time` |
| Q22 | 命名：共享宏模块 `defs`、判定层 `behaviors::rolls`（备选 `resolution`/`chance`） | 先按 `defs` / `rolls` |

## 10. 里程碑与验收

| # | 内容 | 验收（关键断言） |
|---|---|---|
| ✅ M1 | `defs.rs` 宏（各域注册表 `build()` 覆盖校验随后续域落地） | 已落地：`src/defs.rs` 单测；`AttributeId` 已改用宏 |
| ✅ M2 | `atoms::attribute` 重构为 L0 纯数据 + 唯一写入口 | 已落地：`atoms/attribute/{defs,systems,mod}.rs`；L0 内无多组件查询 |
| ✅ M3 | `behaviors::modifiers` + `behaviors::attributes` | 已落地：`tests/attribute.rs` 11 条 + 模块内 8 条；无脏消息不重算 |
| M4 | ~~`atoms::resource`（含 Q18 决策）+ 属性→上限映射~~ **已推迟（Q18）** | 等法力/耐力有真实需求时再做 |
| M5 | `behaviors::{damage, resistance, rolls, damage_pipeline}` | 同种子同输入结果一致；规避不进管线；减伤不透支上限 |
| M6 | `behaviors::status` + `BattleClock` | 免疫/叠加/刷新/净化/到期五条用例；状态判定在伤害之后 |
| M7 | L2 内容与表现接线 | 表现只读 `DamageResolvedMessage` / `StatusResolvedMessage`，无公式 |

## 11. 自检清单

- [ ] 新增一项（属性/资源/伤害类型/状态）是否只改一处？
- [ ] 遗漏定义是在**编译期**还是**加载期**报错？（不能是"运行到才发现"）
- [ ] L1 有没有直接写 L0 组件？（应该发消息）
- [ ] 管线里有没有 `Modifier` / `StatusInstance` / RNG？（I3、I4）
- [ ] 每个组件的写入口是否唯一？
- [ ] 伤害类型是否仍然不认识减伤与状态判定？
- [ ] 状态的数值效果是否通过"派生的修饰符"而不是塞进修饰符列表？
- [ ] 监听器是否作用域化，实体销毁能自动清理？
- [ ] 单文件是否 < 500 行（R26）？
