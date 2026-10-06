//! **L2 战斗面板**：HUD 能不能从新栈里读出该显示的东西。
//!
//! 面板自己不碰公式、不改世界，只读四处现成的只读面：
//! `Attributes`（资源）/ 状态实例（`StatusIdentity` + 宿主解析）/ 技能生命周期标记
//! （`Ready` / `Invoking` / `Cooling`）/ `ThreatWindow`。
//!
//! 这一组用例钉住"面板拿得到、且拿对了"：资源有当前值与上限、技能放出去之后
//! 槽位不再是"就绪"、**冷却中的技能会被门控拒绝**（而且这个理由会进 UI 反馈）。

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::transform::TransformPlugin;
use voxelith_abilities::ability::{Cooling, Ready};
use voxelith_abilities::builder::build_ability;
use voxelith_abilities::casting::CastRequest;
use voxelith_abilities::counter::Invoking;
use voxelith_abilities::numeric::RegenRules;
use voxelith_abilities::threat::ThreatWindow;
use voxelith_abilities::{Active, Attributes, InvokedBy, InvokerTarget, SubstateOf};

use voxelith_prime::combat::{CombatContent, CombatContentPlugin, Loadout, Playable};
use voxelith_prime::presentation::{CombatFeedback, SlotState, pool_rows, slot_state, threat_line};

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
    // 面板的两个系统（`CombatPanelPlugin` 里的 UI 部分要真渲染，测试只装数据侧的两个）。
    app.init_resource::<CombatFeedback>()
        .add_systems(Update, voxelith_prime::presentation::record_rejections);
    app
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

/// 沿 `SubstateOf` 走到状态机根。
///
/// 面板的系统里用 `resolve_root(&Query<..>, e)`（系统参数版本）；测试手里只有 `World`，
/// 所以手动走一遍——两条路走的是同一个关系，结果一致。
fn root_of(world: &World, entity: Entity) -> Entity {
    let mut current = entity;
    for _ in 0..32 {
        match world.get::<SubstateOf>(current) {
            Some(parent) => current = parent.0,
            None => break,
        }
    }
    current
}

/// 面板判"这个技能现在在哪一段"用的就是这三个标记（这里是它的读法）。
fn slot_state_of(app: &mut App, ability: Entity) -> SlotState {
    let mut states = app
        .world_mut()
        .query_filtered::<(Entity, Has<Ready>, Has<Invoking>, Has<Cooling>), With<Active>>();
    let rows: Vec<(Entity, bool, bool, bool)> = states
        .iter(app.world())
        .map(|(entity, ready, invoking, cooling)| (entity, ready, invoking, cooling))
        .collect();

    let mut best = SlotState::Dormant;
    for (state, ready, invoking, cooling) in rows {
        if root_of(app.world(), state) != ability {
            continue;
        }
        let candidate = slot_state(ready, invoking, cooling);
        best = match (best, candidate) {
            (SlotState::Invoking, _) | (_, SlotState::Invoking) => SlotState::Invoking,
            (SlotState::Cooling, _) | (_, SlotState::Cooling) => SlotState::Cooling,
            (_, SlotState::Ready) => SlotState::Ready,
            _ => best,
        };
    }
    best
}

#[test]
fn the_panel_reads_the_new_stacks_pools() {
    let mut app = test_app();
    app.update();

    let actor = playable(&mut app);
    let rules = app.world().resource::<RegenRules>().clone();
    assert!(!rules.is_empty(), "内容装配该把回复规则装上");

    let attributes = app
        .world()
        .entity(actor)
        .get::<Attributes>()
        .expect("可玩角色有属性")
        .clone();
    let rows = pool_rows(&attributes, &rules);

    let health = rows
        .iter()
        .find(|row| row.name == "Health")
        .expect("资源清单里该有 Health");
    assert_eq!(health.current, 100.0, "生命当前值");
    assert_eq!(
        health.max, 100.0,
        "上限来自 `MaxHealth = Constitution * 10`"
    );
    assert_eq!(health.ratio(), 1.0);

    assert!(
        rows.iter().any(|row| row.name == "Mana"),
        "九条资源都该出现在面板上（清单来自 regen 规则）"
    );
}

#[test]
fn a_slot_leaves_ready_once_its_skill_is_releasing() {
    let mut app = test_app();
    app.update();
    let actor = playable(&mut app);
    let slot = slot_of(&app, "basic_attack");
    let ability = app
        .world()
        .entity(actor)
        .get::<Loadout>()
        .expect("有技能栏")
        .0[slot];

    assert_eq!(
        slot_state_of(&mut app, ability),
        SlotState::Ready,
        "开局技能是就绪的"
    );

    app.world_mut().write_message(CastRequest {
        caster: actor,
        ability,
    });
    app.update();

    assert_ne!(
        slot_state_of(&mut app, ability),
        SlotState::Ready,
        "放出去之后槽位不该还显示就绪（面板靠这个把按钮变灰）"
    );
}

#[test]
fn a_cooling_skill_is_rejected_instead_of_silently_eating_the_cost() {
    let mut app = test_app();
    app.update();
    let actor = playable(&mut app);
    let slot = slot_of(&app, "shield_block");
    let ability = app
        .world()
        .entity(actor)
        .get::<Loadout>()
        .expect("有技能栏")
        .0[slot];

    let action_of = |app: &App| {
        app.world()
            .entity(actor)
            .get::<Attributes>()
            .expect("有属性")
            .value("Action")
    };

    // 第一发：扣 1 点行动力，进状态机。
    let before = action_of(&app);
    app.world_mut().write_message(CastRequest {
        caster: actor,
        ability,
    });
    app.update();
    let after_first = action_of(&app);
    assert_eq!(after_first, before - 1.0, "第一发照常扣费");

    // 第二发（同一个技能、还没就绪）：**该被拒绝**，不能再扣一次费。
    // 少了这条门控，`StartInvoke` 会因为"没有 `#Ready → #Invoking` 的边"被静默忽略，
    // 而费用已经扣掉了——玩家看到的是"按了没反应还掉蓝"。
    app.world_mut().write_message(CastRequest {
        caster: actor,
        ability,
    });
    // 两帧：门控在第一帧写 `CastRejected`，而反馈系统与它同处 `Update`、顺序未定，
    // 所以理由最晚下一帧才进 UI（面板允许这个延迟，见 `record_rejections` 的文档）。
    app.update();
    app.update();

    assert_eq!(
        action_of(&app),
        after_first,
        "冷却中的第二发不该再扣费（白扣费是这条门控要拦的事）"
    );
    let feedback = app.world().resource::<CombatFeedback>();
    assert_eq!(
        feedback.last_rejection.map(|(_, reason)| reason),
        Some("技能还没就绪（冷却中或正在释放）"),
        "被拒的理由该能进 UI 反馈"
    );
}

#[test]
fn the_panel_shows_a_threat_and_the_pure_helpers_agree() {
    let mut app = test_app();
    app.update();
    let player = playable(&mut app);

    // 空窗口：什么也不显示。
    let window = app.world().resource::<ThreatWindow>().clone();
    assert!(threat_line(&window).is_none(), "没威胁就不显示横幅");

    // 造一条真威胁：一个"怪物"放一招 `threatening` 的技能（`goblin_slash`），
    // 于是它的前摇会进窗口。
    let statuses = app.world().resource::<CombatContent>().statuses.clone();
    let ability = app
        .world()
        .resource::<CombatContent>()
        .abilities
        .iter()
        .find(|ability| ability.id == "goblin_slash")
        .expect("内容里该有 goblin_slash")
        .clone();
    // 怪物也得有属性（门控要查需求与费用）：用角色文件里的 `goblin` 那一套，
    // 而不是"和玩家一样"——数值层迁过来之后每个角色有自己的数。
    let base = {
        let content = app.world().resource::<CombatContent>();
        content
            .actors
            .set_for(&content.attributes_source, "goblin")
            .expect("角色文件里该有 goblin")
    };
    let monster = app
        .world_mut()
        .spawn((
            Attributes::new(),
            voxelith_abilities::AttributeInitializer::new(base),
            InvokerTarget::entity(player, IVec2::new(1, 0)),
            Name::new("哥布林"),
        ))
        .id();
    let ability_entity = app
        .world_mut()
        .spawn_scene(build_ability(&ability, &statuses))
        .expect("技能场景能落地")
        .id();
    app.world_mut()
        .entity_mut(ability_entity)
        .insert(InvokedBy(monster));
    app.world_mut().write_message(CastRequest {
        caster: monster,
        ability: ability_entity,
    });
    for _ in 0..3 {
        app.update();
    }

    let window = app.world().resource::<ThreatWindow>().clone();
    assert!(
        threat_line(&window).is_some_and(|line| line.contains("有人正在打我")),
        "面板该显示威胁横幅（窗口 {} 条）",
        window.len()
    );
}

/// 手动推进时钟：先放宽 `max_delta`（默认 250ms 会把步长夹掉），再给一帧固定时长。
fn step(app: &mut App, seconds: f32) {
    let duration = std::time::Duration::from_secs_f32(seconds);
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(duration.max(std::time::Duration::from_millis(1)));
    app.world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(duration));
    app.update();
}

/// 沿技能的 `#Cooldown` 状态找到它那条延时边的计时器——**面板读倒计时走的就是这条路**。
fn cooling_remaining(app: &mut App, ability: Entity) -> Option<f32> {
    let mut states = app
        .world_mut()
        .query_filtered::<(Entity, &Cooling), With<Active>>();
    let cooling: Vec<Entity> = states
        .iter(app.world())
        .map(|(state, _)| state)
        .filter(|state| root_of(app.world(), *state) == ability)
        .collect();

    for state in cooling {
        let Some(transitions) = app.world().get::<voxelith_abilities::Transitions>(state) else {
            continue;
        };
        let edges: Vec<Entity> = transitions.into_iter().copied().collect();
        for edge in edges {
            if let Some(timer) = app.world().get::<voxelith_abilities::EdgeTimer>(edge) {
                return Some(timer.0.remaining_secs());
            }
        }
    }
    None
}

#[test]
fn the_cooldown_countdown_comes_from_the_state_machines_own_timer() {
    let mut app = test_app();
    app.update();
    let actor = playable(&mut app);
    let slot = slot_of(&app, "shield_block");
    let ability = app
        .world()
        .entity(actor)
        .get::<Loadout>()
        .expect("有技能栏")
        .0[slot];

    // 放出去。用的是**真内容**（`shield_block` 前摇 2 秒、冷却 1 秒），所以按秒推进：
    // 先跨过前摇，再在冷却里读倒计时。
    app.world_mut().write_message(CastRequest {
        caster: actor,
        ability,
    });
    for _ in 0..3 {
        step(&mut app, 1.0);
    }
    // 进冷却之后**只推进一点点**，别让 1 秒的冷却整个走完。
    step(&mut app, 0.3);

    let remaining =
        cooling_remaining(&mut app, ability).expect("冷却中该能读到自己那条延时边的计时器");
    // 冷却时长不写死在测试里：读**技能自己的 `Cooldown` 属性**（同一份内容算出来的）。
    let cooldown = app
        .world()
        .entity(ability)
        .get::<voxelith_abilities::Attributes>()
        .expect("技能根上种了计时属性")
        .value("Cooldown");
    assert!(
        remaining > 0.0 && remaining <= cooldown,
        "剩余冷却该落在 (0, {cooldown}] 里，实际 {remaining}"
    );

    // корotко 推进一点：剩余该变少（读的是状态机自己的计时器，不是本层另记的一份）。
    step(&mut app, 0.2);
    let later = cooling_remaining(&mut app, ability).expect("还在冷却");
    assert!(later < remaining, "倒计时该往下走：{remaining} → {later}");
}

#[test]
fn the_remaining_time_is_formatted_for_humans() {
    assert_eq!(voxelith_prime::presentation::format_remaining(0.54), "0.5s");
    assert_eq!(voxelith_prime::presentation::format_remaining(-3.0), "0.0s");
}
