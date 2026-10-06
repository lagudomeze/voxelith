//! **角色定义**：每个角色自己的那一套数（旧 `players.ron` / `monsters.ron`）。
//!
//! ## 与 `attributes.ron` 的分工
//!
//! ```text
//! attributes.ron   全局**定义**：派生表达式（MaxHealth = Constitution * 10）、
//!                  每秒回复（*Regen）、每个名字的默认值
//! actors.ron       每个角色**自己的数**：主属性 + 资源的当前值
//! ```
//!
//! ## ⚠️ 两边不许写同一个名字（加载期拦）
//!
//! 这不是洁癖，是 gauge 的语义：
//!
//! ```text
//! ModifierSet::add = += （累加，不是覆盖）
//! ```
//!
//! 所以"全局写 `Strength` 10、角色再写 12"得到的是 **22**——一个安静的错。
//! 因此 [`ActorCatalog::load`] 要求把全局那份传进来，凡是全局已经定义过的名字
//! 在角色里再出现就报 [`ActorLoadError::OverlapsGlobal`]。
//!
//! ## 组合
//!
//! [`ActorCatalog::build_set`] 把两段拼成一个 `ModifierSet`：**先全局、后角色**
//! （顺序不影响结果——两边的名字集合不相交，这正是上面那条检查换来的性质）。
//!
//! ## 还没迁
//!
//! 旧文件里的 `role`（Player / Monster）/ `faction` / `ai`（技能选择与权重）/
//! `energy_rate` / `traits`（种族标签）——新栈里还没有对应机制，等 AI 与阵营迁过来时补。

use bevy::prelude::*;
use bevy_gauge::modifier_set::ModifierSet;
use serde::Deserialize;

use crate::attributes::{AttributeCatalog, AttributeLoadError, AttributeSetRon, ModifierRon};

/// 一个角色定义。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ActorRon {
    /// 词汇 ID（L2 按它取角色）。
    pub id: String,
    /// 显示名。
    #[serde(default)]
    pub name: String,
    /// 这个角色自己的数（主属性 + 资源当前值）。
    #[serde(default)]
    pub attributes: Vec<ModifierRon>,
}

/// 角色定义表（**Resource**：L2 生成实体时按 id 取）。
#[derive(Resource, Debug, Clone, Default)]
pub struct ActorCatalog {
    actors: Vec<ActorRon>,
}

impl ActorCatalog {
    /// 全部角色（保持文件顺序）。
    pub fn iter(&self) -> impl Iterator<Item = &ActorRon> {
        self.actors.iter()
    }

    /// 有几个角色。
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// 是不是空的。
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// 按 id 取角色定义。
    pub fn get(&self, id: &str) -> Option<&ActorRon> {
        self.actors.iter().find(|actor| actor.id == id)
    }

    /// 解析 + 校验 + 建表。
    ///
    /// `globals` 是 `attributes.ron` 那一份：**它的名字集合就是角色不能碰的名字集合**。
    pub fn load(
        source: &str,
        attributes: &AttributeCatalog,
        globals: &AttributeSetRon,
    ) -> Result<Self, ActorFileError> {
        let actors = parse(source).map_err(|error| ActorLoadError::Syntax(error.to_string()))?;
        validate(&actors, attributes, globals)?;
        Ok(Self { actors })
    }

    /// 把"全局定义 + 这个角色"拼成一套可直接铺在实体上的属性集。
    ///
    /// 顺序是先全局后角色；由于两者名字集合不相交（加载期保证），顺序不影响结果。
    pub fn build_set(&self, globals: &AttributeSetRon, actor: &ActorRon) -> ModifierSet {
        let mut set = globals
            .build_base()
            .expect("全局属性在加载期已经校验过；构造期不该再失败");
        for modifier in &actor.attributes {
            match (&modifier.literal, &modifier.expr) {
                (Some(literal), _) => set.add(&modifier.name, *literal),
                (None, Some(source)) => set.add_expr(&modifier.name, source),
                (None, None) => {}
            }
        }
        set
    }

    /// 按 id 取角色并拼出属性集（L2 生成实体的常用两步）。
    pub fn set_for(&self, globals: &AttributeSetRon, id: &str) -> Option<ModifierSet> {
        self.get(id).map(|actor| self.build_set(globals, actor))
    }
}

/// 解析 `.ron` 文本（与别的内容同一套 RON 选项）。
pub fn parse(source: &str) -> Result<Vec<ActorRon>, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
}

/// **一步到位**：两份文本 →（全局定义 / 属性台账 / 角色表）。
///
/// 顺序是内容本身的依赖方向，写错顺序只会在加载期报"引用了未登记的属性"，
/// 所以这里把三步封成一个入口，测试与 L2 都不必各写一遍。
pub fn load_from_sources(
    attributes_source: &str,
    actors_source: &str,
) -> Result<(AttributeSetRon, AttributeCatalog, ActorCatalog), ActorFileError> {
    let globals = crate::attributes::parse(attributes_source)
        .map_err(|error| ActorLoadError::Syntax(error.to_string()))?;
    let mut catalog = AttributeCatalog::default();
    catalog.add(&globals);
    let actors = ActorCatalog::load(actors_source, &catalog, &globals)?;
    Ok((globals, catalog, actors))
}

/// 加载角色定义时能犯的错。
#[derive(Debug, Clone, PartialEq)]
pub enum ActorLoadError {
    /// `.ron` 语法错误。
    Syntax(String),
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
    /// 某条属性本身不合法（值二选一 / 表达式编译失败 / 名字为空）。
    BadAttribute {
        /// 哪个角色。
        id: String,
        /// 底层错误（复用属性那一套）。
        error: AttributeLoadError,
    },
    /// 引用了没登记过的属性名。
    UnknownAttribute {
        /// 哪个角色。
        id: String,
        /// 引用的名字。
        name: String,
        /// 台账里到底有哪些名字。
        known: std::collections::BTreeSet<String>,
    },
    /// **与全局定义重名**：会累加而不是覆盖（见模块文档）。
    OverlapsGlobal {
        /// 哪个角色。
        id: String,
        /// 撞上的名字。
        name: String,
    },
}

impl std::fmt::Display for ActorLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "角色文件的语法错误：{message}"),
            Self::EmptyId { index } => write!(f, "第 {index} 个角色没有 id"),
            Self::DuplicateId { id } => write!(f, "角色 id `{id}` 重复"),
            Self::BadAttribute { id, error } => write!(f, "角色 `{id}` 的属性不合法：{error}"),
            Self::UnknownAttribute { id, name, known } => write!(
                f,
                "角色 `{id}` 引用了未登记的属性 `{name}`；已知属性：{known:?}"
            ),
            Self::OverlapsGlobal { id, name } => write!(
                f,
                "角色 `{id}` 写了 `{name}`，但它已经在 `attributes.ron` 里定义过——\
                 两边会**累加**（gauge 的 `+=`）而不是覆盖；角色只能写自己的数"
            ),
        }
    }
}

impl std::error::Error for ActorLoadError {}

/// 加载整份角色文件的错误类型（与单条校验同一个枚举，省一层包装）。
pub type ActorFileError = ActorLoadError;

/// 校验一批角色定义：id 唯一、属性登记过、**不与全局重名**。
fn validate(
    actors: &[ActorRon],
    attributes: &AttributeCatalog,
    globals: &AttributeSetRon,
) -> Result<(), ActorLoadError> {
    let global_names = globals.reserved_names();

    let mut seen = std::collections::BTreeSet::new();
    for (index, actor) in actors.iter().enumerate() {
        let id = actor.id.trim().to_string();
        if id.is_empty() {
            return Err(ActorLoadError::EmptyId { index });
        }
        if !seen.insert(id.clone()) {
            return Err(ActorLoadError::DuplicateId { id });
        }

        for modifier in &actor.attributes {
            if modifier.name.trim().is_empty() {
                return Err(ActorLoadError::BadAttribute {
                    id,
                    error: AttributeLoadError::EmptyName { index },
                });
            }
            if global_names.contains(modifier.name.trim()) {
                return Err(ActorLoadError::OverlapsGlobal {
                    id,
                    name: modifier.name.clone(),
                });
            }
            if !attributes.contains(&modifier.name) {
                return Err(ActorLoadError::UnknownAttribute {
                    id,
                    name: modifier.name.clone(),
                    known: attributes.names().clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份最小的全局定义 + 台账。
    fn globals_and_catalog() -> (AttributeSetRon, AttributeCatalog) {
        let globals = crate::attributes::parse(
            r#"(
                base: [
                    (name: "Constitution", literal: 0.0),
                    (name: "MaxHealth",    expr: "Constitution * 10.0"),
                ],
            )"#,
        )
        .expect("全局语法没问题");
        let mut catalog = AttributeCatalog::default();
        catalog.add(&globals);
        (globals, catalog)
    }

    #[test]
    fn writing_a_globally_defined_name_is_rejected() {
        // `MaxHealth` 已经在全局里定义过：角色再写会**累加**（10 倍体质 + 999），
        // 是个安静的错，所以加载期就拦。
        let (globals, catalog) = globals_and_catalog();
        let error = ActorCatalog::load(
            r#"[(id: "a", attributes: [(name: "MaxHealth", literal: 999.0)])]"#,
            &catalog,
            &globals,
        )
        .expect_err("该报错");
        assert!(
            matches!(error, ActorLoadError::OverlapsGlobal { .. }),
            "错误该说清是撞名：{error}"
        );
    }

    #[test]
    fn ids_must_be_unique_and_names_known() {
        let (globals, catalog) = globals_and_catalog();

        let duplicate = ActorCatalog::load(r#"[(id: "same"), (id: "same")]"#, &catalog, &globals)
            .expect_err("id 重复该报错");
        assert!(matches!(duplicate, ActorLoadError::DuplicateId { .. }));

        let unknown = ActorCatalog::load(
            r#"[(id: "a", attributes: [(name: "Luck", literal: 1.0)])]"#,
            &catalog,
            &globals,
        )
        .expect_err("没登记的名字该报错");
        assert!(matches!(unknown, ActorLoadError::UnknownAttribute { .. }));
    }
}
