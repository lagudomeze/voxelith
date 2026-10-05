//! 词汇表（**Resource**）：把配置里的**字符串**映射成运行时的**词汇 ID**。
//!
//! 五张表（池 / 属性 / 状态 / 技能 / **特性**），每张一张 [`NameTable`]：
//!
//! | ID | 表 | 谁用 |
//! |---|---|---|
//! | [`ResourceId`] | `resources` | 池（HP / 法力 / 行动 / 反应） |
//! | [`StatId`] | `stats` | 属性（力量 / 护甲 / …） |
//! | [`StatusId`] | `statuses` | 状态（格挡 / 眩晕 / 中毒 / 狂暴） |
//! | [`SkillId`] | `skills` | 技能（在 `skills.ron` 里按出现顺序编号） |
//! | [`ActorTagId`] | `tags` | 角色**特性**（亡灵 / 构装体 / 野兽……） |
//!
//! **字符串永远不进热路径**：加载期解析一次，之后运行时只跑 ID。
//! 数量上限由 `u16` 给出（65535 种），远超内容规模。
//!
//! ID 类型与 [`NameTable`] 本身住在 L0（[`crate::atoms::vocabulary`]）：它们是纯数据，
//! 而 `Resources` / `Cooldowns` / `Stats` 拿它们当字段类型——L0 不该反过来依赖 L1。
//! 这里只留**聚合与解析**（[`Vocab`]），因为它绑的是"内容"这件事。

use bevy_ecs::prelude::*;

// 词汇原子从 L0 转出：`behaviors::content::ResourceId` 这类老路径照旧可用。
pub use crate::atoms::vocabulary::{
    ActorTagId, NameTable, ResourceId, SkillId, StatId, StatusId, UnknownName,
};

/// 词汇表（**Resource**）：五张表 + 反查用的名字表。
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
    /// 角色**特性**（亡灵 / 构装体 / 野兽……）。
    pub tags: NameTable,
    resource_names: Vec<String>,
    stat_names: Vec<String>,
    status_names: Vec<String>,
    skill_names: Vec<String>,
    tag_names: Vec<String>,
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

    /// 登记一种角色特性。
    pub fn add_tag(&mut self, name: &str) -> ActorTagId {
        let id = self.tags.register(name);
        push_name(&mut self.tag_names, id, name);
        ActorTagId(id)
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

    /// 解析角色特性名。
    pub fn tag(&self, name: &str) -> Result<ActorTagId, UnknownName> {
        self.tags
            .id(name)
            .map(ActorTagId)
            .ok_or_else(|| UnknownName::new("tag", name))
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

    /// 角色特性名。
    pub fn tag_name(&self, id: ActorTagId) -> &str {
        name_of(&self.tag_names, id.0)
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
