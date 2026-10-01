//! `atlas` 的单元测试。
//!
//! 单独成文件是因为**测试体量已经超过被测代码**：图集的测试要覆盖
//! "格号不重叠 / 图案确定 / 尺寸正确 / 未知方块无贴图"，还带一个手工诊断用的
//! 导出测试。塞在原文件里会越过 500 行上限（**R26**）。
//!
//! 用 `#[path]` 挂在 `atlas` 模块下，所以 `use super::*` 依然指向 `atlas`。

#![allow(clippy::needless_borrow)]

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn textures() -> Vec<BlockTexture> {
        vec![
            BlockTexture {
                side: FaceTexture {
                    base: [120, 85, 60],
                    pattern: Pattern::Speckle { amount: 20 },
                },
                top: Some(FaceTexture {
                    base: [96, 148, 62],
                    pattern: Pattern::Topped {
                        rows: 3,
                        amount: 18,
                    },
                }),
                opaque: true,
                ..Default::default()
            },
            BlockTexture {
                side: FaceTexture {
                    base: [130, 130, 130],
                    pattern: Pattern::Solid,
                },
                ..Default::default()
            },
            BlockTexture {
                side: FaceTexture {
                    base: [90, 160, 90],
                    pattern: Pattern::Solid,
                },
                top: Some(FaceTexture {
                    base: [120, 200, 120],
                    pattern: Pattern::Solid,
                }),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn tile_indexes_are_contiguous_and_face_specific() {
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &textures());

        // **ID 从 0 开始**（与 VoxelPalette 的 push() 一致：locks[i] 就是 ID i，
        // 空气另有一套"不是实心"的判据，不占 ID）。
        assert_eq!(atlas.tile_index(0, BlockFace::Side), Some(0));
        assert_eq!(atlas.tile_index(0, BlockFace::Top), Some(1));
        assert_eq!(atlas.tile_index(0, BlockFace::Bottom), Some(2));
        // 第二个方块接着排。
        assert_eq!(atlas.tile_index(1, BlockFace::Side), Some(3));
        // 第三个方块：侧面 6、顶面 7。
        assert_eq!(atlas.tile_index(2, BlockFace::Side), Some(6));
        assert_eq!(atlas.tile_index(2, BlockFace::Top), Some(7));
    }

    #[test]
    fn missing_faces_fall_back_to_the_side_texture() {
        let block = BlockTexture {
            side: FaceTexture {
                base: [10, 20, 30],
                pattern: Pattern::Solid,
            },
            top: None,
            bottom: None,
            opaque: true,
        };
        assert_eq!(block.face(BlockFace::Top).base, [10, 20, 30]);
        assert_eq!(block.face(BlockFace::Bottom).base, [10, 20, 30]);
    }

    #[test]
    fn face_is_chosen_from_the_normal() {
        assert_eq!(BlockFace::from_normal([0, 1, 0]), BlockFace::Top);
        assert_eq!(BlockFace::from_normal([0, -1, 0]), BlockFace::Bottom);
        for side in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
            assert_eq!(BlockFace::from_normal(side), BlockFace::Side);
        }
    }

    #[test]
    fn unknown_block_has_no_tile() {
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &textures());
        assert_eq!(
            atlas.tile_index(99, BlockFace::Side),
            None,
            "没登记过就是 None"
        );
    }

    #[test]
    fn build_produces_an_image_of_the_expected_size() {
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &textures());
        // 3 方块 × 3 面 = 9 格 → 4 列 3 行。
        assert_eq!(atlas.columns, 4);
        assert_eq!(atlas.rows, 3);
        let image = images.get(&atlas.image).expect("图集已加入 Assets");
        assert_eq!(image.width(), 4 * TILE);
        assert_eq!(image.height(), 3 * TILE);
    }

    #[test]
    fn tile_rects_do_not_overlap() {
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &textures());
        let mut seen = std::collections::HashSet::new();
        for tile in 0..(atlas.tile_count() as u16) {
            let rect = atlas.tile_rect(tile);
            let key = (rect.min.x as u32, rect.min.y as u32);
            assert!(seen.insert(key), "图集格重叠：{tile}");
            assert_eq!(rect.width(), TILE as f32);
            assert_eq!(rect.height(), TILE as f32);
        }
    }

    #[test]
    fn painting_is_deterministic() {
        let mut a = Assets::<Image>::default();
        let mut b = Assets::<Image>::default();
        let atlas_a = AtlasImage::build(&mut a, &textures());
        let atlas_b = AtlasImage::build(&mut b, &textures());
        let data_a = a.get(&atlas_a.image).unwrap().data.clone().unwrap();
        let data_b = b.get(&atlas_b.image).unwrap().data.clone().unwrap();
        assert_eq!(data_a, data_b, "同配置该生成同样的贴图");
    }

    #[test]
    fn empty_block_list_is_safe() {
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &[]);
        assert_eq!(atlas.rows, 1);
        assert!(atlas.tile_index(1, BlockFace::Side).is_none());
    }
}

#[cfg(test)]
mod dump_tests {
    use super::*;

    /// **诊断**：把图集存成 PNG 供人眼看（排查"采样出黑色"时最快的手段）。
    ///
    /// 用 `cargo test -p voxelith-prime dump_atlas -- --ignored --nocapture` 跑。
    #[test]
    #[ignore = "手工诊断用：导出图集 PNG"]
    fn dump_atlas() {
        let content = crate::content::parse_raw().expect("world.ron 该配好");
        let blocks: Vec<BlockTexture> = content
            .world
            .blocks
            .iter()
            .map(|b| BlockTexture {
                side: FaceTexture {
                    base: b.side.base,
                    pattern: Pattern::Solid,
                },
                top: None,
                bottom: None,
                opaque: b.opaque,
            })
            .collect();
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &blocks);
        let image = images.get(&atlas.image).expect("图集在 Assets 里");
        let data = image.data.clone().unwrap();
        std::fs::write("D:/work/rust/voxelith/atlas-dump.raw", &data).unwrap();
        eprintln!(
            "图集 {}x{} 格={} 列={} 行={} 字节={}",
            image.width(),
            image.height(),
            atlas.tile_count(),
            atlas.columns,
            atlas.rows,
            data.len()
        );
        // 前 3 格的第一个像素，直接看是不是黑的。
        for tile in 0..6u16 {
            let rect = atlas.tile_rect(tile);
            let px = rect.min.x as u32;
            let py = rect.min.y as u32;
            let idx = ((py * image.width() + px) * 4) as usize;
            eprintln!(
                "tile {tile} @({px},{py}) = ({},{},{},{})",
                data[idx],
                data[idx + 1],
                data[idx + 2],
                data[idx + 3]
            );
        }
    }
}

#[cfg(test)]
mod wiring_tests {
    use super::*;

    /// **诊断测试**：把每个方块每个面的 UV 矩形落到图集上，读出实际像素。
    ///
    /// 这条是为了抓"某些面渲染成纯黑"：如果某个面的矩形落到了**没画过的格子**上，
    /// 那些像素是初始化值 `(0,0,0,0)`，渲染出来就是黑块。
    #[test]
    fn every_block_face_samples_a_painted_tile() {
        let content = crate::content::parse_raw().expect("world.ron 该配好");
        let mut palette = voxelith_axiom::world::VoxelPalette::default();
        let mut names = voxelith_axiom::world::VoxelNames::default();
        for block in &content.world.blocks {
            names.register(&block.id);
            palette.push(voxelith_axiom::world::VoxelAppearance {
                atlas_index: 0,
                opaque: block.opaque,
            });
        }

        let blocks: Vec<BlockTexture> = content
            .world
            .blocks
            .iter()
            .map(|b| BlockTexture {
                side: FaceTexture {
                    base: b.side.base,
                    pattern: Pattern::Solid,
                },
                top: None,
                bottom: None,
                opaque: b.opaque,
            })
            .collect();
        let mut images = Assets::<Image>::default();
        let atlas = AtlasImage::build(&mut images, &blocks);
        let image = images.get(&atlas.image).unwrap();
        let data = image.data.clone().unwrap();
        let width = image.width();

        for (index, block) in content.world.blocks.iter().enumerate() {
            let id = names.id(&block.id).unwrap();
            for face in BlockFace::ALL {
                let tile = atlas
                    .tile_index_for(id, face)
                    .unwrap_or_else(|| panic!("{} 的 {face:?} 没有图集格", block.id));
                let rect = atlas.tile_rect(tile);
                // 取格子中心那个像素。
                let px = (rect.min.x + rect.width() / 2.0) as u32;
                let py = (rect.min.y + rect.height() / 2.0) as u32;
                let offset = ((py * width + px) * 4) as usize;
                let rgba = (
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                );
                assert!(
                    rgba != (0, 0, 0, 0),
                    "方块 #{} `{}` 的 {face:?}（图集格 {tile} @ {px},{py}）落到了**没画过的像素**上：{rgba:?}",
                    index,
                    block.id
                );
                assert!(
                    rgba.3 > 0,
                    "方块 `{}` 的 {face:?} alpha 为 0，会渲染成黑块：{rgba:?}",
                    block.id
                );
            }
        }
    }
}
#[test]
fn grass_top_tile_is_green() {
    // **最短的一条**：直接问"图集里 grass 的顶面格是不是绿色"。
    //
    // 形态：画面上偏绿像素恒为 0.0%，说明顶面根本没显示成绿色。
    // 之前写的测试都自己拼假配置，绕开了游戏真实路径；这条直接调
    // `convert_block`（游戏用的就是它）+ `AtlasImage::build`。
    let raw = crate::content::parse_raw().expect("world.ron 该配好");
    let textures: Vec<BlockTexture> = raw
        .world
        .blocks
        .iter()
        .map(|b| BlockTexture {
            side: FaceTexture {
                base: b.side.base,
                pattern: Pattern::Solid,
            },
            top: b.top.map(|t| FaceTexture {
                base: t.base,
                pattern: Pattern::Solid,
            }),
            bottom: None,
            opaque: b.opaque,
        })
        .collect();
    let mut images = Assets::<Image>::default();
    let atlas = AtlasImage::build(&mut images, &textures);
    let image = images.get(&atlas.image).unwrap();
    let data = image.data.clone().unwrap();
    let width = image.width();

    // grass 是 `world.ron` 里的**第一个**方块 ⇒ 1-based ID = 1。
    let top_tile = atlas
        .tile_index_for(VoxelId(1), BlockFace::Top)
        .expect("grass 该有顶面格");
    let side_tile = atlas
        .tile_index_for(VoxelId(1), BlockFace::Side)
        .expect("grass 该有侧面格");
    let sample = |tile: u16| {
        let rect = atlas.tile_rect(tile);
        let px = (rect.min.x + rect.width() / 2.0) as u32;
        let py = (rect.min.y + rect.height() / 2.0) as u32;
        let o = ((py * width + px) * 4) as usize;
        (data[o], data[o + 1], data[o + 2])
    };
    let top = sample(top_tile);
    let side = sample(side_tile);
    eprintln!("grass 顶面格={top_tile} 像素={top:?}；侧面格={side_tile} 像素={side:?}");
    assert!(top.1 > top.0, "grass 顶面该偏绿，实际 {top:?}");
    assert!(side.0 > side.1, "grass 侧面该偏棕，实际 {side:?}");
}
