//! **角色的三条正交轴**：引擎角色 / 阵营 / 特性（见 docs/combat-design.md §4.0）。
//!
//! 从 `super`（`actor`）切出来是因为那个文件越过了 R26 的 500 行。
//! 切法按**语义**而不是按行数：这里只放"一个角色是什么"（标记、角色值、阵营、特性、
//! 以及按引擎角色轴筛查询的两个过滤器）；数值类（池 / 属性 / 冷却 / 状态槽 / 能量）
//! 留在 `super`。

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryFilter;
use bevy_reflect::Reflect;

use crate::atoms::vocabulary::ActorTagId;

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

/// **引擎角色轴**的过滤器：**听输入**的那一侧。
///
/// 它筛的是"谁听输入"，**不是**"谁是自己人"：一个由 AI 驱动的友方 NPC 是
/// `Faction::Player`，却不该被它选中。写法与判据见
/// [docs/bevy-queries.md](../../../../../docs/bevy-queries.md) §2。
#[derive(QueryFilter)]
pub struct InputDriven {
    /// 引擎角色轴的玩家侧。
    player: With<Player>,
}

/// **引擎角色轴**的过滤器：**自己 tick**（攒能量、跑 AI）的那一侧，与 [`InputDriven`] 成对。
#[derive(QueryFilter)]
pub struct AiDriven {
    /// 引擎角色轴的非玩家侧。
    not_player: Without<Player>,
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

/// 角色身上的**特性**标签集合（可同时有多个）。
///
/// ## 为什么存 `Vec<ActorTagId>` 而不是一个枚举
///
/// "亡灵 / 构装体 / 野兽"这些取值**随游戏内容增长**：
/// [docs/layers.md](../../../../../docs/layers.md) §9 的判据是
/// **"这个集合会随游戏内容增长吗"** —— 会，所以走 `vocabulary.ron` + ID，
/// 于是**加一种种族只改内容**，不动 Rust（与"加内容不改代码"一致）。
///
/// 曾经它是个 `enum ActorTag`，代价是每加一种特性都要动引擎代码；
/// 而且那个枚举住在 L0，等于把游戏世界观焊进了原子层。
///
/// **与 [`Faction`] 的区别不是"是不是标签"，而是"带不带规则"**：
/// `Faction::hostile_to` 是一条引擎级的敌对规则，所以它是枚举；
/// 特性只是用来比较相等性的名字，所以它是词汇。
///
/// 如果某类特性将来涨到"带参数"（"亡灵抗性 30%"），那就该升级成
/// 带数值的属性（`Stats`），而不是继续往词汇表里堆名字。
#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct ActorTags(pub Vec<ActorTagId>);

impl ActorTags {
    /// 是否带某个特性。
    pub fn has(&self, tag: ActorTagId) -> bool {
        self.0.contains(&tag)
    }
}
