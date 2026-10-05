//! 内容加载：**已装载的描述结构 → 运行时定义**。
//!
//! 这一层不碰文件、不碰 `AssetServer`（**R101 精神**）：
//! 六份 `.ron` 由 [`super::asset`] 的加载器反序列化成描述结构，
//! 由 [`super::manifest`] 的清单拼成 [`RawContent`]，这里只负责
//! **字符串 → 词汇 ID** 的翻译与注入。

use bevy::prelude::*;
use voxelith_axiom::atoms::actor::{Pool, Resources, Stats};
use voxelith_axiom::behaviors::content::descriptor::{ActorRon, SkillRon, StatusRon, VocabRon};
use voxelith_axiom::behaviors::content::descriptor_world::WorldRon;
use voxelith_axiom::behaviors::content::{
    ActorTemplate, LoadedContent, PoolTemplate, ResourceId, ReusedDefs, StatId, StatusId,
    load_all_reusing, resource_labels, stat_labels, status_labels,
};

use super::asset::{ContentAssetKind, RonAssetError};

/// 内容加载失败：启动期直接 panic（内容配错必须立刻叫出来）。
#[derive(Debug)]
pub enum ContentError {
    /// 某份 `.ron` 读不动 / 解析不了。
    Asset(RonAssetError),
    /// 清单实体不在（`ContentPlugin` 装了但 `PreStartup` 的装载没跑）。
    MissingManifest,
    /// 某份配置还没落地到 `Assets<A>` 里就去取了。
    ///
    /// 正常路径走不到（[`super::manifest::load_content_manifest`] 会等到全部就绪）；
    /// 这是给"热重载时正在重新装载"这一刻兜底的。
    AssetNotLoaded {
        /// 哪一份。
        kind: ContentAssetKind,
    },
    /// 语义不对（引用了没登记的词汇 / 漏配池）。
    Load(voxelith_axiom::behaviors::content::LoaderError),
}

impl core::fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Asset(error) => write!(formatter, "{error}"),
            Self::MissingManifest => write!(formatter, "找不到内容清单实体（`ContentManifest`）"),
            Self::AssetNotLoaded { kind } => {
                write!(formatter, "配置 {} 还没装载完", kind.file())
            }
            Self::Load(error) => write!(formatter, "{error}"),
        }
    }
}

impl core::error::Error for ContentError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Asset(error) => Some(error),
            Self::Load(error) => Some(error),
            Self::MissingManifest | Self::AssetNotLoaded { .. } => None,
        }
    }
}

impl From<RonAssetError> for ContentError {
    fn from(error: RonAssetError) -> Self {
        Self::Asset(error)
    }
}

/// 六份原始描述（**Resource**：`PreStartup` 里就建好，所以任何 `Startup` 都读得到）。
#[derive(Resource, Debug, Clone)]
pub struct RawContent {
    /// 词汇表。
    pub vocabulary: VocabRon,
    /// 技能。
    pub skills: Vec<SkillRon>,
    /// 状态。
    pub statuses: Vec<StatusRon>,
    /// 玩家模板。
    pub players: Vec<ActorRon>,
    /// 怪物模板。
    pub monsters: Vec<ActorRon>,
    /// 体素世界。
    pub world: WorldRon,
}

impl RawContent {
    /// 玩家 + 怪物（`load_all` 只认一张角色表，靠 `role` 分组）。
    pub fn actors(&self) -> Vec<ActorRon> {
        self.players.iter().chain(&self.monsters).cloned().collect()
    }
}

/// 把原始描述翻译成运行时定义（字符串 → ID）并**注入 Resource**。
///
/// 首次加载与热重载走的是**同一条链路**（[`load_content_reusing`]），
/// 差别只在有没有旧目录可以复用定义实体。
pub fn load_content(
    commands: &mut Commands,
    raw: &RawContent,
) -> Result<LoadedContent, ContentError> {
    load_content_reusing(commands, raw, None)
}

/// 同 [`load_content`]，但可以复用已有的定义实体（热重载用）。
///
/// 复用是必须的：`Action` / `ActiveStatus` 直接指着定义实体，换实体 = 悬空引用，
/// 而悬空引用不报错（见 [`ReusedDefs`]）。
pub fn load_content_reusing(
    commands: &mut Commands,
    raw: &RawContent,
    reuse: Option<&ReusedDefs>,
) -> Result<LoadedContent, ContentError> {
    let content = load_all_reusing(
        commands,
        &raw.vocabulary,
        &raw.skills,
        &raw.statuses,
        &raw.actors(),
        &raw.world,
        reuse,
    )
    .map_err(ContentError::Load)?;

    install(commands, &content, raw);
    Ok(content)
}

/// 把翻译结果注入 Resource。
///
/// 目录先落，生成实体时才查得到技能 / 状态的**定义实体**；显示名表也在这里一次性整理好，
/// 运行时（日志 / UI）只读。
pub fn install(commands: &mut Commands, content: &LoadedContent, raw: &RawContent) {
    commands.insert_resource(content.vocab.clone());
    commands.insert_resource(content.skills.clone());
    commands.insert_resource(content.statuses.clone());
    commands.insert_resource(Labels {
        pools: resource_labels(&content.vocab, &raw.vocabulary),
        stats: stat_labels(&content.vocab, &raw.vocabulary),
        statuses: status_labels(&content.vocab, &raw.vocabulary),
    });
    commands.insert_resource(ContentData {
        pools: content.pools.clone(),
        players: content.players.clone(),
        monsters: content.monsters.clone(),
        world: content.world.clone(),
    });
}

/// 显示名表（日志 / UI 用；**不参与任何结算**）。
#[derive(Resource, Debug, Clone, Default)]
pub struct Labels {
    /// 池的显示名（按词汇 ID）。
    pub pools: std::collections::HashMap<ResourceId, String>,
    /// 属性的显示名。
    pub stats: std::collections::HashMap<StatId, String>,
    /// 状态的显示名。
    pub statuses: std::collections::HashMap<StatusId, String>,
}

/// 内容数据 Resource：角色模板与池参数（生成别的实体时还能用）。
#[derive(Resource, Debug, Clone, Default)]
pub struct ContentData {
    /// 池的生成参数。
    pub pools: std::collections::HashMap<ResourceId, PoolTemplate>,
    /// 玩家模板。
    pub players: Vec<ActorTemplate>,
    /// 怪物模板。
    pub monsters: Vec<ActorTemplate>,
    /// 体素世界（地形 + 方块表）。
    pub world: voxelith_axiom::behaviors::content::WorldTemplate,
}

/// 按词汇 ID 造一份池集合。
pub fn make_pools(
    templates: &[(ResourceId, f32)],
    defaults: &std::collections::HashMap<ResourceId, PoolTemplate>,
) -> Resources {
    let mut pool = Resources::default();
    for (id, value) in templates {
        let (max, regen, start_full) = defaults
            .get(id)
            .map(|template| (template.max, template.regen, template.start_full))
            .unwrap_or((*value, 0.0, true));
        let current = if start_full { max } else { *value };
        pool.define(
            *id,
            Pool {
                current,
                max,
                regen,
            },
        );
    }
    pool
}

/// 按词汇 ID 造一份属性。
pub fn make_stats(templates: &[(StatId, f32)]) -> Stats {
    Stats::from_base(templates.iter().copied())
}

/// **同步读盘**：直接读 `assets/data/*.ron` 并解析（**测试专用**）。
///
/// 为什么留着这条路：`src/` 里的单元测试（图集、地形、网格）只想要一份
/// `RawContent` 去断言纯函数，不该为了它去装 `AssetPlugin` + `ScenePlugin` +
/// 泵帧等异步装载。**运行时走的是 `AssetServer`**（见 [`super::manifest`]），
/// 两份入口读的是同一批文件、同一套描述结构。
///
/// 只在测试构建里存在，所以出货的二进制里没有 `include_str!` 也不读工作区路径。
#[cfg(test)]
pub fn parse_raw() -> Result<RawContent, ContentError> {
    /// 资产根：`crates/voxelith-prime` 往上退两级。
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

    fn read<A: serde::de::DeserializeOwned>(kind: ContentAssetKind) -> Result<A, ContentError> {
        let path = format!("{ROOT}/{}", kind.file());
        let text = std::fs::read_to_string(&path).map_err(|error| RonAssetError {
            file: kind.file().to_owned(),
            message: format!("读取 {path} 失败：{error}"),
        })?;
        super::asset::parse_ron(&text, kind.file()).map_err(ContentError::Asset)
    }

    Ok(RawContent {
        vocabulary: read(ContentAssetKind::Vocabulary)?,
        skills: read(ContentAssetKind::Skills)?,
        statuses: read(ContentAssetKind::Statuses)?,
        players: read(ContentAssetKind::Players)?,
        monsters: read(ContentAssetKind::Monsters)?,
        world: read(ContentAssetKind::World)?,
    })
}
