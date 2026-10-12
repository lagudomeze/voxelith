//! Voxelith 的核心模型。
//!
//! 这一层**只有 `bevy_ecs`**——不依赖 App、不依赖资产、不依赖渲染，所以逻辑可以无渲染测试。
//! 与引擎无关的语义写在 `docs/design/` 顶层；这里只落已经定下来的那部分。
//!
//! 目前四个模块，边界是“**机制**”与“**具体有哪些量**”：
//!
//! | 模块 | 管什么 |
//! | --- | --- |
//! | [`modifier`] | 修正：一条 [`modifier::Modifier`]（平加 / 比例两种载荷）、指回主人的关系、遍历修正的迭代器 |
//! | [`value`] | 读出接口 [`value::Resolvable`]：基础值 + Σ每条修正的贡献；以及两种形状的说明 |
//! | [`attribute`] | 六项标准属性——每个就是一个 newtype（`Mark = Self`）+ 修正别名 |
//! | [`resource`] | 资源池：当前值 + 基础上限（上限有独立标记）+ 修正别名 |
//!
//! **可修正的量不需要泛型载体**：单值型直接用自己当标记（`type Mark = Self`），
//! 资源池用“上限”那个标记——所以“加一项属性”就是往 `attribute` 的清单里加一行。

pub mod attribute;
pub mod modifier;
pub mod resource;
pub mod value;

/// 常用类型的入口。
///
/// **注意**：使用方通常同时 glob 导入 `bevy_ecs::prelude::*`，所以这里导出的名字
/// 不能与 `bevy_ecs::prelude` 撞（踩过 `Add`、`Resource`、`Single`，见
/// `docs/design/bevy/01-attributes-and-resources.md`）。
pub mod prelude {
    pub use crate::attribute::*;
    pub use crate::modifier::*;
    pub use crate::resource::*;
    pub use crate::value::*;
}
