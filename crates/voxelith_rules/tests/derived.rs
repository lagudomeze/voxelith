//! 派生公式的关键行为：bsn 里写路径、值算对、**属性修正会流进派生**、缺配置要吵、从文件真加载。
//!
//! 注意这里**没有任何链接类型**——归属是库自带的 `Affects` / `Modifiers`，所以
//! `RulesPlugin` 没有泛型参数，"挂两个插件各算一半"那种情况从结构上就不可能。

use bevy_app::prelude::*;
use bevy_asset::{AssetPlugin, AssetServer, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 资产根是**仓库根的 `assets/`**，用绝对路径钉住，免得受工作目录影响。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/../../assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        ScenePlugin,
        RulesPlugin,
    ));
    app
}

/// 场景里能直接用**路径**指定配置；公式写在主人身上。
#[test]
fn bsn_puts_the_formula_on_the_owner() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Constitution(12.0)
            Health(30.0)
            HealthMax(60.0)
            HealthMaxFromConstitution { config: "rules/health_from_constitution.ron" }
        })
        .expect("spawn_scene 失败")
        .id();

    let formula = world.get::<HealthMaxFromConstitution>(hero).unwrap();
    assert_eq!(
        formula.config.path().and_then(|p| p.path().to_str()),
        Some("rules/health_from_constitution.ron")
    );
}

/// 三段顺序跑完：体质 → 派生的修正 → 上限的有效值。
#[test]
fn constitution_drives_the_max() {
    let mut app = test_app();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });

    let world = app.world_mut();
    let hero = world
        .spawn((
            Constitution(12.0),
            Health(30.0),
            Max::<Health>::new(60.0),
            HealthMaxFromConstitution::new(handle),
        ))
        .id();

    app.update();

    let world = app.world();
    assert_eq!(world.get::<Total<Constitution>>(hero).unwrap().value, 12.0);
    assert_eq!(
        world.get::<Modifier<Max<Health>>>(hero).unwrap().flat,
        60.0,
        "体质 12 × 每点 5（结果写在主人自己身上）"
    );
    assert_eq!(
        world.get::<Total<Max<Health>>>(hero).unwrap().value,
        120.0,
        "基础上限 60 ＋ 派生 60"
    );
}

/// **装备给的体质要真的抬高血量上限**——派生读的是有效值，不是基础值（`SY-58`）。
#[test]
fn equipment_given_constitution_flows_into_the_max() {
    let mut app = test_app();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });

    let world = app.world_mut();
    let hero = world
        .spawn((
            Constitution(12.0),
            Health(30.0),
            Max::<Health>::new(60.0),
            HealthMaxFromConstitution::new(handle),
        ))
        .id();
    // 装备：+3 体质（就是一条普通的修正，归属靠 Affects）
    world.spawn((Affects(hero), ModConstitution::flat(3.0)));

    app.update();

    let world = app.world();
    assert_eq!(
        world.get::<Total<Constitution>>(hero).unwrap().value,
        15.0,
        "12 + 3（装备）"
    );
    assert_eq!(
        world.get::<Modifier<Max<Health>>>(hero).unwrap().flat,
        75.0,
        "体质 15 × 每点 5——装备给的体质也进来了"
    );
    assert_eq!(world.get::<Total<Max<Health>>>(hero).unwrap().value, 135.0);
}

/// **没写 config 就报错**，不是静默当 0。
#[test]
#[should_panic(expected = "没有写 config")]
fn missing_config_panics() {
    let mut app = test_app();
    let world = app.world_mut();

    world.spawn((
        Constitution(12.0),
        Health::default(),
        HealthMaxFromConstitution::new(Handle::default()),
    ));

    app.update();
}

/// **值没变就不写**：第二帧不再产生变更（否则会连锁触发下游的变更检测）。
#[test]
fn unchanged_value_is_not_written_again() {
    #[derive(Resource, Default)]
    struct Touches(u32);

    fn count_touches(
        modifiers: Query<(), Changed<Modifier<Max<Health>>>>,
        mut touches: ResMut<Touches>,
    ) {
        touches.0 += modifiers.iter().count() as u32;
    }

    let mut app = test_app();
    app.init_resource::<Touches>();
    // 数在 PostUpdate 里，保证排在重算与派生之后
    app.add_systems(PostUpdate, count_touches);

    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });
    let world = app.world_mut();
    world.spawn((
        Constitution(12.0),
        Health::default(),
        HealthMaxFromConstitution::new(handle),
    ));

    // 前两帧：公式补上那条修正（走命令，可能跨一次 flush）
    app.update();
    app.update();
    let settled = app.world().resource::<Touches>().0;
    assert!(settled >= 1, "该产生那条修正");

    // 从这以后值没变，就不该再动它
    app.update();

    assert_eq!(
        app.world().resource::<Touches>().0,
        settled,
        "值没变，不该再动它"
    );
}

/// 端到端：**真从 `.ron` 文件加载**。
///
/// 这里顺带演示**装配层的加载闸门**——派生要求配置一定在，所以进游戏前
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
            Health(30.0),
            Max::<Health>::new(60.0),
            HealthMaxFromConstitution::new(handle),
        ))
        .id();
    app.update();

    assert_eq!(
        app.world().get::<Total<Max<Health>>>(hero).unwrap().value,
        120.0,
        "配置应该从 .ron 加载出来，算进上限"
    );
}
