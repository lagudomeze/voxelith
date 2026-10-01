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
//! 数值语义见 [docs/combat-design.md](../../../../../docs/combat-design.md) §2。

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use bevy_time::Time;

use crate::behaviors::content::{ResourceId, SkillId, StatId};

/// 角色标记：有战斗数据、能行动、能持有状态。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
pub struct Actor;

/// 玩家标记：`CombatPhase` 只用它判断"空槽等输入"。
///
/// 它是**引擎角色**（"谁来下指令"），不是阵营——阵营见 [`Faction`]。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
pub struct Player;

/// 怪物标记：能量 tick 与 AI 只遍历它。
///
/// 同样是**引擎角色**（"谁自己动"）；一个实体可以既没有它、也没有 [`Player`]（纯道具）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
pub struct Monster;

/// 引擎角色的**可比较值**（[`Player`] / [`Monster`] 的枚举形态）。
///
/// 标记组件适合做查询过滤，但没法当数据比较、也没有"未知"这个状态；
/// 而"这一招归谁"（`Skill::roles`）需要前者。所以同一个概念有两个表示：
/// 组件用于**筛选**，本枚举用于**判定**。
///
/// 放在 L0（而不是内容层）的理由：内容层的 RON 结构要与运行时判定用**同一个**类型，
/// 否则"内容里写 `Player`、判定时比 `Player`"之间还要再翻译一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Deserialize, Reflect)]
pub enum ActorRole {
    /// 玩家侧：听输入，不攒能量。
    Player,
    /// 怪物侧：自己攒能量、跑 AI。
    #[default]
    Monster,
}

/// 从实体身上的标记组件读出角色（两个都没有 → `None`）。
pub fn actor_role(is_player: bool, is_monster: bool) -> Option<ActorRole> {
    match (is_player, is_monster) {
        (true, _) => Some(ActorRole::Player),
        (_, true) => Some(ActorRole::Monster),
        _ => None,
    }
}

/// 阵营：决定"谁打谁"（`Requirement::TargetIsEnemy` 用它）。
///
/// 与 [`Player`] / [`Monster`] 的区别是**正交的两个问题**：
///
/// - [`Player`] / [`Monster`]：引擎怎么驱动它（听输入 / 自己 tick）；
/// - [`Faction`]：它把人当自己人还是敌人。
///
/// 分开之后"被魅惑的怪物帮玩家打"、"友方 NPC 由 AI 驱动"这类情形都不需要改引擎。
#[derive(
    Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Deserialize, Reflect,
)]
#[reflect(Component)]
pub enum Faction {
    /// 玩家侧。
    #[default]
    Player,
    /// 怪物侧。
    Monster,
    /// 中立：跟谁都不敌对，也可以被任何效果作用。
    Neutral,
}

impl Faction {
    /// 是否敌对（中立对任何人都不是敌人）。
    pub fn hostile_to(self, other: Faction) -> bool {
        self != other && self != Faction::Neutral && other != Faction::Neutral
    }
}

/// 角色**特性**标签（内容可配，供 `Requirement::CasterHasTag` 用）。
///
/// 这里只放"跨越阵营的性状"：亡灵既可能是玩家也可能是怪物，
/// 所以它**不是**阵营（那就该用 [`Faction`]），而是特性。
///
/// 新增特性的成本：加一个变体 + 在 `vocabulary.ron` / 内容里用上它。
/// 如果某类判定将来涨到十几个选项、还带参数（"抗性 30%"），那就该从
/// "枚举标签"升级成"带数值的属性"（`Stats`），而不是继续堆枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, Reflect)]
pub enum ActorTag {
    /// 亡灵：不吃治疗、吃驱散之类的判定基础。
    Undead,
    /// 构装体：免疫中毒 / 精神类状态。
    Construct,
    /// 野兽：可被驯服 / 安抚。
    Beast,
}

/// 角色身上的特性标签集合（可同时有多个）。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct ActorTags(pub Vec<ActorTag>);

impl ActorTags {
    /// 是否带某个标签。
    pub fn has(&self, tag: ActorTag) -> bool {
        self.0.contains(&tag)
    }
}

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
        let traits = ActorTags(vec![ActorTag::Undead]);

        assert_eq!(player_faction, Faction::Player);
        assert!(traits.has(ActorTag::Undead));
        assert!(!traits.has(ActorTag::Beast));
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
