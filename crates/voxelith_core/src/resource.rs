//! 资源池。
//!
//! 每条资源一个零大小**标记**（它同时就是这条资源“上限”那个量），值用 [`Pool`] 载体存。
//!
//! **当前值是状态**——由事件改，不走修正；**上限走修正**，与属性共用同一套机制。
//! 哪些资源人人都有、哪些按角色配置，见 `docs/design/01-attributes-and-resources.md`。

use crate::modifier::{Flat, Modifier, Ratio};
use crate::value::Pool;

/// 血量上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthMax;
/// 精力上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct StaminaMax;

// ---- 载体别名 ----

/// 血量。
pub type Health = Pool<HealthMax>;
/// 精力。
pub type Stamina = Pool<StaminaMax>;

// ---- 修正别名 ----

/// 血量上限的修正。
pub type ModHealthMax = Modifier<HealthMax>;
/// 精力上限的修正。
pub type ModStaminaMax = Modifier<StaminaMax>;

/// 血量上限平加。
pub type FlatHealthMax = Flat<HealthMax>;
/// 血量上限比例。
pub type RatioHealthMax = Ratio<HealthMax>;
/// 精力上限平加。
pub type FlatStaminaMax = Flat<StaminaMax>;
/// 精力上限比例。
pub type RatioStaminaMax = Ratio<StaminaMax>;
