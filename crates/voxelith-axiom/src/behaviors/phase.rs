//! 战斗相位（**State**）+ 派生数据：**时间流不流**由相位决定。
//!
//! ```text
//! Resolving        ── 时间流动：action / status / 冷却正常推进
//! AwaitingInput    ── 冻结：玩家行动槽为空，等主动输入
//! AwaitingCounter  ── 冻结：有未处理的威胁且存在可用反制，等反制决策
//! ```
//!
//! 相位只决定"时间流不流"（[`drive_virtual_time`](crate::behaviors::time_scale::drive_virtual_time)），
//! 逻辑系统只读 `Res<Time>`、完全不需要知道相位——冻结时 `delta` 就是 `0`。
//!
//! ## 判据（只有三条）
//!
//! | 条件 | 含义 |
//! |---|---|
//! | `player_idle` | 存在 `Player` 实体，且它的 `ActiveActions` 为空（或没有该组件） |
//! | `has_threat` | **`ThreatWindow` 非空**（未处理的、针对 PC 的威胁集合） |
//! | `has_counter` | `AvailableSkills` 里至少有一个带 `COUNTER` 标签的技能 |
//!
//! ```text
//! Resolving:        player_idle → AwaitingInput；has_threat && has_counter → AwaitingCounter
//! AwaitingInput:    两者都不成立 → Resolving
//! AwaitingCounter:  两者都不成立 → Resolving
//! ```
//!
//! ## 冻结的生效延迟**恰好一帧**（实现契约）
//!
//! `update_phase` 写下的相位，在**次帧 `PreUpdate` 开头**成为 `State`，
//! 而倍率在**再下一帧 `First`** 被读到。所以要钉的性质是
//! "任意时刻 `delta != 0` ⟺ `State` 是 `Resolving`"，而不是"相位一变当帧就停"。
//! 细节与反例见 [`time_scale`](crate::behaviors::time_scale)。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::prelude::*;

use crate::atoms::action::ActiveActions;
use crate::atoms::actor::InputDriven;
use crate::behaviors::skill::{Skill, SkillTags};
use crate::behaviors::threat::ThreatWindow;

/// 战斗相位（**States**）。
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombatPhase {
    /// 时间流动：所有 action / status / 冷却正常推进。
    #[default]
    Resolving,
    /// 冻结：玩家行动槽为空，等主动输入。
    AwaitingInput,
    /// 冻结：有威胁且存在可用反制，等反制决策。
    AwaitingCounter,
}

/// 当前可用技能（**Resource**，派生数据）：每帧由 `compute_available_skills` 重算。
///
/// 它是 L2 与判定共享的只读视图；**不参与**任何战斗结算（结算自己再校验一遍）。
#[derive(Resource, Debug, Clone, Default)]
pub struct AvailableSkills(Vec<Entity>);

impl AvailableSkills {
    /// 清空（每帧开头）。
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// 追加一个可用技能。
    pub fn push(&mut self, skill: Entity) {
        if !self.0.contains(&skill) {
            self.0.push(skill);
        }
    }

    /// 全部可用技能（顺序稳定）。
    pub fn all(&self) -> &[Entity] {
        &self.0
    }

    /// 是否包含某个技能。
    pub fn contains(&self, skill: Entity) -> bool {
        self.0.contains(&skill)
    }

    /// 有没有带某个标签的可用技能（**纯查询组合**，与谁持有目录无关）。
    pub fn any_tagged(&self, tag: SkillTags, lookup: impl Fn(Entity) -> Option<SkillTags>) -> bool {
        self.0
            .iter()
            .any(|&skill| lookup(skill).is_some_and(|tags| tags.contains(tag)))
    }

    /// 有没有带某个标签的可用技能（走查询时用这个便捷包装）。
    pub fn has_tagged(&self, skills: &Query<&Skill>, tag: SkillTags) -> bool {
        self.any_tagged(tag, |entity| {
            skills.get(entity).ok().map(|skill| skill.tags)
        })
    }

    /// 数量。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 战斗日志（**Resource**，追加式）：L2 只读，读完自己清。
///
/// 用 Resource 而不是 `Message` 是为了让**效果执行器不必持有消息写入器**
/// （执行器只有 `EffectContext` 那几个入口，见 [docs/combat-design.md](../../../../docs/combat-design.md) §2.3）。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct CombatLog(Vec<String>);

impl CombatLog {
    /// 追加一条。
    pub fn push(&mut self, text: String) {
        self.0.push(text);
    }

    /// 全部条目。
    pub fn entries(&self) -> &[String] {
        &self.0
    }

    /// 清空（L2 消费后）。
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// 有没有新日志（L2 用来决定要不要重绘）。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 相位转移：按"玩家是否空槽 / 有没有未处理的威胁 / 有没有反制"决定下一相位。
///
/// 因为只看这三个条件，它是**幂等**的：重复调用不会来回抖。
pub fn update_phase(
    phase: Res<State<CombatPhase>>,
    mut next: ResMut<NextState<CombatPhase>>,
    players: Query<Option<&ActiveActions>, InputDriven>,
    window: Res<ThreatWindow>,
    available: Res<AvailableSkills>,
    skills: Query<&Skill>,
) {
    let want = desired_phase(phase.get(), &players, &window, &available, &skills);
    if let Some(want) = want {
        next.set(want);
    }
}

/// 目标相位：返回 `None` 表示"维持现状"。
pub fn desired_phase(
    current: &CombatPhase,
    players: &Query<Option<&ActiveActions>, InputDriven>,
    window: &ThreatWindow,
    available: &AvailableSkills,
    skills: &Query<&Skill>,
) -> Option<CombatPhase> {
    let player_idle = players
        .iter()
        .any(|actions| actions.is_none_or(|actions| actions.is_empty()));
    let has_threat = window.is_open();
    let has_counter = available.has_tagged(skills, SkillTags::COUNTER);

    match current {
        CombatPhase::Resolving => {
            if player_idle {
                Some(CombatPhase::AwaitingInput)
            } else if has_threat && has_counter {
                Some(CombatPhase::AwaitingCounter)
            } else {
                None
            }
        }
        CombatPhase::AwaitingInput | CombatPhase::AwaitingCounter => {
            if player_idle {
                Some(CombatPhase::AwaitingInput)
            } else if has_threat && has_counter {
                Some(CombatPhase::AwaitingCounter)
            } else {
                Some(CombatPhase::Resolving)
            }
        }
    }
}

/// 注册相位域：状态与派生资源。
///
/// **系统不在这里注册**：`update_phase` 的跨域顺序由装配层
/// [`CombatPlugin`](crate::behaviors::combat::CombatPlugin) 独占（**Q14**：
/// 只在一个地方注册系统，否则同一个系统会在一帧里跑两遍）。
pub struct PhasePlugin;

impl Plugin for PhasePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<CombatPhase>()
            .init_resource::<AvailableSkills>()
            .init_resource::<CombatLog>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_window_reports_no_activity() {
        let window = ThreatWindow::default();
        assert!(!window.is_open());
    }

    #[test]
    fn available_skills_deduplicate_and_look_up_tags() {
        // 用一张"实体 → 标签"的小表代替世界查询：`any_tagged` 只看组合逻辑。
        let mut world = World::new();
        let counter = world.spawn_empty().id();
        let attack = world.spawn_empty().id();
        let tags = |entity: Entity| {
            if entity == counter {
                Some(SkillTags::COUNTER)
            } else if entity == attack {
                Some(SkillTags::ATTACK)
            } else {
                None
            }
        };

        let mut available = AvailableSkills::default();
        available.push(counter);
        available.push(counter); // 去重
        available.push(attack);
        assert_eq!(available.len(), 2);

        assert!(available.any_tagged(SkillTags::COUNTER, tags));
        assert!(available.any_tagged(SkillTags::ATTACK, tags));
        assert!(!available.any_tagged(SkillTags::SPELL, tags));
    }

    #[test]
    fn the_log_appends_and_clears() {
        let mut log = CombatLog::default();
        log.push("命中！".into());
        log.push("重击！".into());
        assert_eq!(log.entries().len(), 2);
        log.clear();
        assert!(log.entries().is_empty());
    }
}
