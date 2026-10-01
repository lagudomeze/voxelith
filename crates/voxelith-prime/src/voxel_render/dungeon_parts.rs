//! **件名映射**：把布局里的"件"翻译成具体的模型文件。
//!
//! 从 `dungeon_build` 拆出来是为了 **R26 / R93 的单文件 500 行上限** ——
//! 解析器 + 它那堆"对着真实清单核对名字"的测试加起来很长。
//!
//! 分层：布局（`dungeon_layout`）产出 `Placement`，
//! 本模块把 `DungeonPart` 映射成文件名，`dungeon_build` 负责 spawn。

use super::dungeon_layout::DungeonPart;

/// 件名映射的配置文件。
const DUNGEON_RON: &str = include_str!("../../../../assets/data/dungeon.ron");

/// 默认件名套件（与 `dungeon.ron` 的 `default_kit` 对齐）。
pub const DEFAULT_PART_KIT: &str = "dungeon";

/// 一个套件的"件名 → 模型文件"映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartNames {
    /// 套件短名。
    pub kit: String,
    /// 五类件的模型名（不含 `.glb`）。
    pub floor: String,
    pub wall: String,
    pub wall_corner: String,
    pub door: String,
    pub stairs: String,
}

impl PartNames {
    /// 取某一类件的模型名。
    ///
    /// 覆盖**全部**变体（含 `Stairs`）—— 配置里已经有 `stairs` 字段，
    /// 布局补上楼梯时**不用再改这里**。
    pub fn model_for(&self, part: DungeonPart) -> &str {
        match part {
            DungeonPart::Floor => &self.floor,
            DungeonPart::Wall => &self.wall,
            DungeonPart::WallCorner => &self.wall_corner,
            DungeonPart::Door => &self.door,
            DungeonPart::Stairs => &self.stairs,
        }
    }
}

/// 全部件名映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonParts {
    /// 默认套件。
    pub default_kit: String,
    /// 各套件。
    pub kits: Vec<PartNames>,
}

impl DungeonParts {
    /// 按短名找一套；找不到退回默认。
    pub fn kit(&self, key: &str) -> &PartNames {
        self.kits
            .iter()
            .find(|k| k.kit == key)
            .or_else(|| self.kits.iter().find(|k| k.kit == self.default_kit))
            .unwrap_or_else(|| &self.kits[0])
    }
}

/// 解析好的配置（**只解析一次**）。
///
/// ## 为什么缓存
///
/// 解析失败是**配置错误**，该在启动期喊出来 —— 但缓存之后
/// "喊出来"只发生一次，而不是每次调用都重新解析一遍。
pub fn loaded_parts() -> Result<&'static DungeonParts, String> {
    use std::sync::OnceLock;
    static CACHED: OnceLock<Result<DungeonParts, String>> = OnceLock::new();
    CACHED
        .get_or_init(|| parse_parts(DUNGEON_RON))
        .as_ref()
        .map_err(|err| err.clone())
}

/// 解析 `dungeon.ron`。
///
/// ## 为什么手写解析而不是派生 `Deserialize`
///
/// 格式是我们自己写的、**结构固定**（一个 `default_kit` + 若干 `kits`，
/// 每套五个字符串字段）。为它引 serde 派生、把 `content` 模块的类型体系
/// 扩一圈，代码量比这个小解析器大得多。
///
/// 但**校验一点没松**：[`tests`] 里对着**真实的模型清单**
/// （`assets/models/*/models.txt`）逐个核对每个名字都存在 ——
/// 因为"名字写错"**不会报任何错**，只是那一格空着。
pub fn parse_parts(text: &str) -> Result<DungeonParts, String> {
    let default_kit = field(text, "default_kit").ok_or("缺少 `default_kit`")?;

    // ---- 切出每个套件的块 ----
    //
    // ⚠️ **不要找 `(key:`**：RON 里 `(` 与 `key:` 之间**有换行和缩进**
    // （实际写的是 `(\n            key: "dungeon",`），
    // 所以那种模式一条都匹配不到 —— 而症状是"`kits` 是空的"，
    // 看起来像**配置写错了**，其实是解析器把格式写死了。
    // （这个 bug 真的发生过，被 `the_config_parses_with_every_kit` 抓到。）
    //
    // 正确做法是**按括号深度**切：深度 1 的一对 `( ... )` 就是一个块。
    let kits_at = text.find("kits:").ok_or("缺少 `kits`")?;
    let body = &text[kits_at..];
    let mut kits = Vec::new();
    let mut depth = 0i32;
    let mut block_start: Option<usize> = None;
    for (index, ch) in body.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                if depth == 1 {
                    block_start = Some(index);
                }
            }
            ')' => {
                if depth == 1
                    && let Some(start) = block_start.take()
                {
                    let block = &body[start..=index];
                    if let Some(kit) = parse_one_kit(block)? {
                        kits.push(kit);
                    }
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    if kits.is_empty() {
        return Err("`kits` 是空的".to_owned());
    }
    Ok(DungeonParts { default_kit, kits })
}

/// 从一段文本里取 `key: "value"` 的 `value`。
fn field(block: &str, key: &str) -> Option<String> {
    let at = block.find(&format!("{key}:"))?;
    let rest = &block[at + key.len() + 1..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].to_owned())
}

/// 解析一个 `( key: "…", floor: "…", … )` 块。
///
/// 返回 `Ok(None)` 表示这不是一个套件块（没有 `key` 字段）。
fn parse_one_kit(block: &str) -> Result<Option<PartNames>, String> {
    let Some(kit) = field(block, "key") else {
        return Ok(None);
    };
    let need = |name: &str| -> Result<String, String> {
        field(block, name).ok_or_else(|| format!("套件 `{kit}` 缺少 `{name}`"))
    };
    Ok(Some(PartNames {
        floor: need("floor")?,
        wall: need("wall")?,
        wall_corner: need("wall_corner")?,
        door: need("door")?,
        stairs: need("stairs")?,
        // `kit` 放最后：`need` 闭包借着它，先移动会报 E0505。
        kit,
    }))
}
