//! **战斗存档往返**：存 → 改 → 读 → 回到存档那一刻。
//!
//! 这条链路把 `moonshine-save` 真正用起来（此前它只装了观察者、没有任何实体带 `Save`），
//! 也把新栈的两个"不能直接进档"的东西串起来：
//!
//! ```text
//! gauge 的 `Attributes` 不能反射 ⇒ 存档里放显式快照（`SavedCombat`）
//! 技能是实体 ⇒ 存档里放内容 id（`AbilityId`），读档按 id 重建
//! ```

use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformPlugin;
use moonshine_save::prelude::*;
use voxelith_abilities::Attributes;
use voxelith_abilities::ability::AbilityId;
use voxelith_abilities::casting::CastRequest;

use voxelith_prime::combat::{CombatContent, CombatContentPlugin, Loadout, Playable};
use voxelith_prime::combat_save::{
    CombatSavePlugin, SaveCombat, SavedActor, SavedAttribute, SavedCombat,
};

fn save_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("voxelith-combat-save-{name}.ron"));
    path
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        StatesPlugin,
        InputPlugin,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        voxelith_abilities::plugin(),
        CombatContentPlugin,
        CombatSavePlugin,
        // moonshine 的默认事件观察者（真应用里由 `EcosystemPlugin` 装）。
        SavePlugin,
    ));
    // 确定的步长（存/读与"打一下"都在这个时钟上）。
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(16),
    ));
    app
}

/// 只装观察者的最小插件（真应用里在 `EcosystemPlugin` 里）。
struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(save_on_default_event)
            .add_observer(load_on_default_event);
    }
}

fn playable(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Playable>>()
        .single(app.world())
        .expect("该有一个可玩角色")
}

fn value(app: &App, entity: Entity, name: &str) -> f32 {
    app.world()
        .entity(entity)
        .get::<Attributes>()
        .expect("有属性")
        .value(name)
}

/// 花掉 1 点行动力（用真内容放一招，费用 1）。
///
/// ⚠️ 两次要用**不同的技能**：同一个技能第二次会被门控拒掉（"还没就绪"），
/// 费用也就不会扣——那是 Round 23 特意修掉的行为。
fn spend_one_action(app: &mut App, actor: Entity, id: &str) {
    let slot = app
        .world()
        .resource::<CombatContent>()
        .abilities
        .iter()
        .position(|ability| ability.id == id)
        .unwrap_or_else(|| panic!("内容里该有 {id}"));
    let ability = app
        .world()
        .entity(actor)
        .get::<Loadout>()
        .expect("有技能栏")
        .0[slot];
    app.world_mut().write_message(CastRequest {
        caster: actor,
        ability,
    });
    for _ in 0..4 {
        app.update();
    }
}

#[test]
fn saving_writes_a_file_that_carries_the_snapshot() {
    let path = save_path("writes");
    let mut app = test_app();
    app.update();

    let actor = playable(&mut app);
    spend_one_action(&mut app, actor, "basic_attack");
    assert_eq!(value(&app, actor, "Action"), 2.0, "花掉 1 点，存档时是 2");

    app.world_mut()
        .write_message(SaveCombat { path: path.clone() });
    for _ in 0..3 {
        app.update();
    }

    let text = std::fs::read_to_string(&path).expect("存档文件该写出来了");
    assert!(
        text.contains("SavedCombat") || text.contains("SavedActor"),
        "存档里该有我们的快照组件：{text:.200}"
    );
    assert!(
        text.contains("basic_attack"),
        "技能栏该按**内容 id** 存进去：{text:.200}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn applying_a_snapshot_pushes_it_back_into_the_numeric_layer() {
    // 这一条**绕开 moonshine 的读档**，单独验证"把快照推回数值层 + 重建技能栏"这一半。
    //
    // 为什么分开验：moonshine 的读档是**重建实体**（不是就地覆盖），而我们只存了快照组件
    // ⇒ 读档后角色只剩下那几个组件（连 `Playable` 都没有）。要让读档真正可用，
    // 得在之后按 `SavedActor.template` **重建一个完整角色**——那一步还没做（见模块文档）。
    let mut app = test_app();
    app.update();
    let actor = playable(&mut app);
    assert_eq!(value(&app, actor, "Action"), 3.0);

    // 造一份"存档时刻"的快照：行动力 2、技能栏只有一招。
    app.world_mut().entity_mut(actor).insert(SavedCombat {
        attributes: vec![SavedAttribute {
            name: "Action".to_string(),
            value: 2.0,
        }],
        bar: vec!["basic_attack".to_string()],
    });
    app.world_mut()
        .insert_resource(voxelith_prime::combat_save::CombatLoadPending { frames: 2 });
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        value(&app, actor, "Action"),
        2.0,
        "`push_set` 该把行动力设成快照里的 2（不是加上去）"
    );
    let bar: Vec<Entity> = app
        .world()
        .entity(actor)
        .get::<Loadout>()
        .expect("有技能栏")
        .0
        .clone();
    assert_eq!(bar.len(), 1, "技能栏该被重建成快照里的那一招");
    assert_eq!(
        app.world()
            .entity(bar[0])
            .get::<AbilityId>()
            .map(|id| id.0.as_str()),
        Some("basic_attack"),
        "重建出来的技能该是快照里那个 id"
    );
}
