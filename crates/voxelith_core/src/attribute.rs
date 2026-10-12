//! 六项标准属性。
//!
//! 每项一个零大小**标记**（就是它的名字），值用 [`Value`] 载体存。
//! 标记必须 `Clone + Copy + Default`——泛型组件的 derive 会给泛型参数加上同样的约束。
//!
//! 六项是**引擎能力**（加第七项要改代码）；它们本身**不带规则**，“力量影响什么”写在用到它的 system 里。

use crate::carrier::Value;
use crate::modifier::{Flat, Ratio};

/// 力量：物理行为的强度。
#[derive(Clone, Copy, Debug, Default)]
pub struct StrengthStat;
/// 敏捷：命中与闪避。
#[derive(Clone, Copy, Debug, Default)]
pub struct DexterityStat;
/// 体质：血量上限与回复，物理豁免。
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstitutionStat;
/// 感知：精神强度、暴击与侦测。
#[derive(Clone, Copy, Debug, Default)]
pub struct PerceptionStat;
/// 魔力：法术强度与法力上限。
#[derive(Clone, Copy, Debug, Default)]
pub struct MagicStat;
/// 意志：法术与精神豁免，精力上限与回复。
#[derive(Clone, Copy, Debug, Default)]
pub struct WillpowerStat;

// ---- 载体别名：数据文件与 `bsn!` 里用的名字 ----

/// 力量。
pub type Strength = Value<StrengthStat>;
/// 敏捷。
pub type Dexterity = Value<DexterityStat>;
/// 体质。
pub type Constitution = Value<ConstitutionStat>;
/// 感知。
pub type Perception = Value<PerceptionStat>;
/// 魔力。
pub type Magic = Value<MagicStat>;
/// 意志。
pub type Willpower = Value<WillpowerStat>;

// ---- 修正别名 ----

/// 力量平加。
pub type FlatStrength = Flat<StrengthStat>;
/// 敏捷平加。
pub type FlatDexterity = Flat<DexterityStat>;
/// 体质平加。
pub type FlatConstitution = Flat<ConstitutionStat>;
/// 感知平加。
pub type FlatPerception = Flat<PerceptionStat>;
/// 魔力平加。
pub type FlatMagic = Flat<MagicStat>;
/// 意志平加。
pub type FlatWillpower = Flat<WillpowerStat>;

/// 力量比例。
pub type RatioStrength = Ratio<StrengthStat>;
/// 敏捷比例。
pub type RatioDexterity = Ratio<DexterityStat>;
/// 体质比例。
pub type RatioConstitution = Ratio<ConstitutionStat>;
/// 感知比例。
pub type RatioPerception = Ratio<PerceptionStat>;
/// 魔力比例。
pub type RatioMagic = Ratio<MagicStat>;
/// 意志比例。
pub type RatioWillpower = Ratio<WillpowerStat>;
