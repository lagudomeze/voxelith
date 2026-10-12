//! 用 BSN 组合核心组件：证明确认过的组件形状真的能被 `bsn!` 表达。
//!
//! 这是测试，所以 `bevy_scene` 只在 `dev-dependencies` 里——运行时那一层不需要它。
//!
//! 这个文件同时验证一件容易忽略的事：**本 crate 的 prelude 与 `bevy_ecs::prelude` 能同时 glob 导入**。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ScenePlugin,
    ));
    app
}

/// 泛型 turbofish 与别名两条路都能走。
#[test]
fn enum_modifier_can_be_written_both_ways() {
    let mut app = test_app();
    let world = app.world_mut();

    let raw = world
        .spawn_scene(bsn! {
            #Raw
            Strength { base: 10.0 }
            Modifiers [
                Modifier::<StrengthStat>::Flat(Flat::<StrengthStat> { amount: 5.0 }),
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let aliased = world
        .spawn_scene(bsn! {
            #Aliased
            Strength { base: 10.0 }
            Modifiers [
                ModStrength::Flat(FlatStrength { amount: 5.0 }),
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let raw_value = world.get::<Strength>(raw).unwrap().resolved(world, raw);
    let aliased_value = world.get::<Strength>(aliased).unwrap().resolved(world, aliased);
    assert_eq!(raw_value, 15.0);
    assert_eq!(aliased_value, 15.0);
}

/// 整棵“角色 + 它的修正”都写在 `bsn!` 里——将来配置化就是这个形态。
///
/// 注意：`Modifiers [...]` 列表里**不写** `ModifiesOf(#Name)`。
/// `RelationshipTarget` 的 `Relationship` 关联类型让宏知道对方是谁并自动补上；
/// 手写一遍会得到 `Bundle has duplicate components`。
#[test]
fn hero_with_modifiers_can_be_written_entirely_in_bsn() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength { base: 10.0 }
            Health { current: 30.0, base_max: 60.0 }
            Modifiers [
                ModStrength::Flat(FlatStrength { amount: 5.0 }),
                ModStrength::Ratio(RatioStrength { value: 0.5 }),
                ModHealthMax::Flat(FlatHealthMax { amount: 20.0 }),
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    assert_eq!(world.get::<Modifiers>(hero).unwrap().len(), 3);

    let strength = world.get::<Strength>(hero).unwrap().resolved(world, hero);
    assert_eq!(strength, 20.0); // 10 + 5 + 10 × 0.5

    let health = world.get::<Health>(hero).unwrap();
    assert_eq!(health.resolved_max(world, hero), 80.0); // 60 + 20
    assert_eq!(health.current, 30.0, "当前值不走修正");
}
