//! 词汇表（**Resource**）：把配置里的**字符串**映射成运行时的**词汇 ID**。
//!
//! 四个 ID 都是 `u16` newtype，对应四张表：
//!
//! | ID | 表 | 谁用 |
//! |---|---|---|
//! | [`ResourceId`] | `resources` | 池（HP / 法力 / 行动 / 反应） |
//! | [`StatId`] | `stats` | 属性（力量 / 护甲 / …） |
//! | [`StatusId`] | `statuses` | 状态（格挡 / 眩晕 / 中毒 / 狂暴） |
//! | [`SkillId`] | `skills` | 技能（在 `skills.ron` 里按出现顺序编号） |
//!
//! **字符串永远不进热路径**：加载期解析一次，之后运行时只跑 ID。
//! 数量上限由 `u16` 给出（65535 种），远超内容规模。

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

macro_rules! vocab_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        // Reflect 是为了**调试通道**（BRP）：没有它，Resources / Stats 这些
        // 组件的字段在 world.query 里读不出来（**静默跳过，不报错**）。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
        pub struct $name(pub u16);
    };
}

vocab_id!(ResourceId, "资源池标识（HP / 法力 / 行动 / 反应……）。");
vocab_id!(StatId, "属性标识（力量 / 护甲 / 敏捷……）。");
vocab_id!(StatusId, "状态标识（格挡 / 眩晕 / 中毒 / 狂暴……）。");
vocab_id!(SkillId, "技能标识（按 `skills.ron` 的出现顺序编号）。");

/// 一张「名字 → ID」表。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameTable {
    by_name: HashMap<String, u16>,
}

impl NameTable {
    /// 查 ID。
    pub fn id(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied()
    }

    /// 已登记的名字数量。
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// 按名字遍历（加载期报错时用来列清单）。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// 是否已经登记过这个名字。
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// 登记一个名字，返回分配到的下标（重复登记返回**已存在**的下标）。
    pub fn register(&mut self, name: &str) -> u16 {
        let next = self.by_name.len() as u16;
        *self.by_name.entry(name.to_owned()).or_insert(next)
    }
}

/// 词汇表（**Resource**）：四张表 + 反查用的名字表。
///
/// 反查（ID → 名字）用 `Vec<String>`：ID 就是下标，日志与 UI 都直接读它。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Vocab {
    /// 资源池。
    pub resources: NameTable,
    /// 属性。
    pub stats: NameTable,
    /// 状态。
    pub statuses: NameTable,
    /// 技能。
    pub skills: NameTable,
    resource_names: Vec<String>,
    stat_names: Vec<String>,
    status_names: Vec<String>,
    skill_names: Vec<String>,
}

impl Vocab {
    /// 登记一种资源池。
    pub fn add_resource(&mut self, name: &str) -> ResourceId {
        let id = self.resources.register(name);
        push_name(&mut self.resource_names, id, name);
        ResourceId(id)
    }

    /// 登记一个属性。
    pub fn add_stat(&mut self, name: &str) -> StatId {
        let id = self.stats.register(name);
        push_name(&mut self.stat_names, id, name);
        StatId(id)
    }

    /// 登记一种状态。
    pub fn add_status(&mut self, name: &str) -> StatusId {
        let id = self.statuses.register(name);
        push_name(&mut self.status_names, id, name);
        StatusId(id)
    }

    /// 登记一个技能。
    pub fn add_skill(&mut self, name: &str) -> SkillId {
        let id = self.skills.register(name);
        push_name(&mut self.skill_names, id, name);
        SkillId(id)
    }

    /// 解析资源名。
    pub fn resource(&self, name: &str) -> Result<ResourceId, UnknownName> {
        self.resources
            .id(name)
            .map(ResourceId)
            .ok_or_else(|| UnknownName::new("resource", name))
    }

    /// 解析属性名。
    pub fn stat(&self, name: &str) -> Result<StatId, UnknownName> {
        self.stats
            .id(name)
            .map(StatId)
            .ok_or_else(|| UnknownName::new("stat", name))
    }

    /// 解析状态名。
    pub fn status(&self, name: &str) -> Result<StatusId, UnknownName> {
        self.statuses
            .id(name)
            .map(StatusId)
            .ok_or_else(|| UnknownName::new("status", name))
    }

    /// 解析技能名。
    pub fn skill(&self, name: &str) -> Result<SkillId, UnknownName> {
        self.skills
            .id(name)
            .map(SkillId)
            .ok_or_else(|| UnknownName::new("skill", name))
    }

    /// 资源名（日志 / UI）。
    pub fn resource_name(&self, id: ResourceId) -> &str {
        name_of(&self.resource_names, id.0)
    }

    /// 属性名。
    pub fn stat_name(&self, id: StatId) -> &str {
        name_of(&self.stat_names, id.0)
    }

    /// 状态名。
    pub fn status_name(&self, id: StatusId) -> &str {
        name_of(&self.status_names, id.0)
    }

    /// 技能名。
    pub fn skill_name(&self, id: SkillId) -> &str {
        name_of(&self.skill_names, id.0)
    }
}

fn push_name(names: &mut Vec<String>, id: u16, name: &str) {
    let index = id as usize;
    if names.len() <= index {
        names.resize(index + 1, String::new());
    }
    names[index] = name.to_owned();
}

fn name_of(names: &[String], id: u16) -> &str {
    names.get(id as usize).map_or("<unnamed>", String::as_str)
}

/// 词汇表里没有这个名字（加载期错误，见 [`super::LoaderError`]）。
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, derive_more::Error)]
#[display("unknown {kind} name `{name}` (not declared in vocabulary.ron)")]
pub struct UnknownName {
    /// 哪张表（resource / stat / status / skill）。
    pub kind: &'static str,
    /// 未登记的名字。
    pub name: String,
}

impl UnknownName {
    /// 构造。
    pub fn new(kind: &'static str, name: &str) -> Self {
        Self {
            kind,
            name: name.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_assigned_in_registration_order() {
        let mut vocab = Vocab::default();
        assert_eq!(vocab.add_resource("hp"), ResourceId(0));
        assert_eq!(vocab.add_resource("mana"), ResourceId(1));
        assert_eq!(vocab.add_resource("hp"), ResourceId(0), "重复登记不新开槽");
        assert_eq!(vocab.resources.len(), 2);
    }

    #[test]
    fn every_table_is_independent() {
        let mut vocab = Vocab::default();
        let hp = vocab.add_resource("hp");
        let hp_stat = vocab.add_stat("hp");
        assert_eq!(hp, ResourceId(0));
        assert_eq!(hp_stat, StatId(0), "两张表各自从 0 开始");
    }

    #[test]
    fn lookup_round_trips_through_names() {
        let mut vocab = Vocab::default();
        let armor = vocab.add_stat("armor");
        assert_eq!(vocab.stat("armor"), Ok(armor));
        assert_eq!(vocab.stat_name(armor), "armor");
        assert_eq!(vocab.stat("nope"), Err(UnknownName::new("stat", "nope")));
    }

    #[test]
    fn unknown_names_report_which_table() {
        let vocab = Vocab::default();
        assert_eq!(vocab.resource("x").unwrap_err().kind, "resource");
        assert_eq!(vocab.status("x").unwrap_err().kind, "status");
        assert_eq!(vocab.skill("x").unwrap_err().kind, "skill");
    }
}
