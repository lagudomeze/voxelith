# 实体组合、渲染与 UI 边界（R78–R89）

## 第一部分：玩家 / 怪物 / 角色（R78–R82）

### 1. 玩家和怪物不是模块，而是组件的组合体（R78）

不存在 `player` 模块或 `monster` 模块。玩家是"一组组件的实体"，怪物也是。

```
玩家实体   = Actor + Player + Faction::Player + Resources + Stats + ActorTags + ... 
怪物实体   = Actor + Monster + Faction::Monster + Resources + Stats + ActorTags + MonsterDef + ...
```

> **三条正交的轴**（详见 [combat-design.md](combat-design.md) §4.0）：
>
> | 轴 | 组件 | 回答 |
> |---|---|---|
> | 引擎角色 | `Player` / `Monster` 标记 | 谁听输入、谁自己 tick |
> | 阵营 | `Faction` | 谁打谁 |
> | 特性 | `ActorTags` | 它是什么东西（亡灵 / 野兽 / 构装体） |
>
> "玩家侧的亡灵"就是同时带 `Player` + `Faction::Player` + `ActorTags([Undead])`——
> 三条轴各写各的，不需要任何特例。

### 2. 生成逻辑放独立 `spawn` 模块或各模块的工厂函数（R79）

- 通用生成放在 `spawn` 模块（工厂函数 + 预制体构建）。
- 领域特有生成放在该领域的工厂（例：`world::spawn_chunk_entities`）。

```rust
// ✅ 工厂函数风格：组合来自不同模块的组件
pub fn spawn_monster(commands: &mut Commands, pools: Resources, faction: Faction) -> Entity {
    commands.spawn((
        Actor,
        Monster,                     // 引擎角色：自己 tick
        faction,                     // 阵营：内容给的
        pools,                       // 来自 atoms::actor（池）
        Stats::default(),            // 来自 atoms::actor（属性）
        ActorTags(vec![ActorTag::Undead]), // 特性：跨阵营的性状
        // Sprite 由 L2 表现层追加，不在数据层生成
    )).id()
}
```

> 组件的**来源模块**：`Resources` / `Stats` / `Faction` 来自 `atoms::actor`，
> `Position` 来自 `movement`（待做），`Sprite` 来自 `presentation`。（R81）
> 数据层工厂**不要**顺手加 `SpriteBundle`——那是 L2 的职责（R14、R18）。

### 3. 共用系统，不同驱动源（R80）

玩家和怪物共用同一套 `movement`、`combat` 系统，区别只在**谁写驱动数据**：

| 实体 | 驱动源 | 写入的组件 |
|---|---|---|
| 玩家 | 输入系统（L2） | `PlayerInput` / `DesiredMove` |
| 怪物 | AI 系统（L2） | `AiBrain` 输出 / `DesiredMove` |

`movement` 系统只读"期望方向/速度"，不关心它是输入还是 AI 产生的。

### 4. `combat` 不认识"怪物"概念（R82）

`combat` 只认识 `Resources`、`Stats`、`Faction`、`ActorTags` 等组件。代码里出现
`Enemy` 这类"阵营类型判断"即违规——需要区分敌我时，用数据组件 `Faction` 表达；
需要区分"是什么东西"时，用数据组件 `ActorTags` 表达。

`Player` / `Monster` 标记是允许的例外，但它们只回答**引擎怎么驱动它**（听输入 / 自己 tick），
不回答"谁打谁"——所以 AI、相位、输入系统可以用它们，战斗判定不可以用。
**两者不可互相顶替**：用 `With<Monster>` 判"是不是敌人"是违规（友方 NPC 也会是 `Monster`，
而玩家侧的亡灵也仍然是 `Player`）。

## 第二部分：渲染与 UI 的边界（R83–R89）

### 1. 渲染 ≠ UI（R83）

两者是不同概念，禁止混在一个模块里当同义词使用。

| 概念 | 指什么 | 归属 | 举例 |
|---|---|---|---|
| 渲染（Rendering） | **3D 世界**表现 | `voxel_render` / `presentation`（R84） | 体素网格、材质、图集、粒子、3D 模型 |
| 贴图（Texture） | 渲染素材 | **渲染**，不是 UI（R85） | 方块图集、怪物贴图 |
| UI | **屏幕空间**元素 | `presentation`（R84、R86） | 血条、准星、菜单、HUD |

### 2. 素材与位置的职责（R87、R88）

- 素材由**渲染层**加载（`AssetServer` 只出现在 L2）。
- 位置由**数据层**提供（R76、R88）。
- 渲染层可以读位置，**绝不自己发明位置**（R88、R107）。

### 3. 数据模块禁止碰资产（R89、R101）

| 禁止在数据模块出现 | 原因 |
|---|---|
| `AssetServer`、`Handle<Image>`、`Handle<Mesh>` | 数据层与资产管线无关，且会被 `bevy_asset` 拖进 `axiom`，破坏 R5 |
| `SpriteBundle`、`Mesh3d`、`Text` | 渲染类型属于 L2（R14、R18） |
| `Transform` | 表现层组件，L0 禁用（R10） |

> 数据层需要表达空间位置时，使用**自己的数据组件**（如 `Position`、`ChunkPos`），由 L2 的同步系统映射到 `Transform`。这样 L0/L1 不依赖 `bevy_transform`。

### 4. 双向边界图

```
数据层（axiom：atoms / behaviors）
  Position / ChunkPos / Health / Velocity      ← 只存数据
        │  事件（Message）                    ← 唯一上行通道
        ▼
表现层（prime：voxel_render / presentation）
  Transform / Mesh / Sprite / UI / AssetServer  ← 只读数据、只生成表现
        │  输入 / 驱动（DesiredMove 等）
        ▼
  回到数据层：通过事件或驱动组件，绝不直接写核心数据（R16、R100）
```

### 5. 渲染/UI 自检清单

- [ ] 这段代码在画 3D 世界，还是在画屏幕 UI？归属模块对不对？（R83、R84）
- [ ] 我是不是在渲染代码里算了一个"世界坐标"？（R88 → 应该问 `world`）
- [ ] 我是不是在数据模块 import 了 `bevy_asset` / `Sprite` / `Transform`？（R89、R101）
- [ ] 我是不是在数据层工厂函数里顺手加了 `SpriteBundle`？（R14）
- [ ] UI 代码有没有被塞进 `voxel_render`？（R84）
