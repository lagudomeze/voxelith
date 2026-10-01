//! 内容加载：读五份 `.ron` → 反序列化 → 交给 L1 解析。

use bevy::prelude::*;
use voxelith_axiom::atoms::actor::{Pool, Resources, Stats};
use voxelith_axiom::behaviors::content::descriptor::{ActorRon, SkillRon, StatusRon};
use voxelith_axiom::behaviors::content::descriptor_world::WorldRon;
use voxelith_axiom::behaviors::content::{
    ActorTemplate, LoadedContent, PoolTemplate, ResourceId, StatId, StatusId, VocabRon, load_all,
    resource_labels, stat_labels, status_labels,
};

/// 五份内容文件（编译期嵌进二进制：改内容要重编，但部署时没有"文件找不到"这类问题）。
const VOCABULARY_RON: &str = include_str!("../../../../assets/data/vocabulary.ron");
const SKILLS_RON: &str = include_str!("../../../../assets/data/skills.ron");
const STATUSES_RON: &str = include_str!("../../../../assets/data/statuses.ron");
const PLAYERS_RON: &str = include_str!("../../../../assets/data/players.ron");
const MONSTERS_RON: &str = include_str!("../../../../assets/data/monsters.ron");
const WORLD_RON: &str = include_str!("../../../../assets/data/world.ron");

/// 内容加载失败：启动期直接 panic（内容配错必须立刻叫出来）。
#[derive(Debug)]
pub enum ContentError {
    /// RON 语法 / 结构不对。
    Parse {
        /// 哪个文件。
        file: &'static str,
        /// 解析器的抱怨。
        message: String,
    },
    /// 语义不对（引用了没登记的词汇 / 漏配池）。
    Load(voxelith_axiom::behaviors::content::LoaderError),
}

impl core::fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse { file, message } => {
                write!(formatter, "failed to parse {file}: {message}")
            }
            Self::Load(error) => write!(formatter, "{error}"),
        }
    }
}

impl core::error::Error for ContentError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::Parse { .. } => None,
        }
    }
}

/// 解析出来的六份原始描述（**Resource**：在 `main` 里就建好，所以任何 `Startup` 都读得到）。
#[derive(Resource, Debug, Clone)]
pub struct RawContent {
    /// 词汇表。
    pub vocabulary: VocabRon,
    /// 技能。
    pub skills: Vec<SkillRon>,
    /// 状态。
    pub statuses: Vec<StatusRon>,
    /// 角色（玩家 + 怪物）。
    pub actors: Vec<ActorRon>,
    /// 体素世界。
    pub world: WorldRon,
}

/// 读六份 `.ron` 并反序列化（**只做解析，不做翻译**）。
///
/// 为什么单独一步、而且要在 `main` 里调用：翻译后的产物（`ContentData`）要通过 `Commands`
/// 注入，而命令要到帧末才落地 —— 先注册的 `Startup` 系统就读不到它。
/// 先把**原始描述**放成 `Resource`，任何 `Startup` 都立刻可用。
pub fn parse_raw() -> Result<RawContent, ContentError> {
    let vocabulary: VocabRon =
        ron::from_str(VOCABULARY_RON).map_err(|error| ContentError::Parse {
            file: "vocabulary.ron",
            message: error.to_string(),
        })?;
    let skills: Vec<SkillRon> = ron::from_str(SKILLS_RON).map_err(|error| ContentError::Parse {
        file: "skills.ron",
        message: error.to_string(),
    })?;
    let statuses: Vec<StatusRon> =
        ron::from_str(STATUSES_RON).map_err(|error| ContentError::Parse {
            file: "statuses.ron",
            message: error.to_string(),
        })?;
    let players: Vec<ActorRon> =
        ron::from_str(PLAYERS_RON).map_err(|error| ContentError::Parse {
            file: "players.ron",
            message: error.to_string(),
        })?;
    let monsters: Vec<ActorRon> =
        ron::from_str(MONSTERS_RON).map_err(|error| ContentError::Parse {
            file: "monsters.ron",
            message: error.to_string(),
        })?;
    let world: WorldRon = ron::from_str(WORLD_RON).map_err(|error| ContentError::Parse {
        file: "world.ron",
        message: error.to_string(),
    })?;

    Ok(RawContent {
        vocabulary,
        skills,
        statuses,
        actors: players.into_iter().chain(monsters).collect(),
        world,
    })
}

/// 把原始描述翻译成运行时定义（字符串 → ID）。
///
/// 返回的 [`LoadedContent`] 里技能 / 状态的定义实体**已经 spawn 完毕**。
pub fn load_content(
    commands: &mut Commands,
    raw: &RawContent,
) -> Result<LoadedContent, ContentError> {
    let vocabulary = &raw.vocabulary;
    let skills = &raw.skills;
    let statuses = &raw.statuses;
    let actors = &raw.actors;
    let world = &raw.world;

    let content = load_all(commands, vocabulary, skills, statuses, actors, world)
        .map_err(ContentError::Load)?;

    // 显示名也在加载期一次性整理好，运行时（日志 / UI）只读。
    commands.insert_resource(Labels {
        pools: resource_labels(vocabulary),
        stats: stat_labels(vocabulary),
        statuses: status_labels(vocabulary),
    });

    Ok(content)
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
