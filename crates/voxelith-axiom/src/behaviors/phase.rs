//! [`CombatPhase`]：半即时战斗的**时间控制状态机**。
//!
//! ```text
//! Resolving        ── 时间流动，怪物攒能量、行动推进
//! AwaitingInput    ── 冻结：玩家空槽，等主动输入
//! AwaitingCounter  ── 冻结：有威胁且存在可用反制
//! ```
//!
//! 相位只决定"时间流不流"（[`drive_virtual_time`](crate::behaviors::time_scale::drive_virtual_time)），
//! 不决定"谁能做什么"——那是需求（`HasThreat` / `NoActiveAction`）的事。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::prelude::*;

use crate::behaviors::action::ActiveActions;
use crate::behaviors::skill::{Skill, SkillTags};

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

/// 挂起的威胁（**Resource**）：怪物"即将生效"的行动。
///
/// 它由 `monster_tick` 登记、由 `Effect::DispelAction` 清空；
/// `update_phase` 只读它，不负责发现"行动已经不存在"。
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingThreat {
    /// 即将生效的行动实体。
    pub action: Option<Entity>,
    /// 威胁的来源（怪物）。
    pub source: Option<Entity>,
    /// 威胁的目标（PC）。
    pub target: Option<Entity>,
}

impl PendingThreat {
    /// 是否处于"有威胁"状态。
    pub fn is_active(&self) -> bool {
        self.action.is_some()
    }
}

/// 当前可用技能（**Resource**，派生数据）：每帧由 `compute_available_skills` 重算。
///
/// 它是 L2 与判定共享的只读视图；**不参与**任何战斗结算（结算自己再校验一遍）。
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
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
/// （执行器只有 `EffectContext` 那几个入口，见 [docs/combat-design.md](../../../../docs/combat-design.md) D6）。
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

    /// 数量。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 相位转移：按"玩家是否空槽 / 有没有威胁 / 有没有反制"决定下一相位。
///
/// 因为只看这三个条件，它是**幂等**的：重复调用不会来回抖。
pub fn update_phase(
    phase: Res<State<CombatPhase>>,
    mut next: ResMut<NextState<CombatPhase>>,
    players: Query<(&crate::atoms::actor::Player, Option<&ActiveActions>)>,
    threat: Res<PendingThreat>,
    available: Res<AvailableSkills>,
    skills: Query<&Skill>,
) {
    let want = desired_phase(phase.get(), &players, &threat, &available, &skills);
    if let Some(want) = want {
        next.set(want);
    }
}

/// 目标相位：返回 `None` 表示"维持现状"。
pub fn desired_phase(
    current: &CombatPhase,
    players: &Query<(&crate::atoms::actor::Player, Option<&ActiveActions>)>,
    threat: &PendingThreat,
    available: &AvailableSkills,
    skills: &Query<&Skill>,
) -> Option<CombatPhase> {
    let player_idle = players
        .iter()
        .any(|(_, actions)| actions.is_none_or(|actions| actions.is_empty()));
    let has_threat = threat.is_active();
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

/// 注册相位域：状态、资源与转移系统。
pub struct PhasePlugin;

impl Plugin for PhasePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<CombatPhase>()
            .init_resource::<PendingThreat>()
            .init_resource::<AvailableSkills>()
            .init_resource::<CombatLog>()
            .add_systems(Update, update_phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_threat_reports_activity() {
        let mut threat = PendingThreat::default();
        assert!(!threat.is_active());

        threat.action = Some(Entity::PLACEHOLDER);
        assert!(threat.is_active());
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
        assert!(available.is_empty());

        available.push(counter);
        available.push(counter);
        assert_eq!(available.len(), 1, "同一个技能不重复登记");
        assert!(available.contains(counter));

        available.push(attack);
        assert!(available.any_tagged(SkillTags::COUNTER, tags));
        assert!(available.any_tagged(SkillTags::ATTACK, tags));
        assert!(!available.any_tagged(SkillTags::SPELL, tags));

        available.clear();
        assert!(available.is_empty());
        assert!(!available.any_tagged(SkillTags::ATTACK, tags));
    }

    #[test]
    fn log_is_append_only_until_cleared() {
        let mut log = CombatLog::default();
        assert!(log.is_empty());

        log.push("命中！".into());
        log.push("被闪避了。".into());
        assert_eq!(
            log.entries(),
            ["命中！".to_owned(), "被闪避了。".to_owned()]
        );
        assert_eq!(log.len(), 2);

        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn phase_defaults_to_resolving() {
        assert_eq!(CombatPhase::default(), CombatPhase::Resolving);
    }
}
