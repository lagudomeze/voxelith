//! **存档**：把玩家的体素改动落盘 / 读回。
//!
//! ## 存档里放什么（**这是本模块最重要的决定**）
//!
//! **只存玩家的改动，不存地形。** 理由有两条，都是硬的：
//!
//! 1. **地形是推导出来的**。`VoxelStore` 里的区块是"生成器输出 + 玩家改动"的混合体。
//!    把整个 store 序列化下来，等于把**推导结果当成了事实来源** ——
//!    玩家的存档会在他改一条地形参数之后**直接失真**（旧存档里是被埋住的土，
//!    而新的生成器认为那里该是空气）。
//! 2. **体积**。一个区块是 32³ = 32768 个体素。49 个区块就是 **160 万**条记录。
//!    而玩家真正改过的可能只有几十个。差 5 个数量级。
//!
//! 生成器是**确定性**的（同参数 + 同坐标 = 同结果，见 `world/generate.rs`），
//! 所以"重新生成 + 重放改动"与"直接存地形"在**同参数下**等价，
//! 而在**参数变化后**前者才是对的。
//!
//! ## 为什么按世界坐标存，不按区块内局部坐标
//!
//! 局部坐标需要同时存"区块坐标 + 区块内偏移"，是两条记录；
//! 世界坐标一条就够。而且**区块大小是可以改的**（`CHUNK_SIZE`），
//! 按世界坐标存的话存档**不随区块大小变化而失效**。
//!
//! ## 格式
//!
//! RON（和内容文件一致），人可读、可 diff、可手改：
//!
//! ```ron
//! (
//!     version: 1,
//!     edits: [
//!         ((pos: (1, 2, 3), id: 2)),
//!     ],
//! )
//! ```
//!
//! `version` 是**必须**的：没有它，将来改格式时无法区分"旧存档"与"坏存档"，
//! 只能报一个含义模糊的解析错误。

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxelith_axiom::world::{TerrainParams, Voxel, VoxelId, VoxelStore};

/// 当前存档格式版本。
///
/// 改格式时**必须**加一，并在 [`SaveData::load`] 里为旧版本写迁移。
/// 不加的话旧存档会被当成"坏存档"，报出的错误与真实原因无关。
pub const SAVE_VERSION: u32 = 1;

/// 一条体素改动：把某个世界坐标上的方块设成某个 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoxelEdit {
    /// 世界坐标（整数格）。
    ///
    /// 用世界坐标而不是"区块 + 局部偏移"：后者要两条记录，
    /// 而且会在 `CHUNK_SIZE` 变化后失效。
    pub pos: (i32, i32, i32),
    /// 方块 ID（`0` = 空气）。
    ///
    /// 存 **ID 而不是名字**：名字表（`VoxelNames`）是稳定契约，
    /// 但存 ID 更紧凑，而且内容里方块顺序是"只追加"的约定
    /// （见 `atlas.rs` 里"三个表的编号必须一致"那段）。
    pub id: VoxelId,
}

/// 存档的全部内容。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    /// 格式版本。见 [`SAVE_VERSION`]。
    #[serde(default)]
    pub version: u32,
    /// 玩家改过的体素。
    #[serde(default)]
    pub edits: Vec<VoxelEdit>,
}

/// 存档读写的错误。
#[derive(Debug)]
pub enum SaveError {
    /// 磁盘读写失败。
    Io(std::io::Error),
    /// 解析失败（文件坏了，或格式版本不认识）。
    Parse(ron::error::SpannedError),
    /// 版本比当前代码新 —— **不能**当坏文件处理，那会丢数据。
    VersionTooNew {
        /// 文件里的版本。
        found: u32,
        /// 当前代码支持的版本。
        supported: u32,
    },
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "存档读写失败：{error}"),
            Self::Parse(error) => write!(f, "存档解析失败：{error}"),
            Self::VersionTooNew { found, supported } => write!(
                f,
                "存档版本 {found} 比当前程序支持的 {supported} 新：\
                 升级游戏再读，不要用旧版本覆盖它（会丢数据）"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveData {
    /// 从体素改动列表构造。
    pub fn from_edits(edits: Vec<VoxelEdit>) -> Self {
        Self {
            version: SAVE_VERSION,
            edits,
        }
    }

    /// 写进文件。
    ///
    /// **先写临时文件再改名**：直接覆盖的话，写到一半崩了会留下一个
    /// 半截存档 —— 而旧存档已经被毁了。改名在同一个文件系统上通常是原子的。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SaveError> {
        let path = path.as_ref();
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| SaveError::Io(std::io::Error::other(error.to_string())))?;

        let temp = path.with_extension("ron.tmp");
        std::fs::write(&temp, text).map_err(SaveError::Io)?;
        std::fs::rename(&temp, path).map_err(SaveError::Io)
    }

    /// 从文件读回。
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SaveError> {
        let text = std::fs::read_to_string(path).map_err(SaveError::Io)?;
        let data: Self = ron::from_str(&text).map_err(SaveError::Parse)?;
        if data.version > SAVE_VERSION {
            return Err(SaveError::VersionTooNew {
                found: data.version,
                supported: SAVE_VERSION,
            });
        }
        Ok(data)
    }

    /// 把改动重放到体素表上。
    ///
    /// **必须在区块加载之后调用**：`VoxelStore::set` 会顺带把目标区块加载起来
    /// （见它的文档），所以顺序其实不敏感 —— 但先加载能少几次分配。
    pub fn apply(&self, store: &mut VoxelStore, terrain: &TerrainParams) {
        for edit in &self.edits {
            let pos = [edit.pos.0, edit.pos.1, edit.pos.2];
            store.set(pos, Voxel { id: edit.id }, terrain);
        }
    }
}

/// 注册存档（目前不注册系统，只提供 API 与资源）。
pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, _app: &mut App) {
        // 有意为空：存档的**触发时机**（退出时存 / 手动存）还没有需求，
        // 现在只有"格式 + 读写 + 重放"这三件事，由调用方按需调。
        // 等接了体素编辑再加"退出时自动存"的系统 —— 那时它才有消费者。
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("voxelith-save-test-{name}.ron"));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// **往返一致**：存下去再读回来，改动一条不少、值一个不差。
    ///
    /// 这是存档最核心的性质。没有它，其余全是空谈。
    #[test]
    fn edits_survive_a_round_trip() {
        let path = scratch("roundtrip");
        let edits = vec![
            VoxelEdit {
                pos: (3, -2, 17),
                id: VoxelId(2),
            },
            VoxelEdit {
                pos: (-8, 0, 0),
                id: VoxelId(0),
            },
        ];
        SaveData::from_edits(edits.clone())
            .save(&path)
            .expect("写存档该成功");

        let back = SaveData::load(&path).expect("读存档该成功");
        assert_eq!(back.edits, edits, "改动该原样回来");
        assert_eq!(back.version, SAVE_VERSION);

        let _ = std::fs::remove_file(&path);
    }

    /// 一份**真的有方块**的地形参数。
    ///
    /// 不能用 `TerrainParams::default()`：它的 `surface` / `soil` / `deep`
    /// **全是 `VoxelId(0)`（空气）** —— 那是 `WorldPlugin` 装配前的占位值，
    /// 生成出来是一个空世界（踩过：写测试时假设"地下一定是实心"，结果一个实心点都没有）。
    fn terrain_with_blocks() -> TerrainParams {
        TerrainParams {
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            ..TerrainParams::default()
        }
    }

    /// **改动真的落到体素表上**（不只是数据搬了个家）。
    #[test]
    fn applying_edits_changes_what_the_store_reports() {
        let terrain = terrain_with_blocks();
        let mut store = VoxelStore::default();

        // **探测**出一个实心点与一个空气点，而不是假设高度。
        let x = 4;
        let z = 4;
        let solid_y = (-40..40)
            .find(|y| store.is_solid([x, *y, z], &terrain))
            .expect("总该有个实心的点");
        let air_y = (solid_y..solid_y + 60)
            .find(|y| !store.is_solid([x, *y, z], &terrain))
            .expect("实心之上总该有空气");

        let solid_pos = [x, solid_y, z];
        let air_pos = [x, air_y, z];

        let data = SaveData::from_edits(vec![
            VoxelEdit {
                pos: (x, air_y, z),
                id: VoxelId(1),
            },
            VoxelEdit {
                pos: (x, solid_y, z),
                id: VoxelId(0),
            },
        ]);
        data.apply(&mut store, &terrain);

        assert!(
            store.is_solid(air_pos, &terrain),
            "改动该把空气变实心（y = {air_y}）"
        );
        assert!(
            !store.is_solid(solid_pos, &terrain),
            "改动该把实心变空气（y = {solid_y}）"
        );
    }

    /// **版本比程序新时必须报错，不能当坏文件**。
    ///
    /// 若把它当解析错误，用户的反应会是"存档坏了，我重开一个" ——
    /// 然后旧存档被覆盖，**数据真的丢了**。
    /// 报一个能读懂的错误，用户就知道该升级游戏而不是开新档。
    #[test]
    fn a_newer_version_is_refused_with_a_clear_reason() {
        let path = scratch("newer");
        // 手写一份"未来版本"的存档。
        std::fs::write(&path, format!("(version: {}, edits: [])", SAVE_VERSION + 1))
            .expect("写文件该成功");

        let error = SaveData::load(&path).expect_err("未来版本该被拒绝");
        assert!(
            matches!(error, SaveError::VersionTooNew { .. }),
            "该报 VersionTooNew，实际：{error}"
        );
        // 错误文字要能指导行动（"升级游戏"），而不是只说"解析失败"。
        let text = error.to_string();
        assert!(text.contains("升级游戏"), "错误信息该给出行动指引：{text}");

        let _ = std::fs::remove_file(&path);
    }

    /// **存档不含地形**：改动数就是文件里的条目数，与"加载了多少区块"无关。
    ///
    /// 这条钉住"不把推导结果当事实来源"这个决定。若哪天有人改成
    /// "把整个 store 序列化"，改动数会变成几十万，这条立刻亮。
    #[test]
    fn the_save_contains_only_edits_not_the_terrain() {
        let terrain = terrain_with_blocks();
        let mut store = VoxelStore::default();
        // 加载一大片地形 —— 但**一个方块都不改**。
        voxelith_axiom::world::load_around(
            &mut store,
            &terrain,
            voxelith_axiom::world::ChunkPos::new(0, 0, 0),
            2,
        );
        assert!(store.loaded_count() > 0, "前提：确实加载了区块");

        let data = SaveData::from_edits(Vec::new());
        let path = scratch("only-edits");
        data.save(&path).expect("写存档该成功");
        let text = std::fs::read_to_string(&path).expect("读文件该成功");

        // 没有改动 ⇒ 文件里不该有任何 `pos:` 条目。
        assert!(
            !text.contains("pos:"),
            "没改过任何方块时存档里不该有改动条目，实际内容：\n{text}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
