//! 状态（增益 / 减益）定义的内容管线：`.ron` → 校验过的定义。
//!
//! 这是旧 `assets/data/statuses.ron` 的继任者。一个状态在旧模型里是
//! `ApplyStatus(status: "guarding", duration: Literal(2.0), who: Caster)`；
//! 在新模型里它是一条**独立的定义**，被技能按 id 引用：
//!
//! ```ron
//! // statuses.ron
//! [
//!     (id: "Guarding", name: "格挡", duration: 2.0, who: Caster,
//!      modifiers: [(name: "Armor", literal: 5.0)]),
//! ]
//! ```
//!
//! ```ron
//! // abilities.ron
//! (id: "shield_block", ..., applies: ["Guarding"]),
//! ```
//!
//! ## 为什么状态是"定义 + 引用"而不是内联
//!
//! 同一个状态会被多个技能挂上（燃烧被火球 / 火墙 / 毒云挂上）。内联的话
//! "燃烧"的数值会在 97 个技能里各抄一遍，改一次要改 97 处；按 id 引用则
//! **一处在 `statuses.ron`**，而且加载期能查"这个 id 到底存不存在"。
//!
//! ## `who` 决定两件事，必须一致
//!
//! | `who` | 挂到谁身上 | 生成位置 | 修饰符目标 |
//! |---|---|---|---|
//! | `Caster` | 自己（`shield_block` 的格挡） | `SpawnConfig::invoker` | `SustainedModifierConfig::invoker` |
//! | `Target` | 对方（燃烧 / 虚弱） | `SpawnConfig::target` | `SustainedModifierConfig::invoker_target` |
//!
//! 这两件事必须一起选对：只改生成位置会让状态**长在你身上、效果打在对面**。
//! 所以 `who` 是状态的属性（内容），构造器两边都从它推。

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::attributes::{self, AttributeCatalog, AttributeLoadError, ModifierRon};

/// 一个状态的定义。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StatusRon {
    /// 词汇 ID（唯一；技能按它引用）。
    pub id: String,
    /// 显示名。
    #[serde(default)]
    pub name: String,
    /// 持续时长（秒）。
    pub duration: f32,
    /// 挂在谁身上。
    pub who: WhoRon,
    /// 生效期间附加的修饰符（**可精确卸下**：`ModifierSet::remove`）。
    #[serde(default)]
    pub modifiers: Vec<ModifierRon>,
    /// **周期效果**（旧 `on_tick`）：每 `every` 秒改一次属性。`None` = 没有周期效果。
    #[serde(default)]
    pub tick: Option<TickRon>,
    /// **硬控**：封掉哪几类技能（旧 `blocks_tags`）。
    ///
    /// 判定落在**释放门控**上：挂着这个状态的人，`tags` 与之有交集的技能放不出来
    /// （见 [`crate::casting`]）。
    #[serde(default)]
    pub blocks: Vec<String>,
    /// **叠层规则**：同一个状态第二次挂到同一个人身上时怎么办（见 [`crate::stacking`]）。
    #[serde(default)]
    pub stacking: StackingRon,
    /// **挂上时**的效果（旧 `on_apply`）。
    ///
    /// ⚠️ 与 `tick` 互斥：带 `tick` 的状态**上状态即第一跳**（见 [`crate::periodic`]），
    /// 那已经覆盖了"挂上时打一下"。两者同时写会在加载期报
    /// [`StatusLoadError::ApplyAndTick`]——而不是静默丢掉一半。
    #[serde(default)]
    pub on_apply: Vec<EffectRon>,
    /// **自然到期时**的效果（旧 `on_expire`）。
    #[serde(default)]
    pub on_expire: Vec<EffectRon>,
    /// **被提前移除时**的效果（旧 `on_remove`）：叠层顶掉、被驱散都属于这一类。
    #[serde(default)]
    pub on_remove: Vec<EffectRon>,
}

/// 一条"改属性"的效果：正数回复、负数伤害。
///
/// 目标由状态的 `who` 决定（与 `tick` 同一个约定），所以这里只有"打多少"。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EffectRon {
    /// 改哪个属性（必须在属性台账里登记过）。
    pub attribute: String,
    /// 改多少。
    pub amount: f32,
}

/// 叠层规则（内容形态）。默认 [`StackingRon::Refresh`]。
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
pub enum StackingRon {
    /// 只保留最新那一个（时长与效果都刷新）。
    #[default]
    Refresh,
    /// 用新的顶掉旧的。
    Replace,
    /// 已经有就不重挂。
    Ignore,
    /// 最多同时 `n` 个（必须 ≥ 1），超了丢最旧的。
    Stack(
        /// 最大层数。
        u32,
    ),
}

impl StackingRon {
    /// 转成运行时组件。
    pub fn to_component(self) -> crate::stacking::Stacking {
        match self {
            StackingRon::Refresh => crate::stacking::Stacking::Refresh,
            StackingRon::Replace => crate::stacking::Stacking::Replace,
            StackingRon::Ignore => crate::stacking::Stacking::Ignore,
            StackingRon::Stack(max) => crate::stacking::Stacking::Stack(max),
        }
    }
}

/// 周期效果（持续伤害 / 持续回复）。
///
/// 旧内容里是 `on_tick: [ModifyResource(pool: "hp", delta: Neg(Literal(3.0)), who: Target)]`。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TickRon {
    /// 间隔（秒），必须为正。
    pub every: f32,
    /// 每次改多少：**正数回复、负数伤害**（与 `ModifyResource` 的 `delta` 一致）。
    ///
    /// 与 [`TickRon::amount_expr`] **二选一**。
    #[serde(default)]
    pub amount: Option<f32>,
    /// 用**表达式**算这一跳（例如 `"MaxHealth / -20.0"`：表达式能读宿主自己的属性）。
    ///
    /// 与 [`TickRon::amount`] **二选一**。
    ///
    /// ## ⚠️ 别在表达式里乘 `<id>Stacks`
    ///
    /// 层数确实是一条属性：每个状态实例给自己那条 `<id>Stacks` 各加 1
    /// （`AttributeModifiers` 本来就是累加），所以宿主身上的值等于层数。
    ///
    /// **但每个实例都会各跳一次**，所以
    ///
    /// ```text
    /// 三层 + 表达式 "-1.0 * CorrodingStacks"  ⇒  N × N = 9 点（双重计数）
    /// ```
    ///
    /// 两种模型只能选一个：
    ///
    /// | 模型 | 层数怎么体现 | 现状 |
    /// |---|---|---|
    /// | **N 个实例各算一次** | 效果**自动**线性叠加（三个实例三次一跳） | ✅ 新栈现在就是这个 |
    /// | 一个实例 + 层数计数器 | 效果写表达式按层数放大（ToME 的 DoT 是这种） | ⬜ 未做 |
    ///
    /// 所以在当前模型下，`<id>Stacks` 是**只读的信息面**（HUD 显示"中毒 ×3"、调试、
    /// 以及"不随实例重复"的效果），而不是周期跳的乘数。
    ///
    /// 另外：**挂上那一跳**发生在层数落定之前（修饰符由 diesel 在 `Update` 里施加，
    /// 而进入生效态与首跳发生在 gearbox 调度里），所以首跳可能按 0 层算。
    /// 要精确控制首跳就把它写成 `on_apply`，把 `tick` 留给周期跳。
    #[serde(default)]
    pub amount_expr: Option<String>,
    /// 改哪个属性（旧 `pool: "hp"` ⇒ `"Health"`）。
    pub attribute: String,
}

/// 状态挂在谁身上。
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum WhoRon {
    /// 施法者自己（增益）。
    Caster,
    /// 施法者的目标（减益）。
    Target,
}

impl WhoRon {
    /// 名字（用来拼模板名 / 写日志）。
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Caster => "caster",
            Self::Target => "target",
        }
    }
}

/// 状态定义台账：id → 定义。
///
/// 技能用字符串引用状态 id，所以"这个 id 存不存在"必须在加载期查
/// （与属性名同一个理由：运行时找不到只会静默什么都不做）。
#[derive(Debug, Clone, Default)]
pub struct StatusCatalog {
    by_id: BTreeMap<String, StatusRon>,
}

impl StatusCatalog {
    /// 登记一条定义（后登记的覆盖先登记的；重复由 [`validate_statuses`] 在加载期拦）。
    pub fn insert(&mut self, status: StatusRon) {
        self.by_id.insert(status.id.clone(), status);
    }

    /// 按 id 查。
    pub fn get(&self, id: &str) -> Option<&StatusRon> {
        self.by_id.get(id)
    }

    /// 有几条。
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// 遍历（注册模板用）。
    pub fn iter(&self) -> impl Iterator<Item = &StatusRon> + '_ {
        self.by_id.values()
    }

    /// 从一批定义建台账（先校验，失败就不建）。
    pub fn build(
        statuses: &[StatusRon],
        attributes: &AttributeCatalog,
    ) -> Result<Self, StatusLoadError> {
        validate_statuses(statuses, attributes)?;
        let mut catalog = Self::default();
        for status in statuses {
            catalog.insert(status.clone());
        }
        Ok(catalog)
    }
}

/// 加载状态定义时能犯的错。
#[derive(Debug, Clone, PartialEq)]
pub enum StatusLoadError {
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
    /// 时长不是有限数。
    NonFiniteDuration {
        /// 哪个状态。
        id: String,
    },
    /// 时长为负。
    NegativeDuration {
        /// 哪个状态。
        id: String,
        /// 写成了多少。
        value: f32,
    },
    /// 修饰符本身不合法（值二选一 / 表达式编译失败 / 名字为空）。
    BadModifier {
        /// 哪个状态。
        id: String,
        /// 底层错误（复用属性那一套）。
        error: AttributeLoadError,
    },
    /// 修饰符引用了没登记过的属性名。
    UnknownAttribute {
        /// 哪个状态。
        id: String,
        /// 引用的名字。
        name: String,
        /// 台账里到底有哪些名字。
        known: std::collections::BTreeSet<String>,
    },
    /// 周期效果不合法（间隔非正 / 数值非有限 / 属性没登记）。
    BadTick {
        /// 哪个状态。
        id: String,
        /// 为什么。
        reason: String,
    },
    /// 写了不认识的硬控类别。
    UnknownBlockTag {
        /// 哪个状态。
        id: String,
        /// 写错的那个名字。
        name: String,
    },
    /// 叠层规则不合法（`Stack(0)`：永远挂不上，是个沉默的错）。
    BadStacking {
        /// 哪个状态。
        id: String,
    },
    /// 进出效果不合法（属性没登记 / 数值非有限）。
    BadEffect {
        /// 哪个状态。
        id: String,
        /// 哪个时机（`on_apply` / `on_expire` / `on_remove`）。
        timing: &'static str,
        /// 为什么。
        reason: String,
    },
    /// 同时写了 `on_apply` 与 `tick`。
    ApplyAndTick {
        /// 哪个状态。
        id: String,
    },
}

impl std::fmt::Display for StatusLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyId { index } => write!(f, "第 {index} 个状态没有 id"),
            Self::DuplicateId { id } => write!(f, "状态 id `{id}` 重复"),
            Self::NonFiniteDuration { id } => write!(f, "状态 `{id}` 的时长不是有限数"),
            Self::NegativeDuration { id, value } => {
                write!(f, "状态 `{id}` 的时长是负数（{value}）")
            }
            Self::BadModifier { id, error } => {
                write!(f, "状态 `{id}` 的修饰符不合法：{error}")
            }
            Self::UnknownAttribute { id, name, known } => write!(
                f,
                "状态 `{id}` 的修饰符引用了未登记的属性 `{name}`；已知属性：{known:?}"
            ),
            Self::BadTick { id, reason } => write!(f, "状态 `{id}` 的周期效果不合法：{reason}"),
            Self::UnknownBlockTag { id, name } => write!(
                f,
                "状态 `{id}` 写了不认识的硬控类别 `{name}`；可用类别：{:?}",
                crate::tags::SkillTag::ALL.map(crate::tags::SkillTag::name)
            ),
            Self::BadStacking { id } => write!(
                f,
                "状态 `{id}` 的 `stacking: Stack(0)` 永远挂不上（最大层数必须 ≥ 1）"
            ),
            Self::BadEffect { id, timing, reason } => {
                write!(f, "状态 `{id}` 的 `{timing}` 不合法：{reason}")
            }
            Self::ApplyAndTick { id } => write!(
                f,
                "状态 `{id}` 同时写了 `on_apply` 与 `tick`：带 `tick` 的状态上状态即第一跳，\
                 那已经覆盖了 on_apply（要分开得等子状态方案）"
            ),
        }
    }
}

impl std::error::Error for StatusLoadError {}

/// 解析 `.ron` 文本（与别的内容同一套 RON 选项）。
pub fn parse(source: &str) -> Result<Vec<StatusRon>, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
}

/// 校验一批状态定义。
pub fn validate_statuses(
    statuses: &[StatusRon],
    attributes: &AttributeCatalog,
) -> Result<(), StatusLoadError> {
    let mut seen = std::collections::BTreeSet::new();
    for (index, status) in statuses.iter().enumerate() {
        let id = status.id.trim().to_string();
        if id.is_empty() {
            return Err(StatusLoadError::EmptyId { index });
        }
        if !seen.insert(id.clone()) {
            return Err(StatusLoadError::DuplicateId { id });
        }
        if !status.duration.is_finite() {
            return Err(StatusLoadError::NonFiniteDuration { id });
        }
        if status.duration < 0.0 {
            return Err(StatusLoadError::NegativeDuration {
                id,
                value: status.duration,
            });
        }

        // 修饰符复用属性那一套（值二选一 + 表达式编译），再查名字台账。
        attributes::build_modifier_set(&status.modifiers).map_err(|error| {
            StatusLoadError::BadModifier {
                id: id.clone(),
                error,
            }
        })?;
        for modifier in &status.modifiers {
            if !attributes.contains(&modifier.name) {
                return Err(StatusLoadError::UnknownAttribute {
                    id: id.clone(),
                    name: modifier.name.clone(),
                    known: attributes.names().clone(),
                });
            }
        }

        // 周期效果：间隔必须为正（否则永远不会响，是个沉默的错），属性要登记过。
        if let Some(tick) = &status.tick {
            if !tick.every.is_finite() || tick.every <= 0.0 {
                return Err(StatusLoadError::BadTick {
                    id,
                    reason: format!("间隔必须是正的有限数（写的是 {}）", tick.every),
                });
            }
            // `amount` 与 `amount_expr` 二选一：两个都写，"这一跳算多少"就没有唯一答案。
            match (&tick.amount, &tick.amount_expr) {
                (Some(_), Some(_)) => {
                    return Err(StatusLoadError::BadTick {
                        id,
                        reason: "`amount` 与 `amount_expr` 只能写一个".to_string(),
                    });
                }
                (None, None) => {
                    return Err(StatusLoadError::BadTick {
                        id,
                        reason: "`amount` 与 `amount_expr` 得写一个".to_string(),
                    });
                }
                (Some(amount), None) => {
                    if !amount.is_finite() {
                        return Err(StatusLoadError::BadTick {
                            id,
                            reason: format!("每一跳的数值不是有限数（写的是 {amount}）"),
                        });
                    }
                }
                (None, Some(source)) => {
                    // 表达式在加载期编译一次：写错当场报，而不是等第一跳才发现。
                    if let Err(error) = attributes::build_modifier_set(&[ModifierRon {
                        name: tick.attribute.clone(),
                        literal: None,
                        expr: Some(source.clone()),
                    }]) {
                        return Err(StatusLoadError::BadTick {
                            id,
                            reason: format!("`amount_expr` 编译不过：{error}"),
                        });
                    }
                }
            }
            if !attributes.contains(&tick.attribute) {
                return Err(StatusLoadError::BadTick {
                    id,
                    reason: format!(
                        "引用了未登记的属性 `{}`；已知属性：{:?}",
                        tick.attribute,
                        attributes.names()
                    ),
                });
            }
        }

        // 硬控类别：认不出就在加载期报错（否则这一类技能**静默**漏掉了控制）。
        crate::tags::SkillTagMask::from_names(status.blocks.iter().map(String::as_str)).map_err(
            |name| StatusLoadError::UnknownBlockTag {
                id: id.clone(),
                name,
            },
        )?;

        // 叠层：`Stack(0)` 意味着这个状态永远挂不上——那是沉默的错，要在加载期拦。
        if matches!(status.stacking, StackingRon::Stack(0)) {
            return Err(StatusLoadError::BadStacking { id });
        }

        // 进出效果：属性要登记过、数值要有限。
        for (timing, effects) in [
            ("on_apply", &status.on_apply),
            ("on_expire", &status.on_expire),
            ("on_remove", &status.on_remove),
        ] {
            for effect in effects {
                if !effect.amount.is_finite() {
                    return Err(StatusLoadError::BadEffect {
                        id: id.clone(),
                        timing,
                        reason: format!("数值不是有限数（写的是 {}）", effect.amount),
                    });
                }
                if !attributes.contains(&effect.attribute) {
                    return Err(StatusLoadError::BadEffect {
                        id: id.clone(),
                        timing,
                        reason: format!(
                            "引用了未登记的属性 `{}`；已知属性：{:?}",
                            effect.attribute,
                            attributes.names()
                        ),
                    });
                }
            }
        }
        if status.tick.is_some() && !status.on_apply.is_empty() {
            return Err(StatusLoadError::ApplyAndTick { id });
        }
    }
    Ok(())
}

/// 一步到位：解析 + 校验 + 建台账。
pub fn load(source: &str, attributes: &AttributeCatalog) -> Result<StatusCatalog, StatusFileError> {
    let statuses = parse(source)?;
    Ok(StatusCatalog::build(&statuses, attributes)?)
}

/// [`load`] 的两类错误。
#[derive(Debug)]
pub enum StatusFileError {
    /// RON 语法 / 结构错误。
    Syntax(ron::error::SpannedError),
    /// 语义错误。
    Semantic(StatusLoadError),
}

impl From<ron::error::SpannedError> for StatusFileError {
    fn from(error: ron::error::SpannedError) -> Self {
        Self::Syntax(error)
    }
}

impl From<StatusLoadError> for StatusFileError {
    fn from(error: StatusLoadError) -> Self {
        Self::Semantic(error)
    }
}

impl std::fmt::Display for StatusFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "RON 语法错误：{error}"),
            Self::Semantic(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StatusFileError {}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTRIBUTES: &str = r#"(
        base: [
            (name: "Strength", literal: 10.0),
            (name: "Armor",    literal: 3.0),
            (name: "Health",   literal: 100.0),
        ],
    )"#;

    const STATUSES: &str = r#"[
        (id: "Guarding", name: "格挡", duration: 2.0, who: Caster,
         modifiers: [(name: "Armor", literal: 5.0)]),
        (id: "Weakened", name: "虚弱", duration: 3.0, who: Target,
         modifiers: [(name: "Strength", expr: "Strength * 0.5")]),
    ]"#;

    fn catalog() -> AttributeCatalog {
        let mut app = bevy::app::App::new();
        app.add_plugins((bevy::time::TimePlugin, bevy_gauge::plugin::AttributesPlugin));
        app.update();
        let mut catalog = AttributeCatalog::default();
        catalog.add(&attributes::parse(ATTRIBUTES).expect("属性 RON 没问题"));
        catalog
    }

    #[test]
    fn a_good_file_builds_a_catalog() {
        let catalog = StatusCatalog::build(&parse(STATUSES).expect("语法没问题"), &catalog())
            .expect("这份状态该能加载");
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.get("Guarding").unwrap().who, WhoRon::Caster);
        assert_eq!(catalog.get("Weakened").unwrap().who, WhoRon::Target);
        assert_eq!(catalog.get("Weakened").unwrap().duration, 3.0);
    }

    #[test]
    fn an_unknown_modifier_attribute_is_caught() {
        let source = r#"[
            (id: "Typo", duration: 1.0, who: Target,
             modifiers: [(name: "Armour", literal: 5.0)]),
        ]"#;
        let error =
            validate_statuses(&parse(source).unwrap(), &catalog()).expect_err("属性名拼错必须报错");
        assert!(
            matches!(error, StatusLoadError::UnknownAttribute { ref name, .. } if name == "Armour"),
            "实际：{error}"
        );
    }

    #[test]
    fn bad_durations_and_duplicate_ids_are_caught() {
        let negative = r#"[
            (id: "Backwards", duration: -1.0, who: Caster),
        ]"#;
        assert!(matches!(
            validate_statuses(&parse(negative).unwrap(), &catalog()),
            Err(StatusLoadError::NegativeDuration { .. })
        ));

        let duplicate = r#"[
            (id: "Same", duration: 1.0, who: Caster),
            (id: "Same", duration: 2.0, who: Caster),
        ]"#;
        assert!(matches!(
            validate_statuses(&parse(duplicate).unwrap(), &catalog()),
            Err(StatusLoadError::DuplicateId { .. })
        ));
    }

    #[test]
    fn a_broken_modifier_expression_is_caught() {
        let source = r#"[
            (id: "Broken", duration: 1.0, who: Caster,
             modifiers: [(name: "Armor", expr: "Armor * ")]),
        ]"#;
        let error =
            validate_statuses(&parse(source).unwrap(), &catalog()).expect_err("表达式写错必须报错");
        assert!(
            matches!(error, StatusLoadError::BadModifier { .. }),
            "实际：{error}"
        );
    }
}
