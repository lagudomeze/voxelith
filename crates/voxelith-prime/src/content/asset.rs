//! 静态配置的**资产类型**与它们的加载器。
//!
//! 六份 `.ron` 各自是一个 [`Asset`]：
//!
//! | 资产 | 文件（资产根下） | 反序列化成 |
//! |---|---|---|
//! | [`VocabularyAsset`] | `data/vocabulary.ron` | `VocabRon` |
//! | [`SkillsAsset`] | `data/skills.ron` | `Vec<SkillRon>` |
//! | [`StatusesAsset`] | `data/statuses.ron` | `Vec<StatusRon>` |
//! | [`PlayersAsset`] | `data/players.ron` | `Vec<ActorRon>` |
//! | [`MonstersAsset`] | `data/monsters.ron` | `Vec<ActorRon>` |
//! | [`WorldAsset`] | `data/world.ron` | `WorldRon` |
//!
//! ## 为什么一个类型配一个加载器（六个都声明 `.ron`）
//!
//! `AssetLoaders::find` 的解析是**按类型优先**的：给了资产类型就先查
//! `type_id_to_loaders[TypeId::of::<A>()]`，只有一个候选就直接用它，扩展名根本不参与。
//! 所以 `load::<SkillsAsset>("data/skills.ron")` 一定拿到 `SkillsAsset` 的加载器，
//! 六个 `.ron` 不会互相抢（重复扩展名的告警只对"同一个资产类型"才会报）。
//!
//! ## 与 L1 的分工没变（R101 精神）
//!
//! 这里只做"字节 → 描述结构"。[`RawContent`](super::RawContent) → 运行时定义的翻译
//! 仍然在 `voxelith_axiom::behaviors::content`，那一侧不认文件、不认 `AssetServer`，
//! 所以无文件系统的环境里照样能测。

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetApp, AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use serde::Deserialize;

use voxelith_axiom::behaviors::content::descriptor::{ActorRon, SkillRon, StatusRon, VocabRon};
use voxelith_axiom::behaviors::content::descriptor_world::WorldRon;

/// 一份静态配置资产：能取出它内部的**描述结构**。
///
/// 存在的意义是让"六份配置"能在泛型代码里一视同仁（拼 `RawContent`、热重载取数据），
/// 而不必为每一份写一遍。
pub trait ContentAsset: Asset {
    /// 内部描述结构的类型（`VocabRon` / `Vec<SkillRon>` / …）。
    type Ron: Clone;

    /// 取出描述结构。
    fn ron(&self) -> &Self::Ron;
}

/// 六份配置的**身份**：报错 / 诊断时用它说出"是哪一份"。
///
/// 文件名在这里写一遍，[`ContentManifest::scene`](super::ContentManifest::scene) 里写一遍
/// （`bsn!` 要字面量）。两者由 `the_manifest_paths_match_the_labels` 对着真实句柄钉住——
/// 抄错了会红，不会静默错位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentAssetKind {
    /// 词汇表。
    Vocabulary,
    /// 技能。
    Skills,
    /// 状态。
    Statuses,
    /// 玩家模板。
    Players,
    /// 怪物模板。
    Monsters,
    /// 体素世界。
    World,
}

impl ContentAssetKind {
    /// 这份配置对应的**文件**（资产根下的相对路径）。
    pub fn file(self) -> &'static str {
        match self {
            Self::Vocabulary => "data/vocabulary.ron",
            Self::Skills => "data/skills.ron",
            Self::Statuses => "data/statuses.ron",
            Self::Players => "data/players.ron",
            Self::Monsters => "data/monsters.ron",
            Self::World => "data/world.ron",
        }
    }
}

/// 解析一份配置文本。
///
/// ## 为什么必须打开 `UNWRAP_NEWTYPES`
///
/// 六份资产类型都是 newtype 包装（`SkillsAsset(Vec<SkillRon>)`）。RON 默认要求
/// newtype 结构在文件里**写全名字**（`SkillsAsset([...])`）——为了包一层资产类型
/// 去改内容文件的写法是本末倒置。打开这个扩展后 newtype 是透明的，
/// `skills.ron` 照旧只写 `[ ... ]`。
pub fn parse_ron<T: serde::de::DeserializeOwned>(
    text: &str,
    file: &str,
) -> Result<T, RonAssetError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::UNWRAP_NEWTYPES)
        .from_str::<T>(text)
        .map_err(|error| RonAssetError {
            file: file.to_owned(),
            message: error.to_string(),
        })
}

/// RON 配置读不动时的抱怨（读不到 / 不是 UTF-8 / 语法不对）。
///
/// **带上文件名**：配置文件是手写的，报错不带文件名等于让人自己猜是哪一份。
#[derive(Debug, Clone)]
pub struct RonAssetError {
    /// 哪份配置（`AssetPath` 的字符串形态）。
    pub file: String,
    /// 具体哪里不对。
    pub message: String,
}

impl core::fmt::Display for RonAssetError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "解析 {} 失败：{}", self.file, self.message)
    }
}

impl core::error::Error for RonAssetError {}

/// 声明一份"RON → 描述结构"的配置资产：资产类型 + 它的加载器。
///
/// 用宏而不是一个泛型加载器，是为了让 `L::type_path()` 每个类型都不同——
/// `AssetLoaders` 会拿它当元数据（`.meta` 文件、诊断）的键，撞在一起是自找麻烦。
macro_rules! ron_asset {
    ($(#[$meta:meta])* $asset:ident, $loader:ident, $ron:ty) => {
        $(#[$meta])*
        #[derive(Asset, TypePath, Debug, Clone, Deserialize)]
        pub struct $asset(pub $ron);

        impl $asset {
            /// 这份配置对外的名字（日志 / 报错用）。
            pub const LABEL: &'static str = stringify!($asset);
        }

        impl ContentAsset for $asset {
            type Ron = $ron;

            fn ron(&self) -> &$ron {
                &self.0
            }
        }

        /// [`ron_asset!`] 为这份配置生成的加载器。
        #[derive(TypePath, Default)]
        pub struct $loader;

        impl AssetLoader for $loader {
            type Asset = $asset;
            type Settings = ();
            type Error = RonAssetError;

            async fn load(
                &self,
                reader: &mut dyn Reader,
                _settings: &(),
                context: &mut LoadContext<'_>,
            ) -> Result<$asset, RonAssetError> {
                // 先把文件名搬成 `String`：后面要跨 `.await`，不能借着 `context` 不放。
                let file = context.path().to_string();

                let mut bytes = Vec::new();
                reader
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|error| RonAssetError {
                        file: file.clone(),
                        message: format!("读取失败：{error}"),
                    })?;

                let text = core::str::from_utf8(&bytes).map_err(|error| RonAssetError {
                    file: file.clone(),
                    message: format!("不是 UTF-8：{error}"),
                })?;

                parse_ron::<$ron>(text, &file).map($asset)
            }

            fn extensions(&self) -> &[&str] {
                &["ron"]
            }
        }
    };
}

ron_asset!(
    /// 词汇表：资源池 / 属性 / 状态的名字与默认值。
    VocabularyAsset,
    VocabularyAssetLoader,
    VocabRon
);
ron_asset!(
    /// 技能定义。
    SkillsAsset,
    SkillsAssetLoader,
    Vec<SkillRon>
);
ron_asset!(
    /// 状态定义。
    StatusesAsset,
    StatusesAssetLoader,
    Vec<StatusRon>
);
ron_asset!(
    /// 玩家模板。
    PlayersAsset,
    PlayersAssetLoader,
    Vec<ActorRon>
);
ron_asset!(
    /// 怪物模板。
    MonstersAsset,
    MonstersAssetLoader,
    Vec<ActorRon>
);
ron_asset!(
    /// 体素世界（地形参数 + 方块表）。
    WorldAsset,
    WorldAssetLoader,
    WorldRon
);

/// 注册六份配置资产与它们的加载器。
///
/// **必须在任何 `load` 之前调用**：`init_asset` 决定 `Assets<A>` 存不存在，
/// 而 `AssetServer::load` 只认已经注册过的类型。
pub fn register_content_assets(app: &mut App) {
    // `init_asset` 要拿 `AssetServer`；没有它会在 Bevy 内部报一句很难懂的
    // "Resource does not exist"，这里先给一句人话。
    assert!(
        app.world().contains_resource::<bevy::asset::AssetServer>(),
        "`ContentPlugin` 要求先注册 `AssetPlugin`：六份静态配置现在是 `Asset`"
    );

    app.init_asset::<VocabularyAsset>()
        .init_asset::<SkillsAsset>()
        .init_asset::<StatusesAsset>()
        .init_asset::<PlayersAsset>()
        .init_asset::<MonstersAsset>()
        .init_asset::<WorldAsset>()
        .register_asset_loader(VocabularyAssetLoader)
        .register_asset_loader(SkillsAssetLoader)
        .register_asset_loader(StatusesAssetLoader)
        .register_asset_loader(PlayersAssetLoader)
        .register_asset_loader(MonstersAssetLoader)
        .register_asset_loader(WorldAssetLoader);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_config_asset_has_a_distinct_label() {
        let labels = [
            VocabularyAsset::LABEL,
            SkillsAsset::LABEL,
            StatusesAsset::LABEL,
            PlayersAsset::LABEL,
            MonstersAsset::LABEL,
            WorldAsset::LABEL,
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "标签是日志里的身份，不能重名");
    }

    #[test]
    fn the_loader_claims_ron_and_nothing_else() {
        assert_eq!(VocabularyAssetLoader.extensions(), &["ron"]);
        assert_eq!(WorldAssetLoader.extensions(), &["ron"]);
    }

    /// 新类型结构在 serde 里是**透明**的：文件内容仍是裸的 `Vec` / 结构体，
    /// 不需要为了包一层资产类型去改 `.ron` 的写法。
    ///
    /// 这条性质靠 `UNWRAP_NEWTYPES` 撑着——**去掉它这个测试立刻红**（RON 会要求
    /// 文件里写 `SkillsAsset([])`），所以它守着一个真的会退化的点。
    #[test]
    fn the_asset_newtype_is_transparent_to_ron() {
        let parsed: SkillsAsset = parse_ron("[]", "data/skills.ron").expect("空技能表该能解析");
        assert!(parsed.0.is_empty());
    }

    #[test]
    fn a_syntax_error_names_the_file() {
        let error =
            parse_ron::<SkillsAsset>("[ (id: ", "data/skills.ron").expect_err("语法错该报错");
        assert!(
            error.to_string().contains("data/skills.ron"),
            "报错必须带文件名：{error}"
        );
    }
}
