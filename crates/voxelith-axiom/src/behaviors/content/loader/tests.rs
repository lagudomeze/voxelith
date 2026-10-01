//! `loader` 的单元测试。
//!
//! 通过 `#[path]` 挂在 `loader` 模块下，所以本文件**就是** `loader::tests`，
//! `super` 指向 `loader` —— 与内联 `mod tests { use super::*; }` 时的写法完全一致。
//!
//! 单独成文件的原因是测试体量大于被测代码（拆开前 `mod.rs` 已经 560 行，越过 **R26**）。

use super::*;

use super::*;
// 只在测试里构造描述结构时用到的类型（生产路径走 `resolve_defs`）。
use crate::atoms::actor::{ActorTag, Faction};
use crate::behaviors::content::descriptor::{
    ActorRon, AiChoiceRon, ConditionRon, CostRon, EffectRon, ResourceRon, RoleRon, StatRon,
    StatusDefRon, TargetingRon, ValueRon, WhoRon,
};
use crate::behaviors::skill::Skill;

fn vocabulary() -> VocabRon {
    VocabRon {
        resources: vec![
            ResourceRon {
                id: "hp".into(),
                name: "生命".into(),
                max: 100.0,
                regen: 0.0,
                start_full: true,
            },
            ResourceRon {
                id: "action".into(),
                name: "行动".into(),
                max: 1.0,
                regen: 0.0,
                start_full: true,
            },
        ],
        stats: vec![StatRon {
            id: "strength".into(),
            name: "力量".into(),
        }],
        statuses: vec![StatusDefRon {
            id: "stunned".into(),
            name: "眩晕".into(),
        }],
    }
}

fn attack_skill() -> SkillRon {
    SkillRon {
        id: "basic_attack".into(),
        name: "普通攻击".into(),
        icon: "attack.png".into(),
        tags: vec![crate::behaviors::skill::SkillTag::ATTACK],
        roles: Vec::new(),
        duration: 1.0,
        requirements: vec![RequirementRon::Resource {
            pool: "action".into(),
            min: 1.0,
        }],
        costs: vec![CostRon {
            pool: "action".into(),
            amount: 1.0,
        }],
        targeting: TargetingRon::CurrentSelection,
        effects: vec![EffectRon::ModifyResource {
            pool: "hp".into(),
            delta: ValueRon::Neg(Box::new(ValueRon::CasterStat("strength".into()))),
            who: WhoRon::Target,
        }],
    }
}

#[test]
fn strings_are_resolved_into_ids() {
    let mut world = World::new();
    let mut commands = world.commands();
    let content = load_all(
        &mut commands,
        &vocabulary(),
        &[attack_skill()],
        &[],
        &[],
        &WorldRon::default(),
    )
    .expect("词汇表与技能都配齐了");

    let strength = content.vocab.stat("strength").unwrap();
    let hp = content.vocab.resource("hp").unwrap();
    assert_eq!(content.vocab.resource_name(hp), "hp");

    let skill_id = content.vocab.skill("basic_attack").unwrap();
    let entity = *content.skills.get(skill_id).unwrap();
    let _ = world.flush();
    let skill = world.get::<Skill>(entity).unwrap();
    assert_eq!(
        skill.costs[0].pool,
        content.vocab.resource("action").unwrap()
    );
    assert_eq!(skill.requirements.len(), 1);
    match &skill.effects[0] {
        crate::behaviors::effect::Effect::ModifyResource { pool, .. } => {
            assert_eq!(*pool, hp)
        }
        other => panic!("效果应为 ModifyResource，实为 {other:?}"),
    }
    assert_eq!(skill.requirements.len(), 1);
    assert_eq!(strength, content.vocab.stat("strength").unwrap());
}

#[test]
fn unknown_pool_names_are_rejected_at_load_time() {
    let mut world = World::new();
    let mut commands = world.commands();
    let mut skill = attack_skill();
    skill.costs[0].pool = "mana".into();

    let error = load_all(
        &mut commands,
        &vocabulary(),
        &[skill],
        &[],
        &[],
        &WorldRon::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, LoaderError::UnknownName(_)),
        "未知池名必须在加载期报错，实为 {error:?}"
    );
}

#[test]
fn declared_but_unused_pools_are_rejected() {
    let mut world = World::new();
    let mut commands = world.commands();
    // 只有 hp 被用到，action 没人引用 → 报错。
    let skill = SkillRon {
        costs: Vec::new(),
        requirements: Vec::new(),
        ..attack_skill()
    };
    let error = load_all(
        &mut commands,
        &vocabulary(),
        &[skill],
        &[],
        &[],
        &WorldRon::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        LoaderError::UnusedResource {
            name: "action".into()
        }
    );
}

#[test]
fn actor_ai_references_must_exist() {
    let mut world = World::new();
    let mut commands = world.commands();
    let monster = ActorRon {
        id: "goblin".into(),
        name: "哥布林".into(),
        role: RoleRon::Monster,
        ai: vec![AiChoiceRon {
            skill: "goblin_slash".into(),
            when: ConditionRon::Always,
            weight: 1.0,
        }],
        ..Default::default()
    };
    let error = load_all(
        &mut commands,
        &vocabulary(),
        &[attack_skill()],
        &[],
        &[monster],
        &WorldRon::default(),
    )
    .unwrap_err();
    assert!(matches!(error, LoaderError::UnknownSkill { .. }));
}

#[test]
fn monster_role_requires_ai_choices() {
    // `role: Monster` 却没有 AI → 生成出来是个站着挨打的靶子，必须在启动期报错。
    let mut world = World::new();
    let mut commands = world.commands();
    let monster = ActorRon {
        id: "statue".into(),
        name: "石像".into(),
        role: RoleRon::Monster,
        ..Default::default()
    };
    let error = load_all(
        &mut commands,
        &vocabulary(),
        &[attack_skill()],
        &[],
        &[monster],
        &WorldRon::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        LoaderError::MonsterWithoutAi {
            id: "statue".into()
        }
    );
}

#[test]
fn duplicate_actor_ids_are_rejected() {
    let mut world = World::new();
    let mut commands = world.commands();
    // 用 `role: Player` 构造，免得先撞上"怪物必须有 AI"那条检查。
    let one = ActorRon {
        id: "goblin".into(),
        role: RoleRon::Player,
        ..Default::default()
    };
    let two = ActorRon {
        id: "goblin".into(),
        role: RoleRon::Player,
        ..Default::default()
    };
    let error = load_all(
        &mut commands,
        &vocabulary(),
        &[attack_skill()],
        &[],
        &[one, two],
        &WorldRon::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        LoaderError::DuplicateActor {
            id: "goblin".into()
        }
    );
}

#[test]
fn actor_templates_carry_their_pools_and_stats() {
    let mut world = World::new();
    let mut commands = world.commands();
    let monster = ActorRon {
        id: "goblin".into(),
        name: "哥布林".into(),
        role: RoleRon::Monster,
        resources: vec![("hp".into(), 30.0), ("action".into(), 1.0)],
        stats: vec![("strength".into(), 4.0)],
        ai: vec![AiChoiceRon {
            skill: "basic_attack".into(),
            when: ConditionRon::Always,
            weight: 1.0,
        }],
        energy_rate: 1.0,
        energy_threshold: 3.0,
        faction: Faction::Monster,
        traits: vec![ActorTag::Undead],
    };

    let content = load_all(
        &mut commands,
        &vocabulary(),
        &[attack_skill()],
        &[],
        &[monster],
        &WorldRon::default(),
    )
    .expect("哥布林引用的是已定义的技能");

    assert_eq!(content.monsters.len(), 1);
    let template = &content.monsters[0];
    assert_eq!(template.key, "goblin");
    assert_eq!(template.resources.len(), 2);
    assert_eq!(template.stats[0].1, 4.0);
    assert_eq!(template.definition.ai.len(), 1);
    assert_eq!(content.pools.len(), 2, "池的生成参数也在产物里");
    assert_eq!(template.faction, Faction::Monster, "阵营原样带出");
    assert_eq!(template.traits, vec![ActorTag::Undead], "特性原样带出");
}

/// **端到端**：内容作者在 `.ron` 里写一条曲线，从解析到求值都走通。
///
/// 这条测试的意义是证明"数值设计**不用改代码**"——
/// 作者说清"1 级想要多少、5 级想要多少、上限在哪"，中间的形状由引擎给。
#[test]
fn a_scale_curve_written_in_ron_reaches_evaluation() {
    use crate::behaviors::content::descriptor::{CurveRon, ScaleRon};
    use crate::behaviors::content::loader::resolve::resolve_value;
    use crate::behaviors::value::{EvalContext, eval};

    // 内容作者写的形态：伤害 1 级 20、5 级 80，且永远不超过 100。
    //
    // 输入用属性（演示"曲线吃什么也是配置"），锚点用 **Stat**（10→100）——
    // 这正是 ToME4 的分工：喂属性就用属性锚点，喂等级才用 1→5。
    let ron = ValueRon::Scale(ScaleRon {
        input: Box::new(ValueRon::Literal(10.0)),
        low: 20.0,
        high: 80.0,
        curve: CurveRon::Limit(100.0),
        anchors: crate::behaviors::content::descriptor::AnchorsRon::Stat,
        shift: 0.0,
        add: 0.0,
    });

    let vocab = Vocab::default();
    let value = resolve_value(&ron, &vocab).expect("曲线该能解析");

    // 锚点契约：Stat 锚点下，输入 10 → 20、输入 100 → 80。
    let mut ctx = EvalContext::default();
    ctx.skill_power = 10.0;
    let at_low = eval(&value, &ctx);
    assert!(
        (at_low - 20.0).abs() < 1e-3,
        "输入 10（Stat 下锚点）该得 20，实际 {at_low}"
    );

    // 输入换成 100：把曲线单独拿出来求值（输入是字面量，所以直接调 Scale）。
    if let crate::behaviors::value::Value::Scale(scale) = &value {
        let at_high = scale.eval(100.0).expect("合法参数该能求值");
        assert!(
            (at_high - 80.0).abs() < 1e-3,
            "输入 100（Stat 上锚点）该得 80，实际 {at_high}"
        );
        // 上限：往大里推也不能越过。
        let at_huge = scale.eval(100_000.0).expect("合法参数该能求值");
        assert!(at_huge <= 100.0 + 1e-3, "不该越过上限 100，实际 {at_huge}");
    } else {
        panic!("该解析成 Value::Scale");
    }
}
