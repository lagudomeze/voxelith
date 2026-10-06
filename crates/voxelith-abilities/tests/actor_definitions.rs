//! **角色定义**：每个角色自己的数，以及"上限按角色算"。
//!
//! 这一组用例钉住数值层迁移最关键的性质：**同一份全局定义 + 不同的角色数 ⇒ 不同的派生上限**。
//! 以前所有实体共用一份属性，`MaxHealth` 对谁都一样；现在它随该角色的 `Constitution` 走。

use bevy::asset::AssetPlugin;
use bevy::prelude::*;

use voxelith_abilities::actors::{self, ActorLoadError};
use voxelith_abilities::attributes;
use voxelith_abilities::{AttributeInitializer, Attributes};

const ATTRIBUTES: &str = include_str!("../../../assets/data/attributes.ron");
const ACTORS: &str = include_str!("../../../assets/data/actors.ron");

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
    ));
    app
}

fn value(app: &App, entity: Entity, name: &str) -> f32 {
    app.world()
        .entity(entity)
        .get::<Attributes>()
        .expect("有属性")
        .value(name)
}

#[test]
fn the_real_actors_derive_their_own_caps() {
    let (globals, _catalog, actors) =
        actors::load_from_sources(ATTRIBUTES, ACTORS).expect("属性与角色文件该能加载");
    let mut app = test_app();

    // 上限全是 `Constitution * 10` 派生的，所以三个角色的血量与上限各不相同。
    for (id, strength, health, max_health) in [
        ("player", 10.0, 100.0, 100.0),
        ("goblin", 6.0, 60.0, 60.0),
        ("troll", 14.0, 120.0, 120.0),
    ] {
        let set = actors
            .set_for(&globals, id)
            .unwrap_or_else(|| panic!("角色文件里该有 `{id}`"));
        let entity = app
            .world_mut()
            .spawn((Attributes::new(), AttributeInitializer::new(set)))
            .id();
        app.update();

        assert_eq!(value(&app, entity, "Strength"), strength, "`{id}` 的力量");
        assert_eq!(value(&app, entity, "Health"), health, "`{id}` 的血量");
        assert_eq!(
            value(&app, entity, "MaxHealth"),
            max_health,
            "`{id}` 的上限该按它自己的体质派生出来"
        );
    }
}

#[test]
fn a_role_writing_a_globally_defined_name_is_a_load_error() {
    // `MaxHealth` 在全局定义里；角色再写一遍在 gauge 里是**累加**（不是覆盖），
    // 所以加载期就拦——这类错一旦漏过去，表现是"血量莫名其妙多了一截"。
    let broken = r#"[
        (id: "player", attributes: [(name: "MaxHealth", literal: 999.0)]),
    ]"#;
    let error = actors::load_from_sources(ATTRIBUTES, broken).expect_err("该报错");
    assert!(
        matches!(error, ActorLoadError::OverlapsGlobal { .. }),
        "该报撞名，实际：{error}"
    );
}

#[test]
fn the_globals_file_keeps_only_definitions() {
    // 这一条钉住"分工"：`attributes.ron` 里**不该**再有主属性的数值
    // （它们在 `actors.ron`）。判据是 `reserved_names()` 里没有 `Strength`。
    let globals = attributes::parse(ATTRIBUTES).expect("属性文件");
    let reserved = globals.reserved_names();
    assert!(
        !reserved.contains("Strength"),
        "主属性该由角色定义给值；全局定义只留派生与回复：{reserved:?}"
    );
    assert!(reserved.contains("MaxHealth"), "上限是全局派生");
    assert!(reserved.contains("HealthRegen"), "每秒回复是全局定义");
    assert!(
        globals.declared_names().contains("Strength"),
        "但名字必须在词汇里登记（否则角色文件会被判成拼错）"
    );
}
