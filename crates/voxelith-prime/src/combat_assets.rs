//! **内容走资产管线**：让 `bevy_asset_loader` 真正干活。
//!
//! ## 为什么需要这个模块
//!
//! 新栈的内容（`attributes.ron` / `actors.ron` / `status_defs.ron` / `abilities.ron`）
//! 一开始是 `load_combat_content` 用 `std::fs` 直接读的——那样：
//!
//! · `bevy_asset_loader` 的 `LoadingState` **没有任何集合**（状态机装了就空转 ✗）；
//! · 内容不在资产服务器里 ⇒ **没有热重载**、也进不了打包流程。
//!
//! 现在两条路并存，由 [`crate::combat::ContentSource`] 选：
//!
//! ```text
//! Disk（默认）   测试与无资产管线的场景：`std::fs` 直读，同步、确定
//! Assets         真应用：资产集合加载完 → `ContentLoad::Ready` → 从 `Assets<RonSource>` 读
//! ```
//!
//! ## 为什么自己写一个加载器
//!
//! Bevy 自带的文本加载器只认 `.txt`（`extensions() == ["txt"]`），而我们的内容叫 `.ron`。
//! 与其把文件改名（内容边界不该为加载器让路），不如写 20 行：
//! [`RonSource`] + [`RonTextLoader`]——**只把字节读成字符串**，解析仍旧归 `voxelith-abilities`。

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy_asset_loader::prelude::*;

/// 一份内容文件的**原文**（`.ron` 的文本）。
///
/// 故意不解析：解析归 `voxelith-abilities`（那里有台账、校验与报错），
/// 资产层只负责"把文件变成可热重载的句柄"。
#[derive(Asset, TypePath, Debug, Clone)]
pub struct RonSource(pub String);

impl RonSource {
    /// 原文。
    pub fn text(&self) -> &str {
        &self.0
    }
}

/// `.ron` 文本加载器（Bevy 自带的那个只认 `.txt`）。
#[derive(Default, TypePath)]
pub struct RonTextLoader;

impl AssetLoader for RonTextLoader {
    type Asset = RonSource;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(RonSource(String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn extensions(&self) -> &[&str] {
        &["ron"]
    }
}

/// 战斗内容资产集合：四份 `.ron`，加载完才会进 `ContentLoad::Ready`。
#[derive(AssetCollection, Resource)]
pub struct CombatAssets {
    /// 全局属性定义（词汇 + 派生 + 回复）。
    #[asset(path = "data/attributes.ron")]
    pub attributes: Handle<RonSource>,
    /// 角色定义。
    #[asset(path = "data/actors.ron")]
    pub actors: Handle<RonSource>,
    /// 状态定义。
    #[asset(path = "data/status_defs.ron")]
    pub statuses: Handle<RonSource>,
    /// 技能定义。
    #[asset(path = "data/abilities.ron")]
    pub abilities: Handle<RonSource>,
}

impl CombatAssets {
    /// 把四份原文取出来（集合刚加载完那一帧，`Assets<RonSource>` 里都在）。
    pub fn texts(&self, sources: &Assets<RonSource>) -> Option<ContentTexts> {
        Some(ContentTexts {
            attributes: sources.get(&self.attributes)?.text().to_string(),
            actors: sources.get(&self.actors)?.text().to_string(),
            statuses: sources.get(&self.statuses)?.text().to_string(),
            abilities: sources.get(&self.abilities)?.text().to_string(),
        })
    }
}

/// 四份内容的原文（从资产或磁盘来都一样）。
#[derive(Debug, Clone)]
pub struct ContentTexts {
    /// `attributes.ron`。
    pub attributes: String,
    /// `actors.ron`。
    pub actors: String,
    /// `status_defs.ron`。
    pub statuses: String,
    /// `abilities.ron`。
    pub abilities: String,
}

/// 只装"`.ron` 资产 + 集合 + 加载状态机"的插件。
///
/// 真应用里由 `EcosystemPlugin` 装它（与另外 7 个库一起）；测试里也能单独装——
/// 这样"内容走资产管线"这条路是可测的，不必把物理 / 音频 / 粒子拖进无头测试。
pub struct CombatAssetsPlugin;

impl Plugin for CombatAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<RonSource>()
            .register_asset_loader(RonTextLoader)
            .init_state::<crate::ecosystem::ContentLoad>()
            // 集合加载完才翻到 `Ready`：**这是 `bevy_asset_loader` 真正干活的地方**
            // （在此之前它只是装了个空转的状态机）。
            .add_loading_state(
                LoadingState::new(crate::ecosystem::ContentLoad::Loading)
                    .load_collection::<CombatAssets>()
                    .continue_to_state(crate::ecosystem::ContentLoad::Ready),
            )
            // 内容来源切到资产管线。
            .insert_resource(crate::combat::ContentSource::Assets);
    }
}
