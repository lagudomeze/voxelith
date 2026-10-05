//! L0 角色：**三大数据类别**的组件与只碰自己的系统。
//!
//! | 类 | 本质 | 回答 | 组件 |
//! |---|---|---|---|
//! | **Resource** | 池（current / max） | "还剩多少？" | [`Resources`] |
//! | **Stat** | 数值 | "有多强？" | [`Stats`]（+ [`StatModifier`]） |
//! | **Status** | 标记（带时长） | "现在什么情形？" | [`ActorState`] |
//!
//! 关系：**Status 修改 Stat，Status / Resource 门控行为，Stat 参与对抗。**
//!
//! 本模块只碰自己：池的加减在这里（[`Resources::modify`]），属性视图的刷新在这里
//! （[`Stats::refresh`]）。跨组件的语义（谁扣谁、扣多少）属于 L1 的 `Effect`。
//!
//! 角色的**三条正交轴**（引擎角色 / 阵营 / 特性）与它们的过滤器在 [`axes`]，
//! 这里重导出以保持 `atoms::actor::Player` 这类路径不变。
//!
//! 数值语义见 [docs/combat-design.md](../../../../../docs/combat-design.md) §2。

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use bevy_time::Time;

use crate::atoms::vocabulary::{ResourceId, SkillId, StatId};

mod axes;

pub use axes::{
    Actor, ActorRole, ActorTags, AiDriven, Faction, InputDriven, Monster, Player, actor_role,
};

// ------------------------------------------------------------------ Resource

/// 一个池：当前值 / 上限。
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct Pool {
    /// 当前值。
    pub current: f32,
    /// 上限。
    pub max: f32,
    /// 每秒自然恢复量（0 = 不恢复，**R52** 的自然恢复就是它）。
    pub regen: f32,
}

impl Pool {
    /// 满池。
    pub fn full(max: f32) -> Self {
        Self {
            current: max,
            max,
            regen: 0.0,
        }
    }

    /// 池是否已空。
    pub fn is_empty(&self) -> bool {
        self.current <= 0.0
    }

    /// 占池比例（上限为 0 时算 0）。
    pub fn ratio(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            self.current / self.max
        }
    }

    /// 夹取到 `0..=max` 后写回。
    fn clamp(&mut self) {
        self.current = self.current.clamp(0.0, self.max);
    }
}

/// 资源池集合（组件）：`ResourceId → Pool`。
#[derive(Component, Debug, Clone, Default, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Resources {
    /// 池。
    pub pools: std::collections::HashMap<ResourceId, Pool>,
}

impl Resources {
    /// 定义 / 覆盖一个池（加载期与生成期用）。
    pub fn define(&mut self, pool: ResourceId, value: Pool) {
        self.pools.insert(pool, value);
    }

    /// 取池（只读）。
    pub fn pool(&self, pool: ResourceId) -> Option<&Pool> {
        self.pools.get(&pool)
    }

    /// 当前值；没有这个池时按 0 算（内容缺池不会炸运行时）。
    pub fn current(&self, pool: ResourceId) -> f32 {
        self.pools.get(&pool).map_or(0.0, |pool| pool.current)
    }

    /// 上限；没有这个池时按 0 算。
    pub fn max(&self, pool: ResourceId) -> f32 {
        self.pools.get(&pool).map_or(0.0, |pool| pool.max)
    }

    /// 是否够扣（`min` 是需求量）。
    pub fn has_at_least(&self, pool: ResourceId, min: f32) -> bool {
        self.current(pool) >= min
    }

    /// 加减一个池（**唯一写入口**）：正数=回复，负数=消耗 / 伤害；夹到 `0..=max`。
    ///
    /// 返回扣费 / 治疗前的值（内容层做"实际生效多少"的日志时会用）。
    pub fn modify(&mut self, pool: ResourceId, delta: f32) -> f32 {
        let Some(entry) = self.pools.get_mut(&pool) else {
            return 0.0;
        };
        let before = entry.current;
        entry.current += delta;
        entry.clamp();
        before
    }

    /// 按固定速率恢复所有池（`Time` 驱动；冻结时 `delta = 0`，自然暂停）。
    pub fn regenerate(&mut self, delta: Duration) {
        let seconds = delta.as_secs_f32();
        if seconds <= 0.0 {
            return;
        }
        for pool in self.pools.values_mut() {
            if pool.regen != 0.0 {
                pool.current += pool.regen * seconds;
                pool.clamp();
            }
        }
    }
}

// ------------------------------------------------------------------ Stat

/// 一条属性修饰符：由某个来源（装备 / 状态实例）派生。
///
/// 它**不含运算种类**：本项目的属性是单纯叠加（数值模型见 [docs/combat-design.md](../../../../../docs/combat-design.md) §2）。
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct StatModifier {
    /// 修饰哪个属性。
    pub stat: StatId,
    /// 增减量。
    pub delta: f32,
    /// 来源实体（状态实例 / 装备）；按来源整批重建 = 按来源清理。
    pub source: Entity,
}

/// 属性集合（组件，**自成一体**）。
///
/// - `base`：内容层设定的初始值（运行期不变）；
/// - `modifiers`：由 L1 的 `apply_status_modifiers` 每帧从状态**重建**；
/// - `effective`：最终值视图 = `base + Σdelta`，由 [`Stats::refresh`] 刷新。
///
/// 读最终值走 [`Stats::get`]；读账本走 [`Stats::base`] / [`Stats::modifiers`]。
#[derive(Component, Debug, Clone, Default, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Stats {
    base: std::collections::HashMap<StatId, f32>,
    modifiers: Vec<StatModifier>,
    effective: std::collections::HashMap<StatId, f32>,
}

impl Stats {
    /// 用一份初始值构造（`effective` 先等于 `base`）。
    pub fn from_base(base: impl IntoIterator<Item = (StatId, f32)>) -> Self {
        let base: std::collections::HashMap<StatId, f32> = base.into_iter().collect();
        Self {
            effective: base.clone(),
            base,
            modifiers: Vec::new(),
        }
    }

    /// 初始值（运行期不变）。
    pub fn base(&self) -> &std::collections::HashMap<StatId, f32> {
        &self.base
    }

    /// 当前修饰符（只读；由 L1 每帧重建）。
    pub fn modifiers(&self) -> &[StatModifier] {
        &self.modifiers
    }

    /// **最终值视图**：参与对抗、被效果读取的就是它。
    pub fn get(&self, stat: StatId) -> f32 {
        self.effective.get(&stat).copied().unwrap_or(0.0)
    }

    /// 整批替换修饰符（crate 内部：`apply_status_modifiers` 专用）。
    pub(crate) fn replace_modifiers(&mut self, modifiers: Vec<StatModifier>) {
        self.modifiers = modifiers;
    }

    /// **刷新最终值视图**：`base + Σdelta`。
    ///
    /// 由变更方（L1 的派生系统、L0 的加载期生成）调用；没有"脏了 / 算好了"的来回消息。
    pub fn refresh(&mut self) {
        self.effective.clone_from(&self.base);
        for modifier in &self.modifiers {
            *self.effective.entry(modifier.stat).or_insert(0.0) += modifier.delta;
        }
    }
}

// ------------------------------------------------------------------ 冷却 / 状态槽 / 能量

/// 冷却表（组件）：`SkillId → 剩余秒数`。
#[derive(Component, Debug, Clone, Default, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Cooldowns(pub std::collections::HashMap<SkillId, f32>);

impl Cooldowns {
    /// 剩余冷却（没有记录 = 0）。
    pub fn remaining(&self, skill: SkillId) -> f32 {
        self.0.get(&skill).copied().unwrap_or(0.0)
    }

    /// 是否已就绪。
    pub fn is_ready(&self, skill: SkillId) -> bool {
        self.remaining(skill) <= 0.0
    }

    /// 开始冷却（取较大值，避免短冷却顶掉长冷却）。
    pub fn start(&mut self, skill: SkillId, seconds: f32) {
        let remaining = self.0.entry(skill).or_insert(0.0);
        *remaining = remaining.max(seconds);
    }

    /// 推进冷却（`Time` 驱动；冻结时 `delta = 0`，自然暂停）。
    pub fn tick(&mut self, delta: Duration) {
        let seconds = delta.as_secs_f32();
        if seconds <= 0.0 {
            return;
        }
        self.0.retain(|_, remaining| {
            *remaining -= seconds;
            *remaining > 0.0
        });
    }
}

/// 状态槽（组件）：指向挂在身上的 [`ActiveStatus`](crate::behaviors::status::ActiveStatus) 实体。
///
/// 它是 `AttachedTo` 的反向集（由关系组件自动维护），所以"清空状态"就是 `Vec::clear`，
/// 不需要遍历世界。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct ActorState {
    /// 身上的状态实例。
    pub statuses: Vec<Entity>,
}

/// 行动能量（组件）：怪物攒够 `threshold` 就出手。
#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct ActionEnergy {
    /// 当前能量。
    pub current: f32,
    /// 触发阈值。
    pub threshold: f32,
    /// 每秒累积速率。
    pub rate: f32,
}

impl ActionEnergy {
    /// 从 0 开始攒。
    pub fn new(rate: f32, threshold: f32) -> Self {
        Self {
            current: 0.0,
            threshold,
            rate,
        }
    }

    /// 推进能量；返回**本次是否达到阈值**（达到则清零，避免一帧连出多次手）。
    ///
    /// **先累加再判**：于是在 `delta = 0` 的帧上，`current` 已经攒到阈值的怪物
    /// 仍然会在本帧出手（否则"攒够了但这一帧 dt 恰好是 0"会白等一帧）。
    ///
    /// `current` 不会超过阈值：达到就清零，所以不存在"攒了两次的量"。
    pub fn tick(&mut self, delta: Duration) -> bool {
        self.current += self.rate * delta.as_secs_f32();
        if self.current >= self.threshold {
            self.current = 0.0;
            return true;
        }
        false
    }
}

/// 注册 L0 角色域：资源池恢复与冷却推进。
///
/// 两者都只查询**自己**的组件（**R8**）。属性视图的刷新不在这里：它由 L1 的
/// `apply_status_modifiers` 在重建修饰符后调用（跨组件语义属于 L1）。
pub struct ActorPlugin;

impl Plugin for ActorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(bevy_app::Update, (regenerate_resources, tick_cooldowns));
    }
}

/// 按 `regen` 恢复所有池（`Time` 驱动）。
pub fn regenerate_resources(time: Res<Time>, mut actors: Query<&mut Resources>) {
    let delta = time.delta();
    for mut resources in &mut actors {
        resources.regenerate(delta);
    }
}

/// 推进所有冷却（`Time` 驱动）。
pub fn tick_cooldowns(time: Res<Time>, mut actors: Query<&mut Cooldowns>) {
    let delta = time.delta();
    for mut cooldowns in &mut actors {
        cooldowns.tick(delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::vocabulary::ActorTagId;

    #[test]
    fn hostility_is_between_different_non_neutral_factions() {
        assert!(Faction::Player.hostile_to(Faction::Monster));
        assert!(Faction::Monster.hostile_to(Faction::Player));
        assert!(!Faction::Player.hostile_to(Faction::Player), "同阵营不敌对");
        assert!(
            !Faction::Neutral.hostile_to(Faction::Player),
            "中立不主动敌对"
        );
        assert!(
            !Faction::Monster.hostile_to(Faction::Neutral),
            "中立也不被敌对"
        );
    }

    #[test]
    fn undead_is_a_trait_not_a_faction() {
        // 一个实体可以同时是"玩家阵营"与"亡灵"——两个维度互不干扰。
        let player_faction = Faction::Player;
        // 特性的取值是**词汇 ID**（由 `vocabulary.ron` 发号），不再是 Rust 枚举：
        // 加一种种族只改内容，不动引擎。
        let undead = ActorTagId(0);
        let beast = ActorTagId(1);
        let traits = ActorTags(vec![undead]);

        assert_eq!(player_faction, Faction::Player);
        assert!(traits.has(undead));
        assert!(!traits.has(beast));
        assert_eq!(ActorTags::default().0.len(), 0, "默认没有特性");
    }

    #[test]
    fn modify_clamps_into_range() {
        let mut resources = Resources::default();
        resources.define(ResourceId(0), Pool::full(100.0));

        assert_eq!(resources.modify(ResourceId(0), -30.0), 100.0);
        assert_eq!(resources.current(ResourceId(0)), 70.0);

        resources.modify(ResourceId(0), -1000.0);
        assert_eq!(resources.current(ResourceId(0)), 0.0, "不会扣成负数");

        resources.modify(ResourceId(0), 1000.0);
        assert_eq!(resources.current(ResourceId(0)), 100.0, "不会溢出上限");
    }

    #[test]
    fn missing_pool_reads_as_zero_and_modify_is_a_noop() {
        let mut resources = Resources::default();
        assert_eq!(resources.current(ResourceId(9)), 0.0);
        assert!(!resources.has_at_least(ResourceId(9), 1.0));
        assert_eq!(resources.modify(ResourceId(9), -5.0), 0.0);
    }

    #[test]
    fn stats_refresh_adds_modifiers_on_top_of_base() {
        let mut world = World::new();
        let source = world.spawn_empty().id();
        let mut stats = Stats::from_base([(StatId(0), 10.0)]);
        assert_eq!(stats.get(StatId(0)), 10.0);

        stats.replace_modifiers(vec![
            StatModifier {
                stat: StatId(0),
                delta: 5.0,
                source,
            },
            StatModifier {
                stat: StatId(1),
                delta: 3.0,
                source,
            },
        ]);
        stats.refresh();

        assert_eq!(stats.get(StatId(0)), 15.0);
        assert_eq!(stats.get(StatId(1)), 3.0, "基础里没有的属性也能被修饰");
        assert_eq!(stats.get(StatId(2)), 0.0, "没提到的属性算 0");
    }

    #[test]
    fn dropping_modifiers_restores_base() {
        let mut world = World::new();
        let source = world.spawn_empty().id();
        let mut stats = Stats::from_base([(StatId(0), 10.0)]);
        stats.replace_modifiers(vec![StatModifier {
            stat: StatId(0),
            delta: 5.0,
            source,
        }]);
        stats.refresh();
        assert_eq!(stats.get(StatId(0)), 15.0);

        stats.replace_modifiers(Vec::new());
        stats.refresh();
        assert_eq!(stats.get(StatId(0)), 10.0, "来源没了，加成自然消失");
    }

    #[test]
    fn cooldowns_tick_down_and_floor_at_zero() {
        let mut cooldowns = Cooldowns::default();
        assert!(cooldowns.is_ready(SkillId(0)));

        cooldowns.start(SkillId(0), 1.0);
        assert!(!cooldowns.is_ready(SkillId(0)));

        cooldowns.tick(Duration::from_millis(400));
        assert!((cooldowns.remaining(SkillId(0)) - 0.6).abs() < 1e-5);

        cooldowns.tick(Duration::from_secs(1));
        assert!(cooldowns.is_ready(SkillId(0)), "到点即移除记录");
    }

    #[test]
    fn starting_a_cooldown_never_shortens_it() {
        let mut cooldowns = Cooldowns::default();
        cooldowns.start(SkillId(1), 5.0);
        cooldowns.start(SkillId(1), 1.0);
        assert!((cooldowns.remaining(SkillId(1)) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn energy_fires_once_per_threshold() {
        let mut energy = ActionEnergy::new(1.0, 3.0);
        assert!(!energy.tick(Duration::from_secs(2)));
        assert!(energy.tick(Duration::from_secs(1)));
        assert!(!energy.tick(Duration::ZERO), "零时长不动");
        assert!(!energy.tick(Duration::from_secs(1)), "触发后清零重新攒");
    }

    #[test]
    fn regeneration_only_touches_pools_with_a_rate() {
        let mut resources = Resources::default();
        resources.define(ResourceId(0), Pool::full(10.0));
        resources.modify(ResourceId(0), -10.0);
        resources.define(
            ResourceId(1),
            Pool {
                current: 0.0,
                max: 10.0,
                regen: 2.0,
            },
        );

        resources.regenerate(Duration::from_secs(1));
        assert_eq!(resources.current(ResourceId(0)), 0.0, "regen = 0 不动");
        assert_eq!(resources.current(ResourceId(1)), 2.0);
    }
}
