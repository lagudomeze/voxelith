//! 六项标准属性。
//!
//! 每项就是一个 **newtype**（标记＝它自己），所以“修正指向哪个量”就是“修正指向这个类型”。
//! 全写开、不上宏——一项七行，读和改都更直接。
//!
//! 六项是**引擎能力**（加第七项要改代码）；它们本身**不带规则**，“力量影响什么”写在用到它的 system 里。

use crate::value::{Modifier, Value};
use bevy_ecs::prelude::*;

/// 力量：物理行为的强度。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Strength(pub f32);

impl Value for Strength {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 敏捷：命中与闪避。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Dexterity(pub f32);

impl Value for Dexterity {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 体质：血量上限（`SY-56`），物理豁免。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Constitution(pub f32);

impl Value for Constitution {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 感知：精神强度、暴击与侦测。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Perception(pub f32);

impl Value for Perception {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 魔力：法术强度与法力上限。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Magic(pub f32);

impl Value for Magic {
    fn base(&self) -> f32 {
        self.0
    }
}

/// 意志：法术与精神豁免，精力上限与回复。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Willpower(pub f32);

impl Value for Willpower {
    fn base(&self) -> f32 {
        self.0
    }
}

// ---- 修正别名 ----
//
// 修正组件是 `Modifier<Mark>`，属性这边因为标记就是类型自己，别名长这样：

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
