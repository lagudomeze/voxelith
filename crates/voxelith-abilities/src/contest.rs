//! **对抗（Contest）**：旧 `Contest { formula, threshold, crit_margin, outcomes }` 的迁移落点。
//!
//! 旧模型把对抗写成一个**引擎原语**（`Effect::Contest`，公式是闭集枚举）。
//! 新模型里它是一块**可复用部件**：三条消息 + 三个系统，游戏规则留在游戏里，
//! 而 diesel 只管把效果送到位（对照它自己的 `damage_pipeline` 例子：
//! `Attack → Hit → Damage → Killed` 全是用户定义的消息链）。
//!
//! ```text
//! GoOff（diesel 的效果信号）
//!   └─► attack_on_go_off      带 `AttackEffect` 的效果实体 → 发 AttackAttempt
//!         └─► resolve_attack_attempt   读双方 gauge 属性 → 出 AttackOutcome
//!               └─► apply_attack_outcome  按结果改属性 + 写日志
//! ```
//!
//! ## 判据**逐字**沿用旧实现（迁移的对齐基准）
//!
//! ```text
//! raw = attacker - defender
//! Crit    iff raw >= threshold + crit_margin && crit_margin > 0
//! Hit     iff raw >= threshold
//! Miss    否则
//! power   = raw（对抗余量；旧模型叫 `SkillPower`）
//! ```
//!
//! `power` 就是伤害来源：旧内容写 `Neg(SkillPower)`，新内容用
//! `AttackEffect::damage_multiplier`（默认 `1.0`；暴击再乘 `crit_multiplier`）。
//!
//! ## ⚠️ 一个尚未堵上的静默失败
//!
//! [`AttackEffect`] 里的属性名是**字符串**，而 gauge 的 `Attributes::value("打错的名字")`
//! 返回 **`0.0`**（不报错）。⇒ 属性名拼错 = 攻击力/护甲静默变 0，不崩不提示。
//! 可用的堵法：加载期拿我们自己的属性清单（`attributes.ron` 里的名字集合）
//! 校验 `AttackEffect` 引用的名字，报错在加载期——与 `attributes` 模块同一套做法。

use bevy::prelude::*;
use bevy_diesel::effect::GoOff;
use bevy_diesel::invoker::InvokedBy;
use bevy_gauge::prelude::{Attributes, AttributesMut, InstantExt, InstantModifierSet};

/// 对抗结果的三档（旧 `Outcome::Fail / Success / Crit` 里**被内容用到**的那三档）。
///
/// 旧模型还有个 `Fumble`，但只有 `RollUnder` 公式会产生它，而 97 个技能里没有一条用
/// `RollUnder`；迁移时不把它带过来（没有内容依赖的空档位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// 没打中（旧 `Fail`）：不结算伤害。
    Miss,
    /// 打中（旧 `Success`）。
    Hit,
    /// 大成功（旧 `Crit`）。
    Crit,
}

/// 一次攻击的**规则参数**（挂在技能的命中态上；旧 `Contest` 那几个字段）。
///
/// 全字段都可配：默认值就是旧 `basic_attack` 的那一组
/// （`CasterStat("strength") + 5` vs `TargetStat("armor")`，阈值 0，暴击余量 5，暴击 ×1.5）。
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AttackEffect {
    /// 攻击方读哪个属性（旧 `Value::CasterStat`）。
    pub attack_attribute: String,
    /// 防守方读哪个属性（旧 `Value::TargetStat`）。
    pub defense_attribute: String,
    /// 攻击值上的固定修正（旧 `Literal(5.0)`）。
    pub power_bonus: f32,
    /// 成功阈值（旧 `threshold`）。
    pub threshold: f32,
    /// 超出阈值多少算暴击；`0.0` = **关闭暴击**（旧 `crit_margin`，含这条"0 永不 Crit"）。
    pub crit_margin: f32,
    /// 暴击时的伤害倍率（旧 `Mul(SkillPower, Literal(1.5))` 里的 1.5）。
    pub crit_multiplier: f32,
    /// 伤害扣哪个属性（旧 `ModifyResource(pool: "hp")`）。
    pub damage_attribute: String,
}

impl Default for AttackEffect {
    fn default() -> Self {
        Self {
            attack_attribute: "Strength".to_string(),
            defense_attribute: "Armor".to_string(),
            power_bonus: 5.0,
            threshold: 0.0,
            crit_margin: 5.0,
            crit_multiplier: 1.5,
            damage_attribute: "Health".to_string(),
        }
    }
}

/// **请求**：对某个目标做一次对抗（由效果叶系统发出）。
#[derive(Message, Clone, Debug)]
pub struct AttackAttempt {
    /// 发起者（施法者）。
    pub attacker: Entity,
    /// 目标。
    pub defender: Entity,
    /// 这次效果所在的实体（日志与 `@ability` 用）。
    pub ability: Entity,
    /// 规则参数（从效果实体上抄一份，后面的系统就不必再查组件）。
    pub effect: AttackEffect,
}

/// **判定结果**：三档之一 + 对抗余量。
#[derive(Message, Clone, Copy, Debug)]
pub struct AttackOutcome {
    /// 发起者。
    pub attacker: Entity,
    /// 目标。
    pub defender: Entity,
    /// 效果实体。
    pub ability: Entity,
    /// 三档结果。
    pub kind: HitKind,
    /// 对抗余量（旧 `SkillPower`）。
    pub power: f32,
    /// 最终伤害（已按暴击倍率算好；`Miss` 时为 0）。
    pub damage: f32,
}

/// 战斗日志一行（**Message**）：L2 订阅它写 UI / 控制台。
///
/// 旧模型把它塞在 `Effect::Log(text)` 里（效果原语的一部分）；
/// 新模型是消息——表现层订阅，机制层不必知道谁在显示。
#[derive(Message, Clone, Debug)]
pub struct CombatLogLine(pub String);

/// **判据**：与旧 `Formula::Difference` 逐字一致。
///
/// 返回 `(结果, 余量)`；余量在 `Miss` 时也照算（旧模型同样会把它写进 `SkillPower`，
/// 只是 `Fail` 分支通常不用它）。
pub fn difference_verdict(
    attacker: f32,
    defender: f32,
    threshold: f32,
    crit_margin: f32,
) -> (HitKind, f32) {
    let raw = attacker - defender;
    if crit_margin > 0.0 && raw >= threshold + crit_margin {
        (HitKind::Crit, raw)
    } else if raw >= threshold {
        (HitKind::Hit, raw)
    } else {
        (HitKind::Miss, raw)
    }
}

/// 效果叶：`GoOff` 打到带 [`AttackEffect`] 的实体 → 发 [`AttackAttempt`]。
///
/// 与 diesel 自带的叶子系统（`instant_set_system` / `spawn_system`）同一套路：
/// `GoOff` 说"这个效果该响了"，叶子决定怎么响。
pub fn attack_on_go_off(
    mut reader: MessageReader<GoOff<IVec2>>,
    q_effect: Query<&AttackEffect>,
    q_invoked: Query<&InvokedBy>,
    mut attempts: MessageWriter<AttackAttempt>,
) {
    for go_off in reader.read() {
        let Ok(effect) = q_effect.get(go_off.entity) else {
            continue;
        };
        // diesel 的 `InvokedBy` 链：从效果实体一路走到施法者。
        let attacker = q_invoked.root_ancestor(go_off.entity);
        // `GoOff` 只带位置的目标（没有实体）时打不了对抗——跳过。
        let Some(defender) = go_off.target.entity else {
            continue;
        };
        attempts.write(AttackAttempt {
            attacker,
            defender,
            ability: go_off.entity,
            effect: effect.clone(),
        });
    }
}

/// 判定：读双方 gauge 属性 → 三档结果。
pub fn resolve_attack_attempt(
    mut reader: MessageReader<AttackAttempt>,
    q_attributes: Query<&Attributes>,
    mut outcomes: MessageWriter<AttackOutcome>,
) {
    for attempt in reader.read() {
        let attacker_value = q_attributes
            .get(attempt.attacker)
            .map_or(0.0, |attributes| {
                attributes.value(&attempt.effect.attack_attribute)
            });
        let defender_value = q_attributes
            .get(attempt.defender)
            .map_or(0.0, |attributes| {
                attributes.value(&attempt.effect.defense_attribute)
            });

        let (kind, power) = difference_verdict(
            attacker_value + attempt.effect.power_bonus,
            defender_value,
            attempt.effect.threshold,
            attempt.effect.crit_margin,
        );
        let damage = match kind {
            HitKind::Miss => 0.0,
            HitKind::Hit => power,
            HitKind::Crit => power * attempt.effect.crit_multiplier,
        };

        outcomes.write(AttackOutcome {
            attacker: attempt.attacker,
            defender: attempt.defender,
            ability: attempt.ability,
            kind,
            power,
            damage,
        });
    }
}

/// 结算：改属性 + 写日志。
///
/// **只读判定、只写属性**：判定与结算是两个系统，所以"算错了"和"写错了"能分开定位。
pub fn apply_attack_outcome(
    mut reader: MessageReader<AttackOutcome>,
    q_effect: Query<&AttackEffect>,
    mut attributes: AttributesMut,
    mut log: MessageWriter<CombatLogLine>,
) {
    for outcome in reader.read() {
        let attribute = q_effect.get(outcome.ability).map_or_else(
            |_| "Health".to_string(),
            |effect| effect.damage_attribute.clone(),
        );

        match outcome.kind {
            HitKind::Miss => {
                log.write(CombatLogLine("被挡下了。".to_string()));
            }
            HitKind::Hit | HitKind::Crit => {
                if outcome.damage > 0.0 {
                    let mut instant = InstantModifierSet::new();
                    instant.push_sub(&attribute, outcome.damage);
                    attributes.apply_instant(&instant, &[], outcome.defender);
                }
                let text = if outcome.kind == HitKind::Crit {
                    format!("重击！{} 点", outcome.damage)
                } else {
                    format!("命中！{} 点", outcome.damage)
                };
                log.write(CombatLogLine(text));
            }
        }
    }
}

/// 注册对抗部件：三条消息 + 三个系统（顺序是契约：请求 → 判定 → 结算）。
pub struct ContestPlugin;

impl Plugin for ContestPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AttackAttempt>()
            .add_message::<AttackOutcome>()
            .add_message::<CombatLogLine>()
            .add_systems(
                Update,
                (
                    attack_on_go_off,
                    resolve_attack_attempt,
                    apply_attack_outcome,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_decides_hit_or_miss() {
        // 旧实现 `difference_threshold_decides_success` 的同一组数字。
        assert_eq!(difference_verdict(10.0, 5.0, 0.0, 0.0).0, HitKind::Hit);
        // **恰好等于阈值算命中**（`>=`，不是 `>`）——这条边界要在迁移里保住。
        assert_eq!(difference_verdict(10.0, 5.0, 5.0, 0.0).0, HitKind::Hit);
        assert_eq!(difference_verdict(9.0, 5.0, 5.0, 0.0).0, HitKind::Miss);
    }

    #[test]
    fn the_power_is_the_margin() {
        let (kind, power) = difference_verdict(10.0, 5.0, 0.0, 0.0);
        assert_eq!(kind, HitKind::Hit);
        assert_eq!(power, 5.0, "余量就是后续伤害的来源（旧 `SkillPower`）");
    }

    #[test]
    fn a_zero_crit_margin_never_crits() {
        // 旧实现里这条专门有测试：`crit_margin = 0` 时永不 Crit。
        assert_eq!(difference_verdict(7.0, 0.0, 0.0, 0.0).0, HitKind::Hit);
        assert_eq!(difference_verdict(8.0, 0.0, 0.0, 0.0).0, HitKind::Hit);
        assert_eq!(difference_verdict(100.0, 0.0, 0.0, 0.0).0, HitKind::Hit);
    }

    #[test]
    fn crit_needs_both_the_margin_and_the_threshold() {
        // 阈值 0、暴击余量 5：raw 5 是 Hit，raw 5.0 起才 Crit。
        assert_eq!(difference_verdict(5.0, 0.0, 0.0, 5.0).0, HitKind::Crit);
        assert_eq!(difference_verdict(4.9, 0.0, 0.0, 5.0).0, HitKind::Hit);
        // 阈值抬高时，暴击线一起抬高。
        assert_eq!(difference_verdict(4.0, 0.0, 3.0, 1.0).0, HitKind::Crit);
    }
}
