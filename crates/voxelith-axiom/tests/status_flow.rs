//! 集成测试：状态的**生命周期与派生修饰符**（半即时战斗的时间语义）。
//!
//! 与 `combat_flow.rs` 的分工：那边管行动与相位，这边管状态。

#[allow(dead_code)] // 脚手架为两个测试文件共用，各文件只用到一部分
mod support;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::state::State;

use voxelith_axiom::atoms::actor::{ActorState, Resources, Stats};
use voxelith_axiom::behaviors::content::{SkillCatalog, StatId, StatusCatalog, StatusId};
use voxelith_axiom::behaviors::effect::Effect;
use voxelith_axiom::behaviors::phase::CombatPhase;
use voxelith_axiom::behaviors::skill::SkillTags;
use voxelith_axiom::behaviors::status::{ActiveStatus, AttachedTo, RemovalReason, StatusDef};
use voxelith_axiom::behaviors::value::{Value, Who};

use support::*;

/// 给玩家挂一个"每周期掉 2 点"的中毒状态（`duration` 秒）。
fn poison(app: &mut App, player: Entity, duration: f32) -> Entity {
    let def = app
        .world_mut()
        .spawn(StatusDef {
            id: StatusId(0),
            name: "poison".into(),
            default_duration: duration,
            stacking: voxelith_axiom::behaviors::status::Stacking::Refresh,
            modifiers: Vec::new(),
            blocks_tags: SkillTags::NONE,
            on_apply: Vec::new(),
            on_tick: vec![Effect::ModifyResource {
                pool: HP,
                delta: Value::Literal(-2.0),
                who: Who::Target,
            }],
            on_expire: Vec::new(),
            on_remove: Vec::new(),
        })
        .id();
    let status = app
        .world_mut()
        .spawn((
            ActiveStatus {
                def,
                id: Some(StatusId(0)),
                remaining: duration,
                stacks: 1,
                source: player,
                tick_accumulator: 0.0,
            },
            AttachedTo(player),
        ))
        .id();
    app.world_mut()
        .get_mut::<ActorState>(player)
        .unwrap()
        .statuses
        .push(status);
    status
}

#[test]
fn status_ticks_only_while_time_flows() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    // 时间要流动，玩家就不能空槽（空槽 → 相位切 `AwaitingInput` → 时间冻结）。
    keep_resolving(&mut app, player);
    poison(&mut app, player, 5.0);

    step(&mut app, 1.1);

    assert!(
        app.world().get::<Resources>(player).unwrap().current(HP) < 100.0,
        "时间流动时状态每周期结算"
    );
}

#[test]
fn frozen_time_stops_status_ticks() {
    // 玩家空槽 → 相位 `AwaitingInput` → 倍率 0 → `delta` 0：
    // 状态的倒计时与 `on_tick` 都必须停住（这就是"半即时"的暂停语义）。
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    poison(&mut app, player, 5.0);

    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::AwaitingInput,
        "空槽等待输入"
    );

    // 冻结的生效延迟是**恰好一帧**（见 `time_scale::drive_virtual_time` 的说明），
    // 所以这一帧还会跳一次伤害——它是"冻结前最后一帧"的，不是 bug。
    step(&mut app, 3.0);

    // 从"已经冻实"的那一刻取基准，再推进 6 秒：池与倒计时都不该动。
    let before = app.world().get::<Resources>(player).unwrap().current(HP);
    let remaining_before = {
        let mut statuses = app.world_mut().query::<&ActiveStatus>();
        statuses.iter(app.world()).next().unwrap().remaining
    };
    step(&mut app, 3.0);
    step(&mut app, 3.0);

    assert_eq!(
        app.world().get::<Resources>(player).unwrap().current(HP),
        before,
        "冻结时中毒不再跳伤害"
    );
    let remaining_after = {
        let mut statuses = app.world_mut().query::<&ActiveStatus>();
        statuses.iter(app.world()).next().unwrap().remaining
    };
    assert_eq!(
        remaining_after, remaining_before,
        "冻结时倒计时也不走：{remaining_before} → {remaining_after}"
    );
}

#[test]
fn status_expires_and_its_modifiers_disappear() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());

    let def = app
        .world_mut()
        .spawn(StatusDef {
            id: StatusId(0),
            name: "rage".into(),
            default_duration: 1.0,
            stacking: voxelith_axiom::behaviors::status::Stacking::Refresh,
            modifiers: vec![voxelith_axiom::behaviors::status::ModifierDef {
                stat: StatId(0),
                delta: voxelith_axiom::behaviors::value::Value::Literal(5.0),
            }],
            blocks_tags: SkillTags::NONE,
            on_apply: Vec::new(),
            on_tick: Vec::new(),
            on_expire: Vec::new(),
            on_remove: Vec::new(),
        })
        .id();
    let status = app
        .world_mut()
        .spawn((
            ActiveStatus {
                def,
                id: Some(StatusId(0)),
                remaining: 1.0,
                stacks: 1,
                source: player,
                tick_accumulator: 0.0,
            },
            AttachedTo(player),
        ))
        .id();
    app.world_mut()
        .get_mut::<ActorState>(player)
        .unwrap()
        .statuses
        .push(status);

    app.update();
    assert_eq!(
        app.world().get::<Stats>(player).unwrap().get(StatId(0)),
        15.0,
        "状态派生的修饰符进了最终值"
    );

    step(&mut app, 1.5);
    step(&mut app, 0.0);

    assert!(app.world().get_entity(status).is_err(), "到期即销毁实例");
    assert_eq!(
        app.world().get::<Stats>(player).unwrap().get(StatId(0)),
        10.0,
        "状态没了，加成自然消失"
    );
}

#[test]
fn detach_reasons_are_distinguishable() {
    // `RemovalReason` 必须能区分"自然到期"与"被主动移除"（on_expire vs on_remove）。
    assert_ne!(RemovalReason::Expired, RemovalReason::Removed);
}

#[test]
fn skill_catalog_and_status_catalog_start_empty() {
    let app = test_app();
    assert!(
        app.world().get_resource::<SkillCatalog>().is_none(),
        "目录由内容层注入，机制层不预置"
    );
    assert!(app.world().get_resource::<StatusCatalog>().is_none());
}
