//! **L2 战斗内容装配**的端到端测试：读文件 → 生成 → 输入 → 释放。
//!
//! 与 `voxelith-abilities` 里的测试的区别：那些把 RON 当字符串（`include_str!`），
//! 这里**真的从磁盘读**，走的是运行期那条路（`ContentPaths` → `std::fs` → 解析 → 模板）。
//!
//! 这条链路在应用里长这样：
//!
//! ```text
//! Startup  load_combat_content  → 读三份 .ron、建台账、注册状态模板
//!          （apply_deferred）    → 让 `CombatContent` 资源当场可见
//!          spawn_demo_duel       → 生成可玩角色（技能栏 = 内容顺序）+ 目标
//! Update   intent_from_input    → leafwing 动作 → CastIntent{slot}
//!          intent_to_request     → 技能栏翻译 → CastRequest{caster, ability}
//!          （abilities::casting）→ 门控 + 扣费 + 进状态机
//! ```

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::transform::TransformPlugin;
use leafwing_input_manager::prelude::*;
use voxelith_abilities::Attributes;

use voxelith_prime::combat::{CastIntent, CombatContent, CombatContentPlugin, Loadout, Playable};
use voxelith_prime::ecosystem::GameAction;

/// 无窗口的最小 App：时间 / 变换 / 状态 / 输入 / 资产 / 场景 + 三件套 + 内容装配。
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
    ));
    app
}

fn value_of(app: &App, entity: Entity, name: &str) -> f32 {
    app.world()
        .entity(entity)
        .get::<Attributes>()
        .expect("有属性")
        .value(name)
}

fn playable(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Playable>>()
        .single(app.world())
        .expect("该有一个可玩角色")
}

fn slot_of(app: &App, id: &str) -> usize {
    app.world()
        .resource::<CombatContent>()
        .abilities
        .iter()
        .position(|ability| ability.id == id)
        .unwrap_or_else(|| panic!("内容里该有 `{id}`"))
}

#[test]
fn the_app_reads_the_combat_content_from_disk() {
    let mut app = test_app();
    app.update();

    let content = app.world().resource::<CombatContent>();
    assert!(
        content
            .abilities
            .iter()
            .any(|ability| ability.id == "basic_attack"),
        "技能该从 abilities.ron 读进来"
    );
    // 同样不写死条数：查 id。
    for id in ["Guarding", "Weakened", "Burning"] {
        assert!(
            content.statuses.get(id).is_some(),
            "status_defs.ron 里该有 `{id}`"
        );
    }
    assert!(
        content.attributes.contains("Strength"),
        "属性台账该有 Strength（它在 `vocabulary` 里，值由角色定义给）"
    );
    assert!(
        content.actors.get("player").is_some() && content.actors.get("goblin").is_some(),
        "角色文件该提供 player 与 goblin"
    );
    // 不写死条数（内容会一直长）：查"每条该在的都在"。
    for id in [
        "basic_attack",
        "goblin_slash",
        "shield_block",
        "riposte",
        "troll_smash",
    ] {
        assert!(
            content.abilities.iter().any(|ability| ability.id == id),
            "abilities.ron 里该有 `{id}`"
        );
    }
}

#[test]
fn the_demo_duel_gets_a_loadout_in_content_order() {
    let mut app = test_app();
    app.update();

    let caster = playable(&mut app);
    let loadout = app
        .world()
        .entity(caster)
        .get::<Loadout>()
        .expect("可玩角色该有技能栏");
    assert_eq!(
        loadout.0.len(),
        app.world().resource::<CombatContent>().abilities.len(),
        "技能栏 = 内容顺序"
    );
    assert_eq!(slot_of(&app, "basic_attack"), 0, "普攻是第一条");
}

#[test]
fn an_intent_pays_the_cost_and_starts_the_ability() {
    let mut app = test_app();
    app.update();

    let caster = playable(&mut app);
    assert_eq!(value_of(&app, caster, "Action"), 3.0, "行动力起点");

    // 意图层：只说"第几个"，不说"哪个实体"（翻译是技能栏的事）。
    let slot = slot_of(&app, "basic_attack");
    app.world_mut().write_message(CastIntent { caster, slot });
    for _ in 0..8 {
        app.update();
    }

    // 普攻前摇 1.0 秒，测试的时钟走不完 ⇒ 还没打中；
    // 但**费用已付**、而且没有被门控拒绝 —— 这条链（意图→请求→门控→扣费→状态机）通了。
    assert_eq!(
        value_of(&app, caster, "Action"),
        2.0,
        "释放一次花 1 点行动力（说明请求真的走到了 casting）"
    );
}

#[test]
fn the_demo_duel_has_two_different_actors() {
    let mut app = test_app();
    app.update();

    let player = playable(&mut app);
    // 目标 = 那个**有属性但不是可玩角色**的实体（`spawn_demo_duel` 生成的两个之一）。
    // 注意别用 `InvokerTarget` 去找它：那个组件挂在**玩家**身上（"我瞄着谁"）。
    let goblin = app
        .world_mut()
        .query_filtered::<Entity, (With<voxelith_abilities::Attributes>, Without<Playable>)>()
        .iter(app.world())
        .next()
        .expect("该有一个目标（哥布林）");

    let value = |app: &App, entity: Entity, name: &str| {
        app.world()
            .entity(entity)
            .get::<voxelith_abilities::Attributes>()
            .expect("有属性")
            .value(name)
    };

    assert_eq!(value(&app, player, "Health"), 100.0, "玩家照 `actors.ron`");
    assert_eq!(value(&app, player, "MaxHealth"), 100.0);
    assert_eq!(value(&app, goblin, "Health"), 60.0, "哥布林照 `actors.ron`");
    assert_eq!(
        value(&app, goblin, "MaxHealth"),
        60.0,
        "上限按它自己的体质派生（6 × 10）"
    );
    assert_eq!(value(&app, goblin, "Strength"), 6.0, "怪物不再是玩家的数值");
}

#[test]
fn pressing_an_action_key_produces_the_same_chain() {
    let mut app = test_app();
    app.update();

    let caster = playable(&mut app);
    // 绑上按键：Q → 技能 1（内容第一条 = 普攻）。
    app.world_mut()
        .entity_mut(caster)
        .insert(InputMap::<GameAction>::new([(
            GameAction::Ability1,
            KeyCode::KeyQ,
        )]));
    // `InputManagerPlugin` 由生态插件装；这里只装它（音频/渲染/物理在无窗口测试里进不来）。
    app.add_plugins(InputManagerPlugin::<GameAction>::default());
    app.update();

    // 真的按一下键：leafwing 在 `PreUpdate` 读 `ButtonInput` 并更新 `ActionState`，
    // 我们的 `intent_from_input` 在 `Update` 读到 `just_pressed`。
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyQ);
    for _ in 0..8 {
        app.update();
    }

    assert_eq!(
        value_of(&app, caster, "Action"),
        2.0,
        "按键 → 意图 → 请求 → 扣费，整条链路该走通"
    );
}
