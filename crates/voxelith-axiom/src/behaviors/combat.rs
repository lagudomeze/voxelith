//! L1 战斗装配与配置（**Q17 采纳 A**）。
//!
//! 它没有业务系统，只做两件事：
//!
//! 1. 持有 [`CombatConfig`]（**Resource**）：内容层从文件 / 关卡数据构造后**一次注入**；
//! 2. 把各子域的系统按**固定顺序**串起来——**这里是系统顺序的唯一出处**（**Q14**）：
//!    各子域的 Plugin 只负责"资源 + 状态 + 消息"，不注册系统（否则同一个系统会跑两遍）。
//!
//! ## 每帧顺序（显式串行，不靠运气）
//!
//! ```text
//! PreUpdate  drive_virtual_time        按相位设时间倍率（冻结 = 0.0）
//! Update
//!   ① compute_available_skills  派生"当前可用技能"（相位判据要用它）
//!   ② monster_intent            意图：槽空 + 窗口空 + 能量满 → 选招 → 付费用 → StartAction
//!   ③ cast_requests             玩家请求：校验 / 扣费 / 冷却 → StartAction
//!     （ApplyDeferred）         让消息与 Commands 落地
//!   ④ commit_actions            **唯一入口**：检查槽 / 窗口 → 建行动 → 发威胁消息
//!     （ApplyDeferred）         让刚建的行动实体落进世界
//!   ⑤ collect_threats           窗口的唯一写入口（读威胁消息）
//!   ⑥ reap_threats              对账：已处理的威胁离场（含存活兜底）
//!   ⑦ update_phase              相位转移（判据：空槽 / 窗口非空 / 有无反制）
//!   ⑧ tick_actions              推进三个阶段（冻结时 delta = 0，自然暂停）
//!     （ApplyDeferred）         让 ReadyToResolve 落进世界
//!   ⑨ resolve_actions           释放点跑效果 / 后摇结束销毁
//!   ⑩ 状态链                  派生修饰符 → 倒计时 / 周期 / 到期 / 净化
//! ```
//!
//! **三处 `ApplyDeferred` 都是必需的**：提交与推进都通过 `Commands` 写实体与标记，
//! 不显式落一次，下一步那一帧就看不到它们——"瞬发技能同帧结算"会晚一帧。
//!
//! ②–⑦ 排在本帧结算之前：它们决定**下一帧**冻不冻结（冻结的生效延迟恰好一帧，
//! 见 [docs/combat-design.md](../../../../docs/combat-design.md) §6.3），
//! 于是反制窗口永远出现在怪物行动真正生效之前。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ApplyDeferred;

use crate::atoms::actor::ActorPlugin;
use crate::world::WorldPlugin;

use crate::behaviors::action::{
    ActionPlugin, cast_requests, commit_actions, compute_available_skills, resolve_actions,
    tick_actions,
};
use crate::behaviors::contest::{CombatRng, ContestPlugin, DEFAULT_RNG_SEED};
use crate::behaviors::monster::monster_intent;
use crate::behaviors::phase::{PhasePlugin, update_phase};
use crate::behaviors::status::{
    StatusPlugin, apply_status_modifiers, purge_statuses, resolve_status_detaches,
    resolve_status_ticks, tick_statuses,
};
use crate::behaviors::threat::{ThreatPlugin, collect_threats, reap_threats};
use crate::behaviors::time_scale::TimeControlPlugin;

/// 战斗配置（**Resource**）：内容层注入一次。
///
/// 内容数值（技能 / 状态 / 池的上限）都在 RON 里，这里只留**引擎级**参数。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatConfig {
    /// 战斗随机种子（同种子 = 同结果，便于复现与测试）。
    pub rng_seed: u64,
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            rng_seed: DEFAULT_RNG_SEED,
        }
    }
}

impl CombatConfig {
    /// 用默认参数构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// 换一个种子。
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
        self
    }
}

/// 战斗装配器：装配 L0 角色域 + L1 各子域 + 固定系统顺序。
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        let seed = app
            .world()
            .get_resource::<CombatConfig>()
            .map_or(DEFAULT_RNG_SEED, |config| config.rng_seed);
        app.insert_resource(CombatRng::new(seed));

        app.add_plugins((
            ActorPlugin,
            // 体素世界数据（地形 / 方块表 / 区块存储）。
            WorldPlugin,
            ContestPlugin,
            PhasePlugin,
            ThreatPlugin,
            StatusPlugin,
            TimeControlPlugin,
            ActionPlugin,
        ));

        // 跨域顺序就是上面那张表：一整条 `.chain()`，没有歧义、也没有并行冲突。
        //
        // ⚠️ **`update_phase` 必须排在整帧结算之后**（这是契约，不是风格）：
        // 它决定**下一帧**冻不冻结，而本帧的结算（行动推进 / 效果 / 状态 tick）已经跑完，
        // 于是"任意时刻 `delta != 0` ⟺ 相位是 `Resolving`"这条性质才成立。
        // 曾经把它放在 `tick_actions` 之前，症状是**状态的周期伤害晚一帧落地**——
        // 三条 `status_flow` 用例（冻结时不该跳、流动时该跳、到期卸掉修饰符）同时变红。
        app.add_systems(
            Update,
            (
                compute_available_skills,
                monster_intent,
                cast_requests,
                ApplyDeferred,
                commit_actions,
                ApplyDeferred,
                collect_threats,
                reap_threats,
                tick_actions,
                ApplyDeferred,
                resolve_actions,
                (
                    apply_status_modifiers,
                    tick_statuses,
                    resolve_status_ticks,
                    resolve_status_detaches,
                    purge_statuses,
                )
                    // ⚠️ 这一组**内部也是顺序依赖的**，必须自己 chain：
                    // `tick_statuses` 写 `StatusTickMessage`，`resolve_status_ticks` 读它。
                    // 只把它们放成一个元组是**没有顺序保证**的（元组不是链）——
                    // 一旦读取方排在写入方之前，它读到的就是**上一帧**的消息，
                    // 症状是"状态的周期伤害晚一帧落地"（`status_flow` 的三条时间语义用例全错位）。
                    // 旧代码同样没 chain，只是碰巧排对了；重写改变了系统集构成，就翻了过来。
                    .chain(),
                // 状态的伤害是**排队命令**（`effect::apply` 读-改-insert），这里落一次。
                ApplyDeferred,
                update_phase,
            )
                .chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_installs_the_shared_resources() {
        let mut app = App::new();
        app.add_plugins(bevy_state::app::StatesPlugin);
        app.insert_resource(CombatConfig::new());
        app.add_plugins(CombatPlugin);

        let world = app.world();
        assert!(world.get_resource::<CombatRng>().is_some());
        assert!(
            world
                .get_resource::<crate::behaviors::threat::ThreatWindow>()
                .is_some()
        );
        assert!(
            world
                .get_resource::<crate::behaviors::phase::AvailableSkills>()
                .is_some()
        );
        assert!(
            world
                .get_resource::<crate::behaviors::phase::CombatLog>()
                .is_some()
        );
        assert!(
            world
                .get_resource::<crate::behaviors::combat::CombatConfig>()
                .is_some(),
            "内容层注入的配置仍在"
        );
    }

    #[test]
    fn seed_flows_into_the_rng() {
        let mut app = App::new();
        app.add_plugins(bevy_state::app::StatesPlugin);
        app.insert_resource(CombatConfig::new().with_seed(7));
        app.add_plugins(CombatPlugin);
        assert_eq!(
            app.world().get_resource::<CombatRng>().copied(),
            Some(CombatRng::new(7))
        );
    }

    #[test]
    fn the_schedule_builds_without_conflicts() {
        // 参数冲突（比如两处 `&mut Resources`）会在第一次 update 时炸出来。
        // 线上 `Time` / 状态转移由 L2 的 `DefaultPlugins` 提供，这里手工补上。
        let mut app = App::new();
        app.add_plugins((bevy_time::TimePlugin, bevy_state::app::StatesPlugin));
        app.add_plugins(CombatPlugin);
        app.update();
    }
}
