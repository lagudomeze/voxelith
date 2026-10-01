//! **程序化地牢布局**：算出一张"哪个模型放在哪个格子、朝哪边"的表。
//!
//! ## 分层（这一条决定了模块怎么切）
//!
//! | 层 | 做什么 | 在哪 |
//! |---|---|---|
//! | **布局** | 算出 `Vec<Placement>` —— **纯数据，不碰 Bevy** | 本模块 |
//! | **摆放** | 按表 spawn GLB 实体 | [`super::dungeon_build`] |
//!
//! 这么切的好处：布局的规则（房间不许重叠、走廊必须连通）可以**用普通测试验证**，
//! 不用起 App、不用加载素材。
//!
//! ## 为什么需要它（而不是继续用展示台）
//!
//! `model_gallery` 是**素材浏览器**：39 个模型一字排开，只为看清各自长什么样。
//! 它回答不了"**这些件拼起来是什么样**" —— 而那才是模块化套件的意义。
//!
//! 本模块把件拼成**房间 + 走廊 + 门**，于是拼接的正确性（对缝、朝向、门位）
//! 一眼可见。
//!
//! ## 坐标约定
//!
//! - **格坐标**（`IVec2`）：`x` 对应世界 `+X`，`y` 对应世界 `+Z`。
//! - 一格 = [`super::floor_grid::CELL_SPACING`] 世界单位。
//! - 模型放在格**中心**（`(x + 0.5)` 那类偏移在 [`Placement`] 里不带，
//!   由摆放层统一处理）。

use bevy::prelude::*;

/// 一个模型件在布局里扮演的角色。
///
/// **不写死具体文件名** —— 名字来自内容配置（`assets/data/dungeon.ron`），
/// 换一套 Kenney 素材只改配置、不改代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DungeonPart {
    /// 地板（每格一块）。
    Floor,
    /// 直墙（沿走廊方向的墙段）。
    Wall,
    /// 墙角。
    WallCorner,
    /// 门（嵌在墙的开口处）。
    Door,
    /// 楼梯。
    ///
    /// ⚠️ **目前布局还没放楼梯** —— 生成器只铺地板、走廊、墙、门。
    /// 保留这个变体是因为配置文件（`assets/data/dungeon.ron`）里已经写了
    /// `stairs` 字段，删掉它会让配置与实际能力脱节。
    /// 补楼梯时用这个变体即可。
    Stairs,
}

/// 一次摆放：把某个件放在某个格子上，并绕 Y 轴旋转。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// 是哪个件。
    pub part: DungeonPart,
    /// 格坐标（`x` → 世界 X，`y` → 世界 Z）。
    pub cell: IVec2,
    /// 绕 Y 轴的旋转（**90° 的整数倍**）。
    ///
    /// 取整倍数是有意的：模块化件都是轴对齐的，斜着放必然对不上缝。
    /// 存成整数圈数而不是四元数，方便测试比较。
    pub quarter_turns: u8,
}

impl Placement {
    /// 绕 Y 轴的四元数。
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 * self.quarter_turns as f32)
    }
}

/// 一个房间（轴对齐矩形，**含边界**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// 左下角格坐标。
    pub min: IVec2,
    /// 右上角格坐标（含）。
    pub max: IVec2,
}

impl Room {
    /// 宽（格）。
    ///
    /// 布局本身不直接用它（循环用的是 `min..=max`），
    /// 但它是**测试与人的读数**需要的基本属性，而且让 `Room` 自洽。
    #[allow(dead_code)]
    pub fn width(&self) -> i32 {
        self.max.x - self.min.x + 1
    }

    /// 高（格）。
    #[allow(dead_code)]
    pub fn height(&self) -> i32 {
        self.max.y - self.min.y + 1
    }

    /// 中心格。
    pub fn center(&self) -> IVec2 {
        (self.min + self.max) / 2
    }

    /// 两个房间的**内部**是否重叠。
    ///
    /// 判定用内部范围（各缩 1）而不是含边界范围 —— 因为房间之间有**共用墙**，
    /// 用含边界范围会把"紧挨着"误判成"重叠"。
    pub fn overlaps(&self, other: &Room) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
    }

    /// 是否包含某个格子（含边界）。
    #[allow(dead_code)]
    pub fn contains(&self, cell: IVec2) -> bool {
        cell.x >= self.min.x && cell.x <= self.max.x && cell.y >= self.min.y && cell.y <= self.max.y
    }

    /// 四条边上的格子（**不含四角重复**）。
    pub fn boundary(&self) -> Vec<IVec2> {
        let mut out = Vec::new();
        for x in self.min.x..=self.max.x {
            out.push(IVec2::new(x, self.min.y));
            out.push(IVec2::new(x, self.max.y));
        }
        for y in (self.min.y + 1)..self.max.y {
            out.push(IVec2::new(self.min.x, y));
            out.push(IVec2::new(self.max.x, y));
        }
        out
    }
}

/// 布局参数。
#[derive(Debug, Clone, Copy)]
pub struct DungeonSpec {
    /// 随机种子（**固定的 ⇒ 布局可复现**）。
    pub seed: u64,
    /// 要生成几个房间。
    pub rooms: usize,
    /// 房间的最小/最大边长（格）。
    pub room_size: (i32, i32),
    /// 整张地牢的范围（格，含边界）。
    pub bounds: (IVec2, IVec2),
}

impl Default for DungeonSpec {
    fn default() -> Self {
        Self {
            seed: 20_260_101,
            rooms: 5,
            room_size: (3, 6),
            // 范围要**放得进地板**：地板 96 格（-48..48）× 件格距 2.0
            // ⇒ 世界 ±48。地牢范围 ±10 格 = 世界 ±20，稳在地板内。
            bounds: (IVec2::new(-10, -10), IVec2::new(10, 10)),
        }
    }
}

/// **确定性**伪随机数（线性同余）。
///
/// ## 为什么不用 `rand`
///
/// 布局必须**可复现**：同一个种子、同一张图 —— 否则测试没法断言，
/// 排查时也没法复现用户看到的那张图。
/// 而这里只需要"够随机的整数"，不需要统计质量，所以不引依赖。
/// （`axiom` 有非 bevy 依赖的登记制，L2 虽然宽松，但能不引就不引。）
#[derive(Debug, Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // 0 会让 LCG 卡死在 0，避开它。
        Self(seed | 1)
    }

    fn next_u32(&mut self) -> u32 {
        // Numerical Recipes 的常数。
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    /// `lo..=hi` 的整数。
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % (hi - lo + 1) as u32) as i32
    }
}

/// 算出整张地牢。
///
/// ## 算法（刻意保持简单）
///
/// 1. 在范围内随机撒矩形房间，**拒绝与已有房间内部重叠的**；
/// 2. 每个房间铺满地板；
/// 3. 依次把相邻房间用 **L 形走廊**连起来（先横后竖）；
/// 4. 房间边界上放墙，墙的开口处放门；
/// 5. 走廊两侧放墙，走廊两端放楼梯。
///
/// 不追求"地牢有多好看"，追求**件与件之间严丝合缝** ——
/// 因为对缝才是模块化套件唯一会出错的地方。
pub fn generate(spec: &DungeonSpec) -> Vec<Placement> {
    let mut rng = Rng::new(spec.seed);
    let rooms = place_rooms(spec, &mut rng);

    let mut out = Vec::new();
    // ---- 地板：房间内**每一格**都铺 ----
    for room in &rooms {
        for x in room.min.x..=room.max.x {
            for y in room.min.y..=room.max.y {
                out.push(Placement {
                    part: DungeonPart::Floor,
                    cell: IVec2::new(x, y),
                    quarter_turns: 0,
                });
            }
        }
    }

    // ---- 走廊：房间 i 与 i+1 之间 ----
    let mut corridors: Vec<Vec<IVec2>> = Vec::new();
    for pair in rooms.windows(2) {
        let path = l_path(pair[0].center(), pair[1].center());
        corridors.push(path);
    }
    for path in &corridors {
        for cell in path {
            out.push(Placement {
                part: DungeonPart::Floor,
                cell: *cell,
                quarter_turns: 0,
            });
        }
    }

    // ---- 走廊两端各放一段楼梯 ----
    //
    // 楼梯放在走廊的**两端**（贴着房间那一格），下楼的意思 ——
    // 也让 `Stairs` 这个变体真的被用上（否则它就是死代码，
    // 而"配置里有、代码里没用"是最容易悄悄脱节的地方）。
    //
    // 朝向：楼梯要**顺着走廊方向**，所以按路径末端那一格到前一格的
    // 方向决定旋转。
    for path in &corridors {
        if path.len() < 2 {
            continue;
        }
        for (cell, next) in [
            // 起点朝里
            (path[0], path[1]),
            // 终点朝里
            (path[path.len() - 1], path[path.len() - 2]),
        ] {
            let step = next - cell;
            // 沿 +X ⇒ 转 90°；沿 -X ⇒ 转 270°；沿 +Z ⇒ 0°；沿 -Z ⇒ 180°。
            let quarter_turns = if step.x > 0 {
                1
            } else if step.x < 0 {
                3
            } else if step.y > 0 {
                0
            } else {
                2
            };
            out.push(Placement {
                part: DungeonPart::Stairs,
                cell,
                quarter_turns,
            });
        }
    }

    // ---- 墙 + 门 ----
    //
    // 房间边界上放墙；**走廊穿过的那一格**改放门。
    let door_cells: std::collections::HashSet<IVec2> =
        corridors.iter().flatten().copied().collect();
    for room in &rooms {
        for cell in room.boundary() {
            // 角上放墙角件，其余放直墙。**两者旋转规则不同**：
            // 直墙按"沿哪条轴"选，墙角按"在哪个角"选。
            let is_corner = (cell.x == room.min.x || cell.x == room.max.x)
                && (cell.y == room.min.y || cell.y == room.max.y);
            let (part, quarter_turns) = if door_cells.contains(&cell) {
                // 门嵌在墙的开口里 ⇒ 跟着那一侧的墙走。
                (DungeonPart::Door, wall_rotation(room, cell))
            } else if is_corner {
                (DungeonPart::WallCorner, corner_rotation(room, cell))
            } else {
                (DungeonPart::Wall, wall_rotation(room, cell))
            };
            out.push(Placement {
                part,
                cell,
                quarter_turns,
            });
        }
    }

    out
}

/// 随机撒房间，拒绝与已有房间重叠的。
fn place_rooms(spec: &DungeonSpec, rng: &mut Rng) -> Vec<Room> {
    let (lo, hi) = spec.bounds;
    let (min_side, max_side) = spec.room_size;
    let mut rooms: Vec<Room> = Vec::new();
    // 拒绝采样：试 `rooms * 40` 次，够用就停。
    //
    // 为什么不无限循环：范围小的时候可能**永远**找不到不重叠的位置，
    // 那会挂住。给个上限，拿不到就少几个房间 —— 那比死循环好。
    let mut attempts = spec.rooms * 40;
    while rooms.len() < spec.rooms && attempts > 0 {
        attempts -= 1;
        let w = rng.range(min_side, max_side);
        let h = rng.range(min_side, max_side);
        let x = rng.range(lo.x, hi.x - w);
        let y = rng.range(lo.y, hi.y - h);
        let candidate = Room {
            min: IVec2::new(x, y),
            max: IVec2::new(x + w, y + h),
        };
        // 房间之间至少留 1 格空隙，免得墙叠在一起。
        let padded = Room {
            min: candidate.min - IVec2::ONE,
            max: candidate.max + IVec2::ONE,
        };
        if rooms.iter().any(|r| r.overlaps(&padded)) {
            continue;
        }
        rooms.push(candidate);
    }
    rooms
}

/// L 形路径：先沿 X 走，再沿 Y 走。
fn l_path(from: IVec2, to: IVec2) -> Vec<IVec2> {
    let mut out = Vec::new();
    let (mut x, y) = (from.x, from.y);
    while x != to.x {
        out.push(IVec2::new(x, y));
        x += (to.x - x).signum();
    }
    let mut cy = y;
    while cy != to.y {
        out.push(IVec2::new(x, cy));
        cy += (to.y - cy).signum();
    }
    out.push(to);
    out
}

/// 直墙该绕 Y 轴转几个 90°。
///
/// ## 判据是"墙**沿着**哪条轴延伸"，不是"贴在哪条边"
///
/// 模块化墙件在 GLB 里是 **4 宽（沿 X）× 4.15 高**，厚度朝 `-Z`
/// （包围盒 `x: -2..2`、`z: -1.99..0`）。
/// 也就是说**未旋转时它沿着世界 X 延伸**。
///
/// 于是：
///
/// | 贴在哪条边 | 墙要沿着 | 旋转 |
/// |---|---|---|
/// | `-Z` / `+Z` 边（南北墙） | **X** | **0°** |
/// | `-X` / `+X` 边（东西墙） | **Z** | **90°** |
///
/// ## 第一版这里是错的
///
/// 我第一版按"贴 `-Z` 边就转 0°、贴 `-X` 边就转 90°、贴 `+Z` 边就转 180°"
/// 写 —— 那是把**墙的正面朝向**当成了判据，但南北墙**本来就已经沿着 X**，
/// 再转 90° 会让它**垂直于边**、插进房间里。
///
/// 这个 bug 在画面上表现为"南北墙横七竖八"，而不是"少了一堵墙" ——
/// 所以有测试钉住"南北墙不转、东西墙转 90°"。
fn wall_rotation(room: &Room, cell: IVec2) -> u8 {
    if cell.y == room.min.y || cell.y == room.max.y {
        // 南北墙：沿 X，不用转。
        0
    } else if cell.x == room.min.x || cell.x == room.max.x {
        // 东西墙：沿 Z，转 90°。
        1
    } else {
        // 不在边界上（墙角的兜底）—— 不该发生。
        0
    }
}

/// 墙角件该转几个 90°：按它**在哪个角**选，让两条边对上。
///
/// 墙角件在 GLB 里是 **2 宽 × 2 深**（`x: -2..0`、`z: -1.99..0`），
/// 即"占一格的四分之一角"。四个角各转 90°。
fn corner_rotation(room: &Room, cell: IVec2) -> u8 {
    let left = cell.x == room.min.x;
    let bottom = cell.y == room.min.y;
    match (left, bottom) {
        (true, true) => 0,   // 左下角
        (false, true) => 1,  // 右下角
        (false, false) => 2, // 右上角
        (true, false) => 3,  // 左上角
    }
}

#[cfg(test)]
#[path = "dungeon_layout_tests.rs"]
mod tests;
