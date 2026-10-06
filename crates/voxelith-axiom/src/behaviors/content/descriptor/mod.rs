//! RON 描述结构：**内容文件的样子**（[docs/combat-design.md](../../../../../../docs/combat-design.md) §8）。
//!
//! 它们只是数据：字符串一律保持字符串，由 [`super::loader`] 负责解析成词汇 ID。
//! 这样"文件格式"与"运行时数据"是两个概念，改了文件格式不会牵动引擎。
//!
//! 词汇表那一份（`vocabulary.ron`）在 [`vocabulary`]，因为它和技能 / 状态的形状差得远。

use std::collections::HashMap;

use crate::atoms::actor::Faction;
use crate::behaviors::contest::Formula;
use crate::behaviors::skill::SkillTag;

mod vocabulary;

pub use vocabulary::{ResourceRon, StatRon, StatusDefRon, TagRon, VocabRon};
/// 一个技能定义。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SkillRon {
    /// 技能名（词汇表的 `skills` 表按出现顺序编号）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 图标名。
    #[serde(default)]
    pub icon: String,
    /// 标签列表：`[ATTACK, COUNTER]`。
    #[serde(default)]
    pub tags: Vec<SkillTag>,
    /// **谁会这一招**：空 = 谁都能用；否则只有这些角色的 `role` 能用。
    ///
    /// 没有它的话"可用技能"只能按需求 / 消耗筛，于是**哥布林的招式会出现在玩家的技能栏里**
    /// （双方都满足"有行动点、没有正在进行的行动"）。见 `docs/combat-design.md` §3.4。
    #[serde(default)]
    pub roles: Vec<RoleRon>,
    /// 释放时长（秒）：`0.0` = 瞬发。
    #[serde(default)]
    pub duration: f32,
    /// 后摇时长（秒，可省）：释放点之后仍然占着槽的时间。默认 `0.0` = 没有后摇。
    #[serde(default)]
    pub recovery: f32,
    /// 前置需求。
    #[serde(default)]
    pub requirements: Vec<RequirementRon>,
    /// 消耗。
    #[serde(default)]
    pub costs: Vec<CostRon>,
    /// 目标解析。
    pub targeting: TargetingRon,
    /// 效果列表。
    #[serde(default)]
    pub effects: Vec<EffectRon>,
}

/// 技能需求（RON 形态：池 / 状态都写成名字）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub enum RequirementRon {
    /// 某个池至少有 `min`。
    Resource {
        /// 池名。
        pool: String,
        /// 至少多少。
        min: f32,
    },
    /// 当前存在威胁。
    HasThreat,
    /// 自己没有正在进行的行动。
    NoActiveAction,
    /// 自己身上有该状态。
    InStatus(String),
    /// 自己身上没有该状态。
    NotInStatus(String),
    /// 技能不在冷却中。
    OffCooldown,
    /// 目标还活着。
    TargetAlive,
    /// 目标是敌人。
    TargetIsEnemy,
    /// 自己带某个标签。
    CasterHasTag(String),
}

/// 技能消耗（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CostRon {
    /// 池名。
    pub pool: String,
    /// 消耗多少。
    pub amount: f32,
}

/// 目标解析（RON 形态，与运行时 [`Targeting`] 同名同形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum TargetingRon {
    /// 只作用自己。
    SelfOnly,
    /// 作用威胁来源。
    ThreatSource,
    /// 用请求里带的目标。
    CurrentSelection,
    /// 最近的敌人。
    NearestEnemy,
}

/// 效果（RON 形态：技能 / 状态 / 属性 / 池都写成名字）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub enum EffectRon {
    /// 改资源池。
    ModifyResource {
        /// 池名。
        pool: String,
        /// 增减量表达式。
        delta: ValueRon,
        /// 哪一方。
        who: WhoRon,
    },
    /// 施加状态。
    ApplyStatus {
        /// 状态名。
        status: String,
        /// 时长表达式。
        duration: ValueRon,
        /// 给谁。
        who: WhoRon,
    },
    /// 移除状态。
    RemoveStatus {
        /// 状态名。
        status: String,
        /// 从谁身上移除。
        who: WhoRon,
    },
    /// 取消行动。
    DispelAction {
        /// 取消谁的行动。
        who: WhoRon,
    },
    /// 打断。
    Interrupt {
        /// 打断谁。
        who: WhoRon,
    },
    /// 让某一方释放技能。
    SpawnAction {
        /// 技能名。
        skill: String,
        /// 谁释放。
        who: WhoRon,
    },
    /// 写日志。
    Log {
        /// 文案。
        text: String,
    },
    /// 对抗。
    Contest(Box<ContestRon>),
    /// 顺序执行。
    Sequence(Vec<EffectRon>),
    /// 条件分支。
    Conditional {
        /// 判据。
        cond: ConditionRon,
        /// 成立时。
        then: Box<EffectRon>,
        /// 不成立时。
        else_: Box<EffectRon>,
    },
}

/// 对抗（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ContestRon {
    /// 攻击侧表达式。
    pub attacker: ValueRon,
    /// 防御侧表达式。
    pub defender: ValueRon,
    /// 判定方式。
    pub formula: Formula,
    /// 阈值。
    pub threshold: f32,
    /// 大成功余量。
    #[serde(default)]
    pub crit_margin: f32,
    /// 结果 → 效果。
    #[serde(default)]
    pub outcomes: Vec<(OutcomeRon, Vec<EffectRon>)>,
}

/// 对抗结果（RON 形态，与运行时同名同形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum OutcomeRon {
    /// 成功。
    Success,
    /// 失败。
    Fail,
    /// 大成功。
    Crit,
    /// 大失败。
    Fumble,
}

/// 表达式（RON 形态：属性 / 池写成名字）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub enum ValueRon {
    /// 字面量。
    Literal(f32),
    /// 本次结算的强度。
    SkillPower,
    /// 施法者属性。
    CasterStat(String),
    /// 目标属性。
    TargetStat(String),
    /// 某一方的池。
    Resource {
        /// 池名。
        pool: String,
        /// 哪一方。
        who: WhoRon,
    },
    /// 求和。
    Sum(Vec<ValueRon>),
    /// 求积。
    Mul(Box<ValueRon>, Box<ValueRon>),
    /// 取负。
    Neg(Box<ValueRon>),
    /// **伸缩曲线**：把输入映射到一条经过设计的成长曲线上。
    ///
    /// 设计取自 ToME4 的 `combatTalentScale` / `combatTalentLimit`
    /// （只取机制），见 [`crate::behaviors::value::Scale`]。
    ///
    /// 例子——"伤害随技能等级从 20 涨到 80，前期快后期慢，且不超过 100"：
    ///
    /// ```ron
    /// Scale(
    ///     input: CasterStat("level"),
    ///     low: 20.0,
    ///     high: 80.0,
    ///     curve: Limit(100.0),
    /// ),
    /// ```
    Scale(ScaleRon),
}

/// [`ValueRon::Scale`]：伸缩曲线的 RON 形态。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ScaleRon {
    /// 输入（通常是等级）。
    pub input: Box<ValueRon>,
    /// 输入为 `1` 时的目标值。
    pub low: f32,
    /// 输入为 `5` 时的目标值。
    pub high: f32,
    /// 曲线形状。
    #[serde(default)]
    pub curve: CurveRon,
    /// 锚点：`Talent`（1→5，**默认**）或 `Stat`（10→100）。
    ///
    /// **必须与 `input` 配对**：喂等级用 `Talent`，喂属性用 `Stat`。
    /// 混用会让曲线几乎没有增长（见 `Scale` 的文档）。
    #[serde(default)]
    pub anchors: AnchorsRon,
    /// 输入上的偏移。
    #[serde(default)]
    pub shift: f32,
    /// 结果上的偏移。
    #[serde(default)]
    pub add: f32,
}

/// 曲线的锚点对（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum AnchorsRon {
    /// 等级 / 技能点：`1..5`。
    Talent,
    /// 属性值：`10..100`。
    Stat,
}

impl Default for AnchorsRon {
    fn default() -> Self {
        Self::Talent
    }
}

/// 曲线形状（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub enum CurveRon {
    /// `x^power`：`power < 1` 前期快、后期慢（**默认**，最常用）。
    Power(f32),
    /// `log10(x)`：极早熟。
    Log,
    /// 以该值为渐近上限。
    Limit(f32),
}

impl Default for CurveRon {
    fn default() -> Self {
        // 默认 `0.5`：ToME4 的默认值，也是"减益成长"最常见的形状。
        Self::Power(0.5)
    }
}

/// 表达式里的"谁"（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum WhoRon {
    /// 施法者。
    Caster,
    /// 目标。
    Target,
}

/// 效果内部条件（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub enum ConditionRon {
    /// 恒成立。
    Always,
    /// 池占比低于阈值。
    ResourceBelow {
        /// 池名。
        pool: String,
        /// 占比阈值。
        ratio: f32,
    },
    /// 目标有该状态。
    HasStatus(String),
    /// 当前存在威胁。
    HasThreat,
}

/// 一个状态定义。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct StatusRon {
    /// 状态名。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 默认时长（秒）。
    #[serde(default = "default_duration")]
    pub default_duration: f32,
    /// 叠加规则。
    #[serde(default)]
    pub stacking: StackingRon,
    /// 派生修饰符。
    #[serde(default)]
    pub modifiers: Vec<ModifierRon>,
    /// 封锁的技能标签。
    #[serde(default)]
    pub blocks_tags: Vec<SkillTag>,
    /// 施加时。
    #[serde(default)]
    pub on_apply: Vec<EffectRon>,
    /// 每个结算周期。
    #[serde(default)]
    pub on_tick: Vec<EffectRon>,
    /// 自然到期。
    #[serde(default)]
    pub on_expire: Vec<EffectRon>,
    /// 被主动移除。
    #[serde(default)]
    pub on_remove: Vec<EffectRon>,
}

/// 叠加规则（RON 形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
pub enum StackingRon {
    /// 刷新时长。
    #[default]
    Refresh,
    /// 叠层。
    Stack {
        /// 上限。
        max: u8,
    },
    /// 忽略重复施加。
    Ignore,
    /// 替换旧实例。
    Replace,
}

/// 修饰符定义（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ModifierRon {
    /// 属性名。
    pub stat: String,
    /// 增减量表达式。
    pub delta: ValueRon,
}

/// 引擎角色（RON 形态）。
///
/// 直接复用 L0 的 [`ActorRole`](crate::atoms::actor::ActorRole)，**不再单独定义同形枚举**：
/// 加载期解析出来的值要原样进 `Skill::roles` / `ActorTemplate`，两份类型只会多一次翻译
/// （而且"内容写 `Player`、判定比 `RoleRon::Player`"会比不出结果）。
///
/// 默认值是 `Monster`，但 [`ActorRon::role`] 上挂了"漏写就 panic"的 `serde` 默认工厂：
/// 曾经"默认成玩家"导致只写了 AI 的怪物被静默当成 PC 生成，"打不到怪"却不报错。
/// 角色归属是**必须表态**的信息。
pub use crate::atoms::actor::ActorRole as RoleRon;

/// 一个角色模板（**玩家与怪物共用一套字段**）。
///
/// 玩家也是"池 + 属性 + 阵营 + 特性"，所以没必要为它单开一份结构：
/// 差别只有 [`RoleRon`]（听输入 / 自己 tick）与 `ai` / `energy_*`（只有怪物用）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct ActorRon {
    /// 关键名（L2 生成实体的标识；词汇表的 `skills` 之外独立编号）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 引擎角色（**必填**，见 [`RoleRon`]）。
    #[serde(default = "missing_role")]
    pub role: RoleRon,
    /// 初始池：`[("hp", 30), ("action", 1)]`。
    pub resources: Vec<(String, f32)>,
    /// 初始属性：`[("strength", 4)]`。
    pub stats: Vec<(String, f32)>,
    /// AI 候选（只有 `role: Monster` 用得上）。
    pub ai: Vec<AiChoiceRon>,
    /// 能量速率（同上）。
    pub energy_rate: f32,
    /// 能量阈值（同上）。
    pub energy_threshold: f32,
    /// 阵营（**内容可配**：可以是玩家侧、怪物侧或中立）。
    pub faction: Faction,
    /// **特性**标签（亡灵 / 构装体 / 野兽……与阵营无关，可同时有多个）。
    pub traits: Vec<String>,
}

/// `role` 漏写时的落点：**故意 panic**，把"必须表态"变成启动期的明确失败。
///
/// 它是 `serde` 的 `default` 工厂，所以只在字段缺席时被调用——不会影响正常解析。
fn missing_role() -> RoleRon {
    panic!(
        "actor is missing `role`; write `role: Player` or `role: Monster` explicitly \
         (a silent default here once turned a monster into a player)"
    )
}

impl Default for ActorRon {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            role: RoleRon::Monster,
            resources: Vec::new(),
            stats: Vec::new(),
            ai: Vec::new(),
            energy_rate: 1.0,
            energy_threshold: 3.0,
            faction: Faction::Monster,
            traits: Vec::new(),
        }
    }
}

/// AI 候选（RON 形态）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct AiChoiceRon {
    /// 技能名。
    pub skill: String,
    /// 条件。
    #[serde(default = "default_always")]
    pub when: ConditionRon,
    /// 权重。
    #[serde(default = "default_weight")]
    pub weight: f32,
}

fn default_duration() -> f32 {
    5.0
}

fn default_weight() -> f32 {
    1.0
}

fn default_always() -> ConditionRon {
    ConditionRon::Always
}

/// 便捷别名：状态定义表按名字取（加载期用）。
pub type StatusRonTable = HashMap<String, StatusRon>;

/// 便捷别名：角色表按名字取（玩家与怪物共用）。
pub type ActorRonTable = HashMap<String, ActorRon>;
