//! `resolve_*`：把 RON 描述里的**字符串引用**解析成词汇 ID。
//!
//! 只做名字 → ID 的翻译，不做任何语义判断；查不到就报 [`LoaderError`]。

use std::collections::HashSet;

use crate::behaviors::content::descriptor::{
    ConditionRon, ContestRon, EffectRon, OutcomeRon, RequirementRon, ValueRon, WhoRon,
};
use crate::behaviors::content::loader::LoaderError;
use crate::behaviors::content::vocabulary::{ResourceId, Vocab};
use crate::behaviors::contest::{Contest, Outcome};
use crate::behaviors::effect::Effect;
use crate::behaviors::requirement::{Condition, Requirement};
use crate::behaviors::value::{Value, Who};

/// 表达式：`CasterStat("strength")` → `CasterStat(StatId(0))`。
pub fn resolve_value(ron: &ValueRon, vocab: &Vocab) -> Result<Value, LoaderError> {
    Ok(match ron {
        ValueRon::Literal(number) => Value::Literal(*number),
        ValueRon::SkillPower => Value::SkillPower,
        ValueRon::CasterStat(name) => Value::CasterStat(vocab.stat(name)?),
        ValueRon::TargetStat(name) => Value::TargetStat(vocab.stat(name)?),
        ValueRon::Resource { pool, who } => Value::Resource {
            pool: vocab.resource(pool)?,
            who: resolve_who(*who),
        },
        ValueRon::Sum(items) => {
            let mut resolved = Vec::with_capacity(items.len());
            for item in items {
                resolved.push(resolve_value(item, vocab)?);
            }
            Value::Sum(resolved)
        }
        ValueRon::Mul(left, right) => Value::Mul(
            Box::new(resolve_value(left, vocab)?),
            Box::new(resolve_value(right, vocab)?),
        ),
        ValueRon::Neg(inner) => Value::Neg(Box::new(resolve_value(inner, vocab)?)),
        ValueRon::Scale(scale) => Value::Scale(Box::new(resolve_scale(scale, vocab)?)),
    })
}

/// 伸缩曲线：RON → 运行时（曲线的输入本身也是一条子表达式）。
pub fn resolve_scale(
    ron: &super::super::descriptor::ScaleRon,
    vocab: &Vocab,
) -> Result<crate::behaviors::value::Scale, LoaderError> {
    use super::super::descriptor::{AnchorsRon, CurveRon};
    use crate::behaviors::value::{Anchors, Scale, ScalePower};

    let power = match ron.curve {
        CurveRon::Power(power) => ScalePower::Power(power),
        CurveRon::Log => ScalePower::Log,
        CurveRon::Limit(limit) => ScalePower::Limit(limit),
    };
    let anchors = match ron.anchors {
        AnchorsRon::Talent => Anchors::TALENT,
        AnchorsRon::Stat => Anchors::STAT,
    };
    Ok(Scale {
        input: resolve_value(&ron.input, vocab)?,
        low: ron.low,
        high: ron.high,
        power,
        anchors,
        shift: ron.shift,
        add: ron.add,
    })
}

/// 表达式 / 效果里的"谁"。
pub fn resolve_who(who: WhoRon) -> Who {
    match who {
        WhoRon::Caster => Who::Caster,
        WhoRon::Target => Who::Target,
    }
}

/// 技能需求。
pub fn resolve_requirement(
    ron: &RequirementRon,
    vocab: &Vocab,
) -> Result<Requirement, LoaderError> {
    Ok(match ron {
        RequirementRon::Resource { pool, min } => Requirement::Resource {
            pool: vocab.resource(pool)?,
            min: *min,
        },
        RequirementRon::HasThreat => Requirement::HasThreat,
        RequirementRon::NoActiveAction => Requirement::NoActiveAction,
        RequirementRon::InStatus(name) => Requirement::InStatus(vocab.status(name)?),
        RequirementRon::NotInStatus(name) => Requirement::NotInStatus(vocab.status(name)?),
        RequirementRon::OffCooldown => Requirement::OffCooldown,
        RequirementRon::TargetAlive => Requirement::TargetAlive,
        RequirementRon::TargetIsEnemy => Requirement::TargetIsEnemy,
        RequirementRon::CasterHasTag(tag) => Requirement::CasterHasTag(*tag),
    })
}

/// 效果内部条件。
pub fn resolve_condition(ron: &ConditionRon, vocab: &Vocab) -> Result<Condition, LoaderError> {
    Ok(match ron {
        ConditionRon::Always => Condition::Always,
        ConditionRon::ResourceBelow { pool, ratio } => Condition::ResourceBelow {
            pool: vocab.resource(pool)?,
            ratio: *ratio,
        },
        ConditionRon::HasStatus(name) => Condition::HasStatus(vocab.status(name)?),
        ConditionRon::HasThreat => Condition::HasThreat,
    })
}

/// 效果（递归）。
pub fn resolve_effect(ron: &EffectRon, vocab: &Vocab) -> Result<Effect, LoaderError> {
    Ok(match ron {
        EffectRon::ModifyResource { pool, delta, who } => Effect::ModifyResource {
            pool: vocab.resource(pool)?,
            delta: resolve_value(delta, vocab)?,
            who: resolve_who(*who),
        },
        EffectRon::ApplyStatus {
            status,
            duration,
            who,
        } => Effect::ApplyStatus {
            status: vocab.status(status)?,
            duration: resolve_value(duration, vocab)?,
            who: resolve_who(*who),
        },
        EffectRon::RemoveStatus { status, who } => Effect::RemoveStatus {
            status: vocab.status(status)?,
            who: resolve_who(*who),
        },
        EffectRon::DispelAction { who } => Effect::DispelAction {
            who: resolve_who(*who),
        },
        EffectRon::Interrupt { who } => Effect::Interrupt {
            who: resolve_who(*who),
        },
        EffectRon::SpawnAction { skill, who } => Effect::SpawnAction {
            skill: vocab.skill(skill)?,
            who: resolve_who(*who),
        },
        EffectRon::Log { text } => Effect::Log { text: text.clone() },
        EffectRon::Contest(contest) => Effect::Contest(Box::new(resolve_contest(contest, vocab)?)),
        EffectRon::Sequence(list) => {
            let mut resolved = Vec::with_capacity(list.len());
            for effect in list {
                resolved.push(resolve_effect(effect, vocab)?);
            }
            Effect::Sequence(resolved)
        }
        EffectRon::Conditional { cond, then, else_ } => Effect::Conditional {
            cond: resolve_condition(cond, vocab)?,
            then: Box::new(resolve_effect(then, vocab)?),
            else_: Box::new(resolve_effect(else_, vocab)?),
        },
    })
}

/// 对抗（递归；`outcomes` 里的效果也解析）。
pub fn resolve_contest(ron: &ContestRon, vocab: &Vocab) -> Result<Contest, LoaderError> {
    let mut outcomes = Vec::with_capacity(ron.outcomes.len());
    for (outcome, effects) in &ron.outcomes {
        let mut resolved = Vec::with_capacity(effects.len());
        for effect in effects {
            resolved.push(resolve_effect(effect, vocab)?);
        }
        outcomes.push((resolve_outcome(*outcome), resolved));
    }

    Ok(Contest {
        attacker: resolve_value(&ron.attacker, vocab)?,
        defender: resolve_value(&ron.defender, vocab)?,
        formula: ron.formula,
        threshold: ron.threshold,
        crit_margin: ron.crit_margin,
        outcomes,
    })
}

/// 对抗结果（RON → 运行时）。
pub fn resolve_outcome(outcome: OutcomeRon) -> Outcome {
    match outcome {
        OutcomeRon::Success => Outcome::Success,
        OutcomeRon::Fail => Outcome::Fail,
        OutcomeRon::Crit => Outcome::Crit,
        OutcomeRon::Fumble => Outcome::Fumble,
    }
}

/// 收集一个效果里用到的池（加载期"有没有漏配"的覆盖检查）。
pub fn collect_resources(
    effect: &EffectRon,
    vocab: &Vocab,
    out: &mut HashSet<ResourceId>,
) -> Result<(), LoaderError> {
    match effect {
        EffectRon::ModifyResource { pool, delta, .. } => {
            out.insert(vocab.resource(pool)?);
            collect_value_resources(delta, vocab, out)?;
        }
        EffectRon::ApplyStatus { duration, .. } => {
            collect_value_resources(duration, vocab, out)?;
        }
        EffectRon::Contest(contest) => {
            collect_value_resources(&contest.attacker, vocab, out)?;
            collect_value_resources(&contest.defender, vocab, out)?;
            for (_, effects) in &contest.outcomes {
                for effect in effects {
                    collect_resources(effect, vocab, out)?;
                }
            }
        }
        EffectRon::Sequence(list) => {
            for effect in list {
                collect_resources(effect, vocab, out)?;
            }
        }
        EffectRon::Conditional { cond, then, else_ } => {
            if let ConditionRon::ResourceBelow { pool, .. } = cond {
                out.insert(vocab.resource(pool)?);
            }
            collect_resources(then, vocab, out)?;
            collect_resources(else_, vocab, out)?;
        }
        EffectRon::RemoveStatus { .. }
        | EffectRon::DispelAction { .. }
        | EffectRon::Interrupt { .. }
        | EffectRon::SpawnAction { .. }
        | EffectRon::Log { .. } => {}
    }
    Ok(())
}

/// 收集一个表达式里用到的池。
fn collect_value_resources(
    value: &ValueRon,
    vocab: &Vocab,
    out: &mut HashSet<ResourceId>,
) -> Result<(), LoaderError> {
    match value {
        ValueRon::Resource { pool, .. } => {
            out.insert(vocab.resource(pool)?);
        }
        ValueRon::Sum(items) => {
            for item in items {
                collect_value_resources(item, vocab, out)?;
            }
        }
        ValueRon::Mul(left, right) => {
            collect_value_resources(left, vocab, out)?;
            collect_value_resources(right, vocab, out)?;
        }
        ValueRon::Neg(inner) => collect_value_resources(inner, vocab, out)?,
        // 曲线的**输入**也是一条子表达式，它用到的池同样要登记。
        ValueRon::Scale(scale) => collect_value_resources(&scale.input, vocab, out)?,
        ValueRon::Literal(_)
        | ValueRon::SkillPower
        | ValueRon::CasterStat(_)
        | ValueRon::TargetStat(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviors::content::descriptor::{ResourceRon, StatRon, VocabRon};
    use crate::behaviors::content::loader::build_vocab;

    fn vocab() -> Vocab {
        build_vocab(&VocabRon {
            resources: vec![ResourceRon {
                id: "hp".into(),
                name: "生命".into(),
                max: 10.0,
                regen: 0.0,
                start_full: true,
            }],
            stats: vec![StatRon {
                id: "armor".into(),
                name: "护甲".into(),
            }],
            statuses: Vec::new(),
        })
    }

    #[test]
    fn values_resolve_every_reference() {
        let vocab = vocab();
        let ron = ValueRon::Sum(vec![
            ValueRon::CasterStat("armor".into()),
            ValueRon::TargetStat("armor".into()),
            ValueRon::Resource {
                pool: "hp".into(),
                who: WhoRon::Target,
            },
            ValueRon::Literal(1.0),
            ValueRon::SkillPower,
            ValueRon::Neg(Box::new(ValueRon::Literal(2.0))),
        ]);
        let value = resolve_value(&ron, &vocab).unwrap();
        match value {
            Value::Sum(items) => {
                assert_eq!(items[0], Value::CasterStat(vocab.stat("armor").unwrap()));
                assert_eq!(
                    items[2],
                    Value::Resource {
                        pool: vocab.resource("hp").unwrap(),
                        who: Who::Target
                    }
                );
            }
            other => panic!("应为 Sum，实为 {other:?}"),
        }
    }

    #[test]
    fn unknown_references_report_the_table() {
        let vocab = vocab();
        let error = resolve_value(&ValueRon::CasterStat("luck".into()), &vocab).unwrap_err();
        match error {
            LoaderError::UnknownName(unknown) => {
                assert_eq!(unknown.kind, "stat");
                assert_eq!(unknown.name, "luck");
            }
            other => panic!("应为 UnknownName，实为 {other:?}"),
        }
    }

    #[test]
    fn nested_effects_are_all_collected() {
        let vocab = vocab();
        let effect = EffectRon::Contest(Box::new(ContestRon {
            attacker: ValueRon::CasterStat("armor".into()),
            defender: ValueRon::Resource {
                pool: "hp".into(),
                who: WhoRon::Target,
            },
            formula: crate::behaviors::contest::Formula::Difference,
            threshold: 0.0,
            crit_margin: 0.0,
            outcomes: vec![(
                OutcomeRon::Success,
                vec![EffectRon::ModifyResource {
                    pool: "hp".into(),
                    delta: ValueRon::Literal(-1.0),
                    who: WhoRon::Target,
                }],
            )],
        }));

        let mut used = HashSet::new();
        collect_resources(&effect, &vocab, &mut used).unwrap();
        assert_eq!(used.len(), 1);
        assert!(used.contains(&vocab.resource("hp").unwrap()));
    }

    #[test]
    fn conditions_and_requirements_resolve() {
        let vocab = vocab();
        let condition = resolve_condition(
            &ConditionRon::ResourceBelow {
                pool: "hp".into(),
                ratio: 0.3,
            },
            &vocab,
        )
        .unwrap();
        assert_eq!(
            condition,
            Condition::ResourceBelow {
                pool: vocab.resource("hp").unwrap(),
                ratio: 0.3
            }
        );

        let requirement = resolve_requirement(&RequirementRon::HasThreat, &vocab).unwrap();
        assert_eq!(requirement, Requirement::HasThreat);
    }
}
