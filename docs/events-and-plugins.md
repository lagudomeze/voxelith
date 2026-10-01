# 事件通信与 Plugin（R33–R46）

## 第一部分：事件通信（R33–R38）

### 1. 四条铁律

1. 事件定义在**发出它的模块**中。（R33）
2. 每个模块的 `Plugin::build` 注册**自己的**事件。（R34）
3. 禁止全局 `GameEvent` 大枚举。（R35、R97）
4. 禁止集中在一个 `events.rs` 里注册所有事件。（R36）
   - 允许在 `mod.rs` 中 `pub use` 事件方便外部使用。（R37）
5. 跨模块监听事件是允许的：读取方不关心事件谁注册。（R38）

### 2. 为什么事件归属"发出方"

事件是模块的**对外契约**：谁定义契约，谁负责注册、文档和演进。集中注册会导致：

- 所有模块都要 import 一个上帝模块（隐藏耦合）。
- 一个模块加事件，另一个模块要改代码（违反模块自治）。
- `GameEvent` 大枚举最终变成"什么都能塞"的垃圾桶。

### 3. Bevy 0.19 的两套机制（重要）

Bevy 0.19 把"事件"拆成了两套机制，规则里的"事件"**绝大多数**指 **Message（缓冲消息）**：

| 机制 | 定义 | 注册 | 读取 | 一句话 |
|---|---|---|---|---|
| **Message**（缓冲、拉取） | `#[derive(Message)]` | `app.add_message::<M>()`（R34 的落点） | `MessageReader<M>` / `MessageMutator<M>` | 帧内广播，可多读者、可并行、可被拦截改写 |
| **Event / EntityEvent**（观察者、推送） | `#[derive(Event)]` / `#[derive(EntityEvent)]` | `app.add_observer(...)` / `commands.observe(...)`，**不需要** `add_message` | observer 闭包 | 触发即执行，可绑定实体、可沿层级冒泡 |

> ⚠️ `.add_event::<T>()` 已更名为 `.add_message::<M>()`；`EventReader` 已更名为 `MessageReader`。
> 规则 34 的意图是"谁定义谁注册"，注册方法以版本 API 为准。

**选择标准、逐场景判定表、代码模板：见 [bevy-events.md](bevy-events.md)**（项目内的 Bevy 事件机制参考，含本项目的强制约定）。


### 4. 代码模板

**定义 + 注册（发出方模块内）** —— `behaviors/action/mod.rs`（现行示例）

> 下面的形状仍然是规则要求的样子（谁定义谁注册、消息只含数据）。
> 具体类型以代码为准：[`CastRequest`](../crates/voxelith-axiom/src/behaviors/action/mod.rs)
> 定义在 `behaviors::action`，由 `ActionPlugin` 注册（**R33**、**R34**）。

```rust
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// 释放请求：L2 输入 / AI → L1。（R33）
#[derive(Message, Debug, Clone, Copy)]
pub struct CastRequest {
    pub caster: Entity,
    pub skill: Entity,
    pub target: Option<Entity>,
}

/// 消费请求：校验 → 扣费 / 冷却 → 生成行动实例。
pub fn cast_requests(
    mut requests: MessageReader<CastRequest>,
    mut commands: Commands,
    mut params: CastParams,
) {
    for request in requests.read() {
        // …校验与生成…
    }
}

pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut App) {
        // 谁定义谁注册（R34）
        app.add_message::<CastRequest>();
    }
}
```

**跨模块监听（R38）** —— L2 无需知道谁注册了事件

```rust
fn update_hud(mut log: ResMut<CombatLog>, /* ... */) {
    for entry in log.entries() {
        // 表现层工作（只读）
    }
}
```

### 5. 事件设计检查清单

- [ ] 事件只含**数据**，不含渲染句柄（R9）。
- [ ] 事件名是名词短语或过去式（R29）。
- [ ] 事件定义在发出它的模块（R33），注册在同一个模块的 Plugin（R34）。
- [ ] 没有新增 `GameEvent`-style 枚举（R35）。
- [ ] 没有把新事件塞进某个集中 `events.rs` 注册（R36）。

## 第二部分：Plugin 使用（R39–R46）

### 1. 定位

- Plugin 是为了**模块化部署**，不是为了代码分类。（R39）
- 代码多，**只拆 mod，不一定要拆 Plugin**。（R40、R116）
- 只有当满足以下任一条时，才拆独立 Plugin：（R41）
  1. 需要**独立配置**（有配置结构体/泛型参数）。
  2. 需要**独立生命周期**（可单独启用/禁用、feature gate）。
  3. 需要**对外发布**（作为库被别人 `add_plugins`）。

### 2. 嵌套与组合

```rust
// 子插件可以嵌套，但不暴露给 main.rs（R42）
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((HealthPlugin, DamageFormulaPlugin, TargetingPlugin)); // 元组组合（R44）
    }
}
```

- `main.rs` 的 `add_plugins` 列表保持简洁：只列顶层插件。（R43）

```rust
fn main() {
    App::new()
        .add_plugins((DefaultPlugins, VoxelithPlugin)) // 顶层就这两个
        .run();
}
```

- 需要整体复用/配置的插件组，实现 `PluginGroup` trait。（R45）

```rust
pub struct VoxelithPlugins;

impl PluginGroup for VoxelithPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(WorldPlugin)
            .add(CombatPlugin)
            .add(PresentationPlugin)
    }
}
```

- **不为"组合"而创建空壳 Plugin**。（R46、R102）
  —— 一个 `build` 里只有 `app.add_plugins(...)` 且没有任何配置/生命周期的 Plugin，应该直接删掉，改成元组。

### 3. Plugin 拆与不拆的判据

| 情形 | 结论 |
|---|---|
| 模块有 200 行系统，无配置 | **不拆** Plugin，只拆 mod（R40） |
| 模块有 `CombatConfig { crit_rate: f32, ... }` | 拆 Plugin（独立配置，R41.1） |
| 子系统只在某 feature 下编译 | 拆 Plugin（独立生命周期，R41.2） |
| 这个模块要被别的项目 `add_plugins` | 拆 Plugin（对外发布，R41.3） |
| `build()` 里只有一行 `add_plugins((A, B))` | 删掉，改用元组（R46） |

### 4. `combat/plugin.rs` 的边界

`combat/plugin.rs` 只注册**逻辑系统**，不注册 UI 更新、动画播放、粒子特效。（R62）
表现相关系统的注册属于 `presentation` / `voxel_render` 的 Plugin。
