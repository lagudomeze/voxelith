# 调试通道：BRP / MCP / egui 检查器（项目侧）

> 本文说明**项目如何接入**调试通道 —— 代码接线、配置、以及为什么这么做。
> **怎么发查询**（HTTP 请求配方、方法全表、返回形状、PowerShell 坑）见 skill：
> [`.dsh/skills/brp-http/SKILL.md`](../.dsh/skills/brp-http/SKILL.md) 与
> [`reference.md`](../.dsh/skills/brp-http/reference.md)。
>
> 这样分工的原因：查询配方是**操作手册**（要能照着敲，含大量 PowerShell 细节），
> 接线与配置是**项目知识**（要解释设计取舍）。放一起会互相稀释，也容易漂移。
>
> 归属：全部属 L2（`voxelith-prime`），禁止下沉到 `voxelith-axiom`（R5、R91、R99）。

## 0. 三条通道

| 通道 | 给谁用 | 前提 |
|---|---|---|
| **BRP over HTTP** | AI / 脚本 | 组件可反射；`127.0.0.1:15702` |
| **MCP**（`bevy_brp_mcp`） | 支持 MCP 的客户端 | 同上，且**会话启动时**已配置（见 §3） |
| **egui 世界检查器** | 人 | 窗口 + 渲染 + **一个相机**（见 §5） |

三者读的是**同一份世界状态**；MCP 只是 BRP HTTP 的包装，不引入第二套数据。

## 1. 接入：BRP 就是一个插件

不需要自己写系统。`bevy_brp_extras` 会自动补齐缺失的 `RemotePlugin`（BRP 方法集）与
`RemoteHttpPlugin`（HTTP 传输），用法只有一行：

```rust
app.add_plugins(BrpExtrasPlugin::default()); // 默认 127.0.0.1:15702
```

| 组件 | 提供什么 | 是否必需 |
|---|---|---|
| `RemotePlugin`（`bevy_remote`） | BRP 方法：`world.query`、`world.get_components`、`world.mutate_components` 等 | 读组件必需 |
| `RemoteHttpPlugin` | HTTP 传输层 | 外部连接必需 |
| `BrpExtrasPlugin` | 自动补齐上面两个 + 截图 / 发按键 / 优雅关闭 | 便利，含上面全部 |

### 顺序要求：必须在 `DefaultPlugins` 之后

`BrpExtrasPlugin` 的输入注入系统依赖 `InputPlugin` / `WindowPlugin` 注册的消息
（`KeyboardInput`、`CursorMoved`、`MouseButtonInput`）。**用 `DefaultPlugins` 就自然满足。**

> 反面教训：曾用 `MinimalPlugins` 想省掉窗口，结果首帧 panic：
> `MessageReader<CursorMoved>::messages failed validation: Message not initialized`。
> 项目因此**统一使用 `DefaultPlugins`**，不再提供 `MinimalPlugins` 变体 ——
> 多一条分支只换来一个自己造出来的坑。

## 2. 项目里的接入点

全部收在一处，`main.rs` 保持三行：

[`crates/voxelith-prime/src/main.rs`](../crates/voxelith-prime/src/main.rs)

```rust
fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(VoxelithPlugin::new())   // 顶层插件，内部装配逻辑层 + 调试层
        .run();
}
```

[`crates/voxelith-prime/src/debug.rs`](../crates/voxelith-prime/src/debug.rs) 提供：

| 项 | 作用 |
|---|---|
| `install(app, port)` | 装 BRP + egui 检查器 |
| `DEFAULT_BRP_PORT` | `15702`，与 `bevy_brp_mcp` 约定一致 |
| `VoxelithPlugin::new().with_brp_port(p)` | 换端口 |

启动时会生成一个名为 `debug-sample` 的演示实体（`Health` 100/100），
让调试通道一启动就有东西可查。

## 3. MCP 客户端配置

`bevy_brp_mcp` 安装后（`cargo install bevy_brp_mcp`，落在 `~/.cargo/bin`）配置客户端：

```json
"mcpServers": {
  "brp": { "type": "stdio", "command": "bevy_brp_mcp", "args": [], "env": {} }
}
```

**版本对应**（务必对齐）：Bevy 0.19 ↔ `bevy_brp_mcp` 0.22.7 ↔ `bevy_brp_extras` 0.22.x。

### 工具清单（实测 stdio 握手枚举，40+ 个）

MCP 把 BRP 方法包装成带 JSON Schema 的工具：

| 类别 | 代表工具 | 作用 |
|---|---|---|
| **实体 / 组件** | `world_query`、`world_get_components`、`world_mutate_components`、`world_list_components`、`world_insert_components`、`world_spawn_entity`、`world_despawn_entity`、`world_reparent_entities` | 读改组件、增删实体 |
| **资源** | `world_get_resources`、`world_insert_resources`、`world_mutate_resources`、`world_list_resources`、`world_remove_resources` | 读改资源 |
| **监控** | `world_get_components_watch`、`world_list_components_watch`、`brp_list_active_watches`、`brp_stop_watch` | **订阅组件变化并写日志**，适合追数值变化 |
| **bevy_brp_extras** | `brp_extras_screenshot`、`brp_extras_send_keys`、`brp_extras_type_text`、`brp_extras_click_mouse`、`brp_extras_move_mouse`、`brp_extras_get_diagnostics`、`brp_extras_shutdown` | 截图、注入输入、FPS、优雅关闭 |
| **日志** | `brp_list_logs`、`brp_read_log`、`brp_delete_logs` | 读取 MCP 启动的 app 的输出 |

### ⚠️ 配置加完之后必须重启会话

MCP 客户端在**会话启动时**读取配置。**会话中途添加不会让工具出现在当前会话里**，
需要重开对话（甚至重启 DSH）。判断方法：看当前可用工具里有没有 `brp_*` / `world_*`。

在重启前可以直接打 HTTP（MCP 底层就是调它），效果等价 —— 见 §0 的 skill 链接。
这正是 `brp-http` skill 存在的理由：**它不依赖 MCP 是否已注入**。

## 4. 写代码时的约束：组件必须可反射

BRP 用 `ReflectSerializer` 序列化组件。**未注册反射的组件不会报错，只会被跳过** ——
表现为"组件明明挂上了，查询却查不到"。

好消息：Bevy 默认启用 `reflect_auto_register`，会自动注册所有 `#[derive(Reflect)]` 类型
**及其 type data**（含 `#[reflect(Component)]`），**不需要手写注册**。实测 `Health`
仅凭 `#[derive(Component, Reflect)]` + `#[reflect(Component)]` 即可被 `world.query` 读到。

**所以新增可观察组件时的唯一动作是：确保派生 `Reflect` 并标 `#[reflect(Component)]`。**

防漏写靠一条守卫测试：
[`crates/voxelith-prime/tests/debug_visibility.rs`](../crates/voxelith-prime/tests/debug_visibility.rs)
断言组件的注册里带 `ReflectComponent`。新增可观察组件时照抄该测试。

## 5. egui 世界检查器（人用）

Bevy **没有**内置世界检查器（`bevy_dev_tools` 只有 `ci_testing` / `frame_time_graph` /
`schedule_data`），所以用 `bevy-inspector-egui 0.37`（对应 Bevy 0.19 + egui 0.34）：

```rust
app.add_plugins(EguiPlugin::default());        // 必须先
app.add_plugins(WorldInspectorPlugin::new());  // 后，顺序颠倒会 panic
```

它枚举世界里**已注册反射**的组件/资源并逐个展开编辑。

### ⚠️ 必须有相机，否则窗口纯黑且不报错

这是本项目**实际踩到**的坑，也是最容易漏的一个：egui 的 UI 要**经由相机**渲染到窗口。
场景里没有相机时，窗口纯黑、检查器完全不显示，**进程正常、日志无警告**。

实测对比（用 `brp_extras/screenshot` 抓图后数颜色）：

| 状态 | 截图像素统计 |
|---|---|
| 无相机 | 1280×720 **只有 1 种颜色**（纯黑 `(0,0,0)`） |
| 加 `Camera2d` | **146 种颜色**，面板内容落在 x=16–366（左侧面板） |

所以 `debug.rs` 里补了一个 `Camera2d` 作为 egui 的渲染目标：

```rust
commands.spawn((Name::new("egui-camera"), Camera2d));
```

> 它**不是游戏相机**，只是 egui 的渲染目标。接入真实玩法渲染后若已有主相机，
> 应复用主相机，而不是保留两个。
>
> 已加回归测试 `inspector_spawns_a_camera`（去掉相机会立刻转红）。

### ⚠️ 组件名显示为 `FEATURE_DISABLED`

Bevy 的组件名来自 `DebugName`，只有启用 `debug` feature 才有实际字符串。
该 feature **不在** Bevy 默认 features 里，缺了它检查器看起来像空的。
已在 `crates/voxelith-prime/Cargo.toml` 显式开启：

```toml
bevy = { workspace = true, features = ["bevy_remote", "debug"] }
```

## 6. Windows 链接问题（`LNK1102`）

Bevy 全量 debug 构建 + egui 会让 MSVC 的 `link.exe` 撞
`LINK : fatal error LNK1102: out of memory`。这**不是物理内存不足**，
而是 `link.exe` 的 32 位地址空间限制（本机 27.6 GB 内存、21 GB 空闲仍会触发）。

两处配置已解决：

| 文件 | 配置 | 作用 |
|---|---|---|
| [`.cargo/config.toml`](../.cargo/config.toml) | `linker = "rust-lld.exe"` | 换用 Rust 自带 lld，绕开地址空间限制 |
| [`Cargo.toml`](../Cargo.toml) | `[profile.dev.package."*"] debug = "line-tables-only"` | 依赖只留行号级调试信息，链接压力骤降 |

## 7. 速查（项目侧）

| 我想做的 | 做法 |
|---|---|
| 让外部读到组件 | `BrpExtrasPlugin`（一行，在 `DefaultPlugins` 之后）；已封装进 `VoxelithPlugin` |
| 换 BRP 端口 | `VoxelithPlugin::new().with_brp_port(9000)` |
| 人肉浏览组件 | egui 检查器（`EguiPlugin` → `WorldInspectorPlugin`）**+ 一个相机** |
| 窗口一片黑 | 检查有没有相机：egui 需要相机才渲染，且**不报错** |
| 组件查不到 | 检查是否 `#[derive(Reflect)]` + `#[reflect(Component)]` |
| 组件名是 `FEATURE_DISABLED` | 开 bevy 的 `debug` feature |
| 链接 OOM | rust-lld + line-tables-only |
| **发查询 / 改组件 / 截图** | 见 [`.dsh/skills/brp-http/SKILL.md`](../.dsh/skills/brp-http/SKILL.md) |
| 方法全表 / 错误码 | 见 [`reference.md`](../.dsh/skills/brp-http/reference.md) |