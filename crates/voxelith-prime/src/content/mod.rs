//! L2 内容层：**读文件 + 反序列化 + 注入**。
//!
//! 分工（**R101 精神**）：
//!
//! | 谁 | 干什么 |
//! |---|---|
//! | 本模块（L2） | `include_str!` 读 `.ron`、`ron::from_str` 反序列化、把产物 `insert_resource` |
//! | `axiom::behaviors::content`（L1） | 描述结构（`SkillRon` 等）与 **字符串 → 词汇 ID** 的解析 |
//!
//! 这样 `axiom` 仍然在"无文件系统"的环境里可测：它只认描述结构，不认文件。

mod loader;
mod spawn;

pub use loader::{
    ContentData, ContentError, Labels, RawContent, load_content, make_pools, make_stats, parse_raw,
};
pub use spawn::spawn_demo;

use bevy::prelude::*;
use voxelith_axiom::behaviors::content::LoadedContent;

/// 内容装配：把原始描述翻译成运行时定义、注入 Resource，并生成角色。
///
/// **原始描述（[`RawContent`]）在 `main` 里就解析好了**：翻译后的产物要经 `Commands`
/// 注入（延迟到帧末），而图集 / 地形这些 `Startup` 系统需要立刻读到方块定义。
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
        // 两个阶段**必须串行**：`Spawn` 读 `Translate` 注入的 `ContentData`，
        // 而 `Commands` 要到系统之间才落地。体素表现再把图集 / 地形排在 `Translate` 之后。
        app.configure_sets(Startup, (ContentSet::Translate, ContentSet::Spawn).chain())
            .add_systems(
                Startup,
                setup_content_resources.in_set(ContentSet::Translate),
            )
            .add_systems(Startup, spawn_content_actors.in_set(ContentSet::Spawn));
    }
}

/// 翻译内容 → 注入 `Resource`（不生成角色）。
///
/// 在 [`ContentSet::Translate`] 里跑；体素表现的图集 / 地形排在它之后，
/// 因为它们要读翻译后的方块表与地形参数。
pub fn setup_content_resources(mut commands: Commands, raw: Res<RawContent>) {
    let content: LoadedContent =
        load_content(&mut commands, &raw).unwrap_or_else(|error| panic!("内容翻译失败：{error}"));

    // 目录先落 Resource，生成实体时才查得到技能 / 状态的**定义实体**。
    commands.insert_resource(content.vocab.clone());
    commands.insert_resource(content.skills.clone());
    commands.insert_resource(content.statuses.clone());
    commands.insert_resource(ContentData {
        pools: content.pools.clone(),
        players: content.players.clone(),
        monsters: content.monsters.clone(),
        world: content.world.clone(),
    });
}

/// 生成 PC 与怪物（读已经注入的 `ContentData`）。
///
/// 公开是为了让体素表现的那条 `Startup` 链把它排在资源注入之后——
/// 否则它会与注入并列执行，读到还不存在的 `ContentData`。
pub fn spawn_content_actors(mut commands: Commands, data: Res<ContentData>) {
    spawn::spawn_demo(&mut commands, &data);
}
