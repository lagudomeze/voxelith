//! 行动：**那一次行动是一个实体**，前摇是它的属性。
//!
//! 模型见 [`docs/design/04-time-and-actions.md`](../../../docs/design/04-time-and-actions.md)。
//!
//! ```text
//! Skill（技能：静态定义）
//!    │  BeginAction ← 生成过程受各种因素影响
//!    ▼
//! Action（行动实体：一次算定，生成后不再变）
//!    │  到点
//!    ▼
//! ResolveAction（消息）→ 效果怎么处理是下一块
//! ```
//!
//! 三条不变量：
//!
//! 1. [`Action`] 生成后**不变**：`resolves_at` / `skill` / `params` 只由 [`begin_actions`] 写一次
//! 2. 只有 [`Potency`] 会变 —— 它是可修正的量 ✓ "削弱敌人在飞的这次行动" = 给行动实体挂
//!    `Modifier<Potency>` ✓ 对抗读的就是 `SY-41` 说的"**此刻**的强度"
//! 3. 生效前可删（打断 / 取消 / 放弃，同一个操作），生效后不可撤回（`GP-06`）

use bevy_ecs::prelude::*;
use bevy_time::{Fixed, Time};
use core::time::Duration;
use voxelith_core::prelude::*;

use crate::time::round_to_step;

// ==================== 技能与参数 ====================

/// 技能。**先写死在代码里**（`D-011`），配置化以后再谈。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillId {
    /// 火球：指向一个落点。
    Fireball,
}

/// 参数形状：技能声明它要哪一种，释放时校验（形状不对就在那一刻炸 ✓ 而不是运行时静默失效 ✗）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// 不要参数（自增益之类）。
    None,
    /// 要一个落点。
    Point,
}

/// 这次行动的参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionParams {
    /// 不要参数。
    None,
    /// 落在某一格。
    Point { at: TilePos },
}

impl ActionParams {
    /// 这个参数是什么形状。
    pub fn kind(&self) -> ParamKind {
        match self {
            Self::None => ParamKind::None,
            Self::Point { .. } => ParamKind::Point,
        }
    }
}

/// 代价：资源（`GP-26`：**在行动出现之前就扣掉**，取消也不退 `SY-43`）。
///
/// 冷却还没做 —— 它要按技能分别记时，属于效果与技能那一块。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    /// 扣多少法力。
    pub mana: f32,
}

/// 技能定义。现在是硬编码表，以后可能是 `.ron` 资产。
#[derive(Clone, Copy, Debug)]
pub struct SkillDef {
    /// 基础前摇（秒）。**实际前摇要经过各种因素**（`GP-35` 的挖掘就是"耐久 ÷ 速率"）。
    pub base_windup: Duration,
    /// 代价。
    pub cost: Cost,
    /// 要哪种参数。
    pub wants: ParamKind,
    /// 强度基准（`SY-40`：属性 + 技能威力 + 修正）。
    pub base_potency: f32,
}

/// 技能表。**加一个技能＝`SkillId` 加一个变体 + 这里加一行**。
pub fn skill_def(skill: SkillId) -> SkillDef {
    match skill {
        SkillId::Fireball => SkillDef {
            base_windup: Duration::from_millis(400),
            cost: Cost { mana: 10.0 },
            wants: ParamKind::Point,
            base_potency: 6.0,
        },
    }
}

// ==================== 行动实体 ====================

/// **那一次行动**（`GP-25` / `GP-27` 针对的就是它）。生成时一次算定，之后不变。
#[derive(Component, Clone, Copy, Debug)]
pub struct Action {
    /// 前摇结束的游戏时刻（`Time<Fixed>::elapsed()` 那个时间轴上的绝对时刻）。
    pub resolves_at: Duration,
    /// 用的哪个技能。
    pub skill: SkillId,
    /// 这次行动的参数。
    pub params: ActionParams,
}

/// 这次行动的强度（`SY-41` 的"此刻"）。被削弱＝给行动实体挂 `Modifier<Potency>`。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[require(Total<Potency>)]
pub struct Potency(pub f32);

impl Value for Potency {
    fn raw(&self) -> f32 {
        self.0
    }

    fn raw_mut(&mut self) -> &mut f32 {
        &mut self.0
    }
}

/// 这次行动是谁在做。
///
/// 不给它派生 `FromTemplate`：行动与威胁都由代码生成 ✓ 内容里不写 ✓
///（而且带派生的关系组件在 `bsn!` 的名单写法下会报"没指定实体" ✗，两件事只能选一件 ✓）
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = Actions)]
pub struct ActionOf(pub Entity);

/// 某个角色身上正在进行的行动。**0 或 1 个** —— 关系天然满足"一次只做一件事"（`GP-03`）。
///
/// `linked_spawn`：角色没了，行动跟着没 ✓（反过来不成立 ✓ 删行动不影响角色 ✓）
#[derive(Component, Default)]
#[relationship_target(relationship = ActionOf, linked_spawn)]
pub struct Actions(Vec<Entity>);

impl Actions {
    /// 名单里的行动实体。
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }

    /// 有几个。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 空不空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ==================== 消息 ====================

/// 释放一次行动。AI 与玩家输入都发这条 ✓ 机制只认它 ✓（`GP-31`：怎么决定是 AI 的事）
#[derive(Message, Clone, Copy, Debug)]
pub struct BeginAction {
    /// 谁放。
    pub actor: Entity,
    /// 用哪个技能。
    pub skill: SkillId,
    /// 打哪 / 指向谁。
    pub params: ActionParams,
}

/// 前摇走完，这次行动生效。**效果怎么处理是下一块** ✓ 现在只是一条消息。
#[derive(Message, Clone, Copy, Debug)]
pub struct ResolveAction {
    /// 谁放的。
    pub actor: Entity,
    /// 那次行动（写日志/展示用；此刻它已经被删了）。
    pub action: Entity,
    /// 用哪个技能。
    pub skill: SkillId,
    /// 参数。
    pub params: ActionParams,
}

// ==================== 系统 ====================

/// 释放：**校验 → 先删旧行动 → 扣代价 → 生成行动实体**（`GP-26` 的次序）。
///
/// 住 `FixedUpdate` ✓ 与时间轴同频 ✓ 所以一切都按 tick 对齐 ✓
pub fn begin_actions(
    mut begins: MessageReader<BeginAction>,
    clock: Res<Time<Fixed>>,
    mut adjustments: MessageWriter<Adjust<Mana>>,
    casting: Query<&Actions>,
    mut commands: Commands,
) {
    // 这一轮里刚生成的行动：命令要到本轮结束才落地，同帧重复释放得靠它才查得到。
    let mut just_began: Vec<(Entity, Entity)> = Vec::new();

    for begin in begins.read() {
        let def = skill_def(begin.skill);

        // ① 校验参数形状：形状不对就在**释放那一刻**炸 ✓ 别等火球飞歪了才发现 ✗
        assert_eq!(
            begin.params.kind(),
            def.wants,
            "技能 {:?} 要 {:?} 形状的参数，收到的是 {:?}",
            begin.skill,
            def.wants,
            begin.params
        );

        // ② 先删掉当前那次行动（`GP-03`：落新技能会取消当前前摇；代价不退）
        let mut cancelled = false;
        just_began.retain(|(actor, action)| {
            if *actor == begin.actor {
                commands.entity(*action).despawn();
                cancelled = true;
                false
            } else {
                true
            }
        });
        if !cancelled && let Ok(current) = casting.get(begin.actor) {
            for old in current.iter() {
                commands.entity(old).despawn();
            }
        }

        // ③ 扣代价。走 `Adjust`（改值的唯一通路），**已扣不退**（`SY-43`）
        if def.cost.mana > 0.0 {
            adjustments.write(Adjust::<Mana>::lose(begin.actor, def.cost.mana, None));
        }

        // ④ 生成行动实体：前摇一次算定 ✓ 四舍五入到最近一步 ✓
        let windup = round_to_step(skill_windup(&def, &begin.params), clock.timestep());
        let action = commands
            .spawn((
                Action {
                    resolves_at: clock.elapsed() + windup,
                    skill: begin.skill,
                    params: begin.params,
                },
                ActionOf(begin.actor),
                Potency(def.base_potency),
            ))
            .id();
        just_began.push((begin.actor, action));
    }
}

/// 前摇要多久。**这里是"生成过程受各种因素影响"的那个点** ✓
///
/// 现在只返回基础值 ✓ 以后要加"施法速度""减耗"这类修正就改这里 ✓ 结构不用动 ✓
/// （`GP-35` 的挖掘是另一种形状：时长由**目标方块**决定，所以签名里带 `params`）
fn skill_windup(def: &SkillDef, _params: &ActionParams) -> Duration {
    def.base_windup
}

/// 到点：发 [`ResolveAction`]，删掉行动实体（效果由下一块处理）。
pub fn resolve_actions(
    clock: Res<Time<Fixed>>,
    mut resolved: MessageWriter<ResolveAction>,
    mut commands: Commands,
    actions: Query<(Entity, &Action, &ActionOf)>,
) {
    let now = clock.elapsed();

    for (action, state, of) in &actions {
        if now >= state.resolves_at {
            resolved.write(ResolveAction {
                actor: of.0,
                action,
                skill: state.skill,
                params: state.params,
            });
            commands.entity(action).despawn();
        }
    }
}

/// 删掉一个角色当前的行动 —— 取消 / 被打断 / 放弃，机制上是同一个操作。
pub fn cancel_action(commands: &mut Commands, current: &Actions) {
    for action in current.iter() {
        commands.entity(action).despawn();
    }
}
