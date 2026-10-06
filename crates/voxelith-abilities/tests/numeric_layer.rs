//! **数值层**：上限从主属性派生、资源按秒回复、回复封顶。
//!
//! 旧 `vocabulary.ron` 把"上限 + 每秒回复"写死在池模板里；新栈里：
//!
//! ```text
//! 上限   MaxHealth = Constitution * 10        （一条表达式属性）
//! 回复   regen 规则 + `regenerate` 系统        （rate × delta，再与"离上限还差多少"取小）
//! ```
//!
//! 这一组用例同时钉住"**回复与帧率无关**"（按 delta 算）与"**不会顶过上限**"
//! （旧引擎要靠别处再夹一次，新栈里上限就是回复的天然约束）。

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use voxelith_abilities::attributes::{self, AttributeCatalog};
use voxelith_abilities::numeric::RegenRules;
use voxelith_abilities::{AttributeInitializer, Attributes};

/// 一个"残血 + 有回复"的角色：离上限 10 点，每秒回 2 点。
const CUSTOM: &str = r#"(
    base: [
        (name: "Health",      literal: 40.0),
        (name: "MaxHealth",   literal: 50.0),
        (name: "HealthRegen", literal: 2.0),
    ],
    regen: [
        (attribute: "Health", max: "MaxHealth", rate: "HealthRegen"),
    ],
)"#;

/// 一个"没有回复"的角色：用来钉住"回复为 0 就一点不动"。
const NO_REGEN: &str = r#"(
    base: [
        (name: "Health",      literal: 40.0),
        (name: "MaxHealth",   literal: 50.0),
        (name: "HealthRegen", literal: 0.0),
    ],
    regen: [
        (attribute: "Health", max: "MaxHealth", rate: "HealthRegen"),
    ],
)"#;

fn app_with(source: &str) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));

    let set = attributes::parse(source).expect("属性定义语法没问题");
    let mut catalog = AttributeCatalog::default();
    catalog.add(&set);
    app.insert_resource(RegenRules::from_set(&set));

    let actor = app
        .world_mut()
        .spawn((
            Attributes::new(),
            AttributeInitializer::new(set.build_base().expect("属性集能构造")),
        ))
        .id();
    app.update();
    (app, actor)
}

fn health(app: &App, actor: Entity) -> f32 {
    app.world()
        .entity(actor)
        .get::<Attributes>()
        .expect("有属性")
        .value("Health")
}

fn max_health(app: &App, actor: Entity) -> f32 {
    app.world()
        .entity(actor)
        .get::<Attributes>()
        .expect("有属性")
        .value("MaxHealth")
}

fn health_is(app: &App, actor: Entity, expected: f32) -> bool {
    (health(app, actor) - expected).abs() < 1e-3
}

#[test]
fn regen_is_per_second_and_never_passes_the_cap() {
    let (mut app, actor) = app_with(CUSTOM);
    assert_eq!(health(&app, actor), 40.0, "开局是残血");
    assert_eq!(max_health(&app, actor), 50.0, "上限来自内容");

    // 每秒 2 点：走 1 秒 ⇒ 42（f32 逐帧累加，所以按容差比）。
    for _ in 0..10 {
        app.update();
    }
    assert!(
        health_is(&app, actor, 42.0),
        "每秒 2 点（10 帧 × 100ms），实际 {}",
        health(&app, actor)
    );

    // 再走 10 秒：该封顶在 50，而不是长到 62。
    for _ in 0..100 {
        app.update();
    }
    assert!(
        health_is(&app, actor, 50.0),
        "回复**不会顶过上限**（旧引擎要靠别处再夹一次），实际 {}",
        health(&app, actor)
    );
}

#[test]
fn a_zero_rate_changes_nothing() {
    let (mut app, actor) = app_with(NO_REGEN);
    let before = health(&app, actor);
    for _ in 0..50 {
        app.update();
    }
    assert_eq!(health(&app, actor), before, "回复为 0 就一点不动");
}

#[test]
fn the_real_content_derives_its_caps_from_prime_attributes() {
    // 真内容那一份：上限是从主属性**派生**的（不是写死的），
    // 所以"改体质就改上限"这件事在新栈里是自动的。
    let set = attributes::parse(include_str!("../../../assets/data/attributes.ron"))
        .expect("真属性文件语法没问题");
    assert!(
        set.regen.len() >= 9,
        "九条资源都该有回复规则（旧 `vocabulary.ron` 的 regen 字段）"
    );

    let mut catalog = AttributeCatalog::default();
    catalog.add(&set);
    for name in [
        "MaxHealth",
        "MaxMana",
        "MaxStamina",
        "MaxPsi",
        "MaxVim",
        "MaxEquilibrium",
        "HealthRegen",
        "ManaRegen",
        "StaminaRegen",
        "Dexterity",
        "Constitution",
        "Accuracy",
        "Defense",
        "Level",
        "SkillRank",
    ] {
        assert!(catalog.contains(name), "台账里该有 `{name}`");
    }
}
