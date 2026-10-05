//! **词汇表描述**：`vocabulary.ron` 的样子。
//!
//! 五种词汇各一个条目结构。它们只是数据——字符串一律保持字符串，
//! 由 [`super::super::loader`] 在加载期解析成运行时 ID。

/// serde 默认值工厂（start_full 默认满池）。
fn default_true() -> bool {
    true
}
/// `vocabulary.ron`：资源池 / 属性 / 状态 / 特性的名字定义。
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct VocabRon {
    /// 资源池。
    pub resources: Vec<ResourceRon>,
    /// 属性。
    pub stats: Vec<StatRon>,
    /// 状态。
    pub statuses: Vec<StatusDefRon>,
    /// 角色**特性**（亡灵 / 构装体 / 野兽……）。
    pub tags: Vec<TagRon>,
}

/// 一种角色特性（亡灵 / 构装体 / 野兽……）的词汇条目。
///
/// 只有"名字"和"显示名"：特性不带数值，判定就是"有没有这个标签"。
/// 一旦某个特性需要程度（"亡灵抗性 30%"），它就该升级成属性（`StatRon`），
/// 而不是往这里加字段。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TagRon {
    /// 稳定名字（`"undead"`）。
    pub id: String,
    /// 显示名。
    pub name: String,
}

/// 一种资源池的词汇条目。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ResourceRon {
    /// 稳定名字（`"hp"`）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 上限。
    pub max: f32,
    /// 每秒自然恢复。
    #[serde(default)]
    pub regen: f32,
    /// 生成时是否满池（默认满）。
    #[serde(default = "default_true")]
    pub start_full: bool,
}

/// 一个属性的词汇条目。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct StatRon {
    /// 稳定名字（`"strength"`）。
    pub id: String,
    /// 显示名。
    pub name: String,
}

/// 一种状态的词汇条目（`vocabulary.ron` 里只有名字）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct StatusDefRon {
    /// 稳定名字（`"stunned"`）。
    pub id: String,
    /// 显示名。
    pub name: String,
}
