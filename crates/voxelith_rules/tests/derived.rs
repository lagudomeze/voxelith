//! 派生规则的关键行为：bsn 里写路径、值算对、缺配置要吵、从文件真加载。

use bevy_app::prelude::*;
use bevy_asset::{AssetPlugin, AssetServer, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 资产根钉在 crate 目录里，免得受工作目录影响。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        ScenePlugin,
        DerivedRulesPlugin,
    ));
    app
}

/// 场景里能直接用**路径**指定配置，而且 `#[require]` 把结果那条修正带了出来。
#[test]
fn bsn_names_the_config_by_path() {
    let mut app = test_app();
    let world = app.world_mut();

    let rule = world
        .spawn_scene(bsn! {
            #Rule
            DerHealthMaxFromConstitution { config: "rules/health_from_constitution.ron" }
        })
        .expect("spawn_scene 失败")
        .id();

    let derived = world.get::<DerHealthMaxFromConstitution>(rule).unwrap();
    assert_eq!(
        derived.config.path().and_then(|p| p.path().to_str()),
        Some("rules/health_from_constitution.ron")
    );
    assert!(
        world.get::<Modifier<Max<Health>>>(rule).is_some(),
        "#[require] 应该把结果那条 Modifier 带出来"
    );
}

/// 值算对了，而且**读上限的人看到的还是普通修正**——派生的修正与装备给的完全同构。
#[test]
fn rule_writes_the_modifier_and_the_max_follows() {
    let mut app = test_app();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });

    let world = app.world_mut();
    let hero = world
        .spawn((
            Constitution(12.0),
            Health {
                current: 30.0,
                base_max: 60.0,
            },
        ))
        .id();
    world.spawn((DerivedFor(hero), DerHealthMaxFromConstitution::new(handle)));

    app.update();

    let world = app.world();
    let rules = world.get::<DerivedModifiers>(hero).unwrap();
    let first = rules.iter().next().unwrap();
    let modifier = world.get::<Modifier<Max<Health>>>(first).unwrap();
    assert_eq!(modifier.flat, 60.0); // 体质 12 × 每点 5

    let health = world.get::<Health>(hero).unwrap();
    let max = health.resolve_max(
        rules
            .iter()
            .filter_map(|e| world.get::<Modifier<Max<Health>>>(e)),
    );
    assert_eq!(max, 120.0); // 基础上限 60 ＋ 派生 60
}

/// **没写 config 就报错**，不是静默当 0。
#[test]
#[should_panic(expected = "没有写 config")]
fn missing_config_panics() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world.spawn((Constitution(12.0), Health::default())).id();
    world.spawn((
        DerivedFor(hero),
        DerHealthMaxFromConstitution::new(Handle::default()),
    ));

    app.update();
}

/// **值没变就不写**：第二帧不再产生变更（否则会连锁触发下游的变更检测）。
#[test]
fn unchanged_value_is_not_written_again() {
    #[derive(Resource, Default)]
    struct Writes(u32);

    fn count_writes(
        modifiers: Query<(), Changed<Modifier<Max<Health>>>>,
        mut writes: ResMut<Writes>,
    ) {
        writes.0 += modifiers.iter().count() as u32;
    }

    let mut app = test_app();
    app.init_resource::<Writes>();
    // 数在 PostUpdate 里，保证排在同步系统之后
    app.add_systems(PostUpdate, count_writes);

    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });
    let world = app.world_mut();
    let hero = world.spawn((Constitution(12.0), Health::default())).id();
    world.spawn((DerivedFor(hero), DerHealthMaxFromConstitution::new(handle)));

    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 1, "第一帧应该写一次");

    app.update();
    assert_eq!(
        app.world().resource::<Writes>().0,
        1,
        "第二帧值没变，不该再写"
    );
}

/// 端到端：**真从 `.ron` 文件加载**。
///
/// 这里顺带演示**装配层的加载闸门**——`Derived::sync` 要求配置一定在，所以进游戏前
/// 要先把用到的资产等齐（真实项目里这一等就是加载界面）。
#[test]
fn config_loads_from_a_ron_file() {
    let mut app = test_app();

    // 1) 先把文件等齐。此时还没摆规则实体，system 不会撞上缺配置。
    let handle = app
        .world()
        .resource::<AssetServer>()
        .load::<RuleConfig>("rules/health_from_constitution.ron");
    for _ in 0..200 {
        app.update();
        if app
            .world()
            .resource::<Assets<RuleConfig>>()
            .get(&handle)
            .is_some()
        {
            break;
        }
    }
    assert!(
        app.world()
            .resource::<Assets<RuleConfig>>()
            .get(&handle)
            .is_some(),
        "文件应该被加载出来"
    );

    // 2) 摆实体，跑一帧
    let world = app.world_mut();
    let hero = world
        .spawn((
            Constitution(12.0),
            Health {
                current: 30.0,
                base_max: 60.0,
            },
        ))
        .id();
    world.spawn((DerivedFor(hero), DerHealthMaxFromConstitution::new(handle)));
    app.update();

    let world = app.world();
    let rules = world.get::<DerivedModifiers>(hero).unwrap();
    let first = rules.iter().next().unwrap();
    let modifier = world.get::<Modifier<Max<Health>>>(first).unwrap();
    assert_eq!(modifier.flat, 60.0, "配置应该从 .ron 加载出来，算进那条修正");
}
