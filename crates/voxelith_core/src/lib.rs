//! Voxelith 的核心模型。
//!
//! 这一层**只有 `bevy_ecs`**——不依赖 App、不依赖资产、不依赖渲染，所以逻辑可以无渲染测试。
//! 与引擎无关的语义写在 `docs/design/` 顶层；这里只落已经定下来的那部分。
//!
//! 目前三个模块，边界是“**机制**”与“**具体有哪些量**”：
//!
//! | 模块 | 管什么 |
//! | --- | --- |
//! | [`value`] | **可修正的量**这一层的基础代码：修正 [`value::Modifier`]、指回主人的关系、唯一的公式 [`value::resolve_with`]、读出接口 [`value::Value`] / [`value::Resourceable`]、上限标记 [`value::Max`] |
//! | [`attribute`] | 六项标准属性——每个就是一个 newtype（标记＝自己）+ 修正别名 |
//! | [`resource`] | 资源：当前值 + 上限（上限用 `Max<T>` 标记）+ 修正别名 |
//!
//! **加一项属性＝往 `attribute` 的清单里加一行**；加一条资源＝在 `resource` 里写一个 struct
//! 加一句 `impl`。

pub mod attribute;
pub mod resource;
pub mod value;

/// 常用类型的入口。
///
/// **注意**：使用方通常同时 glob 导入 `bevy_ecs::prelude::*`，所以这里导出的名字
/// 不能与 `bevy_ecs::prelude` 撞（踩过 `Add`、`Resource`、`Single`，见
/// `docs/design/bevy/01-attributes-and-resources.md`）。
pub mod prelude {
    pub use crate::attribute::*;
    pub use crate::resource::*;
    pub use crate::value::*;
}
