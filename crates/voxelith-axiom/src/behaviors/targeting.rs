//! 目标解析：`Targeting` 说的"打谁"怎么落到具体实体。
//!
//! 只有"寻找与判定"（**R59**），不含任何技能数值。
//! 为了可测与不锁死查询形态，敌人以**迭代器**传入（调用方给 `Query<Entity, With<Monster>>` 的迭代器）。

use bevy_ecs::prelude::*;

use crate::atoms::actor::Faction;
use crate::behaviors::phase::PendingThreat;
use crate::behaviors::requirement::Targeting;

/// 解析上下文：显式目标 + 威胁登记。
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetingContext {
    /// 请求里带的目标（`CurrentSelection` 用它）。
    pub explicit: Option<Entity>,
    /// 当前挂起的威胁。
    pub threat: Option<PendingThreat>,
}

/// 按 `Targeting` 解析出目标实体（解析不出来返回 `None`）。
///
/// `SelfOnly` / `ThreatSource` 有确定答案；`CurrentSelection` 在没给显式目标时退回"最近的敌人"；
/// `NearestEnemy` 只认敌人集合里的第一个（顺序稳定 = 可测；空间信息落地后换成真正的距离排序）。
pub fn resolve_target(
    targeting: Targeting,
    caster: Entity,
    ctx: &TargetingContext,
    enemies: impl Iterator<Item = Entity>,
) -> Option<Entity> {
    match targeting {
        Targeting::SelfOnly => Some(caster),
        Targeting::ThreatSource => ctx.threat.and_then(|threat| threat.source),
        Targeting::CurrentSelection => match ctx.explicit {
            Some(target) if target != caster => Some(target),
            _ => nearest_enemy(caster, enemies),
        },
        Targeting::NearestEnemy => nearest_enemy(caster, enemies),
    }
}

/// 最近的敌人：当前只按"第一个非自己的敌人"给确定答案。
fn nearest_enemy(caster: Entity, enemies: impl Iterator<Item = Entity>) -> Option<Entity> {
    enemies.into_iter().find(|&enemy| enemy != caster)
}

/// "谁能当我的敌人"的**唯一定义**：阵营敌对。
///
/// 对同一个阵营问两次（"谁敌对"与"谁被打"）在语义上会漂移，
/// 所以两边都走这里：发起攻击时瞄谁、以及"这招现在有没有可用目标"。
pub fn hostile_to<'w, 's>(
    factions: &'w Query<'s, 's, (Entity, &'static Faction)>,
    caster: Entity,
) -> impl Iterator<Item = Entity> + use<'w, 's> {
    let caster_faction = faction_of(factions, Some(caster));
    factions.iter().filter_map(move |(entity, faction)| {
        let hostile =
            entity != caster && caster_faction.is_some_and(|own| own.hostile_to(*faction));
        hostile.then_some(entity)
    })
}

/// 取某个实体的阵营（查不到或没给实体就是 `None`，判定时保守处理）。
pub fn faction_of(
    factions: &Query<(Entity, &'static Faction)>,
    entity: Option<Entity>,
) -> Option<Faction> {
    let entity = entity?;
    factions.get(entity).ok().map(|(_, faction)| *faction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviors::phase::PendingThreat;

    fn world_with_monsters() -> (World, Entity, Entity) {
        let mut world = World::new();
        let caster = world.spawn_empty().id();
        let monster = world.spawn(crate::atoms::actor::Monster).id();
        (world, caster, monster)
    }

    fn monsters(world: &mut World) -> impl Iterator<Item = Entity> + use<> {
        let entities: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::atoms::actor::Monster>>()
            .iter(world)
            .collect();
        entities.into_iter()
    }

    #[test]
    fn self_only_always_points_at_the_caster() {
        let (mut world, caster, _) = world_with_monsters();
        let ctx = TargetingContext::default();
        assert_eq!(
            resolve_target(Targeting::SelfOnly, caster, &ctx, monsters(&mut world)),
            Some(caster)
        );
    }

    #[test]
    fn threat_source_wins_over_the_explicit_target() {
        let (mut world, caster, monster) = world_with_monsters();
        let other = world.spawn_empty().id();
        let ctx = TargetingContext {
            explicit: Some(other),
            threat: Some(PendingThreat {
                action: None,
                source: Some(monster),
                target: Some(caster),
            }),
        };
        assert_eq!(
            resolve_target(Targeting::ThreatSource, caster, &ctx, monsters(&mut world)),
            Some(monster)
        );
    }

    #[test]
    fn current_selection_falls_back_to_an_enemy() {
        let (mut world, caster, monster) = world_with_monsters();

        let with_selection = TargetingContext {
            explicit: Some(monster),
            threat: None,
        };
        assert_eq!(
            resolve_target(
                Targeting::CurrentSelection,
                caster,
                &with_selection,
                monsters(&mut world)
            ),
            Some(monster)
        );

        let without_selection = TargetingContext::default();
        assert_eq!(
            resolve_target(
                Targeting::CurrentSelection,
                caster,
                &without_selection,
                monsters(&mut world)
            ),
            Some(monster),
            "没选目标时退回最近的敌人"
        );
    }

    #[test]
    fn no_enemy_resolves_to_nothing() {
        let mut world = World::new();
        let caster = world.spawn_empty().id();
        let ctx = TargetingContext::default();
        assert_eq!(
            resolve_target(Targeting::NearestEnemy, caster, &ctx, monsters(&mut world)),
            None
        );
        assert_eq!(
            resolve_target(Targeting::ThreatSource, caster, &ctx, monsters(&mut world)),
            None
        );
    }
}
