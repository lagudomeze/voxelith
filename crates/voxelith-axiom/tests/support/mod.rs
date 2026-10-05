//! 集成测试的共用脚手架：真实 `App`、手动时钟、实体工厂。
//!
//! 放在 `tests/support/` 里被各测试文件 `mod support;` 引入（不是每个文件抄一遍）。
//! 它**只做搭台**，不含任何断言——断言留在各自的测试文件里。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::app::StatesPlugin;
use bevy_time::{Time, TimePlugin, TimeUpdateStrategy, Virtual};

use voxelith_axiom::atoms::actor::{
    ActionEnergy, Actor, ActorState, ActorTags, Cooldowns, Faction, Monster, Player, Pool,
    Resources, Stats,
};
use voxelith_axiom::behaviors::action::CastRequest;
use voxelith_axiom::behaviors::combat::CombatPlugin;
use voxelith_axiom::behaviors::content::{ResourceId, SkillId, StatId, StatusId};
use voxelith_axiom::behaviors::effect::Effect;
use voxelith_axiom::behaviors::monster::{AiChoice, MonsterDef};
use voxelith_axiom::behaviors::requirement::Targeting;
use voxelith_axiom::behaviors::skill::{Skill, SkillTags};
use voxelith_axiom::behaviors::status::{ActiveStatus, AttachedTo, Stacking, StatusDef};
pub const HP: ResourceId = ResourceId(0);
pub const ACTION: ResourceId = ResourceId(1);

pub fn test_app() -> App {
    let mut app = App::new();
    // `CombatPlugin` 内部已经装配了 `ActorPlugin` / `ActionPlugin` 等子域。
    app.add_plugins((TimePlugin, StatesPlugin, CombatPlugin));
    // **必须先"点火"一次**：`Time<Real>` 的第一次更新只记下起始时刻、`delta` 仍是 0
    // （`real.rs`：`last_update` 为 `None` 时直接 return）。不先烧掉这一帧，
    // 每个测试的**第一次** `step()` 都会拿到 `delta = 0`，"推进时间"悄悄失效。
    prime_clock(&mut app);
    app
}

/// 烧掉 `Time<Real>` 的初始化帧，让之后的 `step()` 都能拿到真实时长。
fn prime_clock(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        core::time::Duration::ZERO,
    ));
    app.update();
}

/// 手动推进时钟：让 `TimePlugin` 每帧拿一个**固定**的真实时长。
///
/// 用 `TimeUpdateStrategy::ManualDuration` 而不是直接改 `Time<Virtual>`，
/// 是因为 `TimePlugin` 在 `First` 里会把 `Time<Virtual>` 按倍率重算一遍；
/// 手改的数值会被它覆盖。走策略这条路，"倍率 0 → `delta` 0"的冻结语义才和线上一致。
pub fn step(app: &mut App, seconds: f32) {
    // Time<Virtual> 默认 max_delta 是 250ms：单帧真实时长超过它会被夹掉，
    // 所以测试里先把上限放宽，再给一帧时长。
    let duration = core::time::Duration::from_secs_f32(seconds);
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(duration.max(core::time::Duration::from_millis(1)));
    app.world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(duration));
    app.update();
}

/// 让时间保持流动：给玩家塞一个很长的行动，这样相位停在 `Resolving`。
///
/// 玩家空槽时相位会切到 `AwaitingInput` 并把时间冻结——那是**正确行为**，
/// 所以想观察"时间流动下的推进"，就得先让玩家不空闲。
pub fn keep_resolving(app: &mut App, player: Entity) {
    let blocker = spawn_skill(
        app.world_mut(),
        99,
        "blocker",
        600.0,
        SkillTags::ATTACK,
        Vec::new(),
    );
    app.world_mut().write_message(CastRequest {
        caster: player,
        skill: blocker,
        target: None,
    });
    app.update();
}

/// 造一个技能定义实体。
pub fn spawn_skill(
    world: &mut World,
    id: u16,
    name: &str,
    duration: f32,
    tags: SkillTags,
    effects: Vec<Effect>,
) -> Entity {
    world
        .spawn(Skill {
            id: SkillId(id),
            name: name.into(),
            icon: String::new(),
            tags,
            // 测试技能默认**不限定角色**（谁都能用）；要测归属就自己改。
            roles: Vec::new(),
            duration,
            requirements: Vec::new(),
            costs: Vec::new(),
            targeting: Targeting::SelfOnly,
            effects,
        })
        .id()
}

/// 造一个玩家（池 + 属性 + 槽 + 阵营）。
pub fn spawn_player(world: &mut World) -> Entity {
    let mut pools = Resources::default();
    pools.define(HP, Pool::full(100.0));
    pools.define(ACTION, Pool::full(1.0));
    world
        .spawn((
            Actor,
            Player,
            Faction::Player,
            pools,
            Stats::from_base([(StatId(0), 10.0)]),
            Cooldowns::default(),
            ActorState::default(),
        ))
        .id()
}

/// 造一个怪物（阵营可调，用来验证敌我判定走 `Faction` 而不是标记）。
pub fn spawn_monster(world: &mut World, faction: Faction) -> Entity {
    let mut pools = Resources::default();
    pools.define(HP, Pool::full(50.0));
    world
        .spawn((
            Actor,
            Monster,
            faction,
            pools,
            Stats::from_base([(StatId(0), 5.0)]),
            Cooldowns::default(),
            ActorState::default(),
        ))
        .id()
}

/// 造一个会自己攒能量的怪物（`monster_decide` 只遍历带 `ActionEnergy` 的实体）。
///
/// 能量参数用 [`EnergySpec`] 而不是两个裸 `f32`：这两个数**能互换**，
/// 裸参数写反了只会得到一个"阈值 0、永远立刻出手"的怪物，测试却看起来通过了。
pub fn spawn_ai_monster(
    world: &mut World,
    faction: Faction,
    choices: Vec<AiChoice>,
    energy: EnergySpec,
) -> Entity {
    let mut pools = Resources::default();
    pools.define(HP, Pool::full(50.0));
    world
        .spawn((
            Actor,
            Monster,
            faction,
            ActionEnergy {
                current: energy.start,
                rate: energy.rate,
                threshold: energy.threshold,
            },
            MonsterDef {
                name: "test-monster".into(),
                ai: choices,
            },
            pools,
            Stats::from_base([(StatId(0), 5.0)]),
            Cooldowns::default(),
            ActorState::default(),
            ActorTags::default(),
        ))
        .id()
}

/// 怪物的能量参数（显式命名，避免把 `rate` 与 `threshold` 写反）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergySpec {
    /// 出手门槛。
    pub threshold: f32,
    /// 每秒攒多少。
    pub rate: f32,
    /// 初始值。
    pub start: f32,
}

impl EnergySpec {
    /// 从 0 开始、按 `rate`/秒攒到 `threshold` 才出手。
    pub fn charging(threshold: f32, rate: f32) -> Self {
        Self {
            threshold,
            rate,
            start: 0.0,
        }
    }

    /// 一帧就攒满的"秒出"配置（测别的东西时用）。
    pub fn instant() -> Self {
        Self {
            threshold: 1.0,
            rate: 1000.0,
            start: 0.0,
        }
    }
}

/// 给实体挂一个状态实例（定义 + 实例 + 槽登记），返回实例实体。
///
/// `blocks` 是门控标签，`modifiers` 是派生修饰符。
pub fn attach_status(
    world: &mut World,
    host: Entity,
    id: u16,
    name: &str,
    duration: f32,
    blocks: SkillTags,
    on_tick: Vec<Effect>,
) -> Entity {
    let def = world
        .spawn(StatusDef {
            id: StatusId(id),
            name: name.into(),
            default_duration: duration,
            stacking: Stacking::Refresh,
            modifiers: Vec::new(),
            blocks_tags: blocks,
            on_apply: Vec::new(),
            on_tick,
            on_expire: Vec::new(),
            on_remove: Vec::new(),
        })
        .id();
    let status = world
        .spawn((
            ActiveStatus {
                def,
                id: Some(StatusId(id)),
                remaining: duration,
                stacks: 1,
                source: host,
                tick_accumulator: 0.0,
            },
            AttachedTo(host),
        ))
        .id();
    if let Some(mut state) = world.get_mut::<ActorState>(host) {
        state.statuses.push(status);
    }
    status
}
