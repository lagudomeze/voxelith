//! L1 集成测试：**决策槽**（怪物"已经决定、还没出手"的那条决策）。
//!
//! 决策槽是怪物身上的一张**一对一**关系：
//!
//! ```text
//! 决策实体 ──DecidedBy──► 怪物       反向集 DecisionSlot（单个 Entity，linked_spawn）
//! 决策实体 ──SettledBy──► 行动       反向集 Settles（单个 Entity，linked_spawn）
//! ```
//!
//! `monster_ai.rs` 测的是"怎么选出这一招"；这边测**选出来之后存在哪、什么时候消失**。
//!
//! 下面这些路径**错了都不会报错**，只是行为悄悄不对，所以每条都单独钉一个测试：
//!
//! 1. 决定先于执行（窗口只有一格，没轮到的先存着）；
//! 2. 决定写下之后条件再变也不重掷；
//! 3. 行动没了 → 槽自动空出来（包括威胁登记被整个抹掉的反制路径）；
//! 4. 一对一"换源"只解绑不销毁 → 不许留下孤儿决策实体；
//! 5. 怪物没了 → 决策跟着走；
//! 6. 执行不了的决策要退役，不能把槽永久占住。
//!
//! **这些用例一律用 [`charge`] 顶能量、用 `app.update()`（`delta = 0`）走帧**，
//! 不去"推进足够时间"。理由见 [`charge`]：结论不该依赖时间倍率有没有被冻结。

#[allow(dead_code)] // 脚手架为多个测试文件共用，各文件只用到一部分
mod support;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use voxelith_axiom::atoms::actor::{ActionEnergy, ActorState, Faction};
use voxelith_axiom::behaviors::action::CastsSkill;
use voxelith_axiom::behaviors::content::{SkillCatalog, SkillId, StatusId};
use voxelith_axiom::behaviors::monster::{
    AiChoice, AiDecision, DecisionSlot, SettledBy, decision_of,
};
use voxelith_axiom::behaviors::phase::PendingThreat;
use voxelith_axiom::behaviors::requirement::Condition;
use voxelith_axiom::behaviors::skill::SkillTags;

use support::*;

/// 把技能登记进目录（`monster_decide` 通过目录找技能实体）。
fn register(app: &mut App, skill: Entity) {
    let mut catalog = SkillCatalog::default();
    catalog.insert(SkillId(0), skill);
    app.world_mut().insert_resource(catalog);
}

/// 无条件考虑的候选。
fn always(skill: u16, weight: f32) -> AiChoice {
    AiChoice {
        skill: SkillId(skill),
        when: Condition::Always,
        weight,
    }
}

/// 把怪物的能量**直接顶到阈值**：下一步它就会决定。
///
/// 用它而不是"推进足够时间"，是为了让结论**不依赖时间倍率**。玩家空槽时相位会切到
/// `AwaitingInput` 把 `delta` 冻成 0（那是设计要的行为，见 docs/combat-design.md §6），
/// 而 `ActionEnergy::tick` 是"先累加再判"——`current` 已经到阈值的怪物在 `delta = 0`
/// 的帧上照样出手。于是"决策槽什么时候空、什么时候满"这件事跟时间冻不冻结彻底无关。
fn charge(app: &mut App, monster: Entity) {
    let threshold = app
        .world()
        .get::<ActionEnergy>(monster)
        .expect("只有带 `ActionEnergy` 的怪物才攒能量")
        .threshold;
    if let Some(mut energy) = app.world_mut().get_mut::<ActionEnergy>(monster) {
        energy.current = threshold;
    }
}

/// 怪物槽里的那条决策实体（`None` = 还没决定）。
fn held_decision(app: &App, monster: Entity) -> Option<Entity> {
    app.world()
        .get::<DecisionSlot>(monster)
        .and_then(|slot| decision_of(Some(slot)))
}

/// 世界上还活着的决策实体数。
fn decision_count(app: &mut App) -> usize {
    let mut query = app.world_mut().query::<&AiDecision>();
    query.iter(app.world()).count()
}

/// 有槽的怪物数。
fn slotted_monsters(app: &mut App) -> usize {
    let mut query = app
        .world_mut()
        .query_filtered::<Entity, With<DecisionSlot>>();
    query.iter(app.world()).count()
}

/// 这一步是谁出的手？返回（出手的，还在等的）。
fn acting_and_waiting(app: &App, first: Entity, second: Entity) -> (Entity, Entity) {
    let source = app
        .world()
        .resource::<PendingThreat>()
        .source
        .expect("有人出手了");
    let waiting = if source == first { second } else { first };
    (source, waiting)
}

/// 把窗口腾空：销毁威胁行动，并把 `PendingThreat` **整个**清掉。
///
/// 后者正是 `Effect::DispelAction` 的写法（连 `source` 都不留）。
fn free_the_window(app: &mut App) {
    if let Some(action) = app.world().resource::<PendingThreat>().action {
        app.world_mut().entity_mut(action).despawn();
    }
    app.world_mut().insert_resource(PendingThreat::default());
}

/// **决定先于执行**：只有一格威胁窗口，没轮到的怪物也已经决定了，只是还没出手。
#[test]
fn a_monster_decides_before_it_gets_to_act() {
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
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );
    let second = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, first);
    charge(&mut app, second);
    app.update();
    let (acting, waiting) = acting_and_waiting(&app, first, second);

    // 出手的那个：它的决策已经挂到行动上了。
    let taken = held_decision(&app, acting).expect("出手的怪物当然有决策");
    let settled = app
        .world()
        .get::<SettledBy>(taken)
        .expect("决策挂到了行动上");
    assert_eq!(
        Some(settled.0),
        app.world().resource::<PendingThreat>().action,
        "决策挂的是它自己那条行动"
    );

    // 等着的那个：**已经决定了，但还没有行动**。这是决策槽存在的全部理由。
    let pending = held_decision(&app, waiting).expect("等窗口的怪物也已经决定了");
    assert!(
        app.world().get::<SettledBy>(pending).is_none(),
        "还没轮到它出手，决策不该挂到任何行动上"
    );
    let decision = app
        .world()
        .get::<AiDecision>(pending)
        .expect("槽里就是决策本身");
    assert_eq!(decision.skill, SkillId(0));
    assert_eq!(decision.skill_entity, skill, "决定那一刻就解析好了技能实体");
}

/// **决定一旦写下就不改主意**：等窗口的这几帧里条件变了，也照原计划打。
#[test]
fn a_decided_monster_keeps_its_choice_while_it_waits() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    let plain = spawn_skill(
        app.world_mut(),
        0,
        "slash",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    let frenzy = spawn_skill(
        app.world_mut(),
        1,
        "frenzy",
        1.0,
        SkillTags::ATTACK,
        Vec::new(),
    );

    let mut catalog = SkillCatalog::default();
    catalog.insert(SkillId(0), plain);
    catalog.insert(SkillId(1), frenzy);
    app.world_mut().insert_resource(catalog);

    // 5.0 权重那招**只有中毒时才参与**：没中毒时它连候选都算不上。
    let choices = || {
        vec![
            always(0, 1.0),
            AiChoice {
                skill: SkillId(1),
                when: Condition::HasStatus(StatusId(3)),
                weight: 5.0,
            },
        ]
    };
    let first = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        choices(),
        EnergySpec::charging(1.0, 1.0),
    );
    let second = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        choices(),
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, first);
    charge(&mut app, second);
    app.update();
    let (acting, waiting) = acting_and_waiting(&app, first, second);

    // 决定是在**没中毒**的时候做的 → 选中 1.0 权重那招。
    let held = held_decision(&app, waiting).expect("等窗口的怪物已经决定了");
    assert_eq!(
        app.world().get::<AiDecision>(held).map(|d| d.skill_entity),
        Some(plain),
        "没中毒时 5.0 权重那招不参与"
    );

    // 现在让它中毒（5.0 权重那招够格了），再把窗口腾出来。
    attach_status(
        app.world_mut(),
        waiting,
        3,
        "poison",
        10.0,
        SkillTags::NONE,
        Vec::new(),
    );
    free_the_window(&mut app);
    app.world_mut().entity_mut(acting).despawn();

    app.update();
    let fired = app
        .world()
        .resource::<PendingThreat>()
        .action
        .expect("轮到它了");
    assert_eq!(
        app.world().get::<CastsSkill>(fired).map(|cast| cast.0),
        Some(plain),
        "中毒了也照原计划打：决定写下之后条件再变也不重掷"
    );

    // **反向对照**：这条中毒真的看得见——重新顶满能量再走一帧，它就改选 5.0 权重那招了。
    // 没有这一段的话，"没改主意"也可能只是因为条件根本没生效过。
    free_the_window(&mut app);
    charge(&mut app, waiting);
    app.update();
    let re_decided = app
        .world()
        .resource::<PendingThreat>()
        .action
        .expect("重新决定之后又出手了");
    assert_eq!(
        app.world().get::<CastsSkill>(re_decided).map(|cast| cast.0),
        Some(frenzy),
        "中毒真的进了门控：重新决定时 5.0 权重那招胜出"
    );
}

/// **行动没了，槽就空出来**——哪怕威胁登记被整个抹掉（`Effect::DispelAction` 就是这么干的）。
///
/// 这条路径是"靠每帧反推谁该退役"的设计的死角：`DispelAction` 把 `PendingThreat` 连
/// `source` 一起清空，反推无从下手，决策会永久卡在槽里——怪物再也不出手，且什么都不报。
/// 决策挂在行动上（`linked_spawn`），所以这里根本不需要反推。
#[test]
fn the_slot_empties_even_when_the_threat_is_wiped_wholesale() {
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

    let monster = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, monster);
    app.update();
    assert!(held_decision(&app, monster).is_some());

    // 模仿 `Effect::DispelAction`：行动被销毁 + `PendingThreat` 连 `source` 一起清空。
    free_the_window(&mut app);
    app.update();

    assert!(
        held_decision(&app, monster).is_none(),
        "行动没了，决策必须跟着没"
    );
    assert!(
        app.world().get::<DecisionSlot>(monster).is_none(),
        "槽空 = 连组件都没有（0.19 的 `RelationshipTarget` 表达不了 `Option<Entity>`）"
    );

    // 而且它没有被卡死：攒回能量还能再出手。
    charge(&mut app, monster);
    app.update();
    assert!(
        app.world().resource::<PendingThreat>().action.is_some(),
        "槽空出来之后怪物还能重新决定、重新出手"
    );
}

/// **等窗口的怪物不会每帧重写一条决策**（一对一关系的孤儿泄漏防线）。
///
/// 窗口被占着时，等着的怪物有可能反复满足"能量满"；少了 `monster_decide` 里那句
/// "槽里已经有决策 → 跳过"，它每帧都会写下一条新决策。而一对一关系在新源顶掉旧源时
/// **只解绑、不销毁**旧实体——于是每帧漏一个孤儿，而且**什么都不报**。
#[test]
fn a_waiting_monster_does_not_rewrite_its_decision_every_frame() {
    let mut app = test_app();
    let _player = spawn_player(app.world_mut());
    // 10 秒的长招：窗口会被占住很久。
    let skill = spawn_skill(
        app.world_mut(),
        0,
        "long-slash",
        10.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    register(&mut app, skill);

    let first = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );
    let second = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, first);
    charge(&mut app, second);
    app.update();
    let (_, waiting) = acting_and_waiting(&app, first, second);
    assert_eq!(
        (decision_count(&mut app), slotted_monsters(&mut app)),
        (2, 2),
        "两只各写下一条决策"
    );

    // 连着好几帧把等着的那只顶到阈值：它不该再写第二条。
    for round in 0..3 {
        charge(&mut app, waiting);
        app.update();
        let alive = decision_count(&mut app);
        let slotted = slotted_monsters(&mut app);
        assert_eq!(
            alive, slotted,
            "第 {round} 轮漏了孤儿决策：世界上 {alive} 条决策，但只有 {slotted} 个槽"
        );
    }
}

/// **怪物没了，决策跟着走**（`DecisionSlot` 的 `linked_spawn`）。
#[test]
fn a_monster_takes_its_decision_down_with_it() {
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

    let monster = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, monster);
    app.update();
    let decision = held_decision(&app, monster).expect("决定了");
    app.world_mut().entity_mut(monster).despawn();
    // 级联销毁是排队命令，落一帧才看得见。
    app.update();

    assert!(
        app.world().get_entity(decision).is_err(),
        "决策不能脱离它的怪物独立存在"
    );
}

/// **执行不了的决策要退役**：技能实体被搬走之后，那条决策永远执行不了。
///
/// 不退役的话槽被永久占住——怪物再也不出手，而且**什么都不报**。
#[test]
fn a_decision_whose_skill_vanished_does_not_jam_the_slot() {
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
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );
    let second = spawn_ai_monster(
        app.world_mut(),
        Faction::Monster,
        vec![always(0, 1.0)],
        EnergySpec::charging(1.0, 1.0),
    );

    charge(&mut app, first);
    charge(&mut app, second);
    app.update();
    let (_, waiting) = acting_and_waiting(&app, first, second);
    assert!(
        held_decision(&app, waiting).is_some(),
        "等窗口的怪物手里有一条决策"
    );

    // 窗口腾空 + 技能被搬走。
    free_the_window(&mut app);
    app.world_mut().entity_mut(skill).despawn();

    app.update();
    assert!(
        held_decision(&app, waiting).is_none(),
        "执行不了的决策必须退役，否则槽被永久占住"
    );
    assert!(
        app.world().get::<ActorState>(waiting).is_some(),
        "怪物本身还在（退役的是决策，不是怪物）"
    );
}
