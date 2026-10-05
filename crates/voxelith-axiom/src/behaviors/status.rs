//! 状态（Status）：**定义 + 实例 + 生命周期**（[docs/combat-design.md](../../../../docs/combat-design.md) §2、§4）。
//!
//! | | 技能 | 状态 |
//! |---|---|---|
//! | 定义 | [`Skill`](crate::behaviors::skill::Skill) 组件 | [`StatusDef`] 组件 |
//! | 实例 | [`Action`](crate::behaviors::action::Action) 组件 | [`ActiveStatus`] 组件 |
//! | 连接 | `CastsSkill` / `InitiatedBy` | [`AttachedTo`] |
//! | 触发 | 结算时跑 `effects` | 四个时机各跑一组 `Effect` |
//! | 持续 | `duration` | `remaining` |
//! | 叠加 | 槽位约束 `len <= 1` | [`Stacking`] |
//! | **共用** | `Effect` / `Value` / `Contest` | **同一套原语** |
//!
//! 状态的数值效果**从不直接改属性**：它派生 [`StatModifier`]，由
//! [`apply_status_modifiers`] 每帧按来源整批重建。所以"状态到期"会自动掉加成，
//! 不需要任何清理消息，也不会有残留。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::Time;

use crate::atoms::actor::{ActorState, StatModifier, Stats};
use crate::behaviors::content::StatusId;
use crate::behaviors::effect::{Effect, EffectContext, EffectParams, execute_effect};
use crate::behaviors::skill::SkillTags;
use crate::behaviors::value::Value;

/// 状态的数值定义：给某个属性加减多少（数值同样是 `Value`，可引用强度 / 池）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModifierDef {
    /// 修饰哪个属性（加载期已解析成 ID）。
    pub stat: crate::behaviors::content::StatId,
    /// 增减量表达式。
    pub delta: Value,
}

/// 叠加规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stacking {
    /// 重复施加刷新剩余时间，层数不变（默认）。
    #[default]
    Refresh,
    /// 重复施加叠层（上限 `max`）。
    Stack {
        /// 层数上限。
        max: u8,
    },
    /// 重复施加直接忽略。
    Ignore,
    /// 重复施加**替换**旧实例（层数重置为 1）。
    Replace,
}

/// 状态定义（组件，全局共享的**模板**）。
#[derive(Component, Debug, Clone, PartialEq)]
pub struct StatusDef {
    /// 状态标识。
    pub id: StatusId,
    /// 显示名。
    pub name: String,
    /// 默认时长（秒）。
    pub default_duration: f32,
    /// 叠加规则。
    pub stacking: Stacking,
    /// 派生给宿主的属性修饰符。
    pub modifiers: Vec<ModifierDef>,
    /// 带这些标签的技能在状态存续期间**不可用**（纯数据门控）。
    pub blocks_tags: SkillTags,
    /// 施加成功时。
    pub on_apply: Vec<Effect>,
    /// 每个结算周期（秒）。
    pub on_tick: Vec<Effect>,
    /// 自然到期时。
    pub on_expire: Vec<Effect>,
    /// 被主动移除（净化 / 取代）时。
    pub on_remove: Vec<Effect>,
}

// **状态的原子（实例 + 槽）在 L0**：它们零依赖（只引用 `StatusId` / `Entity`）。
// 这份文件留着的是**定义**（引用公式簇，R54/R59 把公式划给 L1）与生命周期系统。
pub use crate::atoms::status::{ActiveStatus, AttachedTo, Statuses};
/// 移除状态的原因：决定跑 `on_expire` 还是 `on_remove`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalReason {
    /// 时长走完。
    Expired,
    /// 被主动移除（净化 / 取代）。
    Removed,
}

/// 请求移除一个状态实例（**Message**：跨系统指令）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachStatusMessage {
    /// 要摘掉的状态实例。
    pub status: Entity,
    /// 为什么摘。
    pub reason: RemovalReason,
}

/// 施加状态（**内容无关的程序化入口**）。
///
/// 只是"新建一个实例"这一种情形；带 [`Stacking`] 语义的完整施加在
/// [`apply_status`](crate::behaviors::effect::execute_effect) 的效果执行器里
/// （`Effect::ApplyStatus`）。两条路都保持"实例是唯一载体"。
pub fn spawn_status(
    commands: &mut Commands,
    def: Entity,
    id: StatusId,
    host: Entity,
    source: Entity,
    duration: f32,
) -> Entity {
    commands
        .spawn((
            ActiveStatus {
                def,
                id: Some(id),
                remaining: duration.max(0.0),
                stacks: 1,
                source,
                tick_accumulator: 0.0,
            },
            AttachedTo(host),
        ))
        .id()
}

/// 状态的结算周期与并行安全：只查自己的组件（**R8**）。
///
/// 推进倒计时、累计结算周期；到期 / 有 `on_tick` 时发 [`DetachStatusMessage`]。
/// 副作用（跑效果）由 [`resolve_status_effects`] 执行，避免系统里同时读写世界。
pub fn tick_statuses(
    time: Res<Time>,
    defs: Query<&StatusDef>,
    mut statuses: Query<(Entity, &mut ActiveStatus)>,
    mut detach: MessageWriter<DetachStatusMessage>,
    mut ticks: MessageWriter<StatusTickMessage>,
) {
    let delta = time.delta_secs();
    if delta <= 0.0 {
        return;
    }

    for (entity, mut status) in &mut statuses {
        let Ok(definition) = defs.get(status.def) else {
            continue;
        };

        if !definition.on_tick.is_empty() {
            status.tick_accumulator += delta;
            if status.tick_accumulator >= 1.0 {
                status.tick_accumulator -= 1.0;
                ticks.write(StatusTickMessage { status: entity });
            }
        }

        status.remaining -= delta;
        if status.remaining <= 0.0 {
            detach.write(DetachStatusMessage {
                status: entity,
                reason: RemovalReason::Expired,
            });
        }
    }
}

/// 状态到了结算周期（**Message**：由 [`resolve_status_effects`] 消费）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusTickMessage {
    /// 哪个状态实例。
    pub status: Entity,
}

/// 结算 `on_tick`（状态的每周期效果）。
pub fn resolve_status_ticks(
    mut commands: Commands,
    mut ticks: MessageReader<StatusTickMessage>,
    resources: Query<&crate::atoms::actor::Resources>,
    stats: Query<&Stats>,
    mut params: EffectParams,
) {
    let requests: Vec<Entity> = ticks.read().map(|message| message.status).collect();
    for status_entity in requests {
        run_timing(
            status_entity,
            Timing::Tick,
            &mut commands,
            &resources,
            &stats,
            &mut params,
        );
    }
}

/// 结算 `on_expire` / `on_remove`，然后把实例真正摘掉。
pub fn resolve_status_detaches(
    mut commands: Commands,
    mut detaches: MessageReader<DetachStatusMessage>,
    resources: Query<&crate::atoms::actor::Resources>,
    stats: Query<&Stats>,
    mut params: EffectParams,
) {
    let requests: Vec<(Entity, RemovalReason)> = detaches
        .read()
        .map(|message| (message.status, message.reason))
        .collect();

    for (status_entity, reason) in requests {
        let timing = match reason {
            RemovalReason::Expired => Timing::Expire,
            RemovalReason::Removed => Timing::Remove,
        };
        run_timing(
            status_entity,
            timing,
            &mut commands,
            &resources,
            &stats,
            &mut params,
        );
        commands.entity(status_entity).despawn();
    }
}

/// 跑某一组时机效果（`on_tick` / `on_expire` / `on_remove`）。
fn run_timing(
    status_entity: Entity,
    timing: Timing,
    commands: &mut Commands,
    resources: &Query<&crate::atoms::actor::Resources>,
    stats: &Query<&Stats>,
    params: &mut EffectParams,
) {
    let (Ok(owner), Ok(instance)) = (
        params.attached.get(status_entity),
        params.statuses.get(status_entity),
    ) else {
        return;
    };
    let Ok(definition) = params.status_defs.get(instance.def) else {
        return;
    };
    let effects = match timing {
        Timing::Tick => &definition.on_tick,
        Timing::Expire => &definition.on_expire,
        Timing::Remove => &definition.on_remove,
    };
    if effects.is_empty() {
        return;
    }

    // 借用拆开：池 / 属性来自本系统自己的查询，全局查询由参数包复制一份。
    let actor = owner.0;
    let caster = instance.source;
    // 三个可变资源的借用与"只读视图"不能重叠：先取裸指针，用时才重借。
    let rng_ptr: *mut crate::behaviors::contest::CombatRng = &mut *params.rng;
    let threat_ptr: *mut crate::behaviors::phase::PendingThreat = &mut *params.threat;
    let log_ptr: *mut crate::behaviors::phase::CombatLog = &mut *params.log;
    // 目录缺席时用空目录兜底：`SpawnAction` / `ApplyStatus` 这类效果自然什么都不做，
    // 但同一组里别的效果（例如中毒的掉血）照常执行。
    let empty_skills = crate::behaviors::content::SkillCatalog::default();
    let empty_statuses = crate::behaviors::content::StatusCatalog::default();
    let skill_catalog = params.skills_catalog().unwrap_or(&empty_skills);
    let status_catalog = params.statuses_catalog().unwrap_or(&empty_statuses);
    let mut context = EffectContext {
        commands,
        skill_catalog,
        status_catalog,
        reads: params.reads_view(resources, stats),
        rng: unsafe { &mut *rng_ptr },
        pending_threat: unsafe { &mut *threat_ptr },
        log: unsafe { &mut *log_ptr },
        skill_power: 0.0,
    };
    for effect in effects {
        execute_effect(effect, caster, Some(actor), &mut context);
    }
}

/// 四个时机里"要跑哪一组效果"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timing {
    /// `on_tick`
    Tick,
    /// `on_expire`
    Expire,
    /// `on_remove`
    Remove,
}

/// 从状态**派生**属性修饰符，并刷新最终值视图。
///
/// 每帧整批重建 → 状态到期后加成自动消失（**不需要清理消息**）。
pub fn apply_status_modifiers(
    actors: Query<(Entity, &ActorState)>,
    statuses: Query<(&ActiveStatus, &AttachedTo)>,
    defs: Query<&StatusDef>,
    _catalogs: Option<Res<crate::behaviors::content::StatusCatalog>>,
    resources: Query<&crate::atoms::actor::Resources>,
    mut stats: Query<&mut Stats>,
) {
    for (actor, state) in &actors {
        if state.statuses.is_empty() && stats.get(actor).map_or(true, |s| s.modifiers().is_empty())
        {
            continue;
        }

        let mut derived: Vec<StatModifier> = Vec::new();
        for &status_entity in &state.statuses {
            let Ok((status, owner)) = statuses.get(status_entity) else {
                continue;
            };
            if owner.0 != actor {
                continue;
            }
            let Ok(definition) = defs.get(status.def) else {
                continue;
            };
            if definition.modifiers.is_empty() {
                continue;
            }
            let eval_ctx = crate::behaviors::value::EvalContext {
                caster_resources: resources.get(status.source).ok(),
                caster_stats: stats.get(status.source).ok().map(|s| &*s),
                target_resources: resources.get(actor).ok(),
                target_stats: stats.get(actor).ok().map(|s| &*s),
                skill_power: 0.0,
            };
            let stacks = status.stacks.max(1) as f32;
            for modifier in &definition.modifiers {
                derived.push(StatModifier {
                    stat: modifier.stat,
                    delta: crate::behaviors::value::eval(&modifier.delta, &eval_ctx) * stacks,
                    source: status_entity,
                });
            }
        }

        if let Ok(mut stats) = stats.get_mut(actor) {
            stats.replace_modifiers(derived);
            stats.refresh();
        }
    }
}

/// 请求净化（**Message**：内容层 / UI 发起）。
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeStatusMessage {
    /// 宿主。
    pub host: Entity,
    /// 只清这一种状态；`None` = 全清。
    pub status: Option<StatusId>,
}

/// 消费净化请求 → 发 [`DetachStatusMessage`]。
pub fn purge_statuses(
    mut requests: MessageReader<PurgeStatusMessage>,
    actors: Query<&ActorState>,
    statuses: Query<&ActiveStatus>,
    defs: Query<&StatusDef>,
    mut detach: MessageWriter<DetachStatusMessage>,
) {
    for request in requests.read() {
        let Ok(state) = actors.get(request.host) else {
            continue;
        };
        for &status_entity in &state.statuses {
            let Ok(instance) = statuses.get(status_entity) else {
                continue;
            };
            let matches = match request.status {
                None => true,
                Some(wanted) => defs.get(instance.def).is_ok_and(|def| def.id == wanted),
            };
            if matches {
                detach.write(DetachStatusMessage {
                    status: status_entity,
                    reason: RemovalReason::Removed,
                });
            }
        }
    }
}

/// 注册状态域：三条消息（**R34**）。
///
/// **系统注册在装配层**（[`CombatPlugin`](crate::behaviors::combat::CombatPlugin)）：
/// 状态的生命周期与"派生修饰符"必须和跨域顺序一起排（状态 → 属性 → 相位），
/// 而且只在一个地方注册才不会让同一个系统在一帧里跑两遍。
pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DetachStatusMessage>()
            .add_message::<StatusTickMessage>()
            .add_message::<PurgeStatusMessage>();
    }
}
