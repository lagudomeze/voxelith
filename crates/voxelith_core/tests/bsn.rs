//! **应用侧怎么挂修正** + 用 BSN 写出来。
//!
//! `voxelith_core` 不管“修正怎么挂到主人身上”（见 `value.rs` 顶部那段说明），
//! 所以这里演示一份自己定的挂法——**和 core 之前那版一模一样，只是现在住在应用侧**：
//!
//! - 一个自定义 relationship（[`EquippedOn`] / [`Equipment`]）把修正实体指回主人；
//! - `linked_spawn` 让它自动清理（卸下即销毁）；
//! - 因为它是一个 `RelationshipTarget`，`bsn!` 里能直接写成列表 `Equipment [ … ]`。
//!
//! 这个文件同时验证一件容易忽略的事：**本 crate 的 prelude 与 `bevy_ecs::prelude` 能同时 glob 导入**。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;

// ==================== 应用侧自己定义的“挂法” ====================

/// 这条修正挂在哪个主人身上。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Equipment)]
struct EquippedOn(Entity);

/// 主人身上自动维护的修正列表。
#[derive(Component, Default)]
#[relationship_target(relationship = EquippedOn, linked_spawn)]
struct Equipment(Vec<Entity>);

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ScenePlugin,
    ));
    app
}

/// 单值属性写成 `Strength(10.0)`（tuple 组件）；修正用泛型或别名都行。
#[test]
fn modifier_can_be_written_with_turbofish_or_alias() {
    let mut app = test_app();
    let world = app.world_mut();

    let raw = world
        .spawn_scene(bsn! {
            #Raw
            Strength(10.0)
            Equipment [
                Modifier::<Strength> { flat: 5.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let aliased = world
        .spawn_scene(bsn! {
            #Aliased
            Strength(10.0)
            Equipment [
                ModStrength { flat: 5.0 },
                ModStrength { flat: -3.0 },   // 减：同一个量上可以加减并存
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let strength = world.get::<Strength>(raw).unwrap();
    let equipment = world.get::<Equipment>(raw).unwrap();
    assert_eq!(
        strength.resolve(equipment.0.iter().filter_map(|e| world.get::<Modifier<Strength>>(*e))),
        15.0
    );

    let strength = world.get::<Strength>(aliased).unwrap();
    let equipment = world.get::<Equipment>(aliased).unwrap();
    assert_eq!(
        strength.resolve(equipment.0.iter().filter_map(|e| world.get::<Modifier<Strength>>(*e))),
        12.0 // 10 + 5 - 3
    );
}

/// 整棵“角色 + 它的修正”都写在 `bsn!` 里——将来配置化就是这个形态。
///
/// 注意：列表里**不写** `EquippedOn(#Name)`。`RelationshipTarget` 的 `Relationship`
/// 关联类型让宏知道对方是谁并自动补上；手写一遍会得到 `Bundle has duplicate components`。
#[test]
fn hero_with_modifiers_can_be_written_entirely_in_bsn() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Health { current: 30.0, base_max: 60.0 }
            Equipment [
                // 一条修正同时带平加和比例
                ModStrength { flat: 5.0, ratio: 0.5 },
                ModHealthMax { flat: 20.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let equipment = world.get::<Equipment>(hero).unwrap();
    assert_eq!(equipment.0.len(), 2);

    let strength = world.get::<Strength>(hero).unwrap();
    assert_eq!(
        strength.resolve(equipment.0.iter().filter_map(|e| world.get::<Modifier<Strength>>(*e))),
        20.0 // 10 + 5 + 10 × 0.5
    );

    let health = world.get::<Health>(hero).unwrap();
    assert_eq!(
        health.resolve_max(
            equipment
                .0
                .iter()
                .filter_map(|e| world.get::<Modifier<Max<Health>>>(*e))
        ),
        80.0 // 60 + 20
    );
    assert_eq!(health.current, 30.0, "当前值不走修正");
}

/// `linked_spawn` 带来的顺带好处：卸下 = 销毁修正实体，主人没了它也跟着走。
#[test]
fn despawning_the_owner_cleans_up_its_modifiers() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Equipment [
                ModStrength { flat: 5.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let modifier = world.get::<Equipment>(hero).unwrap().0[0];
    assert!(world.get_entity(modifier).is_ok());

    world.despawn(hero);
    assert!(world.get_entity(hero).is_err());
    assert!(world.get_entity(modifier).is_err(), "修正实体应随主人一起消失");
}
