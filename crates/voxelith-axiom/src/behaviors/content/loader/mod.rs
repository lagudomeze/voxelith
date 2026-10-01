//! 加载管线：**字符串 → 词汇 ID**，解析发生一次，运行时只跑 ID 版本。
//!
//! ```text
//! vocabulary.ron ─► Vocab（name ⇄ id 双向）
//! skills.ron     ─► 解析字符串 → Skill      ─► spawn 实体 ─► SkillCatalog
//! statuses.ron   ─► 解析字符串 → StatusDef  ─► spawn 实体 ─► StatusCatalog
//! actors 的 .ron    ─► 解析字符串 → 角色模板（L2 自己 spawn 实体）
//! ```
//!
//! **加载期覆盖检查**（漏配不会静默跑起来）：
//!
//! - 技能 / 状态里引用的池、属性、状态、技能名必须存在 → [`LoaderError::UnknownName`]；
//! - 词汇表里声明了池，但没有任何技能 / 状态引用它 → [`LoaderError::UnusedResource`]。
//!
//! 本模块**不读文件**（R101 精神）：描述结构由 L2 反序列化后传进来。

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;

use crate::behaviors::content::catalog::{SkillCatalog, StatusCatalog};
use crate::behaviors::content::descriptor::{
    ActorRon, RequirementRon, SkillRon, StatusRon, VocabRon,
};
use crate::behaviors::content::descriptor_world::WorldRon;
use crate::behaviors::content::vocabulary::{ResourceId, StatId, StatusId, Vocab};

mod resolve;
mod resolve_defs;
mod types;

pub use resolve::{
    collect_resources, resolve_condition, resolve_contest, resolve_effect, resolve_outcome,
    resolve_requirement, resolve_value, resolve_who,
};
pub use types::{ActorTemplate, LoadedContent, LoaderError, PoolTemplate, WorldTemplate};

use resolve_defs::{resolve_actors, resolve_skill, resolve_status, resolve_world};

/// 从六份描述加载全部内容（并 spawn 技能 / 状态的定义实体）。
///
/// `actors_ron` 同时承载玩家与怪物：`role` 决定它归到哪一组。
/// `world_ron` 是体素世界（地形参数 + 方块表），与角色内容互不影响。
#[allow(clippy::too_many_arguments)]
pub fn load_all(
    commands: &mut Commands,
    vocab_ron: &VocabRon,
    skills_ron: &[SkillRon],
    statuses_ron: &[StatusRon],
    actors_ron: &[ActorRon],
    world_ron: &WorldRon,
) -> Result<LoadedContent, LoaderError> {
    let mut vocab = build_vocab(vocab_ron);
    let skills = load_skills(commands, &mut vocab, skills_ron)?;
    let statuses = load_statuses(commands, &vocab, statuses_ron)?;

    // 覆盖检查：声明了却没人引用的池。
    let used = collect_used_resources(skills_ron, statuses_ron, &vocab)?;
    for name in declared_resource_names(&vocab) {
        if !used.contains(&vocab.resource(name)?) {
            return Err(LoaderError::UnusedResource {
                name: name.to_owned(),
            });
        }
    }

    let (players, monsters) = resolve_actors(&vocab, actors_ron)?;
    let world = resolve_world(world_ron);

    Ok(LoadedContent {
        pools: pool_templates(&vocab, vocab_ron)?,
        vocab,
        skills,
        statuses,
        players,
        monsters,
        world,
    })
}

/// 构造词汇表（池 → 属性 → 状态，各自按文件顺序编号）。
pub fn build_vocab(ron: &VocabRon) -> Vocab {
    let mut vocab = Vocab::default();
    for resource in &ron.resources {
        vocab.add_resource(&resource.id);
    }
    for stat in &ron.stats {
        vocab.add_stat(&stat.id);
    }
    for status in &ron.statuses {
        vocab.add_status(&status.id);
    }
    vocab
}

/// 登记技能名并解析技能定义（先有名字，效果才能引用技能）。
fn load_skills(
    commands: &mut Commands,
    vocab: &mut Vocab,
    ron: &[SkillRon],
) -> Result<SkillCatalog, LoaderError> {
    for skill in ron {
        vocab.add_skill(&skill.id);
    }

    let mut catalog = SkillCatalog::default();
    for skill in ron {
        let id = vocab.skill(&skill.id)?;
        let definition = resolve_skill(skill, id, vocab)?;
        let entity = commands.spawn(definition).id();
        catalog.insert(id, entity);
    }
    Ok(catalog)
}

/// 解析状态定义。
fn load_statuses(
    commands: &mut Commands,
    vocab: &Vocab,
    ron: &[StatusRon],
) -> Result<StatusCatalog, LoaderError> {
    let mut catalog = StatusCatalog::default();
    for status in ron {
        let id = vocab.status(&status.id)?;
        let definition = resolve_status(status, id, vocab)?;
        let entity = commands.spawn(definition).id();
        catalog.insert(id, entity);
    }
    Ok(catalog)
}

/// 词汇表里声明的池名（按 ID 排序 = 文件顺序）。
fn declared_resource_names(vocab: &Vocab) -> Vec<&str> {
    let mut all: Vec<(u16, &str)> = vocab
        .resources
        .names()
        .filter_map(|name| vocab.resources.id(name).map(|id| (id, name)))
        .collect();
    all.sort_by_key(|(id, _)| *id);
    all.into_iter().map(|(_, name)| name).collect()
}

/// 池的生成参数。
fn pool_templates(
    vocab: &Vocab,
    ron: &VocabRon,
) -> Result<HashMap<ResourceId, PoolTemplate>, LoaderError> {
    let mut out = HashMap::new();
    for resource in &ron.resources {
        out.insert(
            vocab.resource(&resource.id)?,
            PoolTemplate {
                label: resource.name.clone(),
                max: resource.max,
                regen: resource.regen,
                start_full: resource.start_full,
            },
        );
    }
    Ok(out)
}

/// 收集内容里真正用到的池。
fn collect_used_resources(
    skills: &[SkillRon],
    statuses: &[StatusRon],
    vocab: &Vocab,
) -> Result<HashSet<ResourceId>, LoaderError> {
    let mut used = HashSet::new();
    for skill in skills {
        for cost in &skill.costs {
            used.insert(vocab.resource(&cost.pool)?);
        }
        for requirement in &skill.requirements {
            if let RequirementRon::Resource { pool, .. } = requirement {
                used.insert(vocab.resource(pool)?);
            }
        }
        for effect in &skill.effects {
            resolve::collect_resources(effect, vocab, &mut used)?;
        }
    }
    for status in statuses {
        for effect in status
            .on_apply
            .iter()
            .chain(&status.on_tick)
            .chain(&status.on_expire)
            .chain(&status.on_remove)
        {
            resolve::collect_resources(effect, vocab, &mut used)?;
        }
    }
    Ok(used)
}

/// 池的显示名，按**词汇 ID** 索引（日志 / UI 用）。
///
/// 为什么按 ID 而不是按名字：运行时只有 ID（`Pool` 存在 `HashMap<ResourceId, _>` 里），
/// 名字在加载期就该被用完。按名字索引的表在运行时要先"把 ID 翻回名字"才能查——多一次
/// 字符串查表，而且容易写错（`Vocab::resource_name` 给的是 RON 里的 `id`，**不是** `name`）。
///
/// 顺序沿用 `vocabulary.ron`（`Vocab` 的编号就是文件顺序），所以这里的下标直接当 ID 用。
pub fn resource_labels(ron: &VocabRon) -> HashMap<ResourceId, String> {
    ron.resources
        .iter()
        .enumerate()
        .map(|(index, resource)| (ResourceId(index as u16), resource.name.clone()))
        .collect()
}

/// 属性的显示名，按词汇 ID 索引。
pub fn stat_labels(ron: &VocabRon) -> HashMap<StatId, String> {
    ron.stats
        .iter()
        .enumerate()
        .map(|(index, stat)| (StatId(index as u16), stat.name.clone()))
        .collect()
}

/// 状态的显示名，按词汇 ID 索引。
pub fn status_labels(ron: &VocabRon) -> HashMap<StatusId, String> {
    ron.statuses
        .iter()
        .enumerate()
        .map(|(index, status)| (StatusId(index as u16), status.name.clone()))
        .collect()
}
// ------------------------------------------------------------------ 测试

/// 单元测试（体量大于被测代码，单独成文件；见 `tests.rs`）。
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
