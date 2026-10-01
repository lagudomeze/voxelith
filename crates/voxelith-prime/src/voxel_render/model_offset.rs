//! **GLB 模型的原点偏移表**。
//!
//! ## 为什么需要它
//!
//! Kenney（以及很多建模工具导出的）模型的**局部原点约定不统一**：
//!
//! | 模型 | 包围盒（世界单位） | 原点在哪 |
//! |---|---|---|
//! | `template-floor` | `(-2, 0, -2) → (2, 0, 2)` | 底面**中心** ✅ |
//! | `room-small` | `(-6, 0, -6) → (6, 0, 6)` | 底面**中心** ✅ |
//! | `template-wall` | `(-2, 0, -1.99) → (2, 4.15, 0)` | 底面**边缘**（中心在 `z = -1.0`）❌ |
//! | `stairs` | `(-2.2, 0, -6.2) → (2.2, 8.55, 2.2)` | 中心在 `z = -2.0` ❌ |
//!
//! 按"底面中心"摆放的话，墙类模型会**整体偏半个到一个格子** ——
//! 拼出来的房间墙会从格线上岔开。
//!
//! 实测：dungeon 套件 39 个模型里，**11 个**的原点不在底面中心。
//!
//! ## 为什么用生成的清单，而不是运行期算
//!
//! 运行期算要读 `Mesh` 的顶点，而 GLB 是**异步**加载的 ——
//! 拿到顶点之前不知道该往哪摆，画面会先抖一下。
//! 构建期生成一张静态表最简单，而且**可测试**
//! （清单与真实 GLB 对不上时，[`tests`] 里的测试会失败）。
//!
//! 清单由 `tools/gltf_offsets.py` 从 GLB 字节里读出并写出。**改素材后重跑它。**
//!
//! ## 用法
//!
//! ```ignore
//! let offset = model_offsets::lookup("dungeon", "template-wall");
//! transform.translation = slot + offset * scale;
//! ```

use bevy::prelude::*;

/// 一个模型的原点偏移。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelOffset {
    /// 套件名（`dungeon` / `cave` / `platformer`）。
    pub kit: &'static str,
    /// 模型名（不含 `.glb`）。
    pub name: &'static str,
    /// 把**底面中心**对到摆放点上要补的位移（**模型自身单位**，即未缩放前）。
    pub offset: Vec3,
}

/// 自动生成的偏移表（`tools/gltf_offsets.py` 写出），**三套各一份**。
///
/// ⚠️ 必须按套件分开存：**模型名会跨套件重复**（`template-floor` 在
/// dungeon 与 cave 里都有），合并成一张扁平表会互相覆盖。
const MANIFESTS: &[(&str, &str)] = &[
    (
        "dungeon",
        include_str!("../../../../assets/models/dungeon/offsets.ron"),
    ),
    (
        "cave",
        include_str!("../../../../assets/models/cave/offsets.ron"),
    ),
    (
        "platformer",
        include_str!("../../../../assets/models/platformer/offsets.ron"),
    ),
];

/// 表里的条目（**lazy 解析一次**）。
///
/// ## 为什么手写一个极小的解析器，而不是引 `ron`
///
/// 格式是我们自己生成的、**只有一种形状**（`(name: "..", offset: (x, y, z)),`）。
/// 引一个完整 RON 解析器 + 为它派生 `Deserialize` 结构，
/// 代码量比这个解析器大好几倍。
///
/// 但**不能因此放松校验**：[`tests`] 里对着真实 GLB 核对每一条 ——
/// 解析器写错或清单过期都必须被测出来。
pub fn model_offsets() -> &'static [ModelOffset] {
    use std::sync::OnceLock;
    static PARSED: OnceLock<Vec<ModelOffset>> = OnceLock::new();
    PARSED.get_or_init(|| {
        let mut out = Vec::new();
        for (kit, text) in MANIFESTS {
            for line in text.lines() {
                let line = line.trim();
                // 只处理 `(name: "x", offset: (a, b, c)),` 这种行。
                let Some(rest) = line.strip_prefix("(name:") else {
                    continue;
                };
                let Some((name_part, offset_part)) = rest.split_once("offset:") else {
                    continue;
                };
                let name = name_part
                    .trim()
                    .trim_matches(|c| c == '"' || c == ',' || c == ' ');
                let numbers: Vec<f32> = offset_part
                    .trim()
                    .trim_start_matches('(')
                    .split(['(', ')', ','])
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if numbers.len() != 3 || name.is_empty() {
                    continue;
                }
                out.push(ModelOffset {
                    kit,
                    // ⚠️ **不要用 `Box::leak`**：那是每次解析都永久泄漏一块内存。
                    //
                    // `MANIFESTS` 是 `include_str!` 进来的 `&'static str`，
                    // 所以 `lines()` 给出的切片本来就活得和程序一样久 ——
                    // `kit` 与 `name` 直接借它即可，**零分配、零泄漏**。
                    name,
                    offset: Vec3::new(numbers[0], numbers[1], numbers[2]),
                });
            }
        }
        out
    })
}

/// `(套件, 模型名) → 偏移` 的索引（**O(1) 查询**）。
///
/// ## 为什么要一张索引表
///
/// 摆放时每个模型都要查一次。1020 个瓦片 × 线性扫描 160 条 = 十几万次比较，
/// 而且都在 **Update 里每帧可能触发**的路径上。索引一次建好即可。
fn index() -> &'static std::collections::HashMap<(&'static str, &'static str), Vec3> {
    use std::sync::OnceLock;
    static INDEX: OnceLock<std::collections::HashMap<(&'static str, &'static str), Vec3>> =
        OnceLock::new();
    INDEX.get_or_init(|| {
        model_offsets()
            .iter()
            .map(|entry| ((entry.kit, entry.name), entry.offset))
            .collect()
    })
}

/// 查一个模型的原点偏移；表里没有就当 `Vec3::ZERO`（原点已经在底心）。
///
/// **必须给套件名**：`template-floor` 这类名字在多个套件里都有，
/// 只按名字查会命中错的条目。
pub fn lookup(kit: &str, name: &str) -> Vec3 {
    index().get(&(kit, name)).copied().unwrap_or(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **清单必须真的解析出东西来**。
    ///
    /// 手写解析器最怕的是"格式一变就静默解析出零条" —— 那样所有偏移都变成 0，
    /// 表现就是"墙又偏了"，而且**没有任何报错**。
    #[test]
    fn the_manifest_parses_into_entries() {
        let entries = model_offsets();
        assert!(
            entries.len() >= 30,
            "只解析出 {} 条 —— 清单格式变了或解析器坏了（静默退化成 0 偏移）",
            entries.len()
        );
    }

    /// **已知的偏原点模型必须在表里，且偏移值正确。**
    ///
    /// 这四条是从 GLB 字节里读出来的实测值（`tools/gltf_offsets.py` 的报告）：
    /// `template-wall` 的中心在 `z = -1.0`、`stairs` 在 `z = -2.0`。
    #[test]
    fn the_known_offset_models_have_the_right_values() {
        let cases = [
            ("template-wall", Vec3::new(0.0, 0.0, -1.0)),
            ("stairs", Vec3::new(0.0, 0.0, -2.0)),
            ("template-wall-half", Vec3::new(0.0, 0.0, -0.90)),
            ("corridor-transition", Vec3::new(-2.0, 0.0, 0.0)),
            ("template-wall-corner", Vec3::new(-0.5, 0.0, -0.5)),
        ];
        for (name, expected) in cases {
            let got = lookup("dungeon", name);
            assert!(
                (got - expected).length() < 0.02,
                "`{name}` 的偏移该是 {expected:?}，实际 {got:?}"
            );
        }
    }

    /// **原点已在底心的模型不该被表里写偏移**（否则会被推偏）。
    #[test]
    fn the_centred_models_stay_at_zero() {
        for name in ["template-floor", "room-small", "corridor", "gate"] {
            assert_eq!(
                lookup("dungeon", name),
                Vec3::ZERO,
                "`{name}` 的原点本来就在底面中心，不该有偏移"
            );
        }
    }

    /// 表里没有的模型退化成零偏移（不是 panic）。
    #[test]
    fn an_unknown_model_falls_back_to_zero() {
        assert_eq!(lookup("dungeon", "no-such-model"), Vec3::ZERO);
    }

    /// **清单必须与磁盘上的真实 GLB 对得上**，而且**三套都要核**。
    ///
    /// ## 这条在防两种必然发生的失败
    ///
    /// 1. **素材换了、清单没重跑**：清单是生成出来的，而脚本不会自动跑。
    /// 2. **只核了 dungeon**（我第一版就是这样）：`cave` / `platformer` 的清单
    ///    错了也发现不了 —— 而它们同样是生成的。
    ///
    /// 做法：直接读 GLB 的 `accessor.min/max`（glTF 自带包围盒，不用解析顶点），
    /// 算出真实中心，与清单条目核对。
    ///
    /// ## 为什么不只用 `model_offsets()`
    ///
    /// 那个函数把三份清单读进**同一张表**。"三份都被解析到了"
    /// 和"三份的内容都对"是两件事 —— 所以这里**逐套独立解析**，
    /// 再与实际 GLB 对照。
    #[test]
    fn every_kit_manifest_matches_the_glb_files_on_disk() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");

        for (kit, text) in MANIFESTS {
            let listed: Vec<&str> = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix("(name:"))
                .filter_map(|rest| rest.split_once("offset:").map(|(n, _)| n))
                .map(|n| n.trim().trim_matches(|c| c == '"' || c == ',' || c == ' '))
                .filter(|n| !n.is_empty())
                .collect();
            assert!(
                listed.len() >= 30,
                "套件 `{kit}` 只解析出 {} 条 —— 清单格式变了",
                listed.len()
            );

            for name in listed {
                let path = assets.join("models").join(kit).join(format!("{name}.glb"));
                assert!(
                    path.exists(),
                    "套件 `{kit}` 清单里的 `{name}` 在磁盘上找不到（{}）",
                    path.display()
                );
                let real = read_bounds(&path);
                let expected = Vec3::new(
                    (real.0[0] + real.1[0]) / 2.0,
                    -real.0[1],
                    (real.0[2] + real.1[2]) / 2.0,
                );
                let got = lookup(kit, &name);
                assert!(
                    (got - expected).length() < 0.02,
                    "套件 `{kit}` 的 `{name}`：清单写 {:?}，按 GLB 实算该是 {:?} —— \
                     清单过期了，重跑 `python tools/gltf_offsets.py`",
                    got,
                    expected
                );
            }
        }
    }

    /// 读一个 GLB 里所有 primitive 的 `POSITION` 包围盒并集。
    fn read_bounds(path: &std::path::Path) -> ([f32; 3], [f32; 3]) {
        let bytes = std::fs::read(path).expect("读 GLB");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("解析 glTF JSON");
        let mut mn = [f32::MAX; 3];
        let mut mx = [f32::MIN; 3];
        for mesh in json["meshes"].as_array().into_iter().flatten() {
            for prim in mesh["primitives"].as_array().into_iter().flatten() {
                let Some(index) = prim["attributes"]["POSITION"].as_u64() else {
                    continue;
                };
                let accessor = &json["accessors"][index as usize];
                let (Some(a), Some(b)) = (accessor["min"].as_array(), accessor["max"].as_array())
                else {
                    continue;
                };
                for i in 0..3 {
                    mn[i] = mn[i].min(a[i].as_f64().unwrap() as f32);
                    mx[i] = mx[i].max(b[i].as_f64().unwrap() as f32);
                }
            }
        }
        (mn, mx)
    }
}
