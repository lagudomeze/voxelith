# 半即时战斗系统（落地稿）

> **状态：已落地。** 本文是当前战斗机制的**唯一权威设计**，取代
> [combat-mechanics.md](combat-mechanics.md)（旧伤害管线 / 抵抗 / 判定层方案，已删除）。
>
> 规则依据：[architecture.md](architecture.md)（R1–R7、R116–R117）、[layers.md](layers.md)（R8–R19、R112–R115）、
> [naming.md](naming.md)（R28–R32）、[bevy-events.md](bevy-events.md)（Message/Event 判定）。
>
> 一句话：**PC 空槽时冻结时间等输入，怪物行动威胁 PC 时开反制窗口；引擎只提供受控原语，内容全走 RON。**

## 0. 设计原则

1. **定义与实例分离**：`Skill` / `StatusDef` 是模板（全局共享的实体），`Action` / `ActiveStatus` 是运行时实例。
2. **内容配置化，原语代码化**：会长大的东西（技能 / 状态 / 资源 / 怪物）走 RON；引擎原语（`Value` /
   `Formula` / `Effect` / `Requirement`）走 Rust 枚举。
3. **组合优于枚举**：语义用标记（`SkillTags` / `StatusTags`），不用 `enum ActionKind`。
4. **槽位只有"有没有"**：`ActiveActions.len() <= 1`，不区分种类。占不占槽由 `duration` 决定。
5. **反制是决策窗口，不是行动槽**：由 `CombatPhase` 调度。
6. **效果系统统一**：技能结算与状态生命周期共用同一套 `Effect`。
7. **对抗是一等公民**：伤害 = `Contest(攻 vs 防)` 成功后的 `ModifyResource(HP, -x)`；**没有 `Damage` 原语**。

> **两处已知的待定与缺口**（细节见各自文档，别在这里猜）：
>
> - **原则 4 可能要收窄**。一旦引入**移动**（"走一格"也是时间上的一段），
>   "一次只执行一个动作"就要改成"**一次只执行一个技能动作**；移动是独立通道、受动作阶段门控"。
>   候选与取舍见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) **Q28**，设计见 [movement.md](movement.md)。
> - **本文没有"前摇 / 释放点 / 后摇"这三段**（搜不到这三个词）。现行实现里 `Action` 只有一个
>   `duration`，释放点就是它走完的那一刻，后摇与冷却都还没落地。
>   移动设计要求把这条时间轴补齐，且**顺带修掉 `tick_actions` 的累加式推进**
>   （`action.elapsed += delta` → 绝对时间戳）：见 [movement.md](movement.md) §4。

## 1. 落地决议（相对原始设计稿的收敛）

原始设计稿与已拍板的项目规则有六处冲突，本次落地的取舍如下（都写进代码注释）：

| # | 冲突点 | 决议 | 理由 |
|---|---|---|---|
| D1 | 设计稿 `Effect` 变体引用 `SkillId` / `StatusId`，而定义在运行时是**实体** | 原语里一律用**词汇 ID**（`SkillId` / `StatusId` / `ResourceId` / `StatId`），**运行时实体**由 `SkillCatalog` / `StatusCatalog` 查得 | RON 里只能写 ID；实体句柄是可变的加载结果，不该进数据 |
| D2 | 设计稿目录把 `data/loader.rs`（含文件读取）放在引擎侧 | **RON 描述结构 + 词汇映射 + 解析**在 `axiom`（L1 `content`）；**文件读取**在 `prime`（L2） | R101 精神：L0/L1 不读文件、不碰 `AssetServer`，仍可在无文件系统环境跑测试 |
| D3 | 设计稿 `Stats` 是 `HashMap<StatId, f32>`，与不变量 I1（穷尽 `match` 编译期兜底）、I2（组件自成一体）冲突 | **按拍板全量切动态 ID**：`Stat` = `base` + `modifiers` + `effective`，`StatId` 为 `u16` 词汇 ID | "加内容不改代码"是配置化的真正目标（设计原则 10）；代价是丢掉显式字段的编译期穷尽检查，改由**加载期覆盖检查 + 测试**兜底 |
| D4 | 设计稿只说 `Success/Fail/Crit/Fumble`，未说明差值去向；旧抵抗是数值减伤公式 | `Formula::Difference/Ratio` 用**阈值**判成功；同时把 `attacker - defender`（或比值）写入本次结算的 **`SkillPower`**，供效果当数值用；`RollUnder` 是唯一掷骰的对抗 | 既保留二元结果，又不丢"强多少 → 打多少"的能力（见 [§5](#5-对抗与判定)） |
| D5 | 设计稿 `Effect::DispelAction` 的 `Who::Target` 指向"怪物实体"，但被取消的是**怪物的 action** | `Who::Target` **优先解析为怪物当前的行动实体**（`PendingThreat.action`），无威胁时退回目标本身 | 反制要取消的是"即将生效的 action"，不是怪物这个实体 |
| D6 | 设计稿 `EffectContext` 只查 `(&mut Resources, &mut Stats, ...)`，拿不到状态与行动 | 执行上下文只持有**跨域只读查询**（`StatusDef` / `ActiveStatus` / `Skill`）+ 写入口（`Resources` / `Cooldowns` / `Commands`）；`StatModifiers` 由**每帧派生系统**从状态重建 | R15/R8：公式需要什么就查什么，但写入口保持唯一（`Commands` 增删组件，不给 `&mut StatModifiers`） |

## 2. 三大数据类别

| 类 | 本质 | 回答 | 存储 | 例子 |
|---|---|---|---|---|
| **Resource** | 池（current / max） | "还剩多少？" | `Resources` | HP、Mana、Action、Reaction |
| **Stat** | 数值 | "有多强？" | `Stat` | 力量、护甲、暴击率 |
| **Status** | 标记（带时长） | "现在什么情形？" | `ActorState` + `ActiveStatus` 实体 | 格挡、眩晕、中毒 |

关系：**Status 修改 Stat，Status / Resource 门控行为，Stat 参与对抗。**

```text
Stat   { base: HashMap<StatId,f32>, modifiers: Vec<StatModifier>, effective: HashMap<StatId,f32> }
Resources { pools: HashMap<ResourceId, Pool{current,max}> }
ActorState { statuses: Vec<Entity> }   // → ActiveStatus 实体
```

> **为什么 `Stat` 持有 `modifiers` 而不是让状态直接写**（D3 + I2）：状态的数值效果从不直接改
> `Stat`，而是**派生**成 `StatModifier { stat, delta, source }`；`apply_status_modifiers` 每帧按
> `source = ActiveStatus 实体` 重建整份修饰符，`Stat::refresh` 重算 `effective`。
> 状态到期 → 下一帧派生自然消失，**不需要"清理修饰符"的消息**，也不会有残留。

## 3. 引擎原语（受控闭集）

### 3.1 `Value` — 表达式

```rust
pub enum Value {
    Literal(f32),
    SkillPower,                 // 本次结算的强度（对抗差值 / 比值，见 §5）
    CasterStat(StatId),
    TargetStat(StatId),
    Resource { pool: ResourceId, who: Who },
    Sum(Vec<Value>),
    Mul(Box<Value>, Box<Value>),
    Neg(Box<Value>),
    Scale(Box<Scale>),          // 过两锚点拟合曲线（数值骨干，见 numbers.md §2.1）
    Rescale(Box<Rescale>),      // 分段线性压缩（同上 §2.2）
}
pub enum Who { Caster, Target }
```

求值失败（缺池 / 缺属性 / 曲线参数不合法）**一律回退 0**，绝不 panic：RON 是内容，内容出错不能炸运行时。

> **数值曲线单独成篇**：[numbers.md](numbers.md)。所有成长曲线都走 `Scale` / `Rescale`，
> 不在技能里各写一段 `a * level + b`。设计取自 ToME4（只取机制，不抄实现）。

### 3.2 `Contest` — 对抗

```rust
pub struct Contest {
    pub attacker: Value,
    pub defender: Value,
    pub formula: Formula,
    pub threshold: f32,          // Difference 的单位阈值 / Ratio 的比值阈值 / RollUnder 的掷骰目标
    pub crit_margin: f32,        // 超出阈值多少算 Crit（0 = 关闭）
    pub outcomes: Vec<(Outcome, Vec<Effect>)>,
}
pub enum Formula { Difference, Ratio, RollUnder }
pub enum Outcome { Success, Fail, Crit, Fumble }
```

### 3.3 `Effect` — 世界突变（唯一原语）

```rust
pub enum Effect {
    ModifyResource { pool: ResourceId, delta: Value, who: Who },
    ApplyStatus    { status: StatusId, duration: Value, who: Who },
    RemoveStatus   { status: StatusId, who: Who },
    DispelAction   { who: Who },   // 取消行动（反制核心，见 D5）
    Interrupt      { who: Who },   // 清空行动槽（不取消威胁来源）
    SpawnAction    { skill: SkillId, who: Who },
    Log            { text: String },
    Contest(Box<Contest>),         // 结算包住效果
    Sequence(Vec<Effect>),
    Conditional { cond: Condition, then: Box<Effect>, else_: Box<Effect> },
}
```

**没有 `Damage`**：伤害 = `Contest(攻 vs 防) → ModifyResource(HP, -x)`。

### 3.4 `Requirement` / `Condition` / `Targeting`

```rust
pub enum Requirement {
    Resource { pool: ResourceId, min: f32 },
    HasThreat, NoActiveAction, OffCooldown,
    InStatus(StatusId), NotInStatus(StatusId),
    TargetAlive, TargetIsEnemy,
    CasterHasTag(ActorTagId),
}
pub enum Condition { Always, ResourceBelow { pool: ResourceId, ratio: f32 }, HasStatus(StatusId), HasThreat }
pub enum Targeting { SelfOnly, ThreatSource, CurrentSelection, NearestEnemy }
pub struct Cost { pub pool: ResourceId, pub amount: f32 }
```

## 4. 实体与组件模型

### 4.0 角色的三条**正交**轴（别把它们混成一个枚举）

角色身份被拆成三个互不相干的问题。混在一起的代价是："亡灵玩家"这种完全正常的组合
会变成需要打补丁的特例。

| 轴 | 类型 | 回答的问题 | 变化频率 |
|---|---|---|---|
| **引擎角色** | `Player` / `Monster` 标记组件 | 谁听输入、谁自己 tick？ | 几乎不变（引擎决定） |
| **阵营** | [`Faction`] 组件（`Player` / `Monster` / `Neutral`） | 谁打谁？ | 内容可配 |
| **特性** | [`ActorTags`]（`ActorTagId`：亡灵 / 野兽……） | 它是什么东西？ | **内容定义**（`vocabulary.ron` 的 `tags`），可同时有多个 |

```rust
// 一个"玩家侧的亡灵"——三条轴各写各的，没有任何冲突：
// 特性是词汇 ID：`vocabulary.ron` 里登记 `undead`，加载期换成 ID。
commands.spawn((Actor, Player, Faction::Player, ActorTags(vec![undead_id])));
```

**判定用哪条轴**：

| 想判断 | 用 | 不要用 |
|---|---|---|
| "这招对敌人有效" | `Requirement::TargetIsEnemy` → 比 `Faction` | ❌ 比 `ActorTags`（特性跟敌意无关） |
| "只有亡灵能学" | `Requirement::CasterHasTag(undead_id)` | ❌ 加一个"亡灵阵营" |
| "这个实体要等玩家输入" | `With<Player>` | ❌ 用 `Faction::Player`（一个由 AI 控制的友方 NPC 也是玩家阵营） |

**升级路径**：`ActorTagId` 现在是"没有数值"的纯标签（选项多寡无所谓，加一个只改 `.ron`）。如果某类判定需要程度、
还带参数（"亡灵抗性 30%"），那它就该变成**带数值的属性**（`Stats` + 一条 `Contest`），
而不是继续往枚举里堆变体。见 [§11 扩展手册](#11-扩展手册)。

### 4.1 定义（全局共享实体）

```rust
pub struct Skill {          // SkillId + 名字 + 图标 + 标签 + 时长 + 需求 + 消耗 + 指向 + 效果
    pub id: SkillId, pub name: String, pub icon: String, pub tags: SkillTags,
    pub duration: f32, pub requirements: Vec<Requirement>, pub costs: Vec<Cost>,
    pub targeting: Targeting, pub effects: Vec<Effect>,
}
pub struct StatusDef {      // StatusId + 名字 + 默认时长 + 叠加 + 修饰符 + 门控 + 四个时机
    pub id: StatusId, pub name: String, pub default_duration: f32, pub stacking: Stacking,
    pub modifiers: Vec<ModifierDef>, pub blocks_tags: SkillTags,
    pub on_apply: Vec<Effect>, pub on_tick: Vec<Effect>,
    pub on_expire: Vec<Effect>, pub on_remove: Vec<Effect>,
}
```

`SkillTags` 是**手写位标志**（不加 `bitflags` 依赖，见 [§7](#7-注册与装配)），语义：

| 标志 | 含义 | 门控用法 |
|---|---|---|
| `ATTACK` / `MOVEMENT` / `SPELL` / `HEAL` | 分类 | 被状态的 `blocks_tags` 拦 |
| `COUNTER` | 反制类 | `update_phase` 判断"有没有反制可用" |
| `GUARD` | 防守类 | 内容分类 |
| `INSTANT` | 瞬发 | 与 `duration == 0` 一致（内容自查） |

### 4.2 实例（运行时）

```rust
pub struct Action { pub elapsed: f32, pub duration: f32, pub target: Option<Entity> }
// 关系：Action ──CastsSkill──► Skill 实体；Action ──InitiatedBy──► Actor
//       Action 上的 ActiveActions(Vec<Entity>) 是反向集；Actor 上的 ActiveActions 是"我的槽"
// 标记：ResolveNow（瞬发，同帧结算）/ ReadyToResolve（到时长，待结算）/ Threat（怪物即将生效）

pub struct ActiveStatus { pub def: Entity, pub remaining: f32, pub stacks: u8,
                          pub source: Entity, pub tick_accumulator: f32 }
// 关系：ActiveStatus ──AttachedTo──► Actor；Actor 上的 Statuses(Vec<Entity>) 是反向集

pub struct AiDecision { pub skill: SkillId, pub skill_entity: Entity, pub weight: f32,
                        pub target: Option<Entity> }
// 关系（**两张一对一**，都是 `Entity` 字段而不是 `Vec<Entity>`）：
//   AiDecision ──DecidedBy──► Actor；Actor 上的 DecisionSlot(Entity) 是反向集
//   AiDecision ──SettledBy──► Action；Action 上的 Settles(Entity) 是反向集
// 两个反向集都带 `linked_spawn`：怪物没了决策跟着没；**行动没了决策也跟着没**
```

## 5. 对抗与判定

```text
Difference: raw = attacker - defender        → 成功 iff raw >= threshold
Ratio:      raw = attacker / (defender + ε)  → 成功 iff raw >= threshold
RollUnder:  raw = attacker                    → 掷骰 [0, attacker)，成功 iff roll < threshold
```

- **`SkillPower`**：`Difference` = `raw`；`Ratio` = `raw`；`RollUnder` = `attacker`。
  成功那一刻把它写进本次结算上下文，效果里的 `SkillPower` 就能拿到"强多少"（D4）。
- **Crit / Fumble**：`Difference` / `Ratio` 下 `raw >= threshold + crit_margin` → `Crit`（`crit_margin = 0` 表示关闭）；
  `RollUnder` 下掷出 `< threshold * 0.05` → `Crit`、`>= attacker` → `Fumble`。
- **唯一随机点**：`RollUnder`。随机源是 [`CombatRng`]（SplitMix64，种子来自 `CombatConfig`），
  同种子 = 同结果 → 整条链路可复现、可测。
- **没有匹配的 `Outcome`** → 该 `Contest` 什么都不做（不静默当成 `Fail`，因为"没写 Fail"和"Fail 无效果"语义不同）。

## 6. `CombatPhase` 状态机（时间控制）

```rust
pub enum CombatPhase { #[default] Resolving, AwaitingInput, AwaitingCounter }
```

```text
PreUpdate   drive_virtual_time    phase == Resolving ? 倍率 1.0 : 0.0
Update
  monster_decide          能量满 → 选招 → 写进决策槽（只决定，不出手）
  apply_status_modifiers  从状态重建 StatModifiers → Stat::refresh（状态数值效果）
  compute_available_skills派生数据：当前可用技能集
  monster_act             决策槽有货 + 窗口空 → 生成行动 + 登记 PendingThreat
  update_phase            Resolving ⇄ AwaitingInput ⇄ AwaitingCounter
  cast_requests           消费 CastRequest → 扣费 / 冷却 / 生成 Action（瞬发打 ResolveNow）
  tick_actions            推进 elapsed（冻结时 delta = 0，等价于暂停）
  resolve_actions         执行 Skill.effects → despawn action
  tick_statuses           倒计时 + on_tick；到期前触发 on_expire / on_remove
PostUpdate  （L2 读 AvailableSkills / ActiveActions / DecisionSlot / CombatLog）
```

**冻结做到"逻辑系统零感知"**：倍率为 0 时 `Res<Time>` 的 `delta` 就是 `0`，
所以 `tick_actions` / `tick_statuses` 不需要任何 `if phase == ...` 分支。

相位判据（`update_phase`）：

| 条件 | 含义 |
|---|---|
| `player_idle` | 存在 `Player` 实体，且它的 `ActiveActions` 为空（或没有该组件） |
| `has_threat` | `PendingThreat.action` 是 `Some`，且该 action 实体仍存在（实体没了 = 威胁已解） |
| `has_counter` | `AvailableSkills` 里至少有一个带 `COUNTER` 标签的技能 |

```text
Resolving:        player_idle → AwaitingInput；has_threat && has_counter → AwaitingCounter
AwaitingInput:    两者都不成立 → Resolving
AwaitingCounter:  两者都不成立 → Resolving
```

### 6.1 决策槽：决定与执行是两个时刻（**一对一关系**）

怪物攒满能量之后做两件事：**决定**放哪一招，然后**出手**。这两件事被拆成两个系统，
中间隔着一个"决策槽"——一个怪物身上**同时只有一条**决策，所以它是一对一关系：

```text
monster_decide ──写──► DecisionSlot ──读──► monster_act ──► PendingThreat
```

| 组件 | 在哪 | 关系 |
|---|---|---|
| `AiDecision` | 决策实体（单独 spawn） | 决策本身：技能词汇 ID + 技能实体 + 权重 + 锁定目标 |
| `DecidedBy(Entity)` | 决策实体 | 一对一的关系源 |
| `DecisionSlot(Entity)` | 怪物 | 一对一的关系目标（**字段是单个 `Entity`**），`linked_spawn` |
| `SettledBy(Entity)` | 决策实体 | 决策 → 它落成的行动 |
| `Settles(Entity)` | 行动 | 一对一的目标，`linked_spawn`：**行动没了，决策跟着没** |

**为什么要拆**：挤在一个 `if` 里时，"决定"没有落脚点。

* **决定会被丢弃重掷**：威胁窗口只有一格，被别的怪物占着时只能下一帧重新决定一遍；
  而条件是随时会变的（残血 / 中毒 / 换目标），那实际上是"每帧重掷、留下最后一次"。
  拆开之后决定**留在槽里等窗口**，条件再变也不改主意。
* **决定不可观察**：行动还没生成时，世界上没有任何东西能回答"这只怪打算干什么"。

**代价（不是零成本，得知道）**：

* 费用与冷却在**决定那一刻**付，不是出手那一刻。理由：决定一旦写下就成立，不该因为
  "等窗口"的这几帧里池子变了而变成付不起——付不起的决策会永远卡在槽里。
* `monster_decide` 到 `monster_act` 之间**多一个 `ApplyDeferred`**：决策实体是 `Commands`
  spawn 的，不落一次同步点，本帧的执行就看不到它（见 §7.1 那条"延迟恰好一帧"）。

**两个坑都来自"一对一"本身的语义**（都不报错，只是沉默）：

1. **槽的"空"不是"长度为 0"，是没有 `DecisionSlot` 组件。** 0.19 的 `RelationshipTarget`
   不支持 `Option<Entity>`，所以"没决定"只能表达为拆掉关系。拆掉之后组件本身会被清掉，
   但那次清理是**排队命令**——同一帧里槽可能还在、只是集合空了。所以读槽一律走
   `decision_of()`（内部是 `iter()`），不去直接读字段。
2. **"换一条决策"不会销毁旧决策实体。** 一对一关系在新源顶掉旧源时只做**解绑**：旧决策上的
   `DecidedBy` 被自动移除，实体却留在世界里。所以 `monster_decide` **不给它顶掉的机会**——
   只在槽空时写新决策。谁哪天删掉那个 `slot.is_some() → continue`，旧决策就会静静堆在
   世界里，测试 `a_waiting_monster_does_not_rewrite_its_decision_every_frame` 专门钉这条。

**槽的三条清空路径**（都是关系级联，没有"每帧反推谁该退役"）：

| 路径 | 机制 |
|---|---|
| 行动正常结算 | `resolve_actions` despawn 行动 → `Settles` 级联 despawn 决策 |
| 行动被反制取消 | 同上（`cancel_actions` 也只是 despawn 行动） |
| 怪物自己没了 | `DecisionSlot` 级联 despawn 决策 |

**为什么不能用"反推"**：`Effect::DispelAction` 把 `PendingThreat` **整个**清空（连 `source`
都不留），反推根本无从下手——决策会永久卡在槽里，于是怪物再也不出手，而且什么都不报。
挂到行动上就完全不需要反推。

## 7. 反制机制

**反制 = 带 `COUNTER` 标签、`HasThreat` 需求、`DispelAction` 效果的普通技能。没有反制槽。**

```text
1. monster_decide         → 选招 → 写进决策槽（DecisionSlot，一对一）
2. monster_act            → SpawnAction(goblin_slash, who: Caster) → 怪物 action + PendingThreat
3. update_phase           → has_threat && has_counter → AwaitingCounter（虚拟时间冻结）
4. L2 presentation        → 读 AvailableSkills，显示"反击"
5. 玩家 CastRequest       → { caster: pc, skill: riposte, target: threat.source }
6. cast_requests          → 扣 reaction → spawn Action{duration: 0} + CastsSkill + InitiatedBy + ResolveNow
7. resolve_actions（同帧）→ Contest(反应 vs 攻击力) → Success
                            → DispelAction(who: Target) → 解析为 PendingThreat.action → despawn
                            → 嵌套 Contest → ModifyResource(HP, -20) 给怪物
8. despawn 掉 riposte 自己的 action → ActiveActions 自动清空
                            → **怪物的决策也跟着它的 action 一起没了**（`Settles` + `linked_spawn`）
9. update_phase           → 威胁实体已不存在 → 回 Resolving（恢复流动）
```

**关键点**：`duration = 0` 的 action 在同一帧内 spawn → 结算 → despawn，**从未真正占槽**；
它的合法性完全由 `requirements` 保证。因为 `cast_requests → tick_actions → resolve_actions`
是显式串行链、且链中间落了两处 `ApplyDeferred`，Commands 会在链条的同步点被应用，
所以"同帧结算"真的成立。

### 7.1 冻结的**生效延迟恰好一帧**（实现契约）

"逻辑系统完全不感知相位、只读 `delta`"这个优雅性质有一个代价，必须写下来，
否则很容易踩成"怪物多走一步"：

```text
First          time_system            用当前倍率把本帧 delta 定死
               drive_virtual_time     读 State → 改倍率（改的是下一帧的）
PreUpdate      StateTransition        把上一帧写下的 NextState 落成 State
Update         monster_tick / …       读本帧 delta 推进
               update_phase           算出下一帧的相位（写 NextState）
```

Bevy 在**帧首**就把 `delta` 定死了，而相位要到 `PreUpdate` 才落地，所以：

- 倍率**必须**在 `First` 里改，且必须排在 `time_system` 之后（`bevy_time::time_system` 是公开系统，
  可以用 `.after()` 排序）；
- `update_phase` 那一帧**还没冻结**，**次帧**才冻结；
- 要钉的性质是"任意时刻 `delta != 0` ⟺ `State` 是 `Resolving`"，而不是"相位一变当帧就停"。

**反例**（曾经的写法，真的出过 bug）：把 `drive_virtual_time` 放在 `PreUpdate` 且不排序。
它跑在 `time_system` 之后，倍率改完要等**两帧**才影响 `delta`——
于是 `AwaitingInput` 期间怪物多走一步、中毒多跳一次伤害。
`time_scale` 的 `delta_and_phase_never_disagree` 测试专门钉住这条。

### 7.2 三个踩过的测试陷阱（写新测试前先看这里）

1. **`Time<Real>` 的第一次更新不产生 `delta`**。`real.rs` 里 `last_update` 为 `None` 时直接
   `return`，所以裸 `App` 的**首帧**只记起始时刻。测试脚手架必须先"点火"一帧
   （`support::test_app` 里的 `prime_clock`），否则每个测试的第一次 `step()`
   都拿到 `delta = 0`，"推进时间"悄悄失效、测试却看起来在用时间。
2. **`Time<Virtual>` 默认 `max_delta` 是 250ms**。测试里给一帧 1.5 秒会被夹成 0.25 秒，
   所以 `step()` 先把上限放宽再推进；线上由 `VirtualTimeConfig` 管。
3. **`Commands` 延迟应用**：`spawn` / 插入关系组件（`AttachedTo`）在**下一帧**才可见。
   测试里"造好实体 → 立刻断言系统读到了它"必须多走一帧，否则测的是"空世界"。

`Time<Virtual>` 的 `max_delta` 与本节的顺序契约合起来解释了为什么"改时间"这件事
在测试里比在线上更容易出错：**三个量（real delta / 倍率 / clamp）各自都可能吞掉一次推进**，
而它们都不会报错。

## 8. 配置格式（RON）

**六份**文件都在 `assets/data/`（工作区根）。它们是**真正的 `Asset`**：

```text
ContentManifest（SceneComponent，路径全写在 ContentManifest::scene() 里）
  vocabulary.ron ─► VocabularyAsset ─┐
  skills.ron     ─► SkillsAsset      │
  statuses.ron   ─► StatusesAsset    │  PreStartup：阻塞等到六份全部就绪
  players.ron    ─► PlayersAsset     │  （失败 / 超时立刻 panic，带文件名）
  monsters.ron   ─► MonstersAsset    │
  world.ron      ─► WorldAsset      ─┘
                                    ↓ 拼成 RawContent，insert_resource
                               Startup 照旧（图集 / 地形 / HUD 一行没改）
```

| 文件 | 内容 | 解析结果 |
|---|---|---|
| `vocabulary.ron` | 资源池 / 属性 / 状态 / **特性**的名字与默认值 | `Vocab`（字符串 → 词汇 ID） |
| `skills.ron` | 技能定义 | `SkillCatalog`（SkillId → `Skill` 实体） |
| `statuses.ron` | 状态定义 | `StatusCatalog`（StatusId → `StatusDef` 实体） |
| `players.ron` | **玩家**的池 / 属性 / 阵营 / 特性 | `ActorTemplate`（L2 → `spawn_actor`） |
| `monsters.ron` | **怪物**的同名字段 + AI 权重 + 能量 | 同上 |
| `world.ron` | 体素地形参数 + 方块表 | `WorldTemplate`（图集 / 地形读） |

`players.ron` 与 `monsters.ron` 用**同一个** `ActorRon` 结构，靠必填的 `role: Player | Monster`
分成两组：玩家与怪物都是"池 + 属性 + 阵营 + 特性"，没必要为 PC 单开一份结构。
**`role` 漏写会直接解析失败**（不给默认值）——曾经默认成 `Player`，
于是一个只写了 AI 候选的怪物被静默当成 PC 生成，"打不到怪"却不报错。

**路径只有一处**：`ContentManifest::scene()`。`bsn!` 把 `"data/skills.ron"` 自动转成
`HandleTemplate::Path`（装载时落到 `AssetServer::load`），所以调用方只写
`world.spawn_scene(bsn! { @ContentManifest })`，不需要知道任何一个文件名——
加一份配置只改那一个函数。

### 8.1 为什么在 `PreStartup` **阻塞**等装载

`Startup` 里一串系统（图集 / 材质 / 地形 / 精灵表 / HUD）都假设"内容已经在"，
而它们的参数是 `Option<Res<ContentData>>`——**读不到就静默建一张空图集**，战场空掉
却不报错。与其把那一串系统全改成异步状态机，不如守住
"`Startup` 一定发生在内容就绪之后"这条不变式：`PreStartup` 里泵
`handle_internal_asset_events` 直到六份都 `LoadState::Loaded`。

阻塞是安全的：装载跑在 `IoTaskPool`（独立线程）上，两边不会互相等。六份本地小文件
在毫秒级完成；有 10 秒超时兜底，`LoadState::Failed` 会立刻带着文件名 panic。

### 8.2 热重载（改 `.ron` 不用重编）

`bevy/file_watcher` 重新装载 → `AssetEvent::Modified` → 重新翻译。两条不变式：

1. **同 ID 的定义实体原地更新，不换实体**。`Action` 用 `CastsSkill(Entity)` 指着技能定义
   实体，`ActiveStatus` 用 `def` 指着状态定义实体——换实体就是悬空引用，而悬空引用
   **不报错**，只是那一招 / 那个状态从此画不出来。
2. **词汇表拿旧的当底再登记**。词汇 ID 是按登记顺序发的，不垫底的话"在 `skills.ron`
   中间插一条"会让它后面所有技能的 ID 整体后移一位——等于把 A 的定义悄悄换成 B 的。

改错了（语法错 / 引用了没登记的池）**不 panic**：打一条 error 保留旧目录。启动期才该当场炸，
运行期炸掉用户正在玩的局毫无意义。

**不做的事**：不重新生成角色、不重建地形。那两件事是场景编排不是内容——重跑会把 PC 和
怪物再 spawn 一份、把地形网格再叠一层。想从头来一遍，重启。

加载管线（`axiom::behaviors::content::loader`，**不读文件、不认 `AssetServer`**）：

```text
vocabulary.ron ─► Vocab（Resource：name ⇄ id 双向）
statuses.ron   ─► 解析字符串 → StatusDef   ─► spawn 实体 ─► StatusCatalog（name → Entity）
skills.ron     ─► 解析字符串 → Skill       ─► spawn 实体 ─► SkillCatalog（name → Entity）
players.ron ┐
monsters.ron┘  ─► 解析字符串 → ActorTemplate ─► 按 `role` 分到 players / monsters 两组
```

热重载走的是**同一个** `load_all_reusing`，多带一份 `ReusedDefs`（旧词汇表 + 旧目录）。
`build_vocab_into` 往旧表里追加：已登记的名字保留原 ID，新名字接在后面。

**加载期检查**（都不允许静默跑）：

| 检查 | 报错 |
|---|---|
| 技能 / 状态引用的资源、属性、状态名必须存在 | `UnknownName` |
| 词汇表里声明了、但没人引用的池 | `UnusedResource` |
| 角色 AI 引用了不存在的技能 | `UnknownSkill` |
| 两个角色用了同一个 `id` | `DuplicateActor` |
| `role: Monster` 却没有 AI 候选（生成出来是靶子） | `MonsterWithoutAi` |
| 角色漏写 `role` | RON 解析失败（`role` 必填） |

**字符串永远不进热路径**：解析一次映射成 `u16` 词汇 ID，运行时只跑 ID 版本。

## 9. 模块落点

```text
crates/voxelith-axiom/src/
  atoms/actor.rs              Resources / Pool / Stats / StatModifier / Cooldowns / ActorState /
                              ActionEnergy / Player / Monster / Faction / ActorTags + ActorPlugin
  behaviors/content/vocabulary.rs  词汇 ID 类型 + Vocab + name⇄id 双向表
  behaviors/content/descriptor.rs  RON 描述结构（VocabRon / SkillRon / StatusRon / ActorRon / RoleRon）
  behaviors/content/catalog.rs     SkillCatalog / StatusCatalog（Resource）
  behaviors/content/loader/        加载管线（**不读文件**）
    mod.rs                          流程：load_all / load_all_reusing / build_vocab_into / 池覆盖检查
    resolve.rs                      值 / 效果 / 需求 / 对抗的字符串 → ID
    resolve_defs.rs                 技能 / 状态 / 角色模板
    types.rs                        LoaderError / PoolTemplate / ActorTemplate / LoadedContent / ReusedDefs
  behaviors/value.rs          Value / Who / Provider / eval / EvalContext
  behaviors/contest.rs        Formula / Outcome / Contest / resolve_contest / CombatRng
  behaviors/effect/           Effect / execute_effect / EffectContext + apply.rs / blob.rs / params.rs
  behaviors/requirement.rs    Requirement / Condition / Cost / Targeting / CasterContext /
                              skill_available / requirement_ok / status_snapshot
  behaviors/targeting.rs      resolve_target / hostile_to / faction_of
  behaviors/skill.rs          Skill / SkillId / SkillTags（手写位标志）
  behaviors/status.rs         StatusDef / Stacking / ModifierDef / ActiveStatus / AttachedTo /
                              施加 / 生命周期 / 派生修饰符
  behaviors/action/           Action 实例 / 关系组件 / CastRequest / 推进 / 结算（casting.rs 放释放校验）
  behaviors/phase.rs          CombatPhase / PendingThreat / AvailableSkills / CombatLog / update_phase
  behaviors/time_scale.rs     VirtualTimeConfig / drive_virtual_time / TimeControlPlugin
  behaviors/monster/         怪物 AI（**决定与执行拆开，中间是决策槽**）
    mod.rs                     AiChoice / MonsterDef / Threat / choose_skill / condition_holds
    decision.rs                AiDecision / DecidedBy + DecisionSlot（一对一）/
                               SettledBy + Settles / monster_decide（写槽）/ monster_act（出手）
  behaviors/combat.rs         CombatConfig（种子）/ CombatPlugin（**独占**跨域系统顺序）
  behaviors/mod.rs            L1 领域地图

crates/voxelith-prime/src/
  content/loader.rs           include_str! 五份 RON → Deserialize → axiom loader → insert_resource
  content/spawn.rs            ActorTemplate → 实体（玩家 / 怪物**同一条**路径）
  content/mod.rs              ContentPlugin
  presentation/mod.rs         只读 phase / AvailableSkills / ActiveActions / CombatLog
  voxelith.rs                 VoxelithPlugin（装配 L0 + L1 + L2）
  debug.rs                    BRP + egui 检查器（只读，R15）
```

## 10. 事件与消息边界

| 名称 | 机制 | 定义位置 | 说明 |
|---|---|---|---|
| `CastRequest` | `Message`（请求型） | `behaviors::action` | L2 输入 / AI → L1：请求释放技能（带 caster / skill / target） |
| `DetachStatusMessage` | `Message` | `behaviors::status` | 状态到期 / 被摘；由 `tick_statuses` 或净化发出，结算系统消费 |
| `StatusTickMessage` | `Message` | `behaviors::status` | 状态的每个结算周期；把"倒计时"与"跑效果"拆开 |
| `PurgeStatusMessage` | `Message` | `behaviors::status` | 净化请求（按状态 ID / 按来源） |
| `CombatLog` | **Resource**（追加 + 每帧读） | `behaviors::phase` | 战斗日志文案，L2 只读。效果执行器拿不到 `MessageWriter`，用 Resource 反而降低耦合 |
| `AvailableSkills` / `ActiveActions` / `CombatPhase` | **Resource / 关系组件** | `behaviors::phase` / `action` | 派生数据，L2 只读 |
| `DecisionSlot`（+ `AiDecision`） | **关系组件**（一对一，怪物 → 决策实体） | `behaviors::monster::decision` | "这只怪已经决定了什么、还没出手"。L2 只读 `decision_of(slot)` |
| `DeathEvent` | `EntityEvent` | **待定** | 资源池见底目前没有专门通知；要不要做"归零即事件"见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q27 |

L2 不定义任何战斗数值，只发 `CastRequest`、只读 `AvailableSkills` / `ActiveActions` / `CombatPhase` / `CombatLog`。

## 11. 扩展手册

| 想做什么 | 改哪里 | 需要改代码吗 |
|---|---|---|
| 加一个新技能 | `skills.ron` 加一条 | ❌ |
| 加一种新资源 | `vocabulary.ron` 加一行 | ❌ |
| 加一种新状态 | `statuses.ron` 加一条 | ❌ |
| 加一个新怪物 | `monsters.ron` 加一条（含 `role` / `faction` / `traits`） | ❌ |
| **改玩家的池 / 属性 / 阵营 / 特性** | `players.ron` | ❌ |
| **把玩家换成 AI 控制的友军** | `players.ron` 里该条改 `role: Monster` + 补 `ai` | ❌ |
| 改伤害公式 | 改技能的 `Contest`（`attacker` / `defender` / `threshold`） | ❌ |
| 给角色换个阵营 / 加个特性 | 改对应 `.ron` 的 `faction` / `traits` | ❌ |
| 加一种新**特性**（种族 / 类型） | `vocabulary.ron` 的 `tags` 加一行 + 在 `traits` 里用上 | ❌ |
| 加一种新**阵营** | `Faction` 加变体 + 在 `hostile_to` 里表态 | ✅（罕见） |
| 加一种新对抗方式 | `Formula` 加变体 + `resolve_contest` 分支 | ✅（罕见） |
| 加一种新原子效果 | `Effect` 加变体 + `execute_effect` 分支 | ✅（罕见） |
| 加一种新表达式运算 | `Value` 加变体 + `eval` 分支 | ✅（罕见） |
| 加一种新需求 / 条件 | `Requirement` / `Condition` 加变体 + `requirement_ok` / `eval_condition` 分支 | ✅（受控） |

**判断标准**：这个集合会随游戏**内容**增长吗？会 → RON；不会（是引擎能力）→ Rust。

**别把"属性"做成"标签"**：`ActorTagId` 只适合"没有数值、纯粹用来分支"的性状。
一旦某个性状需要程度 / 抗性 / 成长（"亡灵抗性 30%"），就把它做成 `Stats` 里的一个属性，
让 `Contest` 去比较——枚举变体堆不出数值。

## 12. 依赖与守卫

| crate | 依赖 | 为什么 |
|---|---|---|
| `axiom` | `bevy_app` / `bevy_ecs` / `bevy_reflect` / `bevy_time` | R5 白名单 |
| `axiom` | `serde`（**仅 `derive`，无 `std` 特性开关需求**） | RON 描述结构要 `Deserialize`；引擎不读文件，只用派生 |
| `axiom` | `exn` / `derive_more` | 已登记 |
| `prime` | 完整 `bevy` + `voxelith-axiom` + `ron` | L2 才是"读文件那层"（R101 精神） |

`bitflags` **不引入**：`SkillTags` / `StatusTags` 是 `u32` newtype + 手写 `contains` / `intersects` / `insert`
（不到 60 行，且 `SkillTags` 参与 RON 解析时需要自定义字符串列表解析，本来也绕不开手写）。

`serde` 已登记进 [architecture.md](architecture.md) 的非 bevy 依赖登记表；守卫脚本的 `$registered` 同步加入。

## 13. 与旧方案的差异（迁移记录）

| 旧（combat-mechanics.md） | 新（本文） | 原因 |
|---|---|---|
| `DamageRequest` 四阶段管线（Base → AttackerBonus → DefenderMitigation → Finalize） | `Contest` 一件事包住 | 规则顺序靠注册顺序，组合爆炸；对抗是一等公民 |
| `Resistance` 组件 + `mitigate` 数值减伤 | 抵抗 = `TargetStat(armor)` 参与 `Contest` | 减伤从"阶段"变成"对抗的一侧"，可配置 |
| `behaviors::rolls` 独立的命中 / 暴击判定 | `Formula::RollUnder` + `CombatRng` | 随机收进对抗原语，只有一个随机点 |
| `StatusInstance` + `ChildOf` + `BehaviorId`（内容层注册回调） | `ActiveStatus` + `AttachedTo` + `Effect` 列表 | 状态的行为就是数据，不需要内容层注册 Rust 函数 |
| `Stat`（显式字段 + 穷尽 `match`） | `Stat`（`HashMap` + 词汇 ID） | 加属性不改代码（设计原则 10） |
| `Health`（L0 纯数值执行器，i32） | `Resources` 的 `hp` 池（f32，内容可配 max / regen） | 统一"池"概念；R52 的自然恢复就是 `regen` |
| 阵营与种族混在一个 `ActorTag`（`Player` / `Monster` / `Undead` 并列） | **三条正交轴**：引擎角色标记 / `Faction` / `ActorTags`（特性） | "玩家也可能是亡灵"不该是特例，见 §4.0 |
| 里程碑 M5/M6 骨架 + 73 个测试 | 本系统 + 重写后的测试集 | 整体替换（已拍板） |

## 14. 自检清单

- [ ] 新增内容（技能 / 状态 / 资源 / 怪物）是否只改 `.ron`？
- [ ] `Effect` 是唯一的世界突变原语吗？技能与状态是否共用它？
- [ ] 伤害是不是"对抗成功后改资源"？有没有人偷偷加回 `Damage` 变体？
- [ ] `ActiveActions.len() <= 1` 是唯一槽位约束吗？有没有冒出"反制槽 / 技能种类"枚举？
- [ ] **决策槽是一对一吗？**（字段是不是单个 `Entity`）读槽是不是走的 `decision_of()`、
      `linked_spawn` 两条级联是不是都在？
- [ ] 逻辑系统是否只读 `Res<Time>`、完全不感知冻结？
- [ ] 字符串有没有进热路径？（只在 loader 里出现）
- [ ] **阵营判定用的是 `Faction`、特性判定用的是 `ActorTags`，两者没有互相顶替？**
      （"亡灵"不该出现在 `Faction` 里，"敌人"不该靠 `ActorTags` 判断）
- [ ] 单文件是否 < 500 行（R26）？
