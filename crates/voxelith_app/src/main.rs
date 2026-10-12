//! 装配层：把 `voxelith_core` 的模型挂到一个 Bevy App 上。
//!
//! 现在**还没有可跑的东西**（没有渲染、没有输入、没有时间推进），所以这里只做
//! “装配 + 自检”：造一个英雄、读出它的最终属性与血量上限，然后退出。
//! 真正的循环等有东西可跑时再接上。
//!
//! 注意 [`Equipment`]：**“修正怎么挂到主人身上”是应用侧的事**（core 不管，理由见
//! `voxelith_core::value` 顶部）。这里用的是一份自定义 relationship——`linked_spawn`
//! 让“卸下 = 销毁修正实体”自动成立，而且因为它是 `RelationshipTarget`，
//! `bsn!` 里能直接写成列表。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;

/// 这条修正挂在哪个主人身上。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Equipment)]
struct EquippedOn(Entity);

/// 主人身上自动维护的修正列表。
#[derive(Component, Default)]
#[relationship_target(relationship = EquippedOn, linked_spawn)]
struct Equipment(Vec<Entity>);

fn main() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ScenePlugin,
    ));
    app.add_systems(Startup, setup_hero);
    app.update();
}

/// 一个英雄：两项属性、两条资源、三条修正——全部写在 `bsn!` 里。
fn setup_hero(world: &mut World) {
    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Constitution(12.0)
            Health { current: 30.0, base_max: 60.0 }
            Stamina { current: 20.0, base_max: 40.0 }
            Equipment [
                // 一条修正同时带平加和比例
                ModStrength { flat: 3.0, ratio: 0.5 },
                ModStrength { flat: -2.0 },
                ModHealthMax { flat: 20.0 },
            ]
        })
        .expect("装配英雄失败")
        .id();

    let equipment = world.get::<Equipment>(hero).unwrap();
    let strength_mods = equipment
        .0
        .iter()
        .filter_map(|m| world.get::<Modifier<Strength>>(*m))
        .cloned()
        .collect::<Vec<_>>();
    let health_mods = equipment
        .0
        .iter()
        .filter_map(|m| world.get::<Modifier<Max<Health>>>(*m))
        .cloned()
        .collect::<Vec<_>>();

    let health = world.get::<Health>(hero).unwrap();
    let strength = world
        .get::<Strength>(hero)
        .unwrap()
        .resolve(strength_mods.iter());
    let constitution = world
        .get::<Constitution>(hero)
        .unwrap()
        .resolve(std::iter::empty());
    let max_hp = health.resolve_max(health_mods.iter());

    println!("英雄 {hero:?}（{} 条修正）", equipment.0.len());
    println!("  力量 {strength}　基础 10 ＋3 －2 ＋ 50% 比例（＋3 与 ＋50% 在同一条修正上）");
    println!("  体质 {constitution}　无修正");
    println!(
        "  血量 {} / {max_hp}　上限 = 基础 60 ＋ 20 平加；当前值不走修正",
        health.current()
    );
}
