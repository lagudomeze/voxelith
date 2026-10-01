//! 加载器的**数据形状**：错误类型、模板与加载产物。
//!
//! 与 `mod.rs` 的分工：这里只有类型，`mod.rs` 只有流程。

use std::collections::HashMap;

use crate::atoms::actor::{ActorTag, Faction};
use crate::behaviors::content::catalog::{SkillCatalog, StatusCatalog};
use crate::behaviors::content::descriptor::RoleRon;
use crate::behaviors::content::vocabulary::{ResourceId, StatId, UnknownName, Vocab};
use crate::behaviors::monster::MonsterDef;
use crate::world::{BlockDef, TerrainParams, VoxelNames};

/// 加载期错误：**内容配错必须在启动时叫出来**，不能静默跑。
#[derive(Debug, Clone, PartialEq, derive_more::Display, derive_more::Error)]
pub enum LoaderError {
    /// 引用了词汇表里没有的名字。
    #[display("{_0}")]
    UnknownName(#[error(source)] UnknownName),
    /// 词汇表声明了池，但内容里没人引用（多半是漏配）。
    #[display(
        "resource pool `{name}` is declared in vocabulary.ron but never used by any skill or status"
    )]
    UnusedResource {
        /// 未被引用的池名。
        name: String,
    },
    /// 角色 AI 引用了不存在的技能。
    #[display("actor `{actor}` references unknown skill `{skill}`")]
    UnknownSkill {
        /// 角色名。
        actor: String,
        /// 技能名。
        skill: String,
    },
    /// 同一个 `id` 出现了两次（生成时无法判断要哪一个）。
    #[display("actor id `{id}` is declared more than once")]
    DuplicateActor {
        /// 重复的 id。
        id: String,
    },
    /// `role: Monster` 却没有 AI 候选（生成出来的怪物永远不出手）。
    #[display("monster `{id}` has no ai choices; it would never act")]
    MonsterWithoutAi {
        /// 角色 id。
        id: String,
    },
}

impl From<UnknownName> for LoaderError {
    fn from(value: UnknownName) -> Self {
        Self::UnknownName(value)
    }
}

/// 一个资源池的生成参数（来自 `vocabulary.ron`）。
#[derive(Debug, Clone, PartialEq)]
pub struct PoolTemplate {
    /// 显示名。
    pub label: String,
    /// 上限。
    pub max: f32,
    /// 每秒自然恢复。
    pub regen: f32,
    /// 生成时是否满池。
    pub start_full: bool,
}

/// 一个**角色模板**（[`MonsterDef`] + 生成参数）。
///
/// 玩家与怪物共用它：差别只有 [`RoleRon`]（听输入 / 自己 tick）与
/// `energy_*`（只有怪物用）。这样"PC 也是内容"就自然成立，
/// 不必在 L2 里给玩家另写一份硬编码属性。
#[derive(Debug, Clone, PartialEq)]
pub struct ActorTemplate {
    /// 关键名字（L2 生成实体的标识）。
    pub key: String,
    /// 显示名。
    pub name: String,
    /// 引擎角色。
    pub role: RoleRon,
    /// 阵营。
    pub faction: Faction,
    /// 特性标签。
    pub traits: Vec<ActorTag>,
    /// 行为定义（会挂到实体上）。
    pub definition: MonsterDef,
    /// 初始池。
    pub resources: Vec<(ResourceId, f32)>,
    /// 初始属性。
    pub stats: Vec<(StatId, f32)>,
    /// 能量速率。
    pub energy_rate: f32,
    /// 能量阈值。
    pub energy_threshold: f32,
}

impl ActorTemplate {
    /// 是不是"自己 tick"的角色（怪物）。
    pub fn is_monster(&self) -> bool {
        self.role == RoleRon::Monster
    }
}

/// 体素世界模板（`world.ron` 的产物）。
///
/// 名字在这里就已经翻成 ID 了：运行时只用 `u16`（与技能 / 状态同一套路）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldTemplate {
    /// 地形生成参数（方块字段已填成 ID）。
    pub terrain: TerrainParams,
    /// 方块表（顺序即 ID；`0` 是空气，不在表里）。
    pub blocks: Vec<BlockDef>,
    /// 方块名 → ID（日志 / 调试）。
    pub names: VoxelNames,
    /// 纸片人的摆放参数：`(喂给 custom_size 的尺寸, 离地高度)`。
    ///
    /// `world`（L0）**不认识"精灵"这个概念**，所以这里只搬运两个裸数字，
    /// 语义由 L2 的 `SheetPlacement` 赋予。这样 L0 不必知道渲染的存在（**R3**）。
    pub sprite: (f32, f32),
}

/// 一次加载的全部产物。
#[derive(Debug, Default)]
pub struct LoadedContent {
    /// 词汇表。
    pub vocab: Vocab,
    /// 技能目录。
    pub skills: SkillCatalog,
    /// 状态目录。
    pub statuses: StatusCatalog,
    /// **玩家模板**（`role: Player` 的那些；顺序 = 文件顺序）。
    pub players: Vec<ActorTemplate>,
    /// **怪物模板**（`role: Monster` 的那些）。
    pub monsters: Vec<ActorTemplate>,
    /// 体素世界：地形参数 + 方块表（顺序即 ID，`0` 留给空气）。
    pub world: WorldTemplate,
    /// 池的生成参数（按词汇 ID）。
    pub pools: HashMap<ResourceId, PoolTemplate>,
}
