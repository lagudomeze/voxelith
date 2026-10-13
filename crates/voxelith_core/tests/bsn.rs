//! **归属内置**，用 BSN 写出来是什么样。
//!
//! 归属（[`Affects`]）与名单（[`Modifiers`]）是**库自带、唯一的一条关系**（没有泛型参数，
//! 所以插件不会出现"两个实例各算一半"的情况）。写场景时：
//!
//! - `Modifiers [ … ]`：一次挂一串，`Affects` 由宏自动补；
//! - 自指（派生挂结果用）从 Rust 插：`insert((Affects(自己), ModStrength::flat(2.0)))`；
//! - `linked_spawn`：主人没了，名单上的修正一起没。
//!
//! **来源（装备/增益/天赋）不是第二条归属关系**，而是修正实体上的标签组件——见 `system_read.rs`。

use bevy_app::prelude::*;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;

/// 求和：`raw + Σ贡献(raw)`。
///
/// **真实项目里这段在 `voxelith_rules` 的重算系统里**；core 只负责"一条修正贡献多少"。
fn total(world: &World, owner: Entity, list: &Modifiers) -> f32 {
    let raw = world.get::<Strength>(owner).unwrap().raw();
    raw + list
        .iter()
        .filter_map(|e| world.get::<Modifier<Strength>>(e))
        .map(|m| m.contribution(raw))
        .sum::<f32>()
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        ScenePlugin,
    ));
    app
}

/// 名单写法：一串修正挂在主人身上，`Affects` 由宏自动补。
#[test]
fn modifier_list_in_bsn() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Modifiers [
                ModStrength { flat: 5.0 },
                ModStrength { flat: -3.0 },   // 减：同一个量上可以加减并存
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let list = world.get::<Modifiers>(hero).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(total(world, hero, list), 12.0); // 10 + 5 - 3
    // 归属是自动补的
    for e in list.iter() {
        assert_eq!(world.get::<Affects>(e).unwrap().0, hero);
    }
}

/// 同一条名单里可以混着不同量的修正（名单不按量分）。
#[test]
fn one_list_holds_every_quantity() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Health(30.0)
            HealthMax(60.0)
            Modifiers [
                ModStrength { flat: 5.0 },
                ModHealthMax { flat: 20.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let list = world.get::<Modifiers>(hero).unwrap();
    assert_eq!(list.len(), 2);

    let raw = world.get::<Max<Health>>(hero).unwrap().raw();
    let max_hp = raw
        + list
            .iter()
            .filter_map(|e| world.get::<Modifier<Max<Health>>>(e))
            .map(|m| m.contribution(raw))
            .sum::<f32>();
    assert_eq!(max_hp, 80.0); // 60 + 20
}

/// 自指：修正挂在主人自己身上、并指向自己（派生规则就是这么挂结果的）。
#[test]
fn self_modifier_points_at_itself() {
    let mut world = World::new();
    let hero = world.spawn(Strength(10.0)).id();

    world
        .entity_mut(hero)
        .insert((Affects(hero), ModStrength::flat(2.0)));

    let list = world.get::<Modifiers>(hero).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list.iter().any(|e| e == hero), "自指那条在名单里");
    assert_eq!(total(&world, hero, list), 12.0); // 10 + 2
}

/// `linked_spawn` 带来的顺带好处：主人没了，修正实体跟着走。
#[test]
fn despawning_the_owner_cleans_up_its_modifiers() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Strength(10.0)
            Modifiers [
                ModStrength { flat: 5.0 },
            ]
        })
        .expect("spawn_scene 失败")
        .id();

    let modifier = world.get::<Modifiers>(hero).unwrap().iter().next().unwrap();
    assert!(world.get_entity(modifier).is_ok());

    world.despawn(hero);
    assert!(world.get_entity(hero).is_err());
    assert!(world.get_entity(modifier).is_err(), "修正实体应随主人一起消失");
}
