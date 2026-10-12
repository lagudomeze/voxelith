//! 六项标准属性。
//!
//! 每项就是一个 **newtype**（`Mark = Self`），所以“修正指向哪个量”就是“修正指向这个类型”。
//!
//! 六项是**引擎能力**（加第七项要改代码）；它们本身**不带规则**，“力量影响什么”写在用到它的 system 里。

use crate::modifier::Modifier;
use crate::value::Resolvable;
use bevy_ecs::prelude::*;

/// 声明一项单值属性：组件 + 读出实现。
///
/// 这样“加一项属性”就是往下面的清单里加一行。
macro_rules! single_value {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Component, Clone, Copy, Debug, Default)]
            pub struct $name(pub f32);

            impl Resolvable for $name {
                fn base(&self) -> f32 {
                    self.0
                }
            }
        )+
    };
}

single_value! {
    /// 力量：物理行为的强度。
    Strength,
    /// 敏捷：命中与闪避。
    Dexterity,
    /// 体质：血量上限与回复，物理豁免。
    Constitution,
    /// 感知：精神强度、暴击与侦测。
    Perception,
    /// 魔力：法术强度与法力上限。
    Magic,
    /// 意志：法术与精神豁免，精力上限与回复。
    Willpower,
}

// ---- 修正别名 ----
//
// 修正组件是 `Modifier<Mark>`，属性这边因为 `Mark = Self`，别名长这样：

/// 力量的修正。
pub type ModStrength = Modifier<Strength>;
/// 敏捷的修正。
pub type ModDexterity = Modifier<Dexterity>;
/// 体质的修正。
pub type ModConstitution = Modifier<Constitution>;
/// 感知的修正。
pub type ModPerception = Modifier<Perception>;
/// 魔力的修正。
pub type ModMagic = Modifier<Magic>;
/// 意志的修正。
pub type ModWillpower = Modifier<Willpower>;
