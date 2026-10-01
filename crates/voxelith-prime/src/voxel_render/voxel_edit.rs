//! **体素编辑**：鼠标左键挖、右键放。
//!
//! ## 数据流（每一步都只用已有的东西）
//!
//! ```text
//! 鼠标位置 ──viewport_to_world──► 世界射线
//!                                   │
//!                          VoxelStore::raycast（DDA）
//!                                   │
//!                            VoxelHit { pos, normal }
//!                                   │
//!             ┌─────────────────────┴─────────────────────┐
//!          左键：pos 设为空气                        右键：pos + normal 放方块
//!             └─────────────────────┬─────────────────────┘
//!                          set_and_notify
//!                                   │
//!                        ChunkDirtyMessage
//!                                   │
//!                    rebuild_dirty_chunks → 异步重新网格化
//! ```
//!
//! **没有新造数据流**：DDA 射线、`VoxelStore::set`、`ChunkDirtyMessage`、
//! 脏区块重建，四样都是既有设施且都有单测。这个模块只是**把它们接起来**。
//!
//! ## 为什么"放置位置 = `pos + normal`"
//!
//! `VoxelHit::normal` 是命中面的**朝外法线**。要放在"贴着那个面、朝外"的那一格。
//! 用玩家位置或相机方向去推都是错的（**R69**）——
//! 斜视时那样算出来的位置经常是方块里面。
//!
//! ## 为什么挖的时候要护住地板
//!
//! `TerrainParams::floor_y` 以下全是深层方块。不挡的话玩家能**一路挖穿世界**，
//! 看到下面的清屏色，而且再也没法恢复（那一层不在任何区块的可见面里）。
//! 所以 `y <= floor_y` 一律拒绝。

use bevy::camera::Camera;
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use voxelith_axiom::world::{TerrainParams, Voxel, VoxelId, VoxelStore, WorldConfig};

use super::camera::BattlefieldCamera;

/// 光标能改到多远（格）。超出就不动手 —— 免得隔着半个战场点到别的地方。
pub const REACH: f32 = 96.0;

/// 右键放置的方块 ID（**内容里方块表的第 4 个** = `stone`）。
///
/// 将来接"快捷栏"时把这里换成当前选中的格。现在写死一个值，
/// 是为了让编辑链路先跑通；**不是**忘了做成可配置。
pub const PLACE_BLOCK: VoxelId = VoxelId(3);

/// 一帧的编辑动作（从输入解析出来的**纯数据**）。
///
/// 拆出来的理由：`apply_edit` 的逻辑（能不能挖、放哪儿、护地板）
/// 值得单测，而单测里造不出真实的鼠标事件。
/// 把"解析输入"与"执行动作"分开，前者薄、后者可测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    /// 挖掉命中格。
    Break,
    /// 在命中面之外放一格。
    Place,
}

/// 从"命中结果 + 想做什么"算出**要改哪一格、改成什么**。
///
/// 返回 `None` 表示这个动作不该执行（超出世界底、或者没有命中）。
///
/// **纯函数**：不碰世界，所以可以直接单测边界。
pub fn resolve_edit(
    hit: Option<voxelith_axiom::world::VoxelHit>,
    action: EditAction,
    terrain: &TerrainParams,
) -> Option<([i32; 3], Voxel)> {
    let hit = hit?;
    match action {
        EditAction::Break => {
            // **护住地板**：世界底以下不让挖（挖穿了能一路掉出世界，
            // 而且那一层不在任何区块的可见面里，看不回来）。
            if hit.pos[1] <= terrain.floor_y {
                return None;
            }
            Some((hit.pos, Voxel::default())) // 默认 = 空气
        }
        EditAction::Place => {
            let pos = [
                hit.pos[0] + hit.normal[0],
                hit.pos[1] + hit.normal[1],
                hit.pos[2] + hit.normal[2],
            ];
            // 放置同样护地板：别在世界底以下堆方块（没有可见面，纯粹浪费）。
            if pos[1] < terrain.floor_y {
                return None;
            }
            Some((pos, Voxel::solid(PLACE_BLOCK)))
        }
    }
}

/// 读鼠标，改体素。
///
/// **只在有 3D 相机、有主窗口、有光标时才做事**：任何一样缺失都直接返回，
/// 而不是 panic —— 无头测试里这些东西都不存在。
#[allow(clippy::too_many_arguments)]
pub fn edit_with_mouse(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<BattlefieldCamera>>,
    config: Res<WorldConfig>,
    mut store: ResMut<VoxelStore>,
    mut dirty: MessageWriter<voxelith_axiom::world::ChunkDirtyMessage>,
) {
    // **先决定"要做什么"**，再统一执行一次 —— 一帧最多改一格。
    //
    // 同时按左右键时优先级给**挖**（破坏性动作更可能是玩家意图，
    // 而且顺序执行两次会让"放好的方块立刻被挖掉"，看起来像没反应）。
    let action = if mouse.just_pressed(MouseButton::Left) {
        EditAction::Break
    } else if mouse.just_pressed(MouseButton::Right) {
        EditAction::Place
    } else {
        return;
    };

    // 光标不在窗口里（或者窗口没有光标）就什么都不做。
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };

    apply_edit_at(
        cursor,
        camera,
        camera_transform,
        action,
        &config,
        &mut store,
        &mut dirty,
    );
}

/// **从"屏幕坐标 + 动作"到"改哪一格"的全部逻辑**（系统的可测内核）。
///
/// ## 为什么把这段从系统里抽出来
///
/// 它是**唯一会出错的部分**：相机射线、DDA、护地板、`pos + normal`。
/// 而外面那层（读 `just_pressed`、取光标、`single()` 查询）全是样板，
/// 出错的话表现是"整个功能不响应"，一眼能看出来。
///
/// 抽出来之后测试可以直接给一个屏幕坐标，**不需要造真实光标** ——
/// 这一版测试第一版就是卡在"测试窗口没有光标"上的。
#[allow(clippy::too_many_arguments)]
pub fn apply_edit_at(
    cursor: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    action: EditAction,
    config: &WorldConfig,
    store: &mut VoxelStore,
    dirty: &mut MessageWriter<voxelith_axiom::world::ChunkDirtyMessage>,
) -> bool {
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return false;
    };
    let origin = ray.origin.to_array();
    let direction = ray.direction;
    let direction = [direction.x, direction.y, direction.z];

    // 射线 + 判定 + 写入，**全程只用一次 `&mut store`**。
    let hit = store.raycast(&config.terrain, origin, direction, REACH);
    let Some((pos, voxel)) = resolve_edit(hit, action, &config.terrain) else {
        return false;
    };
    store.set_and_notify(pos, voxel, &config.terrain, dirty)
}

/// 注册体素编辑。
pub struct VoxelEditPlugin;

impl Plugin for VoxelEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, edit_with_mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxelith_axiom::world::VoxelHit;

    fn terrain() -> TerrainParams {
        TerrainParams {
            surface: VoxelId(1),
            soil: VoxelId(2),
            deep: VoxelId(3),
            floor_y: -6,
            ..TerrainParams::default()
        }
    }

    fn hit(pos: [i32; 3], normal: [i32; 3]) -> Option<VoxelHit> {
        Some(VoxelHit { pos, normal })
    }

    /// 挖：把命中格设为空气。
    #[test]
    fn breaking_clears_the_hit_voxel() {
        let t = terrain();
        let (pos, voxel) = resolve_edit(hit([3, 1, 4], [0, 1, 0]), EditAction::Break, &t)
            .expect("地面之上的方块该能挖");
        assert_eq!(pos, [3, 1, 4], "挖的是命中格本身");
        assert!(!voxel.is_solid(), "挖完该是空气");
    }

    /// 放：位置是 `命中格 + 法线`，**不是**命中格本身。
    ///
    /// 这条钉住 **R69**：放在面的外侧。若写成放在命中格，
    /// 玩家会在方块**内部**放东西 —— 看不见，而且看起来像没反应。
    #[test]
    fn placing_goes_along_the_face_normal() {
        let t = terrain();
        // 命中地面上方那格的**底面**（法线朝下），方块该放在它下面。
        let (pos, voxel) =
            resolve_edit(hit([5, 3, 5], [0, -1, 0]), EditAction::Place, &t).expect("该能放");
        assert_eq!(pos, [5, 2, 5], "位置 = 命中格 + 法线（朝下就是下面一格）");
        assert!(voxel.is_solid(), "放下去该是实心");

        // 侧面同理。
        let (pos, _) = resolve_edit(hit([5, 3, 5], [-1, 0, 0]), EditAction::Place, &t).unwrap();
        assert_eq!(pos, [4, 3, 5]);
    }

    /// **挖不穿世界底**。
    ///
    /// `floor_y` 以下全是深层方块；不挡的话玩家能一路挖下去，
    /// 看到清屏色且再也补不回来（那一层不在任何区块的可见面里）。
    #[test]
    fn breaking_stops_at_the_world_floor() {
        let t = terrain();
        // `floor_y = -6`，所以 y = -6 及以下不让挖。
        assert!(
            resolve_edit(hit([0, t.floor_y, 0], [0, 1, 0]), EditAction::Break, &t).is_none(),
            "地板那一层不该能挖"
        );
        assert!(
            resolve_edit(hit([0, t.floor_y - 5, 0], [0, 1, 0]), EditAction::Break, &t).is_none(),
            "地板以下更不该能挖"
        );
        // 地板**之上**一格可以挖。
        assert!(
            resolve_edit(hit([0, t.floor_y + 1, 0], [0, 1, 0]), EditAction::Break, &t).is_some(),
            "地板之上该能挖"
        );
    }

    /// 没命中就什么都不做（而不是"在原点放一个"）。
    #[test]
    fn no_hit_means_no_edit() {
        let t = terrain();
        assert!(resolve_edit(None, EditAction::Break, &t).is_none());
        assert!(resolve_edit(None, EditAction::Place, &t).is_none());
    }

    /// 放置的方块 ID 必须在方块表里（**不是空气**）。
    ///
    /// 这条防的是"改 `PLACE_BLOCK` 时手滑写成 0"—— 那会让右键看起来毫无反应。
    #[test]
    fn the_placed_block_is_not_air() {
        assert!(
            Voxel::solid(PLACE_BLOCK).is_solid(),
            "放置的方块 ID 不能是 0（空气），否则右键看起来没反应"
        );
    }

    /// **从"一条射线"到"store 真的被改、区块被标脏、脏消息发出去"**。
    ///
    /// ## 测的是哪一段，以及为什么是这一段
    ///
    /// 整条链路是：
    /// ```text
    ///   屏幕坐标 ──Bevy::viewport_to_world──► 射线 ──[下面这段]──► store 被改
    /// ```
    /// 左半是 Bevy 的视口数学（有它自己的测试）；**右半是我写的**：
    /// DDA 射线、护地板、`pos + normal`、写入并标脏。
    /// 它会**悄悄**错 —— 挖错格、放在方块内部、改了却忘了标脏 —— 全都不报错。
    #[test]
    fn a_ray_translates_into_a_real_edit_and_marks_the_chunk_dirty() {
        let terrain = terrain();

        // 在一个最小 App 里跑 —— `MessageWriter` 只能在系统里拿到
        // （它没有公开的构造函数）。
        let mut app = App::new();
        app.add_message::<voxelith_axiom::world::ChunkDirtyMessage>();
        app.init_resource::<VoxelStore>();
        app.insert_resource(voxelith_axiom::world::WorldConfig {
            terrain,
            ..default()
        });

        // 用一个系统做这几件事：射线 → 判定 → 写入。
        app.add_systems(
            bevy::app::Startup,
            |config: Res<voxelith_axiom::world::WorldConfig>,
             mut store: ResMut<VoxelStore>,
             mut dirty: MessageWriter<voxelith_axiom::world::ChunkDirtyMessage>| {
                // 从正上方朝下打：一定命中地表那一格，法线朝上。
                let hit = store
                    .raycast(&config.terrain, [0.0, 40.0, 0.0], [0.0, -1.0, 0.0], REACH)
                    .expect("正上方朝下打一定能命中地形");
                assert_eq!(
                    hit.pos[1],
                    config.terrain.surface_height(0, 0),
                    "该命中地表格（y = surface_height(0,0)）"
                );
                assert_eq!(hit.normal, [0, 1, 0], "从上方打来 ⇒ 命中顶面");

                let (pos, voxel) =
                    resolve_edit(Some(hit), EditAction::Break, &config.terrain).expect("地表能挖");
                assert!(
                    store.set_and_notify(pos, voxel, &config.terrain, &mut dirty),
                    "该真的改动（写同样的值会返回 false）"
                );

                assert!(!store.is_solid(pos, &config.terrain), "挖完那格该是空气");
                assert!(
                    store.is_edited(voxelith_axiom::world::ChunkPos::of(pos)),
                    "改完该记成'改动过的区块' —— 否则网格化不会重新派发，\
                     玩家会看到方块挖了却不消失"
                );
            },
        );

        app.update();

        // **脏消息必须发出去**：它是"重新网格化"的唯一触发。
        let mut messages = app
            .world_mut()
            .resource_mut::<bevy::ecs::message::Messages<voxelith_axiom::world::ChunkDirtyMessage>>(
            );
        let written: Vec<_> = messages.drain().collect();
        assert_eq!(written.len(), 1, "该恰好发一条脏消息");
    }

    /// 放置走的也是这条路：射线 → `pos + normal` → 真的多出一格实心方块。
    #[test]
    fn a_ray_can_place_a_block_on_top_of_the_surface() {
        let terrain = terrain();
        let mut store = VoxelStore::default();

        let hit = store
            .raycast(&terrain, [2.0, 40.0, 3.0], [0.0, -1.0, 0.0], REACH)
            .expect("该命中地形");
        let (pos, voxel) =
            resolve_edit(Some(hit), EditAction::Place, &terrain).expect("地表之上能放");

        assert_eq!(pos[1], hit.pos[1] + 1, "放在命中格的正上方一格");
        assert!(!store.is_solid(pos, &terrain), "放之前那里是空气");
        assert!(store.set(pos, voxel, &terrain), "该真的改动");
        assert!(store.is_solid(pos, &terrain), "放之后该是实心");
    }
}
