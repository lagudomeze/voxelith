//! PC 状态面板（左上）与技能列表（左下）。
//!
//! ## 分层（**R15 / R16**）
//!
//! 本模块**只读** L1 数据、**只写** UI 数据：
//!
//! | 干什么 | 允许 |
//! |---|---|
//! | 读 `Resources` / `Stats` / `AvailableSkills` / `CombatPhase` | ✅ |
//! | 写 `Text` / `BackgroundColor` / `Node`（自己 spawn 的 UI 实体） | ✅ |
//! | 改池、写战斗公式、判断"打不打得到" | ❌（那是 L1 的事） |
//! | 点技能 → 发 `CastRequest`（**请求**，不是改数据） | ✅ |
//!
//! ## 布局
//!
//! ```text
//! ┌─────────────────────────────┐
//! │ 冒险者            ← 左上      │
//! │ 生命  82 / 100  ▓▓▓▓▓▓▓░░░  │  ← 每个池一行：内容里配几个就显示几个
//! │ 法力  50 / 50   ▓▓▓▓▓▓▓▓▓▓  │
//! │ 行动   1 / 1    ▓▓▓▓▓▓▓▓▓▓  │
//! │ 反应   3 / 3    ▓▓▓▓▓▓▓▓▓▓  │
//! │                             │
//! │                        （中）│
//! │                             │
//! │ 技能              ← 左下      │
//! │ [ 普通攻击 ]  [ 火球术 ]     │  ← 每帧按 `AvailableSkills` 重建
//! └─────────────────────────────┘
//! ```
//!
//! ## 两条重建规则
//!
//! - **池的行**：按 `Resources` 里的池数量重建（组件里几个就几行；加一种资源不用改 UI 代码）。
//! - **技能按钮**：按 `AvailableSkills` 重建（哪些可用由 L1 判定，UI 不重复判断）。
//!
//! 重建只在"结构变了"时发生；每帧只刷新文字与进度条宽度（**R110**：不做无谓的重建）。

use bevy::prelude::*;
use bevy::ui::widget::Text;
use voxelith_axiom::atoms::actor::Player;
use voxelith_axiom::behaviors::action::CastRequest;
use voxelith_axiom::behaviors::content::{ResourceId, SkillCatalog};
use voxelith_axiom::behaviors::phase::{AvailableSkills, CombatPhase};
use voxelith_axiom::behaviors::skill::Skill;

use crate::content::Labels;

// ------------------------------------------------------------------ 外观常量

/// 面板底色（半透明，免得挡住后面的世界）。
// 面板底色：**压暗 Kenney 的九宫格面板**，好让文字读得清。
//
// Kenney 的面板中心是亮棕 / 亮灰（那是给背包格子用的），直接铺上去
// 浅色文字会糊在背景里（实测过：标题和资源名几乎看不见）。
// 边框仍然来自图块，所以底色调暗不影响美术感。
pub(super) const PANEL_BG: Color = Color::srgba(0.07, 0.05, 0.03, 0.86);
/// 行内文字色。
pub(super) const TEXT: Color = Color::srgb(0.92, 0.92, 0.95);
/// 次要文字（"82 / 100" 这种）。
pub(super) const TEXT_DIM: Color = Color::srgb(0.68, 0.70, 0.76);
/// 进度条底槽。
pub(super) const BAR_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// 进度条填充（正常）。
pub(super) const BAR_FILL: Color = Color::srgb(0.36, 0.74, 0.42);
/// 进度条填充（低于三成，提示"该小心了"）。
pub(super) const BAR_FILL_LOW: Color = Color::srgb(0.85, 0.34, 0.30);
/// 技能按钮底色。
pub(super) const BUTTON_BG: Color = Color::srgba(0.16, 0.18, 0.26, 0.92);
/// 鼠标悬停 / 按下。
pub(super) const BUTTON_HOVER: Color = Color::srgba(0.28, 0.34, 0.48, 0.95);
pub(super) const BUTTON_PRESSED: Color = Color::srgba(0.18, 0.44, 0.62, 0.95);
/// 反制窗口里按钮的强调色（"这一刻该按它"）。
pub(super) const BUTTON_COUNTER: Color = Color::srgba(0.62, 0.34, 0.18, 0.95);

/// 面板宽度、字号、条高。
pub(super) const PANEL_WIDTH: f32 = 250.0;
pub(super) const FONT: f32 = 15.0;
pub(super) const FONT_SMALL: f32 = 13.0;
pub(super) const BAR_HEIGHT: f32 = 9.0;
pub(super) const EDGE: f32 = 14.0;

// ------------------------------------------------------------------ 字体

/// 界面字体：**必须自带 CJK 字体**。
///
/// Bevy 内置的默认字体（`FiraMono-subset.ttf`）**没有汉字字形**——中文标签会渲染成 `□`（豆腐块），
/// 而且**不报错、不警告**。与本项目"内容里全是中文"直接冲突，所以必须换掉。
///
/// 用 `include_bytes!` 而不是 `AssetServer`：HUD 在 `Startup` 就要建文字节点，
/// 异步加载会先渲染一帧"没有字体"。嵌进二进制也顺带消掉"字体文件找不到"这个运行期失败模式
/// （和 `.ron` 用 `include_str!` 是同一个理由）。
const UI_FONT: &[u8] = include_bytes!("../../../../../assets/fonts/NotoSansSC.ttf");

/// 覆盖默认字体槽位（必须在任何文字节点建出来之前跑）。
///
/// `Assets<Font>` 用 `Option`：无头测试（`MinimalPlugins`，不走 `AssetPlugin`）里没有这个资源，
/// 而 HUD 的**结构**测试不需要字体。缺席时安静跳过，窗口模式下一律存在。
fn install_ui_font(fonts: Option<ResMut<Assets<Font>>>) {
    let Some(mut fonts) = fonts else {
        return;
    };
    // `Font::from_bytes` 是唯一构造入口（`Font` 的字段不是 pub）。
    let font = Font::from_bytes(UI_FONT.to_vec());
    fonts
        .insert(AssetId::<Font>::default(), font)
        .unwrap_or_else(|error| panic!("无法覆盖默认字体槽位：{error}"));
}

// ------------------------------------------------------------------ 标记组件

/// HUD 根节点（全屏容器）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudRoot;

/// 左上：PC 状态面板。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudStatusPanel;

/// 左下：技能列表面板。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudSkillsPanel;

/// 一行资源显示：`资源 ID` + 数字文本 + 进度条填充。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudResourceRow {
    /// 哪个资源池。
    pub pool: ResourceId,
}

/// 行动条的**填充**节点（只改宽度）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudActionBar;

/// 行动条右侧的文字（"普通攻击 47%" / "待命"）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudActionLabel;

/// 状态面板的标题（角色名，内容里配什么就显示什么）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudTitle;

/// 资源数字文本（"82 / 100"）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudResourceValue {
    /// 哪个资源池。
    pub pool: ResourceId,
}

/// 进度条填充（只改宽度与颜色）。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudResourceBar {
    /// 哪个资源池。
    pub pool: ResourceId,
}

/// 一个技能按钮：记下它对应哪个技能**定义实体**，点击时原样发给 L1。
#[derive(Component, Debug, Clone, Copy)]
pub struct HudSkillButton {
    /// 技能定义实体（`SkillCatalog` 里的那个）。
    pub skill: Entity,
}

// ------------------------------------------------------------------ 装配

/// 注册 HUD。
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            // 顺序是**契约**，全部在同一个 `Startup` 链里：
            //   1. `build_ui_theme` 上传 Kenney 图块并插入 `UiTheme`；
            //   2. `install_ui_font` 注册 CJK 字体；
            //   3. `spawn_hud` 建节点（要读前两步的产物）。
            //
            // 必须 `.chain()`：三步之间的产物都靠 `Commands` 传递，而同阶段的
            // `Commands` 要到阶段末尾才落地 —— 拆成不同插件、靠"插件注册顺序"
            // 是**保证不了**的（踩过一次：`UiTheme` 还没落地，HUD 就去读它）。
            (crate::ui_theme::load_ui_atlas, install_ui_font, spawn_hud).chain(),
        )
        .add_systems(
            Update,
            (
                // 结构变了才重建（加池 / 换技能组合）。
                sync_resource_rows,
                sync_skill_buttons,
                // 每帧刷新文字与条宽。
                refresh_resource_text,
                refresh_skill_buttons,
                // 行动条是**连续变化**的（进度每帧都在动），必须每帧刷。
                crate::ui_theme::log_loaded_atlas,
                action_bar::refresh_action_bar,
                action_bar::tint_action_label,
                on_skill_clicked,
            ),
        );
    }
}

/// 建出 HUD 骨架（左上状态面板 + 左下技能面板）。
///
/// 根节点铺满全屏但 `Pickable::IGNORE`：它是纯布局容器，
/// 不该把点击从下面的按钮上抢走。
///
/// 面板外观走 [`UiTheme`]（Kenney `ui-pack-adventure` 的九宫格图块），
/// 不再用纯色矩形 —— 纯色那版看起来像调试界面。
fn spawn_hud(mut commands: Commands, atlas: Res<crate::ui_theme::UiAtlas>) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // ---- 左上：PC 状态 ----
            //
            // 外观 = Kenney 九宫格面板 + 一层半透明底色。
            //
            // **为什么还留底色**：Kenney 的面板中心是通透的（那是给背包格子用的），
            // 直接铺在 3D 战场上文字会看不清。底色压暗一点，边框仍然用图块，
            // 这样既好看又可读。
            root.spawn((
                HudStatusPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(EDGE),
                    top: px(EDGE),
                    width: px(PANEL_WIDTH),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    padding: UiRect::all(px(10.0)),
                    ..default()
                },
                crate::ui_theme::panel(&atlas, true),
                BackgroundColor(PANEL_BG),
                Pickable::IGNORE,
            ))
            .with_children(|panel| {
                // 标题行：角色名。资源行随后由 `sync_resource_rows` 补上。
                panel.spawn((
                    HudTitle,
                    Text::new("—"),
                    TextFont::from_font_size(FONT),
                    TextColor(TEXT),
                    Pickable::IGNORE,
                ));

                // ---- 行动条 ----
                //
                // 位置：**标题之下、资源行之上**。
                //
                // 为什么在上而不是在下：`sync_resource_rows` 用 `with_children` 把资源行
                // **追加**到面板尾部，而资源行有几个是运行时才知道的 ——
                // 想在它们下面放东西就得和动态行抢顺序。放上面没这个问题。
                //
                // 而且它本来就该在最显眼处：行动条是"当前正在发生的事"，
                // 资源是静态读数。玩家最需要的可读性是**反制窗口还剩多久**。
                panel
                    .spawn((
                        Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(1.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|row| {
                        row.spawn((
                            HudActionLabel,
                            Text::new("待命"),
                            TextFont::from_font_size(FONT_SMALL),
                            TextColor(TEXT_DIM),
                            Pickable::IGNORE,
                        ));
                        // 槽（底衬）→ 填充（只改宽度）。
                        row.spawn((
                            Node {
                                width: percent(100.0),
                                height: px(action_bar::ACTION_BAR_HEIGHT),
                                ..default()
                            },
                            BackgroundColor(BAR_BG),
                            Pickable::IGNORE,
                        ))
                        .with_children(|track| {
                            track.spawn((
                                HudActionBar,
                                Node {
                                    width: percent(0.0),
                                    height: percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(BUTTON_PRESSED),
                                Pickable::IGNORE,
                            ));
                        });
                    });
            });

            // ---- 左下：技能列表 ----
            root.spawn((
                HudSkillsPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(EDGE),
                    bottom: px(EDGE),
                    width: px(PANEL_WIDTH),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    padding: UiRect::all(px(10.0)),
                    ..default()
                },
                // 技能面板用**灰色**九宫格：与左上状态面板（棕色）区分开，
                // 玩家一眼能分出"我的状态"和"我能做什么"。
                crate::ui_theme::panel(&atlas, false),
                BackgroundColor(PANEL_BG),
                Pickable::IGNORE,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("技能"),
                    TextFont::from_font_size(FONT_SMALL),
                    TextColor(TEXT_DIM),
                    Pickable::IGNORE,
                ));
            });
        });
}

// ------------------------------------------------------------------ 资源行

/// 玩家当前有几个池 —— 用它当"结构版本"，变了才重建行。
/// 可用技能集合变化时重建按钮。
fn sync_skill_buttons(
    mut commands: Commands,
    available: Res<AvailableSkills>,
    catalog: Res<SkillCatalog>,
    skills: Query<&Skill>,
    existing: Query<(Entity, &HudSkillButton)>,
    panels: Query<Entity, With<HudSkillsPanel>>,
) {
    let mut wanted: Vec<Entity> = available
        .all()
        .iter()
        .filter_map(|&skill| {
            let id = skills.get(skill).ok()?.id;
            // 走目录换成**规范实体**：这样按钮点击时发出的实体一定在目录里。
            catalog.get(id).copied()
        })
        .collect();
    wanted.sort_unstable_by_key(|entity| entity.index());
    wanted.dedup();

    let mut current: Vec<Entity> = existing.iter().map(|(_, button)| button.skill).collect();
    current.sort_unstable_by_key(|entity| entity.index());
    current.dedup();
    if current == wanted {
        return;
    }

    for (entity, _) in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(panel) = panels.single() else {
        return;
    };

    for skill_entity in wanted {
        let Ok(skill) = skills.get(skill_entity) else {
            continue;
        };
        // 用 `Skill::name`（内容里的 `name` 字段，"普通攻击"）。
        //
        // **不要**用 `Vocab::skill_name`：词汇表的 `name` 表存的是 RON 里的 `id`
        // （`basic_attack`），那是给加载期做"字符串 → ID"反向查找用的，
        // 不是给人看的标签。显示名在 `SkillRon.name` → `Skill.name` 这条线上。
        let label = if skill.name.is_empty() {
            format!("技能{}", skill.id.0)
        } else {
            skill.name.clone()
        };

        commands.entity(panel).with_children(|panel| {
            panel.spawn((
                HudSkillButton {
                    skill: skill_entity,
                },
                Button,
                Node {
                    width: percent(100.0),
                    padding: UiRect::axes(px(8.0), px(5.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(BUTTON_BG),
                children![(
                    Text::new(label),
                    TextFont::from_font_size(FONT),
                    TextColor(TEXT),
                    Pickable::IGNORE,
                )],
            ));
        });
    }
}

/// 每帧按交互状态改按钮底色（顺便在反制窗口里强调可用的反制）。
fn refresh_skill_buttons(
    phase: Res<State<CombatPhase>>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<HudSkillButton>>,
) {
    let counter_window = matches!(*phase.get(), CombatPhase::AwaitingCounter);
    for (interaction, mut color) in &mut buttons {
        color.0 = match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVER,
            Interaction::None => {
                if counter_window {
                    BUTTON_COUNTER
                } else {
                    BUTTON_BG
                }
            }
        };
    }
}

/// 点击技能 → 发一条 `CastRequest`（**请求**，由 L1 校验与结算）。
///
/// 这里**不做任何判断**：技能可不可用、扣不扣得起，全是 L1 的事（**R17**）。
fn on_skill_clicked(
    buttons: Query<(&Interaction, &HudSkillButton), Changed<Interaction>>,
    players: Query<Entity, With<Player>>,
    mut requests: MessageWriter<CastRequest>,
) {
    let Ok(caster) = players.single() else {
        return;
    };
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        requests.write(CastRequest {
            caster,
            skill: button.skill,
            target: None,
        });
    }
}
mod action_bar;
mod resources;

// sync_resource_rows 由 HudPlugin 注册，所以这里要能被同模块的 Plugin impl 看到。
use resources::{refresh_resource_text, sync_resource_rows};
