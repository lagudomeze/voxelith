//! HUD 的**资源行**：池名、结构签名、行重建、数字与条宽刷新。
//!
//! 从 `hud/mod.rs` 切出来是因为它已经让那个文件越过 500 行（**R26**），
//! 而"资源行"本身是一个内聚的子域：它只关心"有哪些池、每池画一行"。

use bevy::prelude::*;
use voxelith_axiom::atoms::actor::{Player, Resources};
use voxelith_axiom::behaviors::content::{ResourceId, Vocab};

use super::{
    BAR_BG, BAR_FILL, BAR_FILL_LOW, BAR_HEIGHT, FONT_SMALL, HudResourceBar, HudResourceRow,
    HudResourceValue, HudStatusPanel, HudTitle, Labels, TEXT, TEXT_DIM,
};

pub fn pool_signature(pools: Option<&Resources>) -> Vec<ResourceId> {
    let Some(pools) = pools else {
        return Vec::new();
    };
    let mut ids: Vec<ResourceId> = pools.pools.keys().copied().collect();
    // `HashMap` 顺序不保证 → 排序，否则每帧都"结构变了"。
    ids.sort_unstable_by_key(|id| id.0);
    ids
}

/// 池结构变化时重建资源行（**有多少池就有多少行**）。
pub fn sync_resource_rows(
    mut commands: Commands,
    players: Query<(&Resources, &Name), With<Player>>,
    existing: Query<(Entity, &HudResourceRow)>,
    panels: Query<Entity, With<HudStatusPanel>>,
    mut titles: Query<&mut Text, With<HudTitle>>,
    vocab: Option<Res<Vocab>>,
    labels: Option<Res<Labels>>,
    atlas: Res<crate::ui_theme::UiAtlas>,
) {
    let Ok((pools, name)) = players.single() else {
        return;
    };
    // 标题用**内容里的名字**（`Name` 由 L2 按模板 `name` 生成）。
    if let Ok(mut title) = titles.single_mut() {
        let wanted = name.as_str();
        if title.0 != wanted {
            title.0 = wanted.to_owned();
        }
    }

    let structure = pool_signature(Some(pools));

    let mut current: Vec<ResourceId> = existing.iter().map(|(_, row)| row.pool).collect();
    current.sort_unstable_by_key(|id| id.0);
    // **显示名变了也要重建**：池结构没变、但 `vocabulary.ron` 里改过名字（热重载）
    // 时，只比结构的话界面会一直显示旧名字——而内容确实已经换了。
    // `Labels` 每次重新翻译都会被重插，所以 `is_changed()` 正好是"内容换过了"。
    let renamed = labels.as_ref().is_some_and(|labels| labels.is_changed());
    if current == structure && !renamed {
        return;
    }

    // 结构变了：整批重建（这是**低频**事件——加一种资源、换一个 PC）。
    for (entity, _) in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(panel) = panels.single() else {
        return;
    };

    for pool in structure {
        let name = pool_name(pool, &vocab, &labels);
        commands.entity(panel).with_children(|panel| {
            panel
                .spawn((
                    HudResourceRow { pool },
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|row| {
                    // 第一行：名字 …… 数值
                    row.spawn((
                        Node {
                            width: percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|line| {
                        line.spawn((
                            Text::new(name),
                            TextFont::from_font_size(FONT_SMALL),
                            TextColor(TEXT),
                            Pickable::IGNORE,
                        ));
                        line.spawn((
                            HudResourceValue { pool },
                            Text::new("—"),
                            TextFont::from_font_size(FONT_SMALL),
                            TextColor(TEXT_DIM),
                            Pickable::IGNORE,
                        ));
                    });
                    // 第二行：进度条槽
                    row.spawn((
                        Node {
                            width: percent(100.0),
                            height: px(BAR_HEIGHT),
                            ..default()
                        },
                        crate::ui_theme::bar_frame(
                            &atlas,
                            crate::ui_theme::bar_tiles(pool.0 as usize).1,
                        ),
                        BackgroundColor(BAR_BG),
                        Pickable::IGNORE,
                    ))
                    .with_children(|track| {
                        track.spawn((
                            HudResourceBar { pool },
                            Node {
                                width: percent(0.0),
                                height: percent(100.0),
                                ..default()
                            },
                            crate::ui_theme::bar_fill(
                                &atlas,
                                crate::ui_theme::bar_tiles(pool.0 as usize).0,
                            ),
                            BackgroundColor(BAR_FILL),
                            Pickable::IGNORE,
                        ));
                    });
                });
        });
    }
}

/// 池的显示名：优先内容里的 `name`（`Labels` 按词汇 ID 索引），退回 `Vocab` 的内部 id。
///
/// 注意 `Vocab::resource_name` 给的是 RON 里的 **`id`**（`"hp"`），**不是** `name`（`"生命"`）——
/// 想要中文名必须查 `Labels`。
pub fn pool_name(
    pool: ResourceId,
    vocab: &Option<Res<Vocab>>,
    labels: &Option<Res<Labels>>,
) -> String {
    if let Some(labels) = labels
        && let Some(name) = labels.pools.get(&pool)
        && !name.is_empty()
    {
        return name.clone();
    }
    if let Some(vocab) = vocab {
        let id = vocab.resource_name(pool);
        if !id.is_empty() {
            return id.to_owned();
        }
    }
    format!("资源{}", pool.0)
}

/// 每帧刷新资源数字与条宽（**只改文字与宽度**，不动结构）。
pub fn refresh_resource_text(
    players: Query<&Resources, With<Player>>,
    mut values: Query<(&HudResourceValue, &mut Text)>,
    mut bars: Query<(&HudResourceBar, &mut Node, &mut BackgroundColor)>,
) {
    let Ok(pools) = players.single() else {
        return;
    };

    for (value, mut text) in &mut values {
        let current = pools.current(value.pool);
        let max = pools.max(value.pool);
        let rendered = format!("{current:.0} / {max:.0}");
        if text.0 != rendered {
            text.0 = rendered;
        }
    }

    for (bar, mut node, mut color) in &mut bars {
        let max = pools.max(bar.pool);
        let ratio = if max > 0.0 {
            (pools.current(bar.pool) / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        node.width = percent(ratio * 100.0);
        // 低于三成换成警示色（这是**展示**判断，不是战斗规则）。
        let wanted = if ratio < 0.3 { BAR_FILL_LOW } else { BAR_FILL };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}
