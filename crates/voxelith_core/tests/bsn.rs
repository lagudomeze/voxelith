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

#[test]
fn generic_turbofish_works_in_bsn() {
    let mut app = test_app();
    let world = app.world_mut();

    let e = world
        .spawn_scene(bsn! {
            #A
            Strength { base: 10.0 }
            Flat::<StrengthStat> { amount: 5.0 }
        })
        .expect("spawn_scene 失败")
        .id();

    assert_eq!(world.get::<Strength>(e).unwrap().base, 10.0);
    assert_eq!(world.get::<Flat<StrengthStat>>(e).unwrap().amount, 5.0);
}

#[test]
fn alias_works_in_bsn() {
    let mut app = test_app();
    let world = app.world_mut();

    let e = world
        .spawn_scene(bsn! {
            #B
            Strength { base: 20.0 }
            FlatStrength { amount: 3.0 }
        })
        .expect("spawn_scene 失败")
        .id();

    assert_eq!(world.get::<Strength>(e).unwrap().base, 20.0);
    assert_eq!(world.get::<FlatStrength>(e).unwrap().amount, 3.0);
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
                FlatStrength { amount: 5.0 },
                RatioStrength { value: 0.5 },
                FlatHealthMax { amount: 20.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    assert_eq!(world.get::<Modifiers>(hero).unwrap().len(), 3);
    assert_eq!(resolve_value::<StrengthStat>(world, hero), Some(20.0));
    assert_eq!(resolve_max::<HealthMax>(world, hero), Some(80.0));
    // 当前值不走修正
    assert_eq!(world.get::<Health>(hero).unwrap().current, 30.0);
}
