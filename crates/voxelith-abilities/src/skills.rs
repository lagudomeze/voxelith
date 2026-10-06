//! 技能（能力）定义的**内容管线**：`.ron` → 规则参数，错误全在加载期报出来。
//!
//! 这是旧 `assets/data/skills.ron` 的继任者。旧文件把技能写成"内联效果树"；
//! 新文件只写**规则与数值**，行为由三件套搭出来：
//!
//! ```text
//! abilities.ron   规则与数值：前摇 / 冷却 / 命中判据 / 引用哪些属性   ← 本模块
//! Rust 构造器     按这些参数搭 BSN 场景（gearbox 状态 + diesel 效果）
//! ```
//!
//! **为什么行为不能也在 RON 里**：diesel 的模板是 Rust 场景函数（`bsn!`），
//! 这是它刻意的取舍（组合靠类型系统，不靠字符串）。所以内容侧的边界是
//! "**数值与规则在 RON，组合在 Rust**"——这条边界要在每次加内容时守住。
//!
//! ## 加载期校验（价值就在这里）
//!
//! | 错误 | 触发 | 不管的话会怎样 |
//! |---|---|---|
//! | [`AbilityLoadError::EmptyId`] | 没写 `id` | 内容里出现匿名技能，日志与存档都对不上 |
//! | [`AbilityLoadError::DuplicateId`] | 两个技能同名 | 后一个**静默**盖掉前一个（取决于遍历顺序） |
//! | [`AbilityLoadError::UnknownAttribute`] | 规则引用了没登记过的属性名 | **最阴的一条**：gauge 的 `Attributes::value("拼错的名字")` 返回 `0.0` 不报错 ⇒ 攻击力/护甲静默变 0 |
//! | [`AbilityLoadError::NonFinite`] | 写了 `NaN` / `inf` | NaN 会顺着属性图污染一切（比较永远为假） |
//! | [`AbilityLoadError::NegativeTime`] | 前摇 / 冷却为负 | 时间轴倒流，状态机的行为无法预测 |

use std::collections::BTreeSet;

use bevy_gauge::expr::{CompileError, Expr};
use serde::Deserialize;

use crate::attributes::AttributeCatalog;
use crate::contest::AttackEffect;

/// 一个技能的定义。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AbilityRon {
    /// 词汇 ID（唯一；日志、存档、引用都用它）。
    pub id: String,
    /// 显示名。
    #[serde(default)]
    pub name: String,
    /// 前摇（秒）：进入 `#Invoking` 之后多久命中。`0.0` = 瞬发。
    #[serde(default)]
    pub cast_time: f32,
    /// 冷却（秒）：命中后多久回到可用。
    #[serde(default)]
    pub cooldown: f32,
    /// **释放需求**：gauge 表达式，全部满足才能放（旧 `requirements`）。
    ///
    /// 例：`"Action >= 1.0"`。表达式在加载期编译，写错了当场报。
    #[serde(default)]
    pub requires: Vec<String>,
    /// **释放费用**：进状态机之前扣（旧 `costs`）。
    #[serde(default)]
    pub costs: Vec<CostRon>,
    /// **挂哪些状态**（按 id 引用 `statuses.ron`；旧 `ApplyStatus`）。
    ///
    /// ⚠️ 现在**最多一条**：多个状态需要 `SubEffects` 子树（每个状态一个子效果实体，
    /// 各自带 `SpawnConfig`），而**一个实体只能有一个 `SpawnConfig` 组件**。
    /// 写两条会在加载期报 [`AbilityLoadError::TooManyAppliedStatuses`]——
    /// 宁可报错，也不静默丢掉第二条。
    #[serde(default)]
    pub applies: Vec<String>,
    /// **技能类别**（旧 `SkillTags` 的 `tags`）：硬控按类别封技能。
    ///
    /// 名字必须是 [`SkillTag::ALL`](crate::tags::SkillTag::ALL) 里的（大小写不敏感）；
    /// 写错在加载期报 [`AbilityLoadError::UnknownTag`]。
    #[serde(default)]
    pub tags: Vec<String>,
    /// **这一招威胁玩家吗**（旧引擎的"威胁窗口"凭据）。
    ///
    /// `true` 时前摇态会带上 [`ThreatensPlayer`](crate::threat::ThreatensPlayer)，
    /// 于是玩家能在这段前摇里反制。**必须配前摇**：没有前摇就没有反制窗口，
    /// 所以 `threatening: true` + `cast_time <= 0` 在加载期报错。
    #[serde(default)]
    pub threatening: bool,
    /// **这一招能打断别人吗**（反制时用）。
    ///
    /// 旧引擎把"能不能打断"写死在 `Effect::Interrupt` 里；新栈把它拆成**组件 + 一条裁决规则**
    /// （`Interrupts && !SuperArmor`），于是"加一种交互语义"= 加一个组件，而不是改效果枚举。
    #[serde(default)]
    pub interrupts: bool,
    /// **这一招不可被打断吗**（霸体）。
    #[serde(default)]
    pub super_armor: bool,
    /// **净化**：命中后移除目标身上的状态（旧 `Effect::RemoveStatus` / `DispelAction`）。
    #[serde(default)]
    pub dispels: Option<DispelRon>,
    /// **要求施法者此刻空闲**（旧 `NoActiveAction`）。
    ///
    /// 它不是属性表达式（"槽空不空"不是数值），所以由释放门控单独判：
    /// 施法者**另一个**技能正在 `#Invoking` 里时，这一招放不出来。
    #[serde(default)]
    pub requires_idle: bool,
    /// 命中规则；纯辅助技能可以省略。
    #[serde(default)]
    pub attack: Option<AttackRon>,
}

/// 一条费用：扣哪个属性、扣多少（旧 `(pool: "action", amount: 1.0)`）。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CostRon {
    /// 属性名（必须在属性台账里登记过）。
    pub attribute: String,
    /// 数量（非负有限数）。
    pub amount: f32,
}

/// 净化规则（旧 `Effect::RemoveStatus { who, status }` / `DispelAction` 的继任者）。
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct DispelRon {
    /// 一次最多净化几个（必须 ≥ 1）。
    pub max: u32,
    /// 只净化**减益**（以 `who: Target` 挂上去的状态）。
    pub debuffs_only: bool,
}

/// 命中规则（旧 `Contest { .. }` 的继任者）。
///
/// 默认值就是旧 `basic_attack` 的那一组，所以内容里只写要覆盖的字段。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AttackRon {
    /// 攻击方读哪个属性（旧 `CasterStat("strength")`）。
    #[serde(default = "default_attack_attribute")]
    pub attack_attribute: String,
    /// 防守方读哪个属性（旧 `TargetStat("armor")`）。
    #[serde(default = "default_defense_attribute")]
    pub defense_attribute: String,
    /// 攻击值上的固定修正（旧 `Literal(5.0)`）。
    #[serde(default = "default_power_bonus")]
    pub power_bonus: f32,
    /// 成功阈值（旧 `threshold`）。
    #[serde(default)]
    pub threshold: f32,
    /// 暴击余量；`0.0` = 关闭暴击（旧 `crit_margin`）。
    #[serde(default = "default_crit_margin")]
    pub crit_margin: f32,
    /// 暴击伤害倍率（旧 `Mul(SkillPower, Literal(1.5))`）。
    #[serde(default = "default_crit_multiplier")]
    pub crit_multiplier: f32,
    /// 伤害扣哪个属性（旧 `ModifyResource(pool: "hp")`）。
    #[serde(default = "default_damage_attribute")]
    pub damage_attribute: String,
}

fn default_attack_attribute() -> String {
    "Strength".to_string()
}
fn default_defense_attribute() -> String {
    "Armor".to_string()
}
fn default_power_bonus() -> f32 {
    5.0
}
fn default_crit_margin() -> f32 {
    5.0
}
fn default_crit_multiplier() -> f32 {
    1.5
}
fn default_damage_attribute() -> String {
    "Health".to_string()
}

impl Default for AttackRon {
    fn default() -> Self {
        let defaults = AttackEffect::default();
        Self {
            attack_attribute: defaults.attack_attribute,
            defense_attribute: defaults.defense_attribute,
            power_bonus: defaults.power_bonus,
            threshold: defaults.threshold,
            crit_margin: defaults.crit_margin,
            crit_multiplier: defaults.crit_multiplier,
            damage_attribute: defaults.damage_attribute,
        }
    }
}

impl AttackRon {
    /// 转成运行时用的规则参数。
    pub fn to_attack_effect(&self) -> AttackEffect {
        AttackEffect {
            attack_attribute: self.attack_attribute.clone(),
            defense_attribute: self.defense_attribute.clone(),
            power_bonus: self.power_bonus,
            threshold: self.threshold,
            crit_margin: self.crit_margin,
            crit_multiplier: self.crit_multiplier,
            damage_attribute: self.damage_attribute.clone(),
        }
    }
}

/// 加载技能定义时能犯的错。
#[derive(Debug, Clone, PartialEq)]
pub enum AbilityLoadError {
    /// 没写 `id`。
    EmptyId {
        /// 在文件里是第几条（0 起）。
        index: usize,
    },
    /// `id` 重复。
    DuplicateId {
        /// 重复的那个 id。
        id: String,
    },
    /// 数值不是有限数（`NaN` / `inf`）。
    NonFinite {
        /// 哪个技能。
        id: String,
        /// 哪个字段。
        field: &'static str,
    },
    /// 时间为负。
    NegativeTime {
        /// 哪个技能。
        id: String,
        /// 哪个字段。
        field: &'static str,
        /// 写成了多少。
        value: f32,
    },
    /// 规则引用了没登记过的属性名。
    UnknownAttribute {
        /// 哪个技能。
        id: String,
        /// 哪个字段（`attack_attribute` / `defense_attribute` / `damage_attribute`）。
        field: &'static str,
        /// 引用的名字。
        name: String,
        /// 台账里到底有哪些名字（省得人去翻文件）。
        known: BTreeSet<String>,
    },
    /// 释放需求表达式编译失败。
    BadRequirement {
        /// 哪个技能。
        id: String,
        /// 出问题的表达式源码。
        source: String,
        /// gauge 的编译错误。
        error: CompileError,
    },
    /// 费用引用了没登记过的属性名。
    UnknownCostAttribute {
        /// 哪个技能。
        id: String,
        /// 引用的名字。
        name: String,
        /// 台账里到底有哪些名字。
        known: BTreeSet<String>,
    },
    /// 费用数值非法（非有限 / 为负）。
    BadCost {
        /// 哪个技能。
        id: String,
        /// 哪个属性。
        attribute: String,
        /// 写成了多少。
        value: f32,
    },
    /// 引用了没登记过的状态 id。
    UnknownStatus {
        /// 哪个技能。
        id: String,
        /// 引用的状态 id。
        status: String,
        /// 台账里到底有哪些 id。
        known: Vec<String>,
    },
    /// 挂了多个状态（尚未支持）。
    TooManyAppliedStatuses {
        /// 哪个技能。
        id: String,
        /// 写了几条。
        count: usize,
    },
    /// 标了"威胁玩家"却没有前摇。
    ThreatNeedsWindUp {
        /// 哪个技能。
        id: String,
    },
    /// 写了不认识的技能类别。
    UnknownTag {
        /// 哪个技能。
        id: String,
        /// 写错的那个名字。
        name: String,
    },
    /// 净化规则不合法（`max: 0`：永远净化不掉任何东西）。
    BadDispel {
        /// 哪个技能。
        id: String,
    },
}

impl std::fmt::Display for AbilityLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyId { index } => write!(f, "第 {index} 个技能没有 id"),
            Self::DuplicateId { id } => write!(f, "技能 id `{id}` 重复"),
            Self::NonFinite { id, field } => write!(f, "技能 `{id}` 的 `{field}` 不是有限数"),
            Self::NegativeTime { id, field, value } => {
                write!(f, "技能 `{id}` 的 `{field}` 是负数（{value}）")
            }
            Self::UnknownAttribute {
                id,
                field,
                name,
                known,
            } => write!(
                f,
                "技能 `{id}` 的 `{field}` 引用了未登记的属性 `{name}`；已知属性：{known:?}"
            ),
            Self::BadRequirement { id, source, error } => {
                write!(f, "技能 `{id}` 的释放需求 `{source}` 编译失败：{error:?}")
            }
            Self::UnknownCostAttribute { id, name, known } => write!(
                f,
                "技能 `{id}` 的费用引用了未登记的属性 `{name}`；已知属性：{known:?}"
            ),
            Self::BadCost {
                id,
                attribute,
                value,
            } => write!(f, "技能 `{id}` 的费用 `{attribute}` 非法（{value}）"),
            Self::UnknownStatus { id, status, known } => write!(
                f,
                "技能 `{id}` 引用了未登记的状态 `{status}`；已知状态：{known:?}"
            ),
            Self::TooManyAppliedStatuses { id, count } => write!(
                f,
                "技能 `{id}` 挂了 {count} 个状态；一个技能目前最多挂一个（多个需要 SubEffects 子树）"
            ),
            Self::ThreatNeedsWindUp { id } => write!(
                f,
                "技能 `{id}` 标了 `threatening: true` 却没有前摇（`cast_time > 0`）——没有前摇就没有反制窗口"
            ),
            Self::UnknownTag { id, name } => write!(
                f,
                "技能 `{id}` 写了不认识的技能类别 `{name}`；可用类别：{:?}",
                crate::tags::SkillTag::ALL.map(crate::tags::SkillTag::name)
            ),
            Self::BadDispel { id } => {
                write!(f, "技能 `{id}` 的 `dispels.max` 是 0——永远净化不掉任何东西")
            }
        }
    }
}

impl std::error::Error for AbilityLoadError {}

/// 解析 `.ron` 文本（语法与结构）。
///
/// 与 [`crate::attributes::parse`] 同一套 RON 选项（`IMPLICIT_SOME`），
/// 否则内容里要写 `Some(...)` 的噪音。
pub fn parse(source: &str) -> Result<Vec<AbilityRon>, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
}

/// 校验一个技能（语义）。
pub fn validate_one(
    ability: &AbilityRon,
    attributes: &AttributeCatalog,
    statuses: &crate::statuses::StatusCatalog,
) -> Result<(), AbilityLoadError> {
    let id = ability.id.clone();
    for (field, value) in [
        ("cast_time", ability.cast_time),
        ("cooldown", ability.cooldown),
    ] {
        if !value.is_finite() {
            return Err(AbilityLoadError::NonFinite { id, field });
        }
        if value < 0.0 {
            return Err(AbilityLoadError::NegativeTime { id, field, value });
        }
    }

    // 状态引用：查台账（找不到只会静默什么都不挂）。
    if ability.applies.len() > 1 {
        return Err(AbilityLoadError::TooManyAppliedStatuses {
            id,
            count: ability.applies.len(),
        });
    }
    for status in &ability.applies {
        if statuses.get(status).is_none() {
            return Err(AbilityLoadError::UnknownStatus {
                id: id.clone(),
                status: status.clone(),
                known: statuses.iter().map(|status| status.id.clone()).collect(),
            });
        }
    }

    // 威胁必须配前摇：没有前摇就没有"可插入的一段时间"，玩家无从反制。
    if ability.threatening && ability.cast_time <= 0.0 {
        return Err(AbilityLoadError::ThreatNeedsWindUp { id });
    }

    // 技能类别：认不出就在加载期报错（否则硬控会**静默**漏掉这一类）。
    crate::tags::SkillTagMask::from_names(ability.tags.iter().map(String::as_str)).map_err(
        |name| AbilityLoadError::UnknownTag {
            id: id.clone(),
            name,
        },
    )?;

    // 净化：`max: 0` 是沉默的错。
    if ability.dispels.is_some_and(|dispels| dispels.max == 0) {
        return Err(AbilityLoadError::BadDispel { id });
    }

    let Some(attack) = &ability.attack else {
        // 没有命中规则也要查需求与费用：辅助技能一样有代价。
        validate_costs_and_requirements(ability, attributes)?;
        return Ok(());
    };

    for (field, value) in [
        ("power_bonus", attack.power_bonus),
        ("threshold", attack.threshold),
        ("crit_margin", attack.crit_margin),
        ("crit_multiplier", attack.crit_multiplier),
    ] {
        if !value.is_finite() {
            return Err(AbilityLoadError::NonFinite {
                id: id.clone(),
                field,
            });
        }
    }

    // **最阴的一条**：属性名拼错时 gauge 静默返回 0，所以在加载期查台账。
    for (field, name) in [
        ("attack_attribute", &attack.attack_attribute),
        ("defense_attribute", &attack.defense_attribute),
        ("damage_attribute", &attack.damage_attribute),
    ] {
        if !attributes.contains(name) {
            return Err(AbilityLoadError::UnknownAttribute {
                id: id.clone(),
                field,
                name: name.clone(),
                known: attributes.names().clone(),
            });
        }
    }

    validate_costs_and_requirements(ability, attributes)
}

/// 查释放需求与费用（**与有没有命中规则无关**，所以单独一条函数）。
fn validate_costs_and_requirements(
    ability: &AbilityRon,
    attributes: &AttributeCatalog,
) -> Result<(), AbilityLoadError> {
    let id = ability.id.clone();

    // 需求：**加载期编译**。gauge 自己的 `AttributeRequirement::compile` 只在编译失败时
    // `warn!` 然后永远算 false（技能从此放不出来，还不告诉你是谁写错了），
    // 所以这里宁可硬报错。
    for source in &ability.requires {
        Expr::compile(source, None).map_err(|error| AbilityLoadError::BadRequirement {
            id: id.clone(),
            source: source.clone(),
            error,
        })?;
    }

    for cost in &ability.costs {
        if !cost.amount.is_finite() || cost.amount < 0.0 {
            return Err(AbilityLoadError::BadCost {
                id: id.clone(),
                attribute: cost.attribute.clone(),
                value: cost.amount,
            });
        }
        if !attributes.contains(&cost.attribute) {
            return Err(AbilityLoadError::UnknownCostAttribute {
                id: id.clone(),
                name: cost.attribute.clone(),
                known: attributes.names().clone(),
            });
        }
    }
    Ok(())
}

/// 校验整份文件：先查 id（空 / 重复），再逐个查语义。
pub fn validate_all(
    abilities: &[AbilityRon],
    attributes: &AttributeCatalog,
    statuses: &crate::statuses::StatusCatalog,
) -> Result<(), AbilityLoadError> {
    let mut seen = BTreeSet::new();
    for (index, ability) in abilities.iter().enumerate() {
        let id = ability.id.trim();
        if id.is_empty() {
            return Err(AbilityLoadError::EmptyId { index });
        }
        if !seen.insert(id.to_string()) {
            return Err(AbilityLoadError::DuplicateId { id: id.to_string() });
        }
        validate_one(ability, attributes, statuses)?;
    }
    Ok(())
}

/// 一步到位：解析 + 校验。
///
/// 三份内容有**依赖方向**：技能的规则引用属性名与状态 id，
/// 所以属性与状态必须先建好台账，再校验技能。
pub fn load(
    source: &str,
    attributes: &AttributeCatalog,
    statuses: &crate::statuses::StatusCatalog,
) -> Result<Vec<AbilityRon>, LoadError> {
    let abilities = parse(source)?;
    validate_all(&abilities, attributes, statuses)?;
    Ok(abilities)
}

/// [`load`] 的两类错误。
#[derive(Debug)]
pub enum LoadError {
    /// RON 语法 / 结构错误。
    Syntax(ron::error::SpannedError),
    /// 语义错误。
    Semantic(AbilityLoadError),
}

impl From<ron::error::SpannedError> for LoadError {
    fn from(error: ron::error::SpannedError) -> Self {
        Self::Syntax(error)
    }
}

impl From<AbilityLoadError> for LoadError {
    fn from(error: AbilityLoadError) -> Self {
        Self::Semantic(error)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "RON 语法错误：{error}"),
            Self::Semantic(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attributes::{self, AttributeCatalog};

    const ATTRIBUTES: &str = r#"(
        base: [
            (name: "Strength", literal: 10.0),
            (name: "Armor",    literal: 3.0),
            (name: "Health",   literal: 100.0),
        ],
    )"#;

    /// 真内容的样子：默认值够用，所以只写要覆盖的字段。
    const ABILITIES: &str = r#"[
        (
            id: "basic_attack",
            name: "普通攻击",
            cast_time: 1.0,
            cooldown: 1.0,
            attack: (),
        ),
        (
            id: "heavy_slam",
            name: "重击",
            cast_time: 2.0,
            cooldown: 4.0,
            attack: (power_bonus: 12.0, crit_margin: 8.0, crit_multiplier: 2.0),
        ),
    ]"#;

    /// gauge 的属性名与表达式走**进程级 interner**（`OnceLock`），由 `AttributesPlugin`
    /// 初始化；在那之前编译任何表达式都会 panic（"Global interner not initialized"）。
    /// 所以只要内容里有 `requires`（要编译表达式），用例就得先起一个最小 App。
    fn ensure_gauge() {
        let mut app = bevy::app::App::new();
        app.add_plugins((bevy::time::TimePlugin, bevy_gauge::plugin::AttributesPlugin));
        app.update();
    }

    fn catalog() -> AttributeCatalog {
        ensure_gauge();
        let mut catalog = AttributeCatalog::default();
        catalog.add(&attributes::parse(ATTRIBUTES).expect("属性 RON 没问题"));
        catalog
    }

    /// 这些用例不涉及状态，所以给个空台账。
    fn no_statuses() -> crate::statuses::StatusCatalog {
        crate::statuses::StatusCatalog::default()
    }

    #[test]
    fn a_good_file_loads_and_defaults_match_the_old_basic_attack() {
        let abilities = load(ABILITIES, &catalog(), &no_statuses()).expect("这份内容该能加载");
        assert_eq!(abilities.len(), 2);

        let basic = &abilities[0];
        assert_eq!(basic.cast_time, 1.0);
        assert_eq!(basic.cooldown, 1.0);
        let effect = basic.attack.as_ref().unwrap().to_attack_effect();
        // 旧 `basic_attack` 的那一组：CasterStat("strength") + 5，TargetStat("armor")，
        // 阈值 0，暴击余量 5，暴击 ×1.5，扣 hp。
        assert_eq!(effect.attack_attribute, "Strength");
        assert_eq!(effect.defense_attribute, "Armor");
        assert_eq!(effect.power_bonus, 5.0);
        assert_eq!(effect.threshold, 0.0);
        assert_eq!(effect.crit_margin, 5.0);
        assert_eq!(effect.crit_multiplier, 1.5);
        assert_eq!(effect.damage_attribute, "Health");
    }

    #[test]
    fn a_misspelled_attribute_is_caught_at_load_time() {
        // 这正是上一轮留下的静默失败：gauge 的 `value("Strengh")` 会返回 0.0，
        // 攻击力悄悄变成 0，不崩不提示。加载期拿台账拦住它。
        let source = r#"[
            (id: "typo", attack: (attack_attribute: "Strengh")),
        ]"#;
        let error = load(source, &catalog(), &no_statuses()).expect_err("拼错的属性名必须报错");
        match error {
            LoadError::Semantic(AbilityLoadError::UnknownAttribute {
                id, field, name, ..
            }) => {
                assert_eq!(id, "typo");
                assert_eq!(field, "attack_attribute");
                assert_eq!(name, "Strengh");
            }
            other => panic!("该报未知属性，实际：{other}"),
        }
    }

    #[test]
    fn a_misspelled_damage_attribute_is_caught_too() {
        let source = r#"[
            (id: "typo_hp", attack: (damage_attribute: "Hp")),
        ]"#;
        let error = load(source, &catalog(), &no_statuses()).expect_err("扣血属性名也要查");
        assert!(
            matches!(
                error,
                LoadError::Semantic(AbilityLoadError::UnknownAttribute {
                    field: "damage_attribute",
                    ..
                })
            ),
            "实际：{error}"
        );
    }

    #[test]
    fn duplicate_and_empty_ids_are_rejected() {
        let duplicate = r#"[
            (id: "same"),
            (id: "same"),
        ]"#;
        assert_eq!(
            load(duplicate, &catalog(), &no_statuses())
                .expect_err("重名要报错")
                .to_string(),
            "技能 id `same` 重复"
        );

        let empty = r#"[
            (id: "  "),
        ]"#;
        assert_eq!(
            load(empty, &catalog(), &no_statuses())
                .expect_err("没名字要报错")
                .to_string(),
            "第 0 个技能没有 id"
        );
    }

    #[test]
    fn non_finite_and_negative_numbers_are_rejected() {
        let negative = r#"[
            (id: "back_in_time", cast_time: -1.0),
        ]"#;
        assert!(
            matches!(
                load(negative, &catalog(), &no_statuses()).expect_err("负前摇要报错"),
                LoadError::Semantic(AbilityLoadError::NegativeTime {
                    field: "cast_time",
                    ..
                })
            ),
            "负前摇该被拦住"
        );

        // RON 能写 `inf`（`nan` 也一样），而 NaN 会顺着属性图污染一切。
        let infinite = r#"[
            (id: "infinity", attack: (crit_multiplier: inf)),
        ]"#;
        assert!(
            matches!(
                load(infinite, &catalog(), &no_statuses()).expect_err("inf 要报错"),
                LoadError::Semantic(AbilityLoadError::NonFinite {
                    field: "crit_multiplier",
                    ..
                })
            ),
            "非有限数该被拦住"
        );
    }

    #[test]
    fn a_skill_without_an_attack_rule_is_fine() {
        // 纯辅助技能（治疗 / 位移）没有命中规则，不该被当成错误。
        let source = r#"[
            (id: "self_heal", name: "治疗", cast_time: 0.5, cooldown: 3.0),
        ]"#;
        let abilities = load(source, &catalog(), &no_statuses()).expect("没有 attack 也该能加载");
        assert!(abilities[0].attack.is_none());
    }

    #[test]
    fn a_broken_requirement_is_caught_at_load_time() {
        // gauge 自己的 `AttributeRequirement::compile` 编译失败时只 `warn!` 然后永远算 false
        // ——技能从此放不出来，还不告诉你是谁写错了。所以我们宁可硬报错。
        let source = r#"[
            (id: "bad_req", requires: ["Action >= "]),
        ]"#;
        let error = load(source, &catalog(), &no_statuses()).expect_err("需求表达式写错必须报错");
        assert!(
            matches!(
                error,
                LoadError::Semantic(AbilityLoadError::BadRequirement { .. })
            ),
            "实际：{error}"
        );
    }

    #[test]
    fn a_cost_on_an_unknown_attribute_is_caught() {
        let source = r#"[
            (id: "bad_cost", costs: [(attribute: "Mana", amount: 1.0)]),
        ]"#;
        let error = load(source, &catalog(), &no_statuses()).expect_err("费用属性名也要查台账");
        assert!(
            matches!(
                error,
                LoadError::Semantic(AbilityLoadError::UnknownCostAttribute { .. })
            ),
            "实际：{error}"
        );
    }

    #[test]
    fn a_negative_cost_is_caught() {
        let source = r#"[
            (id: "free_money", costs: [(attribute: "Action", amount: -1.0)]),
        ]"#;
        let error = load(source, &catalog(), &no_statuses()).expect_err("负费用要报错");
        assert!(
            matches!(error, LoadError::Semantic(AbilityLoadError::BadCost { .. })),
            "实际：{error}"
        );
    }

    #[test]
    fn a_syntax_error_surfaces_as_a_syntax_error() {
        let error = load("[ (id: ", &catalog(), &no_statuses()).expect_err("半截文件要报错");
        assert!(matches!(error, LoadError::Syntax(_)), "实际：{error}");
    }

    /// **真内容文件**必须能加载（`include_str!`：路径编译期检查，内容测试期校验）。
    ///
    /// 这条把"内容写错了"挡在提交前，而不是等运行时静默算成 0。
    #[test]
    fn the_real_content_files_load() {
        ensure_gauge();
        let mut catalog = AttributeCatalog::default();
        catalog.add(
            &attributes::parse(include_str!("../../../assets/data/attributes.ron"))
                .expect("属性文件语法没问题"),
        );
        let statuses = crate::statuses::load(
            include_str!("../../../assets/data/status_defs.ron"),
            &catalog,
        )
        .expect("状态文件该能加载");

        let abilities = load(
            include_str!("../../../assets/data/abilities.ron"),
            &catalog,
            &statuses,
        )
        .expect("技能文件该能加载");

        let basic = abilities
            .iter()
            .find(|ability| ability.id == "basic_attack")
            .expect("普攻在");
        assert_eq!(basic.cast_time, 1.0);
        assert_eq!(basic.cooldown, 1.0);

        // 哥布林那一招在旧内容里是 `crit_margin: 0.0` ⇒ 永不暴击，这条要如实带过来。
        let goblin = abilities
            .iter()
            .find(|ability| ability.id == "goblin_slash")
            .expect("哥布林劈砍在");
        let goblin_attack = goblin.attack.as_ref().expect("它该有命中规则");
        assert_eq!(goblin_attack.power_bonus, 3.0);
        assert_eq!(goblin_attack.crit_margin, 0.0);
        assert_eq!(goblin_attack.to_attack_effect().crit_multiplier, 1.5);

        // 自身增益那一招暂时没有命中规则（状态施加还没迁过来）。
        let block = abilities
            .iter()
            .find(|ability| ability.id == "shield_block")
            .expect("盾牌格挡在");
        assert!(block.attack.is_none());
        assert_eq!(block.cast_time, 2.0);
    }
}
