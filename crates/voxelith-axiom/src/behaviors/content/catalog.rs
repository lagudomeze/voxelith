//! 目录（**Resource**）：词汇 ID → **定义实体**。
//!
//! 定义是全局共享的模板实体（一个技能一个实体、一种状态一个实体）；
//! 运行时只拿 ID 查实体，字符串只活在加载期。

use std::collections::HashMap;

use bevy_ecs::prelude::*;

use crate::behaviors::content::{SkillId, StatusId};

/// 技能目录：`SkillId → Skill 实体`。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    by_id: HashMap<SkillId, Entity>,
}

impl SkillCatalog {
    /// 登记一个技能定义。
    pub fn insert(&mut self, id: SkillId, entity: Entity) {
        self.by_id.insert(id, entity);
    }

    /// 查定义实体。
    pub fn get(&self, id: SkillId) -> Option<&Entity> {
        self.by_id.get(&id)
    }

    /// 查定义实体（可变）。
    pub fn get_mut(&mut self, id: SkillId) -> Option<&mut Entity> {
        self.by_id.get_mut(&id)
    }

    /// 已登记数量。
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// 遍历（加载期校验用）。
    pub fn iter(&self) -> impl Iterator<Item = (SkillId, Entity)> + '_ {
        self.by_id.iter().map(|(&id, &entity)| (id, entity))
    }
}

/// 状态目录：`StatusId → StatusDef 实体`。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusCatalog {
    by_id: HashMap<StatusId, Entity>,
}

impl StatusCatalog {
    /// 登记一个状态定义。
    pub fn insert(&mut self, id: StatusId, entity: Entity) {
        self.by_id.insert(id, entity);
    }

    /// 查定义实体。
    pub fn get(&self, id: StatusId) -> Option<&Entity> {
        self.by_id.get(&id)
    }

    /// 已登记数量。
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// 遍历。
    pub fn iter(&self) -> impl Iterator<Item = (StatusId, Entity)> + '_ {
        self.by_id.iter().map(|(&id, &entity)| (id, entity))
    }
}
