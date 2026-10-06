//! 需求（Requirement）与条件（Condition）：**门控是纯数据**。
//!
//! [`skill_available`] 是可用性的唯一判据，三关依次过：
//!
//! ```text
//! 1. 状态门控：身上的状态只要有 blocks_tags 与技能标签相交 → 不可用
//! 2. requirements：每一条都得成立
//! 3. costs：每个池都付得起
//! ```
//!
//! 反制技能的合法性完全由这里保证（`HasThreat` + 反应池），**不需要"反制槽"**。

use bevy_ecs::prelude::*;

use crate::atoms::actor::{ActorRole, ActorTags, Cooldowns, Faction, Resources, Stats};
use crate::atoms::vocabulary::ActorTagId;
use crate::behaviors::content::{ResourceId, StatusId};
use crate::behaviors::effect::Blob;
use crate::behaviors::skill::{Skill, SkillTags};
use crate::behaviors::status::{ActiveStatus, StatusDef};
use crate::behaviors::threat::ThreatWindow;

/// 释放技能的前置需求（全部满足才可用）。
///
/// **不派生 `Deserialize`**：RON 里的池 / 状态引用是名字，由
/// [`RequirementRon`](crate::behaviors::content::RequirementRon) + 加载器翻译成 ID。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Requirement {
    /// 某个池至少有 `min`。
    Resource {
        /// 哪个池。
        pool: ResourceId,
        /// 至少多少。
        min: f32,
    },
    /// 当前存在威胁（反制技能用它）。
    HasThreat,
    /// 自己没有正在进行的行动。
    NoActiveAction,
    /// 自己身上有该状态。
    InStatus(StatusId),
    /// 自己身上没有该状态。
    NotInStatus(StatusId),
    /// 技能不在冷却中。
    OffCooldown,
    /// 目标还活着（至少一个池还有存量）。
    TargetAlive,
    /// 目标是敌人（**阵营不同**，见 [`Faction::hostile_to`]）。
    TargetIsEnemy,
    /// 自己带某个标签。
    CasterHasTag(ActorTagId),
}

/// 效果内部的条件判据（比 `Requirement` 少，因为它不需要指向性判断）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Condition {
    /// 恒成立。
    Always,
    /// 某个池的占比低于 `ratio`。
    ResourceBelow {
        /// 哪个池。
        pool: ResourceId,
        /// 占比阈值。
        ratio: f32,
    },
    /// 目标有该状态。
    HasStatus(StatusId),
    /// 当前存在威胁。
    HasThreat,
}

/// 技能消耗。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cost {
    /// 哪个池。
    pub pool: ResourceId,
    /// 消耗多少。
    pub amount: f32,
}

/// 目标如何解析（**R59**：只做"寻找与判定"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Targeting {
    /// 只作用自己。
    SelfOnly,
    /// 作用当前威胁的来源（反制技能用它）。
    ThreatSource,
    /// 用请求里带的目标；没带就退回最近的敌人。
    CurrentSelection,
    /// 最近的敌人（顺序稳定：敌人集合里的第一个）。
    NearestEnemy,
}

/// 一次可用性评估需要的全部输入（**只读快照**，不持有可变借用）。
///
/// 状态用它自己的**快照**（`(状态 ID, 封锁标签)`）而不是"再查一遍世界"：
/// 这样门控判定是纯数据运算，也方便单测直接构造（**R8** 的"只碰自己"由此自然成立）。
pub struct CasterContext<'a, B: Blob> {
    /// 施法者。
    pub actor: Entity,
    /// 施法者的池。
    pub resources: &'a Resources,
    /// 施法者的属性。
    pub stats: Option<&'a Stats>,
    /// 施法者的冷却。
    pub cooldowns: &'a Cooldowns,
    /// 施法者的特性标签（`CasterHasTag`；**与阵营无关**）。
    pub tags: Option<&'a ActorTags>,
    /// 施法者的**引擎角色**：判定"这一招是不是他的"（`Skill::roles`）。
    ///
    /// `None` = 没有角色标记，此时只对"不限定角色"的技能判为可用——
    /// 保守方向，免得把某人的专属招式发给所有人。
    pub role: Option<ActorRole>,
    /// 施法者的阵营（`TargetIsEnemy` 的比较基准）。
    pub faction: Option<Faction>,
    /// 身上状态的门控快照：`(状态 ID, 该状态封锁的标签)`。
    pub statuses: &'a [(StatusId, SkillTags)],
    /// 只读查询集合（`Effect` 相关的派生数据；门控本身不依赖它）。
    pub reads: &'a B,
    /// 当前正在进行的行动数（`NoActiveAction` 的判据）。
    pub active_action_count: usize,
    /// 当前挂起的威胁。
    pub threat: Option<&'a ThreatWindow>,
    /// 指定目标。
    pub target: Option<Entity>,
    /// 目标的池（`TargetAlive`）。
    pub target_resources: Option<&'a Resources>,
    /// 目标的阵营（`TargetIsEnemy`）。
    pub target_faction: Option<Faction>,
}

impl<B: Blob> CasterContext<'_, B> {
    /// 身上是否有该状态。
    pub fn has_status_id(&self, wanted: StatusId) -> bool {
        self.statuses.iter().any(|(id, _)| *id == wanted)
    }

    /// 身上的状态是否封锁了这个标签。
    pub fn blocks(&self, tags: SkillTags) -> bool {
        self.statuses
            .iter()
            .any(|(_, blocked)| blocked.intersects(tags))
    }
}

/// 技能是否可用（**唯一判据**）。
///
/// 三层，顺序即成本（从便宜到贵）：
///
/// 1. **归属**：这一招是不是这个角色的（Skill::roles）——纯数据比较；
/// 2. **门控**：身上有没有状态封锁这个标签；
/// 3. **条件**：需求逐条成立、消耗付得起。
///
/// 1 与 3 必须分开：需求回答"现在放得出来吗"，归属回答"技能栏里该不该有它"。
/// 混在一起就会出现"哥布林的招式出现在玩家的技能栏里"（双方都满足"有行动点"）。
pub fn skill_available<B: Blob>(skill: &Skill, ctx: &CasterContext<'_, B>) -> bool {
    if !role_allows(skill, ctx.role) {
        return false;
    }
    if status_blocks(ctx, skill.tags) {
        return false;
    }
    if !skill
        .requirements
        .iter()
        .all(|requirement| requirement_ok(requirement, skill, ctx))
    {
        return false;
    }
    skill
        .costs
        .iter()
        .all(|cost| ctx.resources.has_at_least(cost.pool, cost.amount))
}

/// 归属判据：技能不限定角色（`roles` 为空）→ 谁都能用；否则要求施法者角色在列表里。
/// 没有角色标记的实体（`None`）只能使用「不限定角色」的技能。
/// 没有角色标记的实体（`None`）只能使用"不限定角色"的技能。
/// 没有角色标记的实体（None）只能使用"不限定角色"的技能。
pub fn role_allows(skill: &Skill, role: Option<ActorRole>) -> bool {
    if skill.roles.is_empty() {
        return true;
    }
    role.is_some_and(|role| skill.roles.contains(&role))
}

/// 状态门控：身上有状态明确"封锁"了这个标签 → 不可用。**纯数据，无代码分支。**
pub fn status_blocks<B: Blob>(ctx: &CasterContext<'_, B>, tags: SkillTags) -> bool {
    ctx.blocks(tags)
}

/// 单条需求是否成立。
pub fn requirement_ok<B: Blob>(
    requirement: &Requirement,
    skill: &Skill,
    ctx: &CasterContext<'_, B>,
) -> bool {
    match requirement {
        Requirement::Resource { pool, min } => ctx.resources.has_at_least(*pool, *min),
        Requirement::HasThreat => threat_active(ctx),
        Requirement::NoActiveAction => ctx.active_action_count == 0,
        Requirement::InStatus(status) => ctx.has_status_id(*status),
        Requirement::NotInStatus(status) => !ctx.has_status_id(*status),
        Requirement::OffCooldown => ctx.cooldowns.is_ready(skill.id),
        Requirement::TargetAlive => ctx
            .target_resources
            .is_some_and(|resources| resources.pools.values().any(|pool| pool.current > 0.0)),
        Requirement::TargetIsEnemy => ctx
            .target_faction
            .zip(ctx.faction)
            .is_some_and(|(target, caster)| caster.hostile_to(target)),
        Requirement::CasterHasTag(tag) => ctx.tags.is_some_and(|tags| tags.has(*tag)),
    }
}

/// 当前是否存在威胁。
pub fn threat_active<B: Blob>(ctx: &CasterContext<'_, B>) -> bool {
    ctx.threat.is_some_and(ThreatWindow::is_open)
}

/// 身上是否有该状态。
pub fn has_status<B: Blob>(ctx: &CasterContext<'_, B>, wanted: StatusId) -> bool {
    ctx.has_status_id(wanted)
}

/// 从"状态槽 + 它们对应的定义"造一份门控快照。
///
/// 调用方（释放校验 / 可用技能 / 怪物 AI）都要这一步，所以放在这里共用一次。
pub fn status_snapshot(
    status_list: &[Entity],
    statuses: &Query<&ActiveStatus>,
    defs: &Query<&StatusDef>,
) -> Vec<(StatusId, SkillTags)> {
    status_list
        .iter()
        .filter_map(|&entity| {
            statuses
                .get(entity)
                .ok()
                .and_then(|instance| defs.get(instance.def).ok())
                .map(|definition| (definition.id, definition.blocks_tags))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::actor::{ActorState, Faction, Pool};
    use crate::behaviors::action::{ActiveActions, InitiatedBy};
    use crate::behaviors::content::{ResourceId, SkillId};

    /// 测试用的空 `Blob` 实现：门控判定本身不需要查询。
    struct NoReads;

    impl Blob for NoReads {
        fn pools(&self, _entity: Entity) -> Option<&Resources> {
            None
        }
        fn stats(&self, _entity: Entity) -> Option<&Stats> {
            None
        }
        fn skills(&self) -> Option<Query<'_, '_, &'static Skill>> {
            None
        }
        fn status_defs(&self) -> Query<'_, '_, &'static StatusDef> {
            unreachable!("门控判定不查状态定义")
        }
        fn statuses(&self) -> Query<'_, '_, &'static ActiveStatus> {
            unreachable!("门控判定不查状态实例")
        }
        fn initiated(&self) -> Option<Query<'_, '_, &'static InitiatedBy>> {
            None
        }
        fn active_actions(&self) -> Option<Query<'_, '_, &'static ActiveActions>> {
            None
        }
        fn actor_states(&self) -> Query<'_, '_, &'static ActorState> {
            unreachable!("门控判定不查状态槽")
        }
        fn cooldowns(&self) -> Option<Query<'_, '_, &'static Cooldowns>> {
            None
        }
    }

    fn skill(requirements: Vec<Requirement>, costs: Vec<Cost>, tags: SkillTags) -> Skill {
        Skill {
            id: SkillId(0),
            name: "test".into(),
            icon: String::new(),
            tags,
            roles: Vec::new(),
            duration: 1.0,
            recovery: 0.0,
            requirements,
            costs,
            targeting: Targeting::SelfOnly,
            effects: Vec::new(),
        }
    }

    /// 造一份评估上下文（门控快照 / 威胁 / 行动数可调）。
    fn context<'a>(
        resources: &'a Resources,
        cooldowns: &'a Cooldowns,
        statuses: &'a [(StatusId, SkillTags)],
        active_action_count: usize,
        threat: Option<&'a ThreatWindow>,
    ) -> CasterContext<'a, NoReads> {
        CasterContext {
            actor: Entity::PLACEHOLDER,
            resources,
            stats: None,
            cooldowns,
            // 默认：没有特性标签、没有角色、没有阵营（`TargetIsEnemy` 因此不成立）。
            tags: None,
            role: None,
            faction: None,
            statuses,
            reads: &NoReads,
            active_action_count,
            threat,
            target: None,
            target_resources: None,
            target_faction: None,
        }
    }

    #[test]
    fn costs_are_checked_against_the_pool() {
        let mut resources = Resources::default();
        resources.define(ResourceId(0), Pool::full(1.0));
        let cooldowns = Cooldowns::default();
        let ctx = context(&resources, &cooldowns, &[], 0, None);

        let affordable = skill(
            Vec::new(),
            vec![Cost {
                pool: ResourceId(0),
                amount: 1.0,
            }],
            SkillTags::NONE,
        );
        assert!(skill_available(&affordable, &ctx));

        let too_expensive = skill(
            Vec::new(),
            vec![Cost {
                pool: ResourceId(0),
                amount: 2.0,
            }],
            SkillTags::NONE,
        );
        assert!(!skill_available(&too_expensive, &ctx));
    }

    #[test]
    fn has_threat_requirement_follows_the_registry() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let counter = skill(vec![Requirement::HasThreat], Vec::new(), SkillTags::COUNTER);

        let no_threat = ThreatWindow::default();
        let ctx = context(&resources, &cooldowns, &[], 0, Some(&no_threat));
        assert!(!skill_available(&counter, &ctx));

        let mut window = ThreatWindow::default();
        window.push(crate::behaviors::threat::Threat {
            action: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
        });
        let ctx = context(&resources, &cooldowns, &[], 0, Some(&window));
        assert!(skill_available(&counter, &ctx));
    }

    #[test]
    fn no_active_action_uses_the_slot_count() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let attack = skill(
            vec![Requirement::NoActiveAction],
            Vec::new(),
            SkillTags::ATTACK,
        );

        let ctx = context(&resources, &cooldowns, &[], 0, None);
        assert!(skill_available(&attack, &ctx));

        let busy = context(&resources, &cooldowns, &[], 1, None);
        assert!(!skill_available(&attack, &busy), "有行动在跑就不能再发起");
    }

    #[test]
    fn off_cooldown_reads_the_skill_id() {
        let resources = Resources::default();
        let mut cooldowns = Cooldowns::default();
        let attack = skill(
            vec![Requirement::OffCooldown],
            Vec::new(),
            SkillTags::ATTACK,
        );

        cooldowns.start(SkillId(0), 3.0);
        let ctx = context(&resources, &cooldowns, &[], 0, None);
        assert!(!skill_available(&attack, &ctx));
    }

    #[test]
    fn caster_tags_gate_the_skill() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let undead_only = skill(
            vec![Requirement::CasterHasTag(ActorTagId(0))],
            Vec::new(),
            SkillTags::NONE,
        );

        let mut ctx = context(&resources, &cooldowns, &[], 0, None);
        assert!(!skill_available(&undead_only, &ctx));

        let tags = ActorTags(vec![ActorTagId(0)]);
        ctx.tags = Some(&tags);
        assert!(skill_available(&undead_only, &ctx));
    }

    #[test]
    fn traits_are_orthogonal_to_faction() {
        // 玩家阵营 + 亡灵特性 = 完全合法的组合（这正是"亡灵是种族不是阵营"的落地）。
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let undead_only = skill(
            vec![Requirement::CasterHasTag(ActorTagId(0))],
            Vec::new(),
            SkillTags::NONE,
        );

        let undead = ActorTags(vec![ActorTagId(0)]);
        let mut ctx = context(&resources, &cooldowns, &[], 0, None);
        ctx.faction = Some(Faction::Player);
        ctx.tags = Some(&undead);
        assert!(
            skill_available(&undead_only, &ctx),
            "玩家阵营的亡灵也能用亡灵技能"
        );

        // 反过来：怪物阵营 + 同一个特性，同样成立。
        ctx.faction = Some(Faction::Monster);
        assert!(skill_available(&undead_only, &ctx));
    }

    #[test]
    fn target_is_enemy_compares_factions_not_traits() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let strike = skill(
            vec![Requirement::TargetIsEnemy],
            Vec::new(),
            SkillTags::ATTACK,
        );

        let mut ctx = context(&resources, &cooldowns, &[], 0, None);
        ctx.faction = Some(Faction::Player);
        ctx.target_faction = Some(Faction::Monster);
        assert!(skill_available(&strike, &ctx), "敌对阵营 = 敌人");

        ctx.target_faction = Some(Faction::Player);
        assert!(!skill_available(&strike, &ctx), "同阵营不是敌人");

        ctx.target_faction = Some(Faction::Neutral);
        assert!(!skill_available(&strike, &ctx), "中立不算敌人");

        // 没有阵营数据时保守判否（而不是默认"是敌人"）。
        ctx.target_faction = None;
        assert!(!skill_available(&strike, &ctx));
    }

    /// **中立单位谁也打不到**（`Faction::Neutral` 的反向不变式）。
    ///
    /// 上一条测的是"玩家视角下中立不是敌人"；这条是**反过来**：
    /// 中立自己**看谁都不是敌人**。两条都要，因为 `hostile_to` 里
    /// "`self` 是中立"与"`other` 是中立"是**两个独立分支** ——
    /// 只测一个方向的话，条件写漏一半测试照样全绿。
    ///
    /// 它撑起的内容承诺：`monsters.ron` 的 `wandering_merchant` 带
    /// `TargetIsEnemy` 门控，所以**永远打不出任何攻击**，无论对手是谁。
    #[test]
    fn a_neutral_actor_has_no_enemies_at_all() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let strike = skill(
            vec![Requirement::TargetIsEnemy],
            Vec::new(),
            SkillTags::ATTACK,
        );

        let mut ctx = context(&resources, &cooldowns, &[], 0, None);
        ctx.faction = Some(Faction::Neutral);

        for other in [Faction::Player, Faction::Monster, Faction::Neutral] {
            ctx.target_faction = Some(other);
            assert!(
                !skill_available(&strike, &ctx),
                "中立对 {other:?} 不该是敌人"
            );
        }

        // 顺手钉一下：中立例外没把正常判定弄坏。
        ctx.faction = Some(Faction::Monster);
        ctx.target_faction = Some(Faction::Player);
        assert!(skill_available(&strike, &ctx), "怪物看玩家该是敌人");
    }

    #[test]
    fn status_blocks_is_pure_data() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let attack = skill(Vec::new(), Vec::new(), SkillTags::ATTACK);

        // 眩晕封锁 ATTACK / MOVEMENT / COUNTER。
        let stunned = [(
            StatusId(0),
            SkillTags::union(SkillTags::ATTACK, SkillTags::MOVEMENT),
        )];
        let ctx = context(&resources, &cooldowns, &stunned, 0, None);
        assert!(!skill_available(&attack, &ctx), "被封锁的标签不可用");

        let spell = skill(Vec::new(), Vec::new(), SkillTags::SPELL);
        assert!(skill_available(&spell, &ctx), "没被封锁的标签照常可用");
    }

    #[test]
    fn in_status_and_not_in_status_read_the_snapshot() {
        let resources = Resources::default();
        let cooldowns = Cooldowns::default();
        let snapshot = [(StatusId(7), SkillTags::NONE)];

        let wants = skill(
            vec![Requirement::InStatus(StatusId(7))],
            Vec::new(),
            SkillTags::NONE,
        );
        let avoids = skill(
            vec![Requirement::NotInStatus(StatusId(7))],
            Vec::new(),
            SkillTags::NONE,
        );

        let ctx = context(&resources, &cooldowns, &snapshot, 0, None);
        assert!(skill_available(&wants, &ctx));
        assert!(!skill_available(&avoids, &ctx));
    }
}
