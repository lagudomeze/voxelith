//! 属性图的**内容管线**：`.ron` → `ModifierSet`，**全部错误在加载期报出来**。
//!
//! ```ron
//! (
//!     base: [
//!         (name: "Strength",  literal: 10.0),
//!         (name: "MaxHealth", expr: "Strength * 10.0 + 50.0"),
//!     ],
//!     modifiers: [
//!         (name: "Damage.Added", expr: "Strength * 0.5"),
//!     ],
//! )
//! ```
//!
//! ## 为什么分 `base` 与 `modifiers`
//!
//! gauge 有两条生命周期，**它们的集合必须分开**：
//!
//! | | 谁用 | 何时 | 能不能卸 |
//! |---|---|---|---|
//! | `base` | [`AttributeInitializer`] | 生成时铺一次 | **不能**（它就是初始值） |
//! | `modifiers` | [`AttributeModifiers`] | 装备 / 增益 / 状态生效时 | **能**（`remove` 精确回收） |
//!
//! 把两者混成一个集合，装备卸下时就只能"反着再加一遍"，数值会漂。
//!
//! ## 加载期校验（这里才是价值所在）
//!
//! `Expr::compile` **不需要世界**就能编译表达式，所以手写错公式时得到的是
//! **加载期的一条错误**（还带名字与源码位置），而不是运行时的静默 0：
//!
//! | 错误 | 触发 |
//! |---|---|
//! | [`AttributeLoadError::EmptyName`] | 名字是空的 |
//! | [`AttributeLoadError::NoValue`] | 既没 `literal` 也没 `expr` |
//! | [`AttributeLoadError::TwoValues`] | 两个都写了（意图不明，**不猜**） |
//! | [`AttributeLoadError::BadExpression`] | 表达式编译失败：括号不配、未知函数、未知标签、空表达式…… |
//!
//! ## ⚠️ 装配顺序是硬约束：**先加 `AttributesPlugin`，再解析内容**
//!
//! gauge 的属性名走一个**进程级**字符串 interner（`OnceLock<Arc<ThreadedRodeo>>`），
//! 它由 `AttributesPlugin` 初始化。在那之前碰任何属性名或表达式 —— 包括
//! `ModifierSet::add` 与 `Expr::compile` —— 都会 panic：
//!
//! ```text
//! Global interner not initialized - add AttributesPlugin first
//! ```
//!
//! 所以内容管线必须是"插件装配完成 → 再解析 `.ron`"，而不是反过来的"先读文件再建 App"；
//! 将来想做**独立的内容校验 CLI**，也得先起一个只装 `AttributesPlugin` 的极简 App。
//! 这是 `OnceLock`，一旦初始化过就一直在（同进程后续解析都安全）。

use bevy_gauge::expr::{CompileError, Expr};
use bevy_gauge::modifier_set::ModifierSet;
use ron::extensions::Extensions;
use serde::Deserialize;

/// 一份属性定义（对应一个 `.ron` 文件）。
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct AttributeSetRon {
    /// **词汇**：这一份属性表里"有哪些名字"（只有名字，没有值）。
    ///
    /// 它解决一个真实的混淆：**"名字登记过"与"值由谁给"是两件事**。
    /// 主属性（`Strength` 等）由**角色定义**给值，但它们的名字必须在词汇里，
    /// 否则校验会把角色文件里合法的写法当成拼错。
    #[serde(default)]
    pub vocabulary: Vec<String>,
    /// 生成时铺一次的基础属性（**不可卸**；也是角色定义**不能复用**的名字集合）。
    #[serde(default)]
    pub base: Vec<ModifierRon>,
    /// 可装可卸的修饰符（装备 / 增益 / 状态）。
    #[serde(default)]
    pub modifiers: Vec<ModifierRon>,
    /// **自然回复规则**：哪条资源按哪条回复属性长、封顶在哪个上限（见 [`crate::numeric`]）。
    ///
    /// 旧引擎把"上限 + 每秒回复"写死在池模板里（而且按池名**全局**生效）；
    /// 新栈把它变成内容 + 按角色读的属性，于是"法师每秒回 1 点法力"可以只装在法师身上。
    #[serde(default)]
    pub regen: Vec<RegenRon>,
}

/// 一条自然回复规则。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RegenRon {
    /// 长哪条资源（例如 `"Health"`）。
    pub attribute: String,
    /// 封顶在哪个上限属性（例如 `"MaxHealth"`）。
    pub max: String,
    /// 每秒长多少由哪条属性决定（例如 `"HealthRegen"`）。
    pub rate: String,
}

/// 一条属性：字面量**或**表达式，二选一。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModifierRon {
    /// 属性名（gauge 里是 `"MaxHealth"` 这样的字符串键）。
    pub name: String,
    /// 字面量数值。
    #[serde(default)]
    pub literal: Option<f32>,
    /// 表达式源码（`"Strength * 10.0 + 50.0"`，可引用 `@source` 与 `{TAG}`）。
    #[serde(default)]
    pub expr: Option<String>,
}

/// 加载属性定义时能犯的错。
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeLoadError {
    /// 名字是空的。
    EmptyName {
        /// 在文件里是第几条（0 起）。
        index: usize,
    },
    /// 既没 `literal` 也没 `expr`。
    NoValue {
        /// 出问题的属性名。
        name: String,
    },
    /// 两个都写了。
    TwoValues {
        /// 出问题的属性名。
        name: String,
    },
    /// 表达式编译失败。
    BadExpression {
        /// 出问题的属性名。
        name: String,
        /// 出问题的表达式源码（原样带回来，方便定位）。
        source: String,
        /// gauge 的编译错误。
        error: CompileError,
    },
}

impl std::fmt::Display for AttributeLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName { index } => write!(f, "第 {index} 条属性没有名字"),
            Self::NoValue { name } => {
                write!(f, "属性 `{name}` 既没 literal 也没 expr")
            }
            Self::TwoValues { name } => {
                write!(f, "属性 `{name}` 同时写了 literal 与 expr（二选一）")
            }
            Self::BadExpression {
                name,
                source,
                error,
            } => write!(f, "属性 `{name}` 的表达式 `{source}` 编译失败：{error:?}"),
        }
    }
}

impl std::error::Error for AttributeLoadError {}

/// 解析 `.ron` 文本。
///
/// 只解析语法与结构；语义（值二选一、表达式能否编译）在
/// [`AttributeSetRon::build_base`] / [`AttributeSetRon::build_modifiers`] 里查。
///
/// ⚠️ 开了 `IMPLICIT_SOME`：RON **默认要求 `Option` 字段显式写 `Some(...)`**
/// （关着的时候报的是 `ExpectedOption`，位置指向逗号，很难联想到）；
/// 那样内容文件里全是 `literal: Some(10.0)` 的噪音。开了之后写 `literal: 10.0` 就行，
/// 省略字段仍然是 `None`（`#[serde(default)]`）。
///
/// 另一种写法是在文件头写 `#![enable(implicit_some)]`，但那要求**每个文件**都记得加，
/// 漏了就是同一个 `ExpectedOption`；统一在代码里开，内容侧就不用管。
pub fn parse(source: &str) -> Result<AttributeSetRon, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME)
        .from_str(source)
}

/// 把一条 `.ron` 条目塞进集合（校验 + 编译）。
fn push_entry(
    set: &mut ModifierSet,
    index: usize,
    entry: &ModifierRon,
) -> Result<(), AttributeLoadError> {
    if entry.name.trim().is_empty() {
        return Err(AttributeLoadError::EmptyName { index });
    }
    match (&entry.literal, &entry.expr) {
        (Some(_), Some(_)) => Err(AttributeLoadError::TwoValues {
            name: entry.name.clone(),
        }),
        (None, None) => Err(AttributeLoadError::NoValue {
            name: entry.name.clone(),
        }),
        (Some(value), None) => {
            set.add(&entry.name, *value);
            Ok(())
        }
        (None, Some(source)) => {
            // **加载期编译**：`tags: None` 表示这份表达式不引用 `{TAG}`；
            // 引用了就会在这里报 `UnknownTag`，而不是运行时静默算成 0。
            Expr::compile(source, None).map_err(|error| AttributeLoadError::BadExpression {
                name: entry.name.clone(),
                source: source.clone(),
                error,
            })?;
            set.add_expr(&entry.name, source);
            Ok(())
        }
    }
}

impl AttributeSetRon {
    /// 生成时铺一次的那一套（`base`）。
    pub fn build_base(&self) -> Result<ModifierSet, AttributeLoadError> {
        build_modifier_set(&self.base)
    }

    /// 可装可卸的那一套（`modifiers`）。
    ///
    /// 与 `base` **分开构造**：装备卸下时 `ModifierSet::remove` 才能精确回收。
    pub fn build_modifiers(&self) -> Result<ModifierSet, AttributeLoadError> {
        build_modifier_set(&self.modifiers)
    }

    /// 这份定义里出现过的**属性名**（`base` + `modifiers`，已去重排序）。
    ///
    /// **这一份"全局定义"占了哪些名字**：`base` 与 `modifiers` 里写过的那些。
    ///
    /// 角色定义（[`crate::actors`]）不能再用这些名字——gauge 的 `ModifierSet::add`
    /// 是**累加**，两边写同一个名字得到的是和而不是覆盖。
    pub fn reserved_names(&self) -> std::collections::BTreeSet<String> {
        self.base
            .iter()
            .chain(self.modifiers.iter())
            .map(|entry| entry.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// 给别的**内容**做交叉校验用：技能的 `AttackEffect` 用字符串引用属性名，
    /// 而 gauge 的 `Attributes::value("拼错的名字")` 返回 **`0.0` 不报错**，
    /// 所以"这个名字到底存不存在"必须在加载期查（见 [`AttributeCatalog`]）。
    pub fn declared_names(&self) -> std::collections::BTreeSet<String> {
        self.vocabulary
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .chain(self.reserved_names())
            .collect()
    }
}

/// **公开**的构造入口：从一组修饰符条目造集合。
///
/// 状态定义（`statuses`）复用它与同一套校验——状态的修饰符和属性的修饰符
/// 是同一件事，不该有两份规则。
pub fn build_modifier_set(entries: &[ModifierRon]) -> Result<ModifierSet, AttributeLoadError> {
    let mut set = ModifierSet::new();
    for (index, entry) in entries.iter().enumerate() {
        push_entry(&mut set, index, entry)?;
    }
    Ok(set)
}

/// **属性名台账**：把多份属性定义合起来，回答"这个名字存在吗"。
///
/// 一次内容加载要跨文件校验（角色属性在 `players.ron`、怪物属性在 `monsters.ron`、
/// 技能规则引用它们的名字），所以台账是**累积**的：
///
/// ```text
/// AttributeCatalog::default()
///   .add(&players)?.add(&monsters)?
///   → skills::validate_all(&abilities, &catalog)?   // 拼错的属性名在这里被拦住
/// ```
#[derive(Debug, Clone, Default)]
pub struct AttributeCatalog {
    names: std::collections::BTreeSet<String>,
}

impl AttributeCatalog {
    /// 登记一份属性定义里的全部名字。
    pub fn add(&mut self, set: &AttributeSetRon) {
        self.names.extend(set.declared_names());
    }

    /// 这个名字登记过吗。
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name.trim())
    }

    /// 全部名字（错误信息里列出来，省得人去翻文件）。
    pub fn names(&self) -> &std::collections::BTreeSet<String> {
        &self.names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use bevy_gauge::prelude::{AttributeInitializer, Attributes, AttributesPlugin};

    const GOOD: &str = r#"(
        base: [
            (name: "Strength",  literal: 10.0),
            (name: "MaxHealth", expr: "Strength * 10.0 + 50.0"),
        ],
        modifiers: [
            (name: "Damage.Added", expr: "Strength * 0.5"),
        ],
    )"#;

    /// 属性名/表达式要在 gauge 的**全局 interner** 就绪之后才能碰，
    /// 而那个 interner 由 `AttributesPlugin` 初始化——所以每个用例都先起 App。
    fn app_with_gauge() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AttributesPlugin));
        app
    }

    #[test]
    fn a_good_file_splits_into_two_sets() {
        let _app = app_with_gauge();
        let ron = parse(GOOD).expect("语法没问题");
        let base = ron.build_base().expect("base 能构造");
        let modifiers = ron.build_modifiers().expect("modifiers 能构造");
        assert_eq!(base.len(), 2, "base 只有两条");
        assert_eq!(modifiers.len(), 1, "modifiers 与 base 分开");
    }

    #[test]
    fn expressions_and_literals_both_land_in_gauge() {
        // App 要在构造集合**之前**起来（全局 interner）。
        let mut app = app_with_gauge();
        let ron = parse(GOOD).expect("语法没问题");
        let base = ron.build_base().expect("base 能构造");

        let entity = app
            .world_mut()
            .spawn((Attributes::new(), AttributeInitializer::new(base)))
            .id();
        app.update();

        let attributes = app
            .world()
            .entity(entity)
            .get::<Attributes>()
            .expect("有属性");
        assert_eq!(attributes.value("Strength"), 10.0);
        assert_eq!(
            attributes.value("MaxHealth"),
            150.0,
            "`Strength * 10.0 + 50.0` 在属性图里自动传播"
        );
    }

    #[test]
    fn a_broken_expression_is_a_load_time_error() {
        let _app = app_with_gauge();
        let source = r#"(
            base: [ (name: "Broken", expr: "Strength * ") ],
        )"#;
        let ron = parse(source).expect("RON 语法没问题");
        match ron.build_base() {
            Err(AttributeLoadError::BadExpression {
                name,
                source,
                error,
            }) => {
                assert_eq!(name, "Broken");
                assert_eq!(source, "Strength * ");
                assert_eq!(error, CompileError::UnexpectedEof, "错在哪都报出来了");
            }
            other => panic!("该报表达式错误，实际：{other:?}"),
        }
    }

    #[test]
    fn an_unknown_tag_is_also_caught_at_load_time() {
        let _app = app_with_gauge();
        // 没传 TagResolver，所以 `{FIRE}` 一定解析不了——这正是我们要的"早报"。
        let source = r#"(
            base: [ (name: "Damage", expr: "Base * 1.0{FIRE}") ],
        )"#;
        let ron = parse(source).expect("RON 语法没问题");
        let error = ron.build_base().expect_err("该报未知标签");
        assert!(
            matches!(error, AttributeLoadError::BadExpression { .. }),
            "实际：{error:?}"
        );
    }

    #[test]
    fn a_missing_or_doubled_value_is_rejected() {
        let missing = parse(r#"( base: [ (name: "X") ] )"#).unwrap();
        assert_eq!(
            missing.build_base().expect_err("该报缺少取值"),
            AttributeLoadError::NoValue { name: "X".into() }
        );

        let doubled = parse(r#"( base: [ (name: "X", literal: 1.0, expr: "1.0") ] )"#).unwrap();
        assert_eq!(
            doubled.build_base().expect_err("该报两个都写了"),
            AttributeLoadError::TwoValues { name: "X".into() }
        );

        let nameless = parse(r#"( base: [ (name: "  ", literal: 1.0) ] )"#).unwrap();
        assert_eq!(
            nameless.build_base().expect_err("该报没有名字"),
            AttributeLoadError::EmptyName { index: 0 }
        );
    }

    #[test]
    fn a_ron_syntax_error_never_reaches_the_builder() {
        assert!(parse("( base: [ (name: ").is_err());
    }
}
