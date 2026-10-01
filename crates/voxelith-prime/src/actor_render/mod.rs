//! L2 角色表现：**2D 纸片人**（面向相机的精灵 + 走路动画）。
//!
//! ## 为什么是"纸片人"
//!
//! 俯视角战术战场上，一帧精灵比 3D 模型便宜得多，也更容易做出辨识度
//! （MC 风格的方块地形 + 2D 立绘角色）。精灵是垂直于地面的四边形，
//! 斜俯视的正交相机看过去正好是正面 —— 不需要跟着相机转。
//!
//! ## 素材
//!
//! 用内置的真实素材（见 [`spritesheet`]）：CC0 的四方向走路循环。
//! 布局是数据（[`SheetLayout`]），换素材只改数字。
//!
//! ## 分工：**表现层只读，不驱动玩法**
//!
//! ```text
//! Movement（本模块的组件）  ──►  ActorVisual（选行 + 翻帧）──►  Sprite::rect
//!     位置 / 朝向 / 速度              行 = 朝向、帧 = 走路循环
//! ```
//!
//! `Movement` 是**表现层的位置**。战斗逻辑本身是"抽象距离"，不需要坐标；
//! 表现层要画在哪里是它自己的事。将来空间维度进 L0 时，
//! 把 `Movement` 换成"从 L0 的位置组件读"即可，本模块其余部分不用动。

mod spritesheet;

pub use spritesheet::{
    ActorSheet, FRAME_SIZE, HERO_SHEET_PATH, SheetLayout, SheetPlacement, SheetRow,
    load_sheet_image,
};

use bevy::prelude::*;
use bevy::sprite::{SpriteAlphaMode, SpriteMesh};
use voxelith_axiom::atoms::actor::{ActorRole, Faction};

use crate::content::ContentData;

/// 朝向（决定用精灵表的哪一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum Facing {
    /// 朝南（屏幕下）——默认。
    #[default]
    South,
    /// 朝北（屏幕上）。
    North,
    /// 朝东（屏幕右）。
    East,
    /// 朝西（屏幕左）。
    West,
}

impl Facing {
    /// 由移动方向（世界 `XZ` 平面）推出朝向。
    ///
    /// 取分量**绝对值较大**的那一轴：斜着走时朝"更像"的方向，
    /// 这样四方向的素材也能表达八方向的移动。
    pub fn from_direction(direction: Vec2) -> Self {
        if direction.length_squared() == 0.0 {
            return Self::default();
        }
        if direction.x.abs() >= direction.y.abs() {
            if direction.x > 0.0 {
                Self::East
            } else {
                Self::West
            }
        } else if direction.y > 0.0 {
            // 世界 `+Z` 在屏幕上朝下（见相机模块的说明）。
            Self::South
        } else {
            Self::North
        }
    }

    /// 用精灵表的哪一行。
    pub fn row(self) -> u32 {
        match self {
            Facing::South => 0,
            Facing::North => 1,
            Facing::East | Facing::West => 2,
        }
    }

    /// 要不要水平翻转（素材的侧视只有朝东那一套）。
    pub fn flip_x(self) -> bool {
        matches!(self, Facing::West)
    }
}

/// 表现层的位置与移动（**组件**）。
///
/// `position` 用世界 `XZ` 平面：`x` 向右、`y` 对应世界 `z`（屏幕上朝下）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Movement {
    /// 世界位置（`XZ` 平面）。
    pub position: Vec2,
    /// 速度（单位/秒）。
    pub velocity: Vec2,
    /// 想要移动的方向（未归一化也算，[`Facing`] 只看分量大小）。
    pub desired: Vec2,
}

impl Movement {
    /// 站住不动。
    pub fn still(position: Vec2) -> Self {
        Self {
            position,
            velocity: Vec2::ZERO,
            desired: Vec2::ZERO,
        }
    }

    /// 正在移动？
    pub fn is_moving(&self) -> bool {
        self.velocity.length_squared() > 1e-6
    }
}

/// 这个角色在走路（**由 [`drive_demo_patrol`] 或将来的输入 / AI 写**）。
///
/// 本模块**只读**它来决定播哪一帧，不自己写。
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Moving;

/// 挂在角色实体上的**渲染意图**（由内容层在生成角色时挂上）。
///
/// 只携带数据（哪个行组、什么配色），不含任何渲染句柄。
#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct ActorVisualIntent {
    /// 这个角色在精灵表里用哪一组行（当前素材只有一个角色，恒为 0）。
    pub sheet_index: u32,
    /// 阵营（决定色调，也用于日志）。
    pub faction: Faction,
}

/// 已建好渲染的角色状态。
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ActorVisual {
    /// 当前行（= 朝向）。
    pub row: u32,
    /// 当前帧。
    pub frame: u32,
    /// 帧计时器（秒）。
    pub elapsed: f32,
    /// 当前是否在走（用来在"刚起步"时把帧重置到 0）。
    pub walking: bool,
}

impl ActorVisual {
    /// 造一个（默认朝南、第 0 帧）。
    pub fn new() -> Self {
        Self {
            row: 0,
            frame: 0,
            elapsed: 0.0,
            walking: false,
        }
    }
}

impl Default for ActorVisual {
    fn default() -> Self {
        Self::new()
    }
}

/// 演示用巡逻参数（**配置**）。
///
/// 现在没有空间维度，所以给每个角色一个来回走的巡逻，让"走路动画"真的动起来。
/// 接入真实移动（输入 / AI）之后把这个组件删掉即可。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct DemoPatrolConfig {
    /// 巡逻幅度（世界单位，从出生点往两侧各走这么远）。
    pub amplitude: f32,
    /// 每秒相位推进多少（决定来回一趟多快）。
    ///
    /// **它同时决定速度**：位置是 mplitude · sin(phase)，速度就是它的导数
    /// mplitude · ω · cos(phase)（ω = phase_per_second · 2π）。
    /// 另开一个 speed 旋钮只会让两者能互相矛盾，所以不设。
    pub phase_per_second: f32,
}

impl Default for DemoPatrolConfig {
    fn default() -> Self {
        Self {
            amplitude: 4.0,
            phase_per_second: 0.12,
        }
    }
}

/// 每个角色的巡逻相位与出生点。
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct DemoPatrol {
    /// 出生点（巡逻中心）。
    pub origin: Vec2,
    /// 相位（弧度）。
    pub phase: f32,
}

/// 精灵的视觉缩放：一格 = 1.6 世界单位（比体素稍大，纸片人才有存在感）。

/// 精灵离地高度（脚站在地表 `y = 0` 上，所以中心抬到半个身位）。

/// 注册角色表现。
pub struct ActorRenderPlugin;

impl Plugin for ActorRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DemoPatrolConfig>()
            // **加载素材**：只发起异步加载，几帧后像素才到。
            // 排在内容翻译之后（建表要读角色数量）。
            .add_systems(
                Startup,
                load_actor_sheet_asset.after(crate::content::ContentSet::Translate),
            )
            // **建表 → 挂精灵 → 每帧推进**：必须 `chain()`。
            //
            // 为什么建表与挂精灵在 `Update` 而不是 `Startup`：
            // 素材是**异步加载**的，`Startup` 那一帧像素还没到
            // （`Assets<Image>` 里查不到句柄）。若在 `Startup` 强行建表，
            // 拿到的是**空图** —— 角色完全不显示而且不报错。
            .add_systems(
                Update,
                (
                    materialize_actor_sheet,
                    attach_visuals,
                    drive_demo_patrol,
                    advance_animation,
                    sync_sprite_rect,
                    sync_sprite_transform,
                )
                    .chain(),
            );
    }
}

/// 加载精灵表素材（**异步**：只发起加载，几帧后才建得起表）。
///
/// ## 为什么要拆成"发起加载"与"建表"两步
///
/// `AssetServer::load` 立刻返回句柄，但**像素还没到**。
/// 若在这里就建 `ActorSheet`，`frame_rect` 是对的、`image` 却是空图 ——
/// 画面上角色**完全不显示且不报错**，看起来像"精灵逻辑坏了"。
///
/// 所以：这个系统只发起加载并把句柄记进 [`SheetAsset`]；
/// [`materialize_actor_sheet`] 每帧检查像素到了没有，到了才建表。
///
/// 对照：烘焙版本（`include_bytes!`）没有这个问题 —— 那是它唯一的优势。
/// 代价是换素材要重跑转码脚本再重编译，所以这里选了标准做法。
pub fn load_actor_sheet_asset(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    // 没有资源设施（结构测试）：跳过。**必填参数会让"只测结构"不可能**。
    let Some(assets) = assets else {
        return;
    };
    let handle = load_sheet_image(&assets);
    commands.insert_resource(SheetAsset { handle });
}

/// 精灵表素材的句柄（**还在加载中时也在**）。
#[derive(Resource, Debug, Clone)]
pub struct SheetAsset {
    /// 贴图句柄。像素到位之前它是一张空图。
    pub handle: Handle<Image>,
}

/// 素材到位后建出 [`ActorSheet`]（只成功一次）。
///
/// 每帧跑，直到建成为止；之后 `ActorSheet` 已存在，本系统直接返回。
pub fn materialize_actor_sheet(
    mut commands: Commands,
    asset: Option<Res<SheetAsset>>,
    existing: Option<Res<ActorSheet>>,
    images: Option<Res<Assets<Image>>>,
    content: Option<Res<ContentData>>,
) {
    if existing.is_some() {
        return;
    }
    let (Some(asset), Some(images)) = (asset, images) else {
        return;
    };
    let Some(image) = images.get(&asset.handle) else {
        // 还没加载完。**这里不能 warn**：加载要几帧，每帧 warn 会刷屏。
        return;
    };
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        warn!("精灵表素材是空图（{width}×{height}），角色将不可见");
        return;
    }

    // **摆放参数来自内容**（`world.ron` 的 `sprite` 段），不在代码里写死：
    // 换素材或换取景时它要跟着改，而那是内容的事，不是引擎的事。
    let mut layout = SheetLayout::default();
    if let Some(content) = content.as_ref() {
        let (world_size, height) = content.world.sprite;
        layout.placement = SheetPlacement { world_size, height };
    }
    let actors = content
        .as_ref()
        .map_or(0, |c| c.players.len() + c.monsters.len());

    // **素材装不下布局就报出来**：`frame_rect` 会切到图外（一片透明），
    // 表现是"角色缺一块"，而且不报错。
    let need_w = layout.columns * layout.frame_size;
    let need_h = layout.row_count() * layout.frame_size;
    if width < need_w || height < need_h {
        warn!(
            "精灵表 {HERO_SHEET_PATH} 只有 {width}×{height}，但布局需要 {need_w}×{need_h} 像素：\
             角色会被切成残缺的图（检查素材或 `SheetLayout`）"
        );
    }

    info!(
        "角色精灵表：{width}×{height}（{HERO_SHEET_PATH}），{} 行，{} 个角色，尺寸 {}",
        layout.row_count(),
        actors,
        layout.placement.world_size
    );
    commands.insert_resource(ActorSheet {
        image: asset.handle.clone(),
        layout,
        width,
        height,
    });
}

/// 给已经有 [`ActorVisualIntent`] 的角色挂上精灵。
fn attach_visuals(
    mut commands: Commands,
    sheet: Option<Res<ActorSheet>>,
    camera: Res<crate::voxel_render::CameraConfig>,
    intents: Query<(Entity, &ActorVisualIntent), Without<ActorVisual>>,
) {
    let Some(sheet) = sheet else {
        return;
    };
    // 精灵朝向跟着**相机方位角**走：等距（45°）下精灵也转 45°，才不会侧对相机。
    let camera_azimuth_rad = camera.azimuth_degrees.to_radians();
    let mut placed = 0_u32;
    for (entity, _intent) in &intents {
        let visual = ActorVisual::new();
        let position = battlefield_slot(placed);
        // **临时测量**：第一个精灵放到 +40 高度（背景是天空，便于测包围盒）、放大到 20。
        let placement = sheet.placement();
        let scale = placement.world_size;
        let sprite_pos = Vec3::new(position.x, placement.height, position.y);
        placed += 1;

        let sprite = commands
            .spawn((
                Name::new("actor-sprite"),
                SpriteMesh {
                    image: sheet.image.clone(),
                    rect: Some(sheet.frame_rect(0, 0)),
                    custom_size: Some(Vec2::splat(scale)),
                    alpha_mode: SpriteAlphaMode::Blend,
                    ..default()
                },
                // **精灵必须面向相机**：相机有方位角（等距要转 45°），
                // 精灵不转就会**侧对相机**，看起来像一张纸的边。
                //
                // 绕 `Y` 轴转 `−方位角`：方位角 0 时不转（精灵本来就在 XY 平面上、
                // 法线朝 +Z），转 45° 后正对来自 `+X+Z` 的相机。
                Transform::from_translation(sprite_pos)
                    .with_rotation(Quat::from_rotation_y(-camera_azimuth_rad)),
            ))
            .id();

        commands
            .entity(entity)
            .insert((
                visual,
                Movement::still(position),
                DemoPatrol {
                    origin: position,
                    // 相位错开，免得所有角色像方阵一样整齐划一。
                    phase: placed as f32 * 1.7,
                },
            ))
            .add_child(sprite);
    }
    if placed > 0 {
        info!("角色表现：挂了 {placed} 个精灵");
    }
}

/// 第 `index` 个角色摆在战场的哪个位置（摆成几行）。
fn battlefield_slot(index: u32) -> Vec2 {
    let per_row = 5;
    let spacing = 6.0;
    let row = (index / per_row) as f32;
    let column = (index % per_row) as f32;
    Vec2::new(
        (column - (per_row as f32 - 1.0) * 0.5) * spacing,
        (row - 0.5) * spacing,
    )
}

/// **演示用**：让每个角色沿 `X` 轴来回巡逻，产生真实的位移与速度。
///
/// 这是"移动"的**临时来源**：玩法层还没有空间维度。接入输入或 AI 之后，
/// 把写入 `Movement` / `Moving` 的职责交给它们，删掉这个系统即可
/// —— `advance_animation` 只依赖 `Moving` 与 `Movement::velocity`，不需要改。
fn drive_demo_patrol(
    time: Res<Time>,
    config: Res<DemoPatrolConfig>,
    mut actors: Query<(&mut Movement, &mut DemoPatrol, Has<Moving>)>,
) {
    let delta = time.delta_secs();
    let omega = config.phase_per_second * std::f32::consts::TAU;
    for (mut movement, mut patrol, _) in &mut actors {
        patrol.phase += delta * omega;
        // 位置走正弦，速度是它的导数 —— 两者自洽，不会出现"位置没动但速度非零"。
        movement.position.x = patrol.origin.x + patrol.phase.sin() * config.amplitude;
        movement.velocity.x = patrol.phase.cos() * config.amplitude * omega;
        movement.desired = Vec2::new(movement.velocity.x, 0.0);
    }
}

/// 推进动画：按朝向选行、按速度决定走不走、按帧率换帧。
fn advance_animation(
    time: Res<Time>,
    sheet: Res<ActorSheet>,
    mut actors: Query<(&mut ActorVisual, &Movement)>,
) {
    let delta = time.delta_secs();
    for (mut visual, movement) in &mut actors {
        let walking = movement.is_moving();
        let row = Facing::from_direction(movement.desired).row();

        // 换行或"刚起步/刚停下"时把帧重置到 0：
        // 否则切换朝向会停在半截的动作上，看起来像抽了一下。
        if row != visual.row || walking != visual.walking {
            visual.row = row;
            visual.walking = walking;
            visual.frame = 0;
            visual.elapsed = 0.0;
            continue;
        }

        // 站住时定格在第 0 帧（素材的第 0 帧就是站姿）。
        if !walking {
            visual.frame = 0;
            continue;
        }

        let frames = sheet.frames(row);
        if frames <= 1 {
            continue;
        }
        visual.elapsed += delta * sheet.frames_per_second(row);
        while visual.elapsed >= 1.0 {
            visual.elapsed -= 1.0;
            visual.frame = (visual.frame + 1) % frames;
        }
    }
}

/// 把当前帧写到精灵的帧矩形（只在该变的时候写：每帧改 `rect` 会打断渲染批处理）。
///
/// ## ⚠️ 这里曾经查 `Sprite`，而实体上是 `SpriteMesh`（动画整个失效）
///
/// 第 5 轮把角色组件从 `Sprite` 换成了 `SpriteMesh`（为了 `alpha_mode: Blend`），
/// 但**这个查询没跟着改**。结果是：
///
/// - `advance_animation` 一直在正常推进 `ActorVisual::frame`（有单测守着）；
/// - 而这里 `sprites.get_mut(child)` **永远失败**，于是 `rect` 从没被更新过；
/// - 画面上角色**永远定格在第 0 帧** —— 但所有测试都是绿的。
///
/// 这正是"**结构测试通过 ≠ 画面对**"的典型：`updates_the_frame_rect` 那条测试
/// 直接构造了 `Sprite` 实体，所以它测的是**测试自己搭的场景**，不是真实世界。
/// 现在改成同时兼容两种组件，并且加一条**用真实生成路径**的测试来防回归。
fn sync_sprite_rect(
    sheet: Res<ActorSheet>,
    visuals: Query<(&ActorVisual, &Children), Changed<ActorVisual>>,
    mut mesh_sprites: Query<&mut SpriteMesh>,
    mut ui_sprites: Query<&mut Sprite>,
) {
    for (visual, children) in &visuals {
        let rect = sheet.frame_rect(visual.row, visual.frame);
        let flip = sheet.flip_x(visual.row);
        for child in children.iter() {
            if let Ok(mut sprite) = mesh_sprites.get_mut(child) {
                if sprite.rect != Some(rect) {
                    sprite.rect = Some(rect);
                }
                if sprite.flip_x != flip {
                    sprite.flip_x = flip;
                }
                continue;
            }
            // 兼容仍然用 `Sprite` 的实体（例如以后加的 HUD 图标）。
            if let Ok(mut sprite) = ui_sprites.get_mut(child) {
                if sprite.rect != Some(rect) {
                    sprite.rect = Some(rect);
                }
                if sprite.flip_x != flip {
                    sprite.flip_x = flip;
                }
            }
        }
    }
}

/// 把 `Movement::position` 同步到精灵的 `Transform`（**表现层的位置真相在组件里**）。
fn sync_sprite_transform(
    actors: Query<(&Movement, &Children), Changed<Movement>>,
    mut transforms: Query<&mut Transform>,
) {
    for (movement, children) in &actors {
        for child in children.iter() {
            if let Ok(mut transform) = transforms.get_mut(child) {
                transform.translation.x = movement.position.x;
                transform.translation.z = movement.position.y;
            }
        }
    }
}

/// 内容层用的便捷构造：给角色算出它的渲染意图。
pub fn intent_for(_index: u32, faction: Faction, _role: ActorRole) -> ActorVisualIntent {
    ActorVisualIntent {
        sheet_index: 0,
        faction,
    }
}
// ------------------------------------------------------------------ 测试

/// 单元测试（体量大于被测代码，单独成文件；见 `tests.rs`）。
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
