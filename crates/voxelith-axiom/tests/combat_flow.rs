//! L1 集成测试：半即时战斗的**行动与相位**契约。
//!
//! 这些测试用**真实的 App**（不是单元测试桩），因为要钉住的正是"系统顺序 + 命令应用时机"
//! 这类只在完整调度里才成立的性质：
//!
//! 1. 瞬发技能**同帧**结算且从不占行动槽；
//! 2. 冻结相位下行动**不推进**（倍率 0 → `delta` 0）；
//! 3. `ActiveActions` 是行动槽的唯一载体，despawn 后自动空出来；
//! 4. 敌我判定走 `Faction`，目标由 `Targeting` 解析（两者都不看标记组件）。
//!
//! 状态的生命周期与派生修饰符在 [`status_flow`](../status_flow.rs) 里测。

#[allow(dead_code)] // 脚手架为两个测试文件共用，各文件只用到一部分
mod support;

use bevy_state::state::State;
use bevy_time::{Time, Virtual};

use voxelith_axiom::atoms::actor::{Faction, Resources};
use voxelith_axiom::behaviors::action::{Action, ActiveActions, CastRequest};
use voxelith_axiom::behaviors::content::SkillId;
use voxelith_axiom::behaviors::effect::Effect;
use voxelith_axiom::behaviors::phase::{AvailableSkills, CombatPhase, PendingThreat};
use voxelith_axiom::behaviors::requirement::{Requirement, Targeting};
use voxelith_axiom::behaviors::skill::{Skill, SkillTags};

use support::*;

#[test]
fn target_is_enemy_resolves_through_faction() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let enemy = spawn_monster(app.world_mut(), Faction::Monster);
    // 一个"只打敌人"的技能：它既要有 `TargetIsEnemy` 需求，也要有能找出目标的 `Targeting`。
    app.world_mut().spawn(Skill {
        id: SkillId(0),
        name: "strike".into(),
        icon: String::new(),
        tags: SkillTags::ATTACK,
        roles: Vec::new(),
        duration: 0.0,
        requirements: vec![Requirement::TargetIsEnemy],
        costs: Vec::new(),
        targeting: Targeting::NearestEnemy,
        effects: Vec::new(),
    });

    app.update();
    app.update();

    // 有敌对阵营在场 → 技能可用（修复前这里恒为空：目标从没被解析过）。
    let available = app.world().resource::<AvailableSkills>();
    assert_eq!(
        available.len(),
        1,
        "敌对目标在场时该技能可用：{available:?}"
    );

    // 把怪物的阵营改成与玩家相同 → 不再是敌人 → 技能不可用。
    *app.world_mut().get_mut::<Faction>(enemy).unwrap() = Faction::Player;
    app.update();
    app.update();
    let available = app.world().resource::<AvailableSkills>();
    assert!(available.is_empty(), "同阵营不该被判为敌人：{available:?}");
}

#[test]
fn availability_is_recomputed_while_time_is_frozen() {
    // 相位冻结后（玩家空槽等输入）可用技能仍然每帧重算：
    // 否则"敌人换阵营 / 状态到期"之后 UI 会一直显示过期的按钮。
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let enemy = spawn_monster(app.world_mut(), Faction::Monster);
    app.world_mut().spawn(Skill {
        id: SkillId(0),
        name: "strike".into(),
        icon: String::new(),
        tags: SkillTags::ATTACK,
        roles: Vec::new(),
        duration: 0.0,
        requirements: vec![Requirement::TargetIsEnemy],
        costs: Vec::new(),
        targeting: Targeting::NearestEnemy,
        effects: Vec::new(),
    });

    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::AwaitingInput,
        "玩家空槽 → 时间冻结"
    );
    assert_eq!(app.world().resource::<AvailableSkills>().len(), 1);

    *app.world_mut().get_mut::<Faction>(enemy).unwrap() = Faction::Player;
    app.update();
    app.update();
    assert!(
        app.world().resource::<AvailableSkills>().is_empty(),
        "冻结时也要重算可用技能"
    );
}

#[test]
fn casting_a_target_enemy_skill_reaches_the_target() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    let enemy = spawn_monster(app.world_mut(), Faction::Monster);
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "strike",
        0.0,
        SkillTags::ATTACK,
        vec![Effect::ModifyResource {
            pool: HP,
            delta: voxelith_axiom::behaviors::value::Value::Literal(-10.0),
            who: voxelith_axiom::behaviors::value::Who::Target,
        }],
    );
    app.world_mut().get_mut::<Skill>(skill).unwrap().targeting = Targeting::NearestEnemy;
    app.world_mut()
        .get_mut::<Skill>(skill)
        .unwrap()
        .requirements = vec![Requirement::TargetIsEnemy];

    app.world_mut().write_message(CastRequest {
        caster: player,
        skill,
        target: None,
    });
    app.update();

    assert_eq!(
        app.world().get::<Resources>(enemy).unwrap().current(HP),
        40.0,
        "目标解析 + 阵营判定都通了，伤害落到敌人身上"
    );
    assert_eq!(
        app.world().get::<Resources>(player).unwrap().current(HP),
        100.0,
        "别打到自己"
    );
}

#[test]
fn instant_skill_resolves_in_the_same_frame_without_taking_the_slot() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "instant",
        0.0,
        SkillTags::ATTACK,
        vec![Effect::ModifyResource {
            pool: HP,
            delta: voxelith_axiom::behaviors::value::Value::Literal(-10.0),
            who: voxelith_axiom::behaviors::value::Who::Caster,
        }],
    );

    // 注册目录（`SkillCatalog` 是 Resource，结算要用它做 `SpawnAction`；这里直接给请求）。
    app.world_mut().write_message(CastRequest {
        caster: player,
        skill,
        target: None,
    });
    app.update();

    let pools = app.world().get::<Resources>(player).expect("玩家有池");
    assert_eq!(pools.current(HP), 90.0, "瞬发技能同帧就生效");

    // 行动实体已经销毁、槽也空着（没有残渣）。
    let mut actions = app.world_mut().query::<&Action>();
    assert_eq!(actions.iter(app.world()).count(), 0, "瞬发不留行动实体");

    let slot = app.world().get::<ActiveActions>(player);
    assert!(
        slot.is_none_or(|slot| slot.is_empty()),
        "瞬发从不真正占槽：{slot:?}"
    );
}

#[test]
fn timed_action_takes_the_slot_until_it_resolves() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "slow",
        1.0,
        SkillTags::ATTACK,
        vec![Effect::ModifyResource {
            pool: HP,
            delta: voxelith_axiom::behaviors::value::Value::Literal(-10.0),
            who: voxelith_axiom::behaviors::value::Who::Caster,
        }],
    );

    app.world_mut().write_message(CastRequest {
        caster: player,
        skill,
        target: None,
    });
    app.update();

    // 还没到时长：行动实体在、槽里有东西、效果没生效。
    assert_eq!(
        app.world().get::<Resources>(player).unwrap().current(HP),
        100.0
    );
    assert!(
        app.world()
            .get::<ActiveActions>(player)
            .is_some_and(|slot| !slot.is_empty()),
        "持续行动占着槽"
    );

    // 推进 5 秒（远超 max_delta 的 250ms 上限，确保一次就过时长），下一帧结算。
    step(&mut app, 5.0);

    assert_eq!(
        app.world().get::<Resources>(player).unwrap().current(HP),
        90.0,
        "到时长即结算"
    );
    assert!(
        app.world()
            .get::<ActiveActions>(player)
            .is_none_or(|slot| slot.is_empty()),
        "结算后槽自动空出来"
    );
}

#[test]
fn frozen_phase_stops_action_progress() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "slow",
        5.0,
        SkillTags::ATTACK,
        Vec::new(),
    );

    app.world_mut().write_message(CastRequest {
        caster: player,
        skill,
        target: None,
    });
    app.update();

    // 有持续行动 → 相位不该是"等输入"（玩家不空闲）。
    assert_ne!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::AwaitingInput,
        "有行动在跑时不冻结"
    );

    // 冻结：倍率 0 时逻辑系统读到的 delta 是 0，行动不会推进。
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_relative_speed(0.0);
    let before = {
        let mut query = app.world_mut().query::<&Action>();
        query.iter(app.world()).next().map(|action| action.elapsed)
    };
    step(&mut app, 1.0);
    let after = {
        let mut query = app.world_mut().query::<&Action>();
        query.iter(app.world()).next().map(|action| action.elapsed)
    };
    assert_eq!(before, after, "冻结时行动进度不动");
    assert!(before.is_some());
}

#[test]
fn timed_skill_is_not_started_while_the_slot_is_busy() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    let first = spawn_skill(
        app.world_mut(),
        0,
        "first",
        2.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    let second = spawn_skill(
        app.world_mut(),
        1,
        "second",
        2.0,
        SkillTags::ATTACK,
        Vec::new(),
    );

    app.world_mut().write_message(CastRequest {
        caster: player,
        skill: first,
        target: None,
    });
    app.update();
    app.world_mut().write_message(CastRequest {
        caster: player,
        skill: second,
        target: None,
    });
    app.update();

    let mut query = app.world_mut().query::<&Action>();
    assert_eq!(
        query.iter(app.world()).count(),
        1,
        "槽位约束：len <= 1，第二个请求被拒"
    );
}

#[test]
fn pending_threat_opens_the_counter_window() {
    let mut app = test_app();
    let player = spawn_player(app.world_mut());
    // 一个带 COUNTER 标签、`HasThreat` 需求的技能（内容里"反击"就是这个形状）。
    app.world_mut().spawn(Skill {
        id: SkillId(0),
        name: "riposte".into(),
        icon: String::new(),
        tags: SkillTags::COUNTER,
        roles: Vec::new(),
        duration: 0.0,
        requirements: vec![voxelith_axiom::behaviors::requirement::Requirement::HasThreat],
        costs: Vec::new(),
        targeting: Targeting::ThreatSource,
        effects: Vec::new(),
    });
    // 威胁行动必须带 `Threat` 标记：`monster_decide` 每帧会清掉"已经不存在 / 没标记"的挂起威胁。
    let threat_action = app
        .world_mut()
        .spawn(voxelith_axiom::behaviors::monster::Threat)
        .id();

    // 玩家**不空闲**：真的放一个持续行动进槽（反制窗口只在"玩家正忙"时才有意义——
    // 空槽时相位优先走"等输入"，见 docs/combat-design.md §6 的判据表）。
    app.world_mut().spawn((
        Action {
            elapsed: 0.0,
            duration: 30.0,
            target: None,
        },
        voxelith_axiom::behaviors::action::InitiatedBy(player),
    ));
    app.update();
    assert!(
        app.world()
            .get::<ActiveActions>(player)
            .is_some_and(|slot| !slot.is_empty()),
        "行动槽里有东西（关系组件维护）"
    );

    // 有威胁 → 反击可用 → 反制窗口。
    app.world_mut().insert_resource(PendingThreat {
        action: Some(threat_action),
        source: None,
        target: Some(player),
    });
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::AwaitingCounter,
        "有威胁 + 有反制 → 反制窗口"
    );

    // 威胁清掉 → 反击不可用 → 回到流动。
    app.world_mut().insert_resource(PendingThreat::default());
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<CombatPhase>>().get(),
        CombatPhase::Resolving,
        "没威胁就不再是反制窗口"
    );
}
