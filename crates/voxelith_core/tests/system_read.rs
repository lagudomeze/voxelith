//! 在真正的 system 里怎么读一个可修正的量。
//!
//! 这个文件顺带把“**修正怎么挂由使用者定**”这件事演示清楚：这里定义**两套**挂法——
//! 装备槽与增益列表——然后两个系统喂给同一个公式的集合不一样：
//!
//! - 面板系统只看装备；
//! - 结算系统看装备 + 增益。
//!
//! 这正是 `voxelith_core` 不该定死一种挂法的原因：**哪些修正算数，是系统的事**。
//!
//! 另外，读的时候**手里已经有组件和关联列表了**（`&Gear` / `&Buffs`），不需要回头
//! 去 `World` 里捞——所以也没有 `resolved(world, owner)` 这种东西。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;

// ==================== 应用侧自己定的两套挂法 ====================

/// 装备带来的修正。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Gear)]
struct FromGear(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = FromGear, linked_spawn)]
struct Gear(Vec<Entity>);

/// 增益带来的修正。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Buffs)]
struct FromBuff(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = FromBuff, linked_spawn)]
struct Buffs(Vec<Entity>);

// ==================== 系统 ====================

/// 两个系统各算出什么，记在这里。
#[derive(Resource, Default, PartialEq, Debug)]
struct Reported {
    /// 只看装备（面板上显示的“装备加成后”）。
    equipment_only: f32,
    /// 装备 + 增益（实际结算用的）。
    effective: f32,
    /// 血量上限（装备加的上限）。
    max_hp: f32,
}

fn setup(world: &mut World) {
    let hero = world
        .spawn((
            Strength(10.0),
            Health {
                current: 30.0,
                base_max: 60.0,
            },
        ))
        .id();

    // 装备：+5 力量、+20 血量上限
    world.spawn((FromGear(hero), ModStrength::flat(5.0)));
    world.spawn((FromGear(hero), ModHealthMax::flat(20.0)));
    // 增益：+2 力量（结算时才算）
    world.spawn((FromBuff(hero), ModStrength::flat(2.0)));
}

fn report(
    heroes: Query<(&Strength, &Health, &Gear, &Buffs)>,
    strength_mods: Query<&Modifier<Strength>>,
    health_mods: Query<&Modifier<Max<Health>>>,
    mut reported: ResMut<Reported>,
) {
    for (strength, health, gear, buffs) in &heroes {
        // 只看装备
        reported.equipment_only = strength.resolve(
            gear.0
                .iter()
                .filter_map(|m| strength_mods.get(*m).ok()),
        );

        // 装备 + 增益：两条链接在一起
        let all = gear
            .0
            .iter()
            .chain(buffs.0.iter())
            .filter_map(|m| strength_mods.get(*m).ok());
        reported.effective = strength.resolve(all);

        // 资源上限：这里只算装备
        reported.max_hp = health.resolve_max(
            gear.0
                .iter()
                .filter_map(|m| health_mods.get(*m).ok()),
        );
    }
}

#[test]
fn a_system_reads_values_straight_from_its_queries() {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.init_resource::<Reported>();
    app.add_systems(Startup, setup);
    app.add_systems(Update, report);
    app.update();

    let reported = app.world().get_resource::<Reported>().unwrap();
    assert_eq!(
        *reported,
        Reported {
            equipment_only: 15.0, // 10 + 5（只有装备）
            effective: 17.0,      // 10 + 5 + 2（装备 + 增益）
            max_hp: 80.0,         // 60 + 20
        }
    );
}
