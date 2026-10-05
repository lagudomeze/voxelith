//! 配置**热重载**：改了 `.ron`，不重编、不重启。
//!
//! ```text
//! 改 assets/data/skills.ron
//!   → AssetServer（file_watcher）重新装载 → AssetEvent::Modified
//!   → 重新翻译：同 ID 的 `Skill` / `StatusDef` **写在原实体上**（见 `ReusedDefs`）
//!   → 换掉 Vocab / SkillCatalog / StatusCatalog / ContentData / Labels
//! ```
//!
//! ## 为什么不重新生成角色、不重建地形
//!
//! 那两件事是**场景编排**，不是"内容"：重跑一遍会把 PC 和怪物再 spawn 一份、
//! 把地形网格再叠一层。热重载解决的痛点是**调数值**（伤害 / 权重 / 时长 / 门控标签），
//! 所以这条链路只到"定义 + 目录 + 角色模板"。想从头来一遍，重启即可。
//!
//! ## 失败保留旧内容
//!
//! 改错了 `.ron`（语法错 / 引用了没登记的池）时**不 panic**：打一条 error，留旧目录继续跑。
//! 启动期策略是"当场炸"，那时还没有可玩的局；运行期炸掉用户正在玩的局则毫无意义。

use bevy::prelude::*;
use voxelith_axiom::behaviors::content::{ReusedDefs, SkillCatalog, StatusCatalog, Vocab};

use super::loader::load_content_reusing;
use super::manifest::{ContentChanges, LoadedContentAssets};

/// 看一眼有没有配置被改动；改了就地重新翻译。
pub fn reload_content(
    mut commands: Commands,
    mut changes: ContentChanges,
    assets: LoadedContentAssets,
    vocab: Option<Res<Vocab>>,
    skills: Option<Res<SkillCatalog>>,
    statuses: Option<Res<StatusCatalog>>,
) {
    if !changes.take() {
        return;
    }

    let raw = match assets.raw() {
        Ok(raw) => raw,
        // 正在重新装载（`Assets<A>` 里暂时没有）——下一帧还会再来一条变更。
        Err(error) => {
            debug!("配置变更但暂时读不到，等下一帧：{error}");
            return;
        }
    };

    // **词汇表也要当底**：不拿旧表垫着，在 `skills.ron` 中间插一条会让它后面所有技能
    // 的 ID 整体后移一位——那等于把 A 的定义悄悄换成了 B 的（不报错）。
    let reuse = ReusedDefs {
        vocab: vocab.map(|vocab| vocab.clone()).unwrap_or_default(),
        skills: skills.map(|catalog| catalog.clone()).unwrap_or_default(),
        statuses: statuses.map(|catalog| catalog.clone()).unwrap_or_default(),
    };

    match load_content_reusing(&mut commands, &raw, Some(&reuse)) {
        Ok(content) => {
            // 旧目录里有、新目录里没有的 ID：已经没人引用，销毁掉。
            for stale in reuse.stale(&content) {
                commands.entity(stale).despawn();
            }
            info!(
                "配置已热重载：{} 个技能 / {} 个状态的定义实体原地更新，角色与地形未重建",
                content.skills.len(),
                content.statuses.len()
            );
        }
        Err(error) => {
            error!("配置改了但翻译不过去，**保留旧内容**：{error}");
        }
    }
}
