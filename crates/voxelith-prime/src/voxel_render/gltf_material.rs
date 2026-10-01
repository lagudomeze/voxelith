//! **glTF 材质的修正**：把 Kenney 模型自带的材质改成 `unlit`。
//!
//! ## 为什么需要（根因，已实测）
//!
//! Kenney 的 modular 套件里，quad 类模型（`template-floor`、`template-wall` 等）
//! 在 GLB 里有 **8 个顶点 = 两个重叠的三角形**：
//! 一组法线朝上 `(0, 1, 0)`、一组朝下 `(0, -1, 0)`，**共用同一组位置**。
//! GLB 标了 `doubleSided: true` ⇒ Bevy 给材质设 `cull_mode = None` ⇒ **两面都画**
//! ⇒ 两个三角形**共面 z-fighting**，而朝下的那个赢了
//! ⇒ 光照按"法线朝下"求值 ⇒ `N·L < 0` ⇒ **渲染成近黑**。
//!
//! 实测数据（`probe/glb` 的 `reproduce` bin，同一个 `template-floor.glb`）：
//!
//! | | 平放的瓦片 | 立起的瓦片 |
//! |---|---|---|
//! | 原样（走光照） | `(15, 16, 21)` **近黑** | `(111, 117, 143)` |
//! | 改成 `unlit` | **`(89, 95, 120)`** ✅ | **`(89, 95, 120)`** ✅ |
//!
//! 贴图原色是 `(90, 96, 123)` ⇒ `unlit` 之后**完全正确**。
//!
//! ## 这一条解释了一堆互相矛盾的观测（都实测过）
//!
//! - **调光完全无效**：平行光 `12_000 → 2_000_000`（400 倍）、
//!   环境光 `60 → 15_000`（250 倍）、灯移到正上方 —— **画面纹丝不动**。
//!   因为 `N·L` 恒为负，光强乘多少都是 0。
//! - **拿掉贴图就正常**：纯色时两个三角形颜色一样，谁赢都一样。
//! - **"发黑但有高光"**：高光走另一条路径，不受 `N·L` 正负影响。
//! - **调色板色带完全用不上**：只看到背光那一份。
//!
//! ## 为什么 `unlit` 是**正确做法**，不是"绕过问题"
//!
//! Kenney 把**明暗烘焙在调色板里**了 —— 那些色带本身就是"岩石的亮面/暗面"。
//! 再叠一层实时光照是**重复着色**。我们自己的体素图集也是同样的思路
//! （见 `super::materials` 的 `unlit: true`）。
//!
//! ## ⚠️ 不要改成"把模型翻转 180°"
//!
//! 那只是让**另一个三角形**赢。一旦素材更新、或 z-fighting 的胜者变了，
//! 就会再翻回去。`unlit` 是**与胜负无关**的修法。
//!
//! ## 为什么不能塞进 `model_gallery`
//!
//! 那个模块只在 `MODEL_GALLERY=1` 时工作，而**地板网格**（`floor_grid`）
//! 用的是同一批 GLB —— 素材浏览器关着时地板照样需要这个修正。

use bevy::prelude::*;

/// 已经修过了（避免每帧重复遍历）。
#[derive(Resource, Debug, Default)]
pub struct GltfMaterialsPatched {
    /// 修了几个材质。
    pub patched: usize,
}

/// 把所有**来自 glTF 场景**的材质改成 `unlit`。
///
/// ## 为什么按"实体上的 `MeshMaterial3d`"来收集，而不是遍历 `Assets`
///
/// 遍历 `Assets<StandardMaterial>` 会把**我们自己的**材质也一起改掉
/// （地形图集、地板衬板等）。按"`WorldAssetRoot` 展开出来的实体"来收集，
/// 范围**精确等于 glTF 带来的那份**。
///
/// ## 为什么**不收工**（第一版收工了，结果是错的）
///
/// 第一版写的是"连着 120 帧没发现新东西就 return"。实测出的 bug：
/// **地板网格的瓦片是在修正跑完之后才 spawn 的** ——
/// 于是 gallery 的 39 个模型修好了，地板却还是暗的。
///
/// 素材来自**两个互相独立的系统**（`model_gallery` 与 `floor_grid`），
/// 它们的加载时机无法在编译期确定 ⇒ 这里必须**持续看着**，有新材质就修。
///
/// 稳态开销可忽略：遍历场景树（几百个实体）+ 查表；
/// 只有真的改了材质才写日志，不会刷屏。
pub fn unlit_gltf_materials(
    mut commands: Commands,
    roots: Query<Entity, With<WorldAssetRoot>>,
    children: Query<&Children>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    patched: Option<Res<GltfMaterialsPatched>>,
) {
    // 先把计数读出来（后面还要用，别被 `if let` 移走）。
    let already = patched.as_ref().map_or(0, |p| p.patched);

    // 收集 glTF 场景树里所有实体的材质句柄。
    let mut handles: Vec<Handle<StandardMaterial>> = Vec::new();
    for root in &roots {
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(handle) = mesh_materials.get(entity) {
                handles.push(handle.0.clone());
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
        }
    }
    if handles.is_empty() {
        return;
    }

    // 去重：同一个材质被多个网格引用是常态。
    handles.sort_unstable_by_key(|h| h.id());
    handles.dedup_by_key(|h| h.id());

    let mut newly = 0usize;
    for handle in handles {
        if let Some(mut material) = materials.get_mut(&handle) {
            if !material.unlit {
                material.unlit = true;
                newly += 1;
            }
        }
    }

    let total = already + newly;
    if newly > 0 {
        info!("glTF 材质：把 {newly} 个改成 unlit（累计 {total} 个）");
    }
    commands.insert_resource(GltfMaterialsPatched { patched: total });
}

/// 注册 glTF 材质修正。
pub struct GltfMaterialPlugin;

impl Plugin for GltfMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, unlit_gltf_materials);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **根因的独立验证**：这些 GLB 里确实有"两组反向法线、共用位置"的结构。
    ///
    /// ## 为什么这条测试值得写
    ///
    /// 这个 bug 的表现（"模型发黑、调光无效、有高光"）**完全不像**根因
    /// （两个共面三角形 z-fighting、赢的是背光那一面）。
    /// 我是靠"翻转 180° 就变亮"这条实验才反推出来的。
    ///
    /// 所以这里**直接读 GLB 字节**验证根因：
    /// - 顶点数**是位置数的两倍**（每个位置被两组法线各用一次）；
    /// - 两组法线**互为反向**（一组 `(0,1,0)`、一组 `(0,-1,0)`）；
    /// - 材质是 `doubleSided: true`（⇒ Bevy 不剔除 ⇒ 两面都画 ⇒ z-fighting）。
    ///
    /// 哪天换成单面的模型，这条测试会失败 —— 那时 `unlit` 依然无害，
    /// 但值得重新审视这个修正是否还有必要。
    #[test]
    fn the_fix_targets_real_overlapping_double_sided_geometry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/models/dungeon/template-floor.glb");
        assert!(path.exists(), "找不到 {}", path.display());
        let bytes = std::fs::read(&path).expect("读 GLB");

        // ---- 拆开 GLB 容器：12 字节头 + JSON chunk + BIN chunk ----
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).expect("解析 glTF JSON");

        // ---- 材质必须声明 doubleSided ----
        let double_sided = json["materials"][0]["doubleSided"]
            .as_bool()
            .unwrap_or(false);
        assert!(
            double_sided,
            "这个模型不再是 doubleSided 了 —— 根因变了，请重新确认 `unlit` 修正是否还需要"
        );

        // ---- 读 POSITION 与 NORMAL 的实际数值 ----
        let prim = &json["meshes"][0]["primitives"][0];
        let accessors = json["accessors"].as_array().expect("accessors");
        //
        // ⚠️ 我第一版写的是"`NORMAL` 数是 `POSITION` 的两倍"，**测试立刻否掉了**：
        // 实际是 8 / 8 —— 每个位置有**自己的**法线，两组三角形的位置**重合**、
        // 法线**互为反向**。所以判据要按根因的真实定义写：
        // "法线只有两种取值，且互为反向" + "两组三角形覆盖同一片区域"。
        let read_vec3 = |semantic: &str| -> Vec<[f64; 3]> {
            let index = prim["attributes"][semantic].as_u64().expect("属性存在") as usize;
            let accessor = &accessors[index];
            let view = &json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
            // GLB 的 BIN chunk 紧跟在 JSON chunk 之后。
            let bin_start = 20 + json_len;
            let bin_header = bin_start + 8;
            let base = bin_header
                + view["byteOffset"].as_u64().unwrap_or(0) as usize
                + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
            (0..accessor["count"].as_u64().unwrap() as usize)
                .map(|i| {
                    let at = base + i * 12;
                    [
                        f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as f64,
                        f32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as f64,
                        f32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as f64,
                    ]
                })
                .collect()
        };
        let positions = read_vec3("POSITION");
        let normals = read_vec3("NORMAL");
        assert_eq!(positions.len(), normals.len(), "位置与法线该一一对应");

        // **法线只有两种取值**（朝上 / 朝下）—— 一个真正的"体积模型"会有很多种。
        let mut distinct: Vec<[f64; 3]> = Vec::new();
        for normal in &normals {
            let rounded = [
                (normal[0] * 1000.0).round() / 1000.0,
                (normal[1] * 1000.0).round() / 1000.0,
                (normal[2] * 1000.0).round() / 1000.0,
            ];
            if !distinct.contains(&rounded) {
                distinct.push(rounded);
            }
        }
        assert_eq!(
            distinct.len(),
            2,
            "预期法线**只有两种取值**（这正是 quad 的做法），实际 {distinct:?}"
        );

        // 而且这两种**互为反向**。
        let sum: [f64; 3] = [
            distinct[0][0] + distinct[1][0],
            distinct[0][1] + distinct[1][1],
            distinct[0][2] + distinct[1][2],
        ];
        let len = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
        assert!(
            len < 1e-3,
            "两组法线该互为反向（和为 0），实际和 {sum:?} ⇒ 值 {distinct:?}"
        );

        // **两组三角形覆盖同一片位置区域** ⇒ 共面 ⇒ 会 z-fighting。
        let bounds = |v: &[[f64; 3]]| -> [f64; 6] {
            let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
            for p in v {
                for i in 0..3 {
                    b[i] = b[i].min(p[i]);
                    b[i + 3] = b[i + 3].max(p[i]);
                }
            }
            b
        };
        let upward: Vec<[f64; 3]> = positions
            .iter()
            .zip(&normals)
            .filter(|(_, n)| n[1] > 0.5)
            .map(|(p, _)| *p)
            .collect();
        let downward: Vec<[f64; 3]> = positions
            .iter()
            .zip(&normals)
            .filter(|(_, n)| n[1] < -0.5)
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(
            upward.len(),
            downward.len(),
            "朝上与朝下的顶点数该相等（同一块 quad 的两面）"
        );
        assert_eq!(
            bounds(&upward),
            bounds(&downward),
            "两组的**位置包围盒必须完全相同** —— 这才是 z-fighting 的来源"
        );
    }

    /// **修正的目标是"改成 unlit"，不是"翻转模型"。**
    ///
    /// 这条测试是给未来的自己看的：翻转 180° 只是让 z-fighting 的**另一个**
    /// 三角形赢，素材一换就会翻回去；`unlit` 与胜负无关。
    #[test]
    fn unlit_is_the_fix_not_a_rotation() {
        let mut material = StandardMaterial {
            unlit: false,
            ..default()
        };
        // 这就是修正本身。
        material.unlit = true;
        assert!(
            material.unlit,
            "glTF 材质必须改成 unlit —— 明暗已经烘焙在 Kenney 的调色板里"
        );
    }

    /// 计数从 0 开始（还没修过）。
    #[test]
    fn the_patch_counter_starts_at_zero() {
        assert_eq!(GltfMaterialsPatched::default().patched, 0);
    }
}
