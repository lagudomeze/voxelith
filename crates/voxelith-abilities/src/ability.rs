//! **技能（能力）的可读面**：让外部（HUD / 调试 / AI）能问出"这一招现在处于哪一段"。
//!
//! gearbox 的状态机对外只暴露 `Active`（哪个状态在生效）与状态实体本身。于是
//! "这个技能能放吗 / 在冷却吗 / 正在挥吗"这个问题，如果没有标记就得靠**认实体**——
//! 而实体 id 不是给内容层看的东西。
//!
//! 所以构造器在三个状态上各挂一个标记，读的人只查标记：
//!
//! | 状态 | 标记 | 意思 |
//! |---|---|---|
//! | `#Ready` | [`Ready`] | 可以放（门控之外没有别的阻碍） |
//! | `#Invoking` | [`Invoking`](crate::counter::Invoking) | 正在释放（前摇 + 命中） |
//! | `#Cooldown` | [`Cooling`] | 冷却中 |
//!
//! ⚠️ **标记不等于"能放"**：门控（需求 / 硬控 / 空闲 / 费用）在
//! [`crate::casting`] 里，只有那里能回答"这一招现在放得出来吗"。标记回答的是
//! "状态机自己在哪一段"——两者是两个问题。
//!
//! 历史说明：[`Invoking`](crate::counter::Invoking) 定义在 `counter`（反制要用它挑
//! "我正忙的技能"），比本模块早；这里补上另外两个并统一说明。

use bevy::prelude::*;

/// 这个技能正停在 `#Ready`（状态机层面可放）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Ready;

/// 这个技能正在冷却（`#Cooldown`）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Cooling;

/// **技能的内容 id**（`abilities.ron` 里的 `id`），挂在技能根上。
///
/// 用途是"**跨存档认出同一个技能**"：技能是实体（不能直接存进档里），
/// 所以存的是 id，读档时按 id 重建。调试与 HUD 也顺手能用。
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct AbilityId(pub String);
