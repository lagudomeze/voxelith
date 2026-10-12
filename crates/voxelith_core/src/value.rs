//! “读出”这件事：可修正的量怎么算出最终值。
//!
//! 两种形状，**都不需要泛型载体**：
//!
//! | 形状 | 哪个值可修正 | 标记 |
//! | --- | --- | --- |
//! | **单值型**（属性、护甲、移动速度……） | 它本身 | 类型自己（`Mark = Self`） |
//! | **资源**（血量、精力……） | **只有上限** | [`Max<T>`] |
//!
//! **当前值不走修正**：它是状态，由事件（伤害、治疗、回复）改。所以资源那边
//! `resolve_max` 就是它的“读出”，不存在“修正当前值”这回事。
//!
//! 读出规则只有一条，在 [`crate::modifier::resolve_with`]：
//!
//! ```text
//! 最终值 = 基础值 + Σ每条修正的贡献(基础值)      // 比例按基础值算
//! ```
//!
//! **这里没有 `resolved(world, owner)` 这种东西**：写 system 时手里已经有组件和
//! 关联列表（`&Modifiers`），把它里面的实体喂给修正查询即可——见 `tests/system_read.rs`。

use crate::modifier::{resolve_with, Modifier};
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 单值型：只有一个基础值（属性、护甲、移动速度、挖掘力……）。
pub trait Resolvable: Component + Sized {
    /// 基础值。
    fn base(&self) -> f32;

    /// 读出：基础值 + Σ每条修正的贡献。
    fn resolve<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Self>>) -> f32 {
        resolve_with(self.base(), modifiers)
    }
}

/// 资源：当前值 + 上限。**只有上限可修正。**
pub trait Resourceable: Component + Sized {
    /// 当前值（状态，由事件改）。
    fn current(&self) -> f32;

    /// 上限的基础值。
    fn base_max(&self) -> f32;

    /// 读出**最终上限**。当前值不走这里。
    fn resolve_max<'a>(&'a self, modifiers: impl Iterator<Item = &'a Modifier<Max<Self>>>) -> f32 {
        resolve_with(self.base_max(), modifiers)
    }
}

/// 上限那个量。
///
/// 资源有两个值，所以上限需要一个名字；单值型只有一个值，标记就是它自己。
#[derive(Clone, Copy, Debug, Default)]
pub struct Max<T>(PhantomData<T>);
