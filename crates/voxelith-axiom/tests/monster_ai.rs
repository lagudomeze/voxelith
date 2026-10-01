//! L1 集成测试：怪物 AI（能量 → 选招 → 生成威胁行动）。
//!
//! 与 `combat_flow.rs` / `status_flow.rs` 的分工：这边只测"怪物怎么决定出手"。
//!
//! 钉住的契约：
//!
//! 1. 攒满能量才出手，且同一时刻只挂一个威胁；
//! 2. AI 候选的 `Condition` 看得见怪物**自己的状态**（`Condition::HasStatus`）；
//! 3. 选招按权重取最大、平局取靠前（确定性，同种子同结果）。

#[allow(dead_code)] // 脚手架为多个测试文件共用，各文件只用到一部分
mod support;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use voxelith_axiom::atoms::actor::{ActionEnergy, Faction};
use voxelith_axiom::behaviors::content::{SkillCatalog, SkillId};
use voxelith_axiom::behaviors::monster::{AiChoice, Threat};
use voxelith_axiom::behaviors::phase::PendingThreat;
use voxelith_axiom::behaviors::requirement::Condition;
use voxelith_axiom::behaviors::skill::SkillTags;

use support::*;

/// 把技能登记进目录（`monster_tick` 通过目录找技能实体）。
fn register(app: &mut App, skill: Entity) {
    let mut catalog = SkillCatalog::default();
    catalog.insert(SkillId(0), skill);
    app.world_mut().insert_resource(catalog);
}

#[test]
fn monster_waits_until_its_energy_fills() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "slash",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    register(&mut app, skill);
    // 能量攒得慢（1.0/秒）且从 0 开始 → 本帧不该出手。
    spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![AiChoice {
            skill: SkillId(0),
            when: Condition::Always,
            weight: 1.0,
        }],
        EnergySpec::charging(1.0, 1.0),
    );

    app.update();
    assert!(
        app.world().resource::<PendingThreat>().action.is_none(),
        "能量没攒满就不该生成威胁"
    );

    // 推进足够时间 → 攒满 → 出手并登记威胁。
    step(&mut app, 1.5);

    let threat = *app.world().resource::<PendingThreat>();
    assert!(threat.source.is_some(), "威胁来源是怪物自己");
    assert!(
        threat
            .action
            .is_some_and(|action| app.world().get::<Threat>(action).is_some()),
        "威胁行动带 `Threat` 标记"
    );
}

#[test]
fn ai_condition_sees_the_monsters_own_status() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let flee = spawn_skill(
        app.world_mut(),
        0,
        "flee",
        1.0,
        SkillTags::MOVEMENT,
        Vec::new(),
    );
    register(&mut app, flee);

    // 只有"中毒"时才会考虑的候选。
    let monster = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![AiChoice {
            skill: SkillId(0),
            when: Condition::HasStatus(voxelith_axiom::behaviors::content::StatusId(3)),
            weight: 1.0,
        }],
        EnergySpec::charging(1.0, 1.0),
    );

    app.update();
    assert!(
        app.world().resource::<PendingThreat>().action.is_none(),
        "没有该状态 → 候选不参与"
    );

    // 挂上对应状态 → 候选参与 → 出手。
    attach_status(
        app.world_mut(),
        monster,
        3,
        "poison",
        10.0,
        SkillTags::NONE,
        Vec::new(),
    );
    app.update();
    // 一帧让关系组件生效（`AttachedTo` 靠 `Commands` 延迟应用），再推进时间攒满能量。
    step(&mut app, 1.5);

    assert!(
        app.world().resource::<PendingThreat>().action.is_some(),
        "有该状态 → 候选参与 → 出手（`HasStatus` 真的看得见怪物自己的状态）"
    );
}

#[test]
fn only_one_threat_at_a_time() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "slash",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    register(&mut app, skill);

    let first = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![AiChoice {
            skill: SkillId(0),
            when: Condition::Always,
            weight: 1.0,
        }],
        EnergySpec::charging(1.0, 1.0),
    );
    let second = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![AiChoice {
            skill: SkillId(0),
            when: Condition::Always,
            weight: 1.0,
        }],
        EnergySpec::charging(1.0, 1.0),
    );

    step(&mut app, 1.5);
    let source = app.world().resource::<PendingThreat>().source;
    assert!(
        source == Some(first) || source == Some(second),
        "只该有一个威胁：{source:?}"
    );

    // 另一个怪物也攒满了：威胁仍然只挂一个（本帧先不出手）。
    let other = if source == Some(first) { second } else { first };
    app.world_mut()
        .get_mut::<ActionEnergy>(other)
        .unwrap()
        .current = 1.0;
    step(&mut app, 0.1);
    assert_eq!(
        app.world().resource::<PendingThreat>().source,
        source,
        "挂起威胁未清掉之前，别的怪物不能顶掉它"
    );
}

#[test]
fn ai_picks_the_highest_weight_and_breaks_ties_by_order() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let low = spawn_skill(
        app.world_mut(),
        0,
        "low",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    let high = spawn_skill(
        app.world_mut(),
        1,
        "high",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    let tie = spawn_skill(
        app.world_mut(),
        2,
        "tie",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );

    let mut catalog = SkillCatalog::default();
    catalog.insert(SkillId(0), low);
    catalog.insert(SkillId(1), high);
    catalog.insert(SkillId(2), tie);
    app.world_mut().insert_resource(catalog);

    let monster = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![
            AiChoice {
                skill: SkillId(0),
                when: Condition::Always,
                weight: 1.0,
            },
            AiChoice {
                skill: SkillId(2),
                when: Condition::Always,
                weight: 5.0,
            },
            AiChoice {
                skill: SkillId(1),
                when: Condition::Always,
                weight: 5.0,
            },
        ],
        EnergySpec::charging(1.0, 1.0),
    );

    step(&mut app, 1.5);
    let action = app
        .world()
        .resource::<PendingThreat>()
        .action
        .expect("出手了");
    let cast = app
        .world()
        .get::<voxelith_axiom::behaviors::action::CastsSkill>(action)
        .expect("行动记录用了哪个技能");
    assert_eq!(
        cast.0, tie,
        "权重相同时取靠前的候选（确定性，不靠 HashMap 顺序）"
    );
    let _ = monster;
}
