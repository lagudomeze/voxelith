//! 内容层（L1）：**词汇表 + RON 描述 + 解析 + 目录**。
//!
//! 这一层回答两个问题：
//!
//! 1. **内容文件长什么样** → [`descriptor`]（纯数据，字符串保持字符串）；
//! 2. **字符串怎么变成运行时 ID** → [`loader`]（解析一次，运行时只跑 ID）。
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`vocabulary`] | 四种词汇 ID + [`Vocab`]（名字 ⇄ ID 双向表） |
//! | [`descriptor`] | RON 描述结构（技能 / 状态 / 怪物 / 词汇表） |
//! | [`loader`] | 解析 + **加载期覆盖检查**（漏配必须在启动时报错） |
//! | [`catalog`] | [`SkillCatalog`] / [`StatusCatalog`]（ID → 定义实体） |
//!
//! **本层不读文件**（R101 精神）：文件读取与反序列化都在 L2（`voxelith-prime`），
//! 读出来把描述结构交给 [`loader::load_all`]，产物作为 Resource 注入。

pub mod catalog;
pub mod descriptor;
pub mod descriptor_world;
pub mod loader;
pub mod vocabulary;

pub use catalog::{SkillCatalog, StatusCatalog};
pub use descriptor::{
    ActorRon, AiChoiceRon, ConditionRon, ContestRon, CostRon, EffectRon, ModifierRon, OutcomeRon,
    RequirementRon, ResourceRon, SkillRon, StackingRon, StatRon, StatusDefRon, StatusRon,
    TargetingRon, ValueRon, VocabRon, WhoRon,
};
pub use loader::{
    ActorTemplate, LoadedContent, LoaderError, PoolTemplate, ReusedDefs, WorldTemplate,
    build_vocab, build_vocab_into, load_all, load_all_reusing, resource_labels, stat_labels,
    status_labels,
};
pub use vocabulary::{NameTable, ResourceId, SkillId, StatId, StatusId, UnknownName, Vocab};
