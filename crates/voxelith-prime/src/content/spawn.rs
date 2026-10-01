//! 生成实体：把加载好的模板变成 ECS 实体（玩家与怪物）。
//!
//! 这里属于 L2（生成 / 编排），所以可以用完整 Bevy（`Name` 等），
//! 但**不写任何战斗数值**（**R17**）：池、属性、AI、阵营、特性全部来自内容文件。
//!
//! 玩家与怪物走**同一条**生成路径，差别只有 [`RoleRon`] 带来的两个东西：
//! 引擎角色标记（`Player` / `Monster`）与 `ActionEnergy`（只有自己 tick 的角色要）。

use bevy::prelude::*;
use voxelith_axiom::atoms::actor::{
    ActionEnergy, Actor, ActorState, ActorTags, Cooldowns, Monster, Player,
};
use voxelith_axiom::behaviors::content::ActorTemplate;
use voxelith_axiom::behaviors::content::descriptor::RoleRon;
use voxelith_axiom::behaviors::monster::MonsterDef;

use super::{make_pools, make_stats};

/// 演示用：按内容文件生成玩家与所有怪物。
pub fn spawn_demo(commands: &mut Commands, data: &crate::content::ContentData) {
    let content = data;
    // 角色顺序 = players 再 monsters：与 ctor_render 的精灵表行组**必须一致**。
    let mut index = 0_u32;
    for template in &content.players {
        let visual = crate::actor_render::intent_for(index, template.faction, template.role);
        spawn_actor(commands, template, content, visual);
        index += 1;
    }
    for template in &content.monsters {
        let visual = crate::actor_render::intent_for(index, template.faction, template.role);
        spawn_actor(commands, template, content, visual);
        index += 1;
    }
}

/// 生成一个角色实体（玩家 / 怪物共用）。
///
/// 内容给什么就挂什么：阵营、特性、池、属性、AI 全部来自模板。
/// 这里只做**装配**——没有"如果名字是 player 就怎样"的分支。
pub fn spawn_actor(
    commands: &mut Commands,
    template: &ActorTemplate,
    content: &crate::content::ContentData,
    visual: crate::actor_render::ActorVisualIntent,
) {
    // `Name` 用**显示名**（内容里的 `name`，如"冒险者"）：它会被 HUD 直接拿来当标题，
    // 而 `key`（`player`）是内容侧的稳定标识，是给数据用的、不是给人看的。
    let mut entity = commands.spawn((
        Name::new(template.name.clone()),
        Actor,
        // 阵营与特性：内容说了算（与引擎角色是正交的两件事）。
        template.faction,
        ActorTags(template.traits.clone()),
        make_pools(&template.resources, &content.pools),
        make_stats(&template.stats),
        Cooldowns::default(),
        ActorState::default(),
        // **渲染意图**（不含句柄）：具体的精灵由 ctor_render 建。
        visual,
    ));

    // 引擎角色：决定"谁听输入、谁自己 tick"。
    // 两个标记是**不同类型**的组件，所以分成两个分支各自 insert。
    match template.role {
        RoleRon::Player => {
            entity.insert(Player);
        }
        RoleRon::Monster => {
            entity.insert((
                Monster,
                ActionEnergy::new(template.energy_rate, template.energy_threshold),
                MonsterDef {
                    name: template.definition.name.clone(),
                    ai: template.definition.ai.clone(),
                },
            ));
        }
    }
}
