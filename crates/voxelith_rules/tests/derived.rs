//! 派生规则的关键行为：bsn 里写路径、值算对、**属性修正会流进派生**、缺配置要吵、从文件真加载。

use bevy_app::prelude::*;
use bevy_asset::{AssetPlugin, AssetServer, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::*;
use bevy_scene::ScenePlugin;
use voxelith_core::prelude::*;
use voxelith_rules::prelude::*;

/// 应用侧自己定的挂法：修正实体指回主人。
///
/// **规则层不认识它**——它是靠 `RulesPlugin::<Equipment>` 那个泛型参数传进去的。
#[derive(Component, FromTemplate)]
#[relationship(relationship_target = Equipment)]
struct EquippedOn(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = EquippedOn, linked_spawn)]
struct Equipment(Vec<Entity>);

/// 资产根是**仓库根的 `assets/`**（内容与代码分开放），用绝对路径钉住，免得受工作目录影响。
/// `CARGO_MANIFEST_DIR` 是 `crates/voxelith_rules`，往上两层就是仓库根。
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/../../assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        ScenePlugin,
        RulesPlugin::<Equipment>::default(),
    ));
    app
}

/// 场景里能直接用**路径**指定配置，规则就写在主人身上，`#[require]` 把结果格子一起带来。
#[test]
fn bsn_puts_the_rule_on_the_owner() {
    let mut app = test_app();
    let world = app.world_mut();

    let hero = world
        .spawn_scene(bsn! {
            #Hero
            Constitution(12.0)
            Health { current: 30.0, base_max: 60.0 }
            DerHealthMaxFromConstitution { config: "rules/health_from_constitution.ron" }
        })
        .expect("spawn_scene 失败")
        .id();

    let derived = world.get::<DerHealthMaxFromConstitution>(hero).unwrap();
    assert_eq!(
        derived.config.path().and_then(|p| p.path().to_str()),
        Some("rules/health_from_constitution.ron")
    );
    assert!(
        world.get::<Modifier<Max<Health>>>(hero).is_some(),
        "#[require] 应该把结果那条 Modifier 带出来"
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
            Health {
                current: 30.0,
                base_max: 60.0,
            },
            DerHealthMaxFromConstitution::new(handle),
        ))
        .id();

    app.update();

    let world = app.world();
    assert_eq!(world.get::<Total<Constitution>>(hero).unwrap().value, 12.0);
    assert_eq!(
        world.get::<Modifier<Max<Health>>>(hero).unwrap().flat,
        60.0,
        "体质 12 × 每点 5"
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
            Health {
                current: 30.0,
                base_max: 60.0,
            },
            DerHealthMaxFromConstitution::new(handle),
        ))
        .id();
    // 装备：+3 体质
    world.spawn((EquippedOn(hero), ModConstitution::flat(3.0)));

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
    // 数在 PostUpdate 里，保证排在重算与派生之后
    app.add_systems(PostUpdate, count_writes);

    let handle = app
        .world_mut()
        .resource_mut::<Assets<RuleConfig>>()
        .add(RuleConfig { per_point: 5.0 });
    let world = app.world_mut();
    world.spawn((
        Constitution(12.0),
        Health::default(),
        DerHealthMaxFromConstitution::new(handle),
    ));

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
            Health {
                current: 30.0,
                base_max: 60.0,
            },
            DerHealthMaxFromConstitution::new(handle),
        ))
        .id();
    app.update();

    assert_eq!(
        app.world().get::<Total<Max<Health>>>(hero).unwrap().value,
        120.0,
        "配置应该从 .ron 加载出来，算进上限"
    );
}
