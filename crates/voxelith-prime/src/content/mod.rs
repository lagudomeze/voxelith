//! L2 内容层：**静态配置的装载 + 翻译 + 注入**。
//!
//! 分工（**R101 精神**）：
//!
//! | 谁 | 干什么 |
//! |---|---|
//! | [`asset`] | 六份 `.ron` 的**资产类型与加载器**（字节 → 描述结构） |
//! | [`manifest`] | [`ContentManifest`]（`SceneComponent`）：**路径的唯一出处** + 启动期等装载 |
//! | [`loader`] | 描述结构 → 运行时定义（字符串 → 词汇 ID）+ 注入 `Resource` |
//! | [`reload`] | 配置改了就地重新翻译（热重载） |
//! | `axiom::behaviors::content`（L1） | 描述结构与解析规则，**不认文件、不认 `AssetServer`** |
//!
//! 这样 `axiom` 仍然在"无文件系统"的环境里可测：它只认描述结构。

mod asset;
mod loader;
mod manifest;
mod reload;
mod spawn;

pub use asset::{
    ContentAsset, ContentAssetKind, MonstersAsset, PlayersAsset, RonAssetError, SkillsAsset,
    StatusesAsset, VocabularyAsset, WorldAsset, register_content_assets,
};
pub use loader::{
    ContentData, ContentError, Labels, RawContent, load_content, load_content_reusing, make_pools,
    make_stats,
};
pub use manifest::{
    ContentChanges, ContentManifest, LoadedContentAssets, load_content_manifest, raw_from_world,
};
pub use spawn::spawn_demo;

#[cfg(test)]
pub use loader::parse_raw;

use bevy::prelude::*;

/// 内容装配：装载六份静态配置、翻译成运行时定义、注入 Resource，并生成角色。
///
/// **装载在 `PreStartup`，翻译在 `Startup`**：前者把"内容一定在"这条不变式守住，
/// 于是后面那些读 `ContentData` 的 `Startup` 系统一行都不用改。
pub struct ContentPlugin;

/// 内容启动的两个阶段（**顺序契约**）。
///
/// 为什么用 `SystemSet` 而不是 `.after(函数)`：别的模块（体素表现）要依赖"内容已翻译好"，
/// 而它不该知道翻译函数叫什么名字。集合名是稳定的接口。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentSet {
    /// 翻译 `.ron` 并注入 `Resource`（技能 / 状态的**定义实体**也在这一步 spawn）。
    Translate,
    /// 生成 PC 与怪物（依赖上一步注入的 `ContentData`）。
    Spawn,
}

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        // 六份配置的资产类型与加载器必须先注册：`AssetServer::load` 只认注册过的类型。
        register_content_assets(app);

        // **`PreStartup` 装载并等到就绪**（阻塞；见 `manifest` 的模块文档）。
        app.add_systems(PreStartup, load_content_manifest)
            // 两个阶段**必须串行**：`Spawn` 读 `Translate` 注入的 `ContentData`，
            // 而 `Commands` 要到系统之间才落地。体素表现再把图集 / 地形排在 `Translate` 之后。
            .configure_sets(Startup, (ContentSet::Translate, ContentSet::Spawn).chain())
            .add_systems(
                Startup,
                setup_content_resources.in_set(ContentSet::Translate),
            )
            .add_systems(Startup, spawn_content_actors.in_set(ContentSet::Spawn))
            // 改 `.ron` → `AssetEvent::Modified` → 原地重新翻译（开发期特性，见 `reload`）。
            .add_systems(Update, reload::reload_content);
    }
}

/// 翻译内容 → 注入 `Resource`（不生成角色）。
///
/// 在 [`ContentSet::Translate`] 里跑；体素表现的图集 / 地形排在它之后，
/// 因为它们要读翻译后的方块表与地形参数。
pub fn setup_content_resources(mut commands: Commands, raw: Res<RawContent>) {
    load_content(&mut commands, &raw).unwrap_or_else(|error| panic!("内容翻译失败：{error}"));
}

/// 生成 PC 与怪物（读已经注入的 `ContentData`）。
///
/// 公开是为了让体素表现的那条 `Startup` 链把它排在资源注入之后——
/// 否则它会与注入并列执行，读到还不存在的 `ContentData`。
pub fn spawn_content_actors(mut commands: Commands, data: Res<ContentData>) {
    spawn::spawn_demo(&mut commands, &data);
}
