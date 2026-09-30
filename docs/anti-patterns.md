# 反模式清单（R96–R107）

按"违规写法 → 为什么错 → 正确做法"组织。每条都给出规则编号与触发检查方式。

## 1. 命名类

### ❌ 用 `base` / `common` / `utils` / `helpers` 当模块名（R96、R21）

**为什么错**：这类名字没有领域含义，会变成"什么都往里塞"的垃圾桶，最终所有模块互相 import。
**正确**：把内容拆到它真正的领域（`Lifetime` → 通用生命周期领域，R61、R63）。
**检查**：`arch-guard.ps1` 第 7 项。

### ❌ 使用 `scense` 命名（R104、R22）

**为什么错**：拼写错误 + 与 Bevy `Scene` 概念混淆。
**正确**：用 `presentation`（表现层）或 `levels` / `map`（关卡数据）。
**检查**：`arch-guard.ps1` 第 8 项。

## 2. 事件类

### ❌ 全局 `GameEvent` 大枚举（R97、R35）

```rust
// ❌
pub enum GameEvent { Damage(..), Heal(..), Move(..), ChunkDirty(..), OpenMenu(..) }
```

**为什么错**：所有模块被迫依赖一个上帝类型；新增事件要改中心文件；跨层边界彻底模糊。
**正确**：每个模块定义自己的 `Message`，谁定义谁注册（R33、R34）。

### ❌ 集中在一个 `events.rs` 注册所有事件（R36）

**为什么错**：与上一条同源，注册权集中在中心文件，模块自治失效。
**正确**：注册写在发出方模块的 `Plugin::build`。模块**内部**按文件切分可以，但注册点必须属于该模块。

## 3. 战斗类

### ❌ `health` 里写护甲 / 闪避 / 暴击逻辑（R98、R47）

**为什么错**：`health` 是纯数值执行器（R56）；公式进来后，改一次平衡要动 L0，L0 变成业务泥潭。
**正确**：`DamageType`、命中、闪避、减伤、暴击全在 L1（R48、R50、R54）。
**检查**：`health.rs` 中出现 `Armor` / `Dodge` / `Crit` / `DamageType` 即违规。

### ❌ 在 L2 算伤害 / 写公式（R17、R100）

**为什么错**：表现层算公式会导致两处逻辑不一致（UI 显示和实际结算不同步），且不可测试。
**正确**：L2 只监听 `DamageRequest` 做表现（R55）。

## 4. 分层污染类

### ❌ L0 / L1 依赖渲染 crate（R99、R5、R14）

```toml
# ❌ crates/voxelith-axiom/Cargo.toml
bevy = { workspace = true }
```

**为什么错**：`bevy` 会拖入 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr`，L0 物理隔离失效（R94）。
**正确**：bevy 家族只允许 `bevy_ecs` / `bevy_app` / `bevy_reflect` / `bevy_time`；
其余第三方工具 crate 走**登记制**——先在 [architecture.md](architecture.md) 的登记表登记（R5 修订，Q21/Q23）。
**检查**：`arch-guard.ps1` 第 2、5 项（第 5 项会同时检查"bevy 白名单"和"非 bevy 登记表"）。

### ❌ L2 直接改 L0/L1 核心数据（R100、R16）

```rust
// ❌ L2
fn cheat(mut q: Query<&mut Health>) { for mut h in &mut q { h.current = 9999; } }
```

**为什么错**：绕过了唯一写入口（R56），数值变化失去可追踪性。
**正确**：发 `ModifyHealthMessage`（治疗/伤害都走它）。

### ❌ 数据组件存 `Handle<Image>`（R103、R10）

**为什么错**：数据层与资产管线耦合，还会把 `bevy_asset` 拖进 `axiom`。
**正确**：数据层只存"用哪张贴图的标识"（如 `VoxelKind`），由 L2 映射到 `Handle<Image>`。

## 5. 渲染 / 数据边界类

### ❌ `world` 模块加载纹理（R101、R75、R89）

**为什么错**：数据模块碰 `AssetServer` 后无法单独测试，也无法在无渲染环境下运行（服务器/单测）。
**正确**：`world` 只提供 `VoxelKind` 等数据，`voxel_render` 加载图集（R73、R87）。

### ❌ 渲染模块自己计算位置（R107、R88、R76）

```rust
// ❌ voxel_render
let world_pos = chunk.x as f32 * 32.0 + local.x as f32;
```

**为什么错**：位置口径出现两份，改区块尺寸就会漏改，渲染与数据错位。
**正确**：位置换算由 `world` 暴露（如 `VoxelPos::to_world()`），渲染层只调用。

### ❌ `combat` 里写 `SpriteBundle` / `Mesh` / `Text`（R105、R58）

**为什么错**：战斗逻辑与表现强耦合，无法无头运行与测试。
**正确**：`combat` 只发事件，表现系统在 `presentation` 里响应。

## 6. 模块 / Plugin 组织类

### ❌ 空壳 Plugin（R102、R46）

```rust
// ❌ build() 里只有 add_plugins
pub struct CombatPlugin;
impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) { app.add_plugins((A, B)); }
}
```

**为什么错**：多一层无信息的间接层，读者要跳文件才知道真实结构。
**正确**：用元组 `(A, B)`（R44）；只有当需要配置/生命周期/对外发布时才建 Plugin（R41）。

### ❌ 为分类而创建空文件夹（R106、R24）

**为什么错**：目录结构变成"设想的架构"而非"实际的结构"，导航成本上升。
**正确**：需要时再建目录；目录内必须有真实内容。

## 7. 反模式速查表

| 规则 | 反模式 | 一句话纠正 |
|---|---|---|
| R96 | `base`/`common`/`utils`/`helpers` 模块名 | 用领域名，共享类型下沉 |
| R97 | 全局 `GameEvent` 大枚举 | 模块自治事件 |
| R98 | `health` 里写护甲/闪避/暴击 | 公式归 L1 |
| R99 | L0/L1 依赖渲染 crate | 只用三个 bevy 子 crate |
| R100 | L2 改核心数据 | 发事件 |
| R101 | `world` 加载纹理 | 渲染层管素材 |
| R102 | 空壳 Plugin | 用元组 |
| R103 | 数据组件存 `Handle<Image>` | 存 `VoxelKind` |
| R104 | `scense` 命名 | `presentation` / `levels` |
| R105 | `combat` 里写渲染类型 | 发事件给表现层 |
| R106 | 为分类建空文件夹 | 需要时再建 |
| R107 | 渲染模块自算位置 | 位置由 `world` 提供 |
