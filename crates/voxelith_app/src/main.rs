//! 装配层：把 `voxelith_core` 的模型挂到一个 Bevy App 上。
//!
//! 现在**还没有可跑的东西**（没有渲染、没有输入、没有时间推进），所以这里只做
//! “装配 + 自检”：造一个英雄、读出它的最终属性与血量上限，然后退出。
//! 真正的循环等有东西可跑时再接上。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;

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
            Modifiers [
                ModStrength::Flat(FlatStrength { amount: 5.0 }),
                ModStrength::Ratio(RatioStrength { value: 0.5 }),
                ModHealthMax::Flat(FlatHealthMax { amount: 20.0 }),
            ]
        })
        .expect("装配英雄失败")
        .id();

    let modifier_count = world.get::<Modifiers>(hero).map(|m| m.len()).unwrap_or(0);
    let strength = world.get::<Strength>(hero).unwrap().resolved(world, hero);
    let constitution = world
        .get::<Constitution>(hero)
        .unwrap()
        .resolved(world, hero);
    let health = world.get::<Health>(hero).unwrap();
    let max_hp = health.resolved_max(world, hero);

    println!("英雄 {hero:?}（{modifier_count} 条修正）");
    println!("  力量 {strength}　基础 10 ＋ 5 平加 ＋ 50% 比例");
    println!("  体质 {constitution}　无修正");
    println!(
        "  血量 {} / {max_hp}　上限 = 基础 60 ＋ 20 平加；当前值不走修正",
        health.current
    );
}
