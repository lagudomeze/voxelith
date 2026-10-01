//! L1 战斗装配与配置（**Q17 采纳 A**）。
//!
//! 它没有业务系统，只做两件事：
//!
//! 1. 持有 [`CombatConfig`]（**Resource**）：内容层从文件 / 关卡数据构造后**一次注入**；
//! 2. 把各子域的系统按**固定顺序**串起来（顺序是契约，见
//!    [docs/combat-design.md](../../../../docs/combat-design.md) §6）。
//!
//! 每帧顺序（**显式串行**，不靠运气）：
//!
//! ```text
//! PreUpdate  drive_virtual_time          按相位设时间倍率（冻结 = 0.0）
//! Update     ① cast_requests             消费释放请求 → 扣费 / 冷却 / 生成行动
//!            （ApplyDeferred）           把刚生成的行动实体落进世界
//!            ② tick_actions              推进时长（冻结时 delta = 0，自然暂停）
//!            （ApplyDeferred）           把 ReadyToResolve 标记落进世界
//!            ③ resolve_actions           执行效果；瞬发技能在这一步同帧结算并 despawn
//!            ④ apply_status_modifiers    从状态派生属性修饰符 → Stat::refresh
//!            ⑤ tick_statuses             状态倒计时 / on_tick / 到期
//!            ⑥ resolve_status_ticks / detaches  状态的时机效果
//!            ⑦ purge_statuses            消费净化请求
//!            ⑧ compute_available_skills  派生"当前可用技能"（给下一步与 L2 看）
//!            ⑨ monster_tick              怪物攒能量 → 生成威胁行动 + 登记 PendingThreat
//!            ⑩ update_phase              相位转移（空槽 → 等输入；有威胁 + 有反制 → 反制窗口）
//! ```
//!
//! **两处 `ApplyDeferred` 是必需的**：`cast_requests` / `tick_actions` 都通过 `Commands`
//! 写标记，而这些命令默认在本系统集末尾才生效；不显式落一次，下一步那一帧就看不到它们，
//! "瞬发技能同帧结算"会晚一帧（见 [docs/combat-design.md](../../../../docs/combat-design.md) §7）。
//!
//! ⑨⑩ 排在最后：它们决定**下一帧**冻结不冻结，而本帧的结算已经跑完——
//! 这样"反制窗口"永远出现在怪物行动真正生效之前。

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::atoms::actor::ActorPlugin;
use crate::world::WorldPlugin;
use bevy_ecs::schedule::ApplyDeferred;

use crate::behaviors::action::{
    ActionPlugin, cast_requests, compute_available_skills, resolve_actions, tick_actions,
};
use crate::behaviors::contest::{CombatRng, ContestPlugin, DEFAULT_RNG_SEED};
use crate::behaviors::monster::monster_tick;
use crate::behaviors::phase::{PhasePlugin, update_phase};
use crate::behaviors::status::{
    StatusPlugin, apply_status_modifiers, purge_statuses, resolve_status_detaches,
    resolve_status_ticks, tick_statuses,
};
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
            StatusPlugin,
            TimeControlPlugin,
            ActionPlugin,
        ));

        // 跨域顺序就是上面那张表：一整条 `.chain()`，没有歧义、也没有并行冲突。
        //
        // **`apply_deferred` 不是装饰**：`cast_requests` 在 `Commands` 里 spawn 行动实体，
        // 而这些命令默认在本系统集末尾才生效；不在这里落一次，`tick_actions` 那一帧
        // 就看不到刚生成的行动，"瞬发技能同帧结算"会晚一帧（见 docs/combat-design.md §7）。
        app.add_systems(
            Update,
            (
                (
                    cast_requests,
                    ApplyDeferred,
                    tick_actions,
                    ApplyDeferred,
                    resolve_actions,
                )
                    .chain(),
                (
                    apply_status_modifiers,
                    tick_statuses,
                    resolve_status_ticks,
                    resolve_status_detaches,
                    purge_statuses,
                ),
                compute_available_skills,
                monster_tick,
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
                .get_resource::<crate::behaviors::phase::PendingThreat>()
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
