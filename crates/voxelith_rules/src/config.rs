//! 规则参数：放在资产里，**一份文件 = 一条规则**。
//!
//! 公式在代码、参数在内容（`SY-59`）。这里只负责把 `.ron` 读成 [`RuleConfig`]。

use bevy_asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy_ecs::error::BevyError;
use bevy_reflect::TypePath;
use serde::Deserialize;

/// 一条派生规则的参数。
///
/// 只管“可变动的参数”——“有哪些规则、挂在谁身上”在 `bsn!` / 模板里声明。
#[derive(Asset, TypePath, Deserialize, Clone, Debug)]
pub struct RuleConfig {
    /// 每点属性给多少（例：“每点体质 +5 血量上限” → `5.0`）。
    pub per_point: f32,
}

/// 把 `.ron` 读成 [`RuleConfig`]。
///
/// `TypePath` 是 `AssetLoader` 的硬要求（它靠类型路径做注册与调试）。
#[derive(Default, TypePath)]
pub struct RuleConfigLoader;

impl AssetLoader for RuleConfigLoader {
    type Asset = RuleConfig;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ron::de::from_bytes::<RuleConfig>(&bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["ron"]
    }
}
