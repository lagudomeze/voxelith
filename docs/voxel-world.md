# 体素世界设计（R64–R77）

## 1. 数据模型

### 区块尺寸：32³（R64）

`CHUNK_SIZE = 32`，一个区块是 32×32×32 个体素。

```rust
pub const CHUNK_SIZE: i32 = 32;
pub const CHUNK_VOLUME: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) as usize;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos { pub x: i32, pub y: i32, pub z: i32 }
```

### 双层存储（R65）

| 层 | 内容 | 存储 |
|---|---|---|
| 程序化层 | 噪声函数生成的**初始**世界 | 无存储，按需计算（确定性函数） |
| 持久化层 | 玩家修改过的体素 | `HashMap<(ChunkPos, LocalPos), Voxel>` |

```rust
/// 只保存"与程序化结果不同"的体素，即玩家改动。
#[derive(Resource, Default)]
pub struct VoxelModifications {
    pub edits: HashMap<(ChunkPos, [u8; 3]), Voxel>,
}
```

### 查询顺序：先持久化，未命中回退程序化（R66）

```rust
pub fn get_voxel(world: &VoxelModifications, pos: VoxelPos) -> Voxel {
    let (chunk, local) = pos.split();
    if let Some(v) = world.edits.get(&(chunk, local)) {
        return *v;                       // 1. 玩家改动优先
    }
    procedural_voxel(pos)                // 2. 回退到噪声函数
}
```

**写入**永远落在持久化层：`edits.insert((chunk, local), voxel)`；程序化层是纯函数，不可写。

## 2. 交互（R67–R70）

### DDA 射线检测定位目标方块（R67）

体素射线用 **DDA（Digital Differential Analyzer）** 逐格步进，而不是细步长采样：无漏格、无重复、开销与距离成正比。

```rust
/// 返回命中的体素与命中面法线。
pub fn raycast_voxels(world: &VoxelWorld, origin: Vec3, dir: Dir3, max_dist: f32) -> Option<VoxelHit>;

pub struct VoxelHit { pub pos: VoxelPos, pub normal: IVec3 }
```

### 编辑操作（R68、R69）

| 操作 | 输入 | 结果 |
|---|---|---|
| 左键破坏 | `set_voxel(pos, Air)`（R68） | 目标格变为空气 |
| 右键放置 | `set_voxel(pos + normal, Solid(type))`（R69） | 相邻格放置方块 |

规则要点：放置位置由**命中面法线**决定（`pos + normal`），不是玩家的位置，也不是渲染层猜的。

### 修改后发出 `ChunkDirtyMessage`（R70）

```rust
#[derive(Message, Debug, Clone, Copy)]
pub struct ChunkDirtyMessage { pub chunk: ChunkPos }
```

- 事件定义在 `world` 模块（发出方）并注册在 `WorldPlugin`（R33、R34）。
- 渲染层监听它重建网格——这是数据层与渲染层之间**唯一**的触发通道。

## 3. 渲染（R71–R73）

### 贪婪网格化（R71）

同一面上的相邻同种方块合并成大四边形，显著减少顶点数与 draw call。

```rust
/// 由 world 数据 + 区块坐标生成网格数据（纯计算，不碰 AssetServer）。
pub fn greedy_mesh(chunk: ChunkPos, world: &VoxelWorld) -> MeshData;
```

### 异步执行，不阻塞主线程（R72）

网格化跑在 `AsyncComputeTaskPool`：

```rust
let task = AsyncComputeTaskPool::get().spawn(async move { greedy_mesh(chunk, world_snapshot) });
// 下一帧 poll 完成情况，完成后在主线程提交网格
```

要点：异步任务里只做**纯数据计算**，不访问 `AssetServer`、不创建实体（`Commands` 不能跨线程用）。

### 纹理图集减少 draw call（R73）

所有方块纹理打进一张图集，网格顶点带 UV 索引到图集格子。**图集加载属于渲染层**（R85、R87）。

## 4. 数据与渲染的严格分离（R74–R77、R101、R107）

### 两个模块，两条铁律

| 模块 | 层 | 可以做 | 绝对不可以 |
|---|---|---|---|
| `world` | L0/L1（数据） | 存体素、噪声生成、`get_voxel`/`set_voxel`、发 `ChunkDirtyMessage` | 加载纹理、碰 `AssetServer`（R75、R89、R101） |
| `voxel_render` | L2 | 贪婪网格化、图集、材质、网格提交 | 自己发明位置（R76、R88、R107） |

### 位置数据只由 `world` 提供（R76、R88）

渲染层调用 `world.get_voxel(...)` / 监听 `ChunkDirtyMessage` 拿到 `ChunkPos` 与体素数据，**从数据推导顶点**。
禁止在渲染层出现 `chunk_pos * CHUNK_SIZE + offset` 这类"自己算世界坐标"的代码——世界坐标换算必须由 `world` 暴露函数（例如 `VoxelPos::to_world()`）统一提供。

### `interaction` 只做射线检测和事件发送（R77）

选中高亮交给 `presentation`：

```rust
// ✅ interaction（L1/L2 边界模块）：只判定 + 发事件
#[derive(Message)]
pub struct VoxelSelectedMessage { pub hit: Option<VoxelHit> }
```

`presentation` 监听 `VoxelSelectedMessage` 画高亮线框。`interaction` 里不出现任何线框/材质代码。

## 5. 模块依赖图

```
world（数据，无渲染依赖）
  │  ChunkDirtyMessage / VoxelSelectedMessage
  ▼
voxel_render（网格化 + 图集 + 提交）      presentation（UI/高亮/其它 3D 表现）
  ▲                                          ▲
  └──────────── interaction（射线 + 事件）───┘
```

允许：`voxel_render → world`（读数据）、`presentation → interaction`（读事件）。
禁止：`world → voxel_render`（R74）、`voxel_render` 自造位置（R107）、`world` 碰 `AssetServer`（R75）。

## 6. 体素系统自检清单

- [ ] `world` 里有没有 `Handle<Image>` / `AssetServer` / `Mesh` 出现？（R75、R103）
- [ ] 新增位置计算是否写在渲染层？（R107 → 应该移到 `world`）
- [ ] 体素改动后是否发了 `ChunkDirtyMessage`？（R70）
- [ ] 网格化是否跑在 `AsyncComputeTaskPool`，异步任务里是否碰了资产或实体？（R72）
- [ ] 破坏/放置是否遵循 `set_voxel(pos, Air)` / `set_voxel(pos + normal, ...)`？（R68、R69）
- [ ] 射线是否用 DDA，而不是细步长采样？（R67）
