//! **格子空间后端**：让 diesel 在不认识 `Vec3`/物理引擎的情况下，按格子做目标选择。
//!
//! diesel 把"空间"抽象成 [`SpatialBackend`]：位置、偏移、收集器、过滤器、运行时上下文
//! 都由游戏自己给。它自带的参考实现只有 `diesel_avian3d`（连续空间 + Avian 物理），
//! 而我们是**格子 + 体素**，所以按它预留的口子自己实现一份
//! （`SpatialBackend::Pos` 的文档原话就是 "Vec3, Vec2, `IVec2`, etc."）。
//!
//! ```text
//! Pos      = IVec2                格子坐标（由 L0 的 CellPos 映射过来）
//! Offset   = GridOffset           以格为单位的位移
//! Gatherer = GridGatherer         半径内收集（切比雪夫：八向移动的步数）
//! Filter   = GridFilter           排序 + 截断，并把命中数量写进 Scope
//! Context  = GridContext          查 `CellPos`
//! ```
//!
//! **收集与过滤分开**：收集只回答"范围内有谁"，过滤才做排序与数量裁剪。两者都是
//! **纯函数**（[`gather_cells`] / [`filter_cells`]），不碰 `World` 就能测——
//! 目标选择是最容易"悄悄选错"的地方，值得单独的确定性测试。
//!
//! `Scope` 里写两个键（diesel 约定的 `@scope` 命名空间，供 gauge 表达式读取）：
//!
//! | 键 | 含义 |
//! |---|---|
//! | `Distance@scope` | 这条目标离原点的格数（切比雪夫） |
//! | `Count@scope` | 本次选择一共命中几个（"每命中一个 +5%"这类表达式要它） |

use bevy::ecs::system::SystemParam;
use bevy::math::IVec2;
use bevy::prelude::*;
use bevy_diesel::backend::SpatialBackend;
use bevy_diesel::target::{Scope, Target};

use voxelith_axiom::atoms::grid::CellPos;

/// 格子空间后端（单元结构体：后端是**类型**，不是实例）。
#[derive(Debug, Clone, Copy, Default)]
pub struct GridBackend;

/// 以格为单位的位移。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridOffset {
    /// 位移（格）。
    pub cell: IVec2,
}

impl GridOffset {
    /// 构造。
    pub const fn new(dx: i32, dy: i32) -> Self {
        Self {
            cell: IVec2::new(dx, dy),
        }
    }
}

/// 收集器：以原点为心、`radius` 格（切比雪夫）内的所有带 [`CellPos`] 的实体。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridGatherer {
    /// 半径（格）。`0` = 只有原点那一格。
    pub radius: i32,
}

impl GridGatherer {
    /// 半径 `radius` 格。
    pub const fn radius(radius: i32) -> Self {
        Self { radius }
    }
}

/// 过滤器：按"离原点更近"排序，然后截断到 `max_targets`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridFilter {
    /// 最多保留几个（`None` = 不截断）。`Some(0)` 会一个都不留——它**不表示**"不限"。
    pub max_targets: Option<usize>,
}

impl GridFilter {
    /// 最多保留 `n` 个。
    pub const fn at_most(n: usize) -> Self {
        Self {
            max_targets: Some(n),
        }
    }
}

/// 运行时上下文：查格子坐标。
///
/// 两个查询都是**只读**的，所以可以并存：`by_entity` 用来按实体点名查，
/// `all` 用来连同实体一起遍历（`Query<&T>` 本身不产出 `Entity`）。
#[derive(SystemParam)]
pub struct GridContext<'w, 's> {
    by_entity: Query<'w, 's, &'static CellPos>,
    all: Query<'w, 's, (Entity, &'static CellPos)>,
}

impl GridContext<'_, '_> {
    /// 某个实体的格坐标。
    pub fn cell_of(&self, entity: Entity) -> Option<IVec2> {
        self.by_entity.get(entity).ok().map(to_ivec2)
    }

    /// 所有 (实体, 格坐标)。
    pub fn cells(&self) -> impl Iterator<Item = (Entity, IVec2)> + '_ {
        self.all.iter().map(|(entity, pos)| (entity, to_ivec2(pos)))
    }
}

fn to_ivec2(pos: &CellPos) -> IVec2 {
    IVec2::new(pos.x, pos.y)
}

/// 切比雪夫距离（格）：八向移动下的步数。
fn chebyshev(a: IVec2, b: IVec2) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// **纯函数**：在 `(实体, 格坐标)` 集合里收集半径内的目标。
///
/// - `exclude`：施法者自己（永远不进结果）。
/// - 半径按**切比雪夫**算；要改成四向口径，只要换成 `manhattan`。
pub fn gather_cells(
    cells: impl Iterator<Item = (Entity, IVec2)>,
    origin: IVec2,
    gatherer: &GridGatherer,
    exclude: Entity,
) -> Vec<(Target<IVec2>, Scope)> {
    let radius = gatherer.radius.max(0);
    let mut out = Vec::new();
    for (entity, cell) in cells {
        if entity == exclude {
            continue;
        }
        let steps = chebyshev(origin, cell);
        if steps > radius {
            continue;
        }
        out.push((
            Target::entity(entity, cell),
            vec![("Distance@scope", steps as f32)],
        ));
    }
    out
}

/// **纯函数**：按距离排序 + 截断，并把命中数量写进每条目标的 `Count@scope`。
///
/// 排序是**稳定**的（同距保持收集顺序），所以同样的世界状态每次都选出同一批目标——
/// 目标选择不能依赖 `Query` 的遍历顺序。
pub fn filter_cells(
    mut targets: Vec<(Target<IVec2>, Scope)>,
    filter: &GridFilter,
    origin: IVec2,
) -> Vec<(Target<IVec2>, Scope)> {
    targets.sort_by_key(|(target, _)| chebyshev(origin, target.position));
    if let Some(max) = filter.max_targets {
        targets.truncate(max);
    }
    let count = targets.len() as f32;
    for (_, scope) in targets.iter_mut() {
        scope.push(("Count@scope", count));
    }
    targets
}

impl SpatialBackend for GridBackend {
    type Pos = IVec2;
    type Offset = GridOffset;
    type Gatherer = GridGatherer;
    type Filter = GridFilter;
    type Context<'w, 's> = GridContext<'w, 's>;

    /// 后端插件：core + **三个 core 没注册的、带 `Context` GAT 的系统**。
    ///
    /// diesel 把这三个留给后端，是因为它们的参数里含 `B::Context`，
    /// 只有后端自己知道怎么取（它的文档原话：*"Backend-specific systems
    /// (`propagate_observer`, `spawn_system`, etc.) must be registered by the
    /// backend's `plugin()` override due to the Context GAT"*）。
    fn plugin() -> impl Plugin
    where
        Self: Sized,
    {
        GridBackendPlugin
    }

    fn apply_offset(
        _ctx: &mut Self::Context<'_, '_>,
        pos: Self::Pos,
        offset: &Self::Offset,
    ) -> Self::Pos {
        pos + offset.cell
    }

    fn distance(a: &Self::Pos, b: &Self::Pos) -> f32 {
        chebyshev(*a, *b) as f32
    }

    fn position_of(ctx: &Self::Context<'_, '_>, entity: Entity) -> Option<Self::Pos> {
        ctx.cell_of(entity)
    }

    fn gather(
        ctx: &mut Self::Context<'_, '_>,
        origin: Self::Pos,
        gatherer: &Self::Gatherer,
        exclude: Entity,
    ) -> Vec<(Target<Self::Pos>, Scope)> {
        // 先把要遍历的东西收成 Vec，避开"同时借 ctx 两次"的问题。
        let cells: Vec<(Entity, IVec2)> = ctx.cells().collect();
        gather_cells(cells.into_iter(), origin, gatherer, exclude)
    }

    fn apply_filter(
        _ctx: &mut Self::Context<'_, '_>,
        targets: Vec<(Target<Self::Pos>, Scope)>,
        filter: &Self::Filter,
        _invoker: Entity,
        origin: Self::Pos,
    ) -> Vec<(Target<Self::Pos>, Scope)> {
        filter_cells(targets, filter, origin)
    }

    fn insert_position(
        commands: &mut EntityCommands,
        _ctx: &Self::Context<'_, '_>,
        pos: Self::Pos,
        _parent: Option<Entity>,
    ) {
        commands.insert(CellPos::new(pos.x, pos.y));
    }
}

/// 格子后端的插件：diesel 的 core + 三个后端专属系统。
///
/// 相位选择（顺序是契约，写错的症状是"技能不触发"而不是报错）：
///
/// ```text
/// GearboxSchedule
///   EntryPhase            状态进入 → 挂上 `Active`
///   DieselSet::Propagation
///       go_off_on_entry   新增 `Active` 且带 `GoOffConfig` → 解析目标 → 发 GoOffOrigin
///       propagate_system  读 GoOffOrigin → 走 SubEffects 树 → 写 PendingGoOffs
///   DieselSet::TargetFilter   （core 的 flush_go_offs 在这里消费 PendingGoOffs）
///   DieselSet::Effects
///       spawn_system      SpawnConfig → 生成子实体
/// ```
///
/// `go_off_on_entry` 与 `propagate_system` **同帧**靠 `.chain()` 连起来：
/// 消息写入后，同帧靠后的读取者能立刻看到它（Bevy 的消息是双缓冲，不是"隔帧"）。
pub struct GridBackendPlugin;

impl Plugin for GridBackendPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(<GridBackend as SpatialBackend>::plugin_core());

        // 传播链里读 `PeriodicTick`，所以**后端自己兜底注册它**：
        // 只装后端（不装 `PeriodicPlugin`）时也不能因为"消息没初始化"而拒绝调度。
        // 注册是幂等的；真正**产生** tick 的是 `PeriodicPlugin` 的时间系统。
        app.add_message::<crate::periodic::PeriodicTick>();

        app.add_systems(
            bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::effect::go_off_on_entry::<GridBackend>,
                // 周期效果（DoT）：到点把"该响了"补成 `GoOffOrigin`——与上一行同一条路，
                // 所以叶子效果系统（`instant_set_system` 等）一行都不用改。
                crate::periodic::fire_periodic_effects::<GridBackend>,
                bevy_diesel::pipeline::propagate_system::<GridBackend>,
            )
                .chain()
                .in_set(bevy_diesel::DieselSet::Propagation),
        );

        app.add_systems(
            bevy_gearbox::GearboxSchedule,
            bevy_diesel::spawn::spawn_system::<GridBackend>.in_set(bevy_diesel::DieselSet::Effects),
        );

        // 持续型修饰符（装备 / 增益）：**必须由后端注册**——它带 `B::Context`，
        // 泛型函数只能在定义 `B` 的地方单态化（见 diesel `gauge_ext/mod.rs` 的注释）。
        app.add_systems(
            Update,
            bevy_diesel::gauge_ext::modifiers::sustained_modifier_apply::<GridBackend>
                .in_set(bevy_diesel::gauge_ext::SustainedModifierSet),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    fn target_at(cell: IVec2) -> (Target<IVec2>, Scope) {
        (Target::position(cell), vec![("Distance@scope", 0.0)])
    }

    #[test]
    fn gather_takes_only_what_is_within_the_radius() {
        let caster = Entity::from_raw_u32(1).unwrap();
        let inside = Entity::from_raw_u32(2).unwrap();
        let outside = Entity::from_raw_u32(3).unwrap();
        let cells = vec![
            (caster, IVec2::ZERO),
            (inside, IVec2::new(3, 3)),  // 切比雪夫 3：在半径 3 内
            (outside, IVec2::new(4, 0)), // 切比雪夫 4：出界
        ];
        let out = gather_cells(
            cells.into_iter(),
            IVec2::ZERO,
            &GridGatherer::radius(3),
            caster,
        );

        assert_eq!(out.len(), 1, "自己要被排除，出界的不要");
        assert_eq!(out[0].0.entity, Some(inside));
        assert_eq!(out[0].0.position, IVec2::new(3, 3));
        assert_eq!(out[0].1, vec![("Distance@scope", 3.0)]);
    }

    #[test]
    fn a_zero_radius_gatherer_is_only_the_origin_cell() {
        let caster = Entity::from_raw_u32(1).unwrap();
        let neighbour = Entity::from_raw_u32(2).unwrap();
        let on_the_cell = Entity::from_raw_u32(3).unwrap();
        let cells = vec![
            (caster, IVec2::ZERO),
            (neighbour, IVec2::new(1, 0)),
            (on_the_cell, IVec2::ZERO),
        ];
        let out = gather_cells(
            cells.into_iter(),
            IVec2::ZERO,
            &GridGatherer::default(),
            caster,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.entity, Some(on_the_cell));
    }

    #[test]
    fn filter_sorts_by_distance_and_caps_the_count() {
        let far = target_at(IVec2::new(5, 0));
        let near = target_at(IVec2::new(1, 0));
        let mid = target_at(IVec2::new(3, 0));
        let out = filter_cells(vec![far, near, mid], &GridFilter::at_most(2), IVec2::ZERO);

        let positions: Vec<IVec2> = out.iter().map(|(target, _)| target.position).collect();
        assert_eq!(
            positions,
            vec![IVec2::new(1, 0), IVec2::new(3, 0)],
            "近的两个留下，且按距离有序"
        );
        assert!(
            out.iter()
                .all(|(_, scope)| scope.contains(&("Count@scope", 2.0))),
            "每条都带上了命中数量：表达式才写得出\"每命中一个 +5%\""
        );
    }

    #[test]
    fn filter_without_a_cap_keeps_everything_but_still_sorts() {
        let out = filter_cells(
            vec![target_at(IVec2::new(9, 9)), target_at(IVec2::new(1, 1))],
            &GridFilter::default(),
            IVec2::ZERO,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0.position, IVec2::new(1, 1));
    }

    #[test]
    fn backend_distance_uses_the_same_metric_as_gather() {
        let a = IVec2::new(-2, 0);
        let b = IVec2::new(1, 3);
        assert_eq!(<GridBackend as SpatialBackend>::distance(&a, &b), 3.0);
    }

    #[test]
    fn offsets_are_applied_in_cells() {
        let mut world = World::new();
        let moved = world
            .run_system_once(|mut ctx: GridContext| {
                <GridBackend as SpatialBackend>::apply_offset(
                    &mut ctx,
                    IVec2::new(2, 2),
                    &GridOffset::new(-1, 3),
                )
            })
            .expect("系统跑得起来");
        assert_eq!(moved, IVec2::new(1, 5));
    }

    #[test]
    fn the_context_reads_real_cell_positions_out_of_the_world() {
        let mut world = World::new();
        let caster = world.spawn(CellPos::new(0, 0)).id();
        let near = world.spawn(CellPos::new(2, 0)).id();
        let far = world.spawn(CellPos::new(9, 9)).id();

        let (found, distance) = world
            .run_system_once(move |mut ctx: GridContext| {
                let found = <GridBackend as SpatialBackend>::gather(
                    &mut ctx,
                    IVec2::ZERO,
                    &GridGatherer::radius(3),
                    caster,
                );
                let distance = <GridBackend as SpatialBackend>::position_of(&ctx, far);
                (found, distance)
            })
            .expect("系统跑得起来");

        assert_eq!(found.len(), 1, "半径 3 内只有 near");
        assert_eq!(found[0].0.entity, Some(near));
        assert_eq!(distance, Some(IVec2::new(9, 9)), "position_of 能点名查");
    }

    #[test]
    fn the_backend_boots_diesels_whole_schedule() {
        // 这一条是"格子后端真的接上了"的凭据：core（状态机 / 属性图 / 消息 / 传播图）
        // 加三个后端专属系统，全部用 `Pos = IVec2` 实例化，跑一帧不炸。
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            <GridBackend as SpatialBackend>::plugin(),
        ));
        app.update();
        app.update();
    }
}
