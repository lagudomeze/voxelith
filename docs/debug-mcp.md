# 调试通道：BRP / MCP 与 egui 检查器

> 目标：让 AI（MCP）与人（egui）都能看到 ECS 里的**组件数据**。
> 归属：全部属 L2（`voxelith-prime`），禁止下沉到 `voxelith-axiom`（R5、R91、R99）。

## 1. BRP 就是一个插件

不需要自己写系统。`bevy_brp_extras` 会自动补齐缺失的 `RemotePlugin`（BRP 方法集）与
`RemoteHttpPlugin`（HTTP 传输），用法只有一行：

```rust
app.add_plugins(BrpExtrasPlugin::default()); // 默认 127.0.0.1:15702
```

| 组件 | 提供什么 | 是否必需 |
|---|---|---|
| `RemotePlugin`（`bevy_remote`） | BRP 方法：`world.query`、`world.get_components`、`world.mutate_components` 等 | 读组件必需 |
| `RemoteHttpPlugin` | HTTP 传输层（默认端口 `15702`） | MCP 连接必需 |
| `BrpExtrasPlugin` | 自动补齐上面两个 + 截图 / 发按键 / 优雅关闭 | 便利，含上面全部 |

### 顺序要求：必须在 `DefaultPlugins` 之后

`BrpExtrasPlugin` 的输入注入系统依赖 `InputPlugin` / `WindowPlugin` 注册的消息
（`KeyboardInput`、`CursorMoved`、`MouseButtonInput`）。**用 `DefaultPlugins` 就自然满足。**

> 反面教训：曾用 `MinimalPlugins` 想省掉窗口，结果首帧 panic：
> `MessageReader<CursorMoved>::messages failed validation: Message not initialized`。
> 项目因此**统一使用 `DefaultPlugins`**，不再提供 `MinimalPlugins` 无头变体 ——
> 多一条分支只换来一个自己造出来的坑。

## 2. 项目里的接入点

全部收在一处，`main.rs` 保持一行：

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

| 函数 | 作用 |
|---|---|
| `install(app, port)` | 装 BRP + egui 检查器 |
| `DEFAULT_BRP_PORT` | `15702`，与 `bevy_brp_mcp` 约定一致 |

要换端口：`VoxelithPlugin::new().with_brp_port(9000)`。

## 3. MCP 配置

`bevy_brp_mcp` 已安装（`~/.cargo/bin/bevy_brp_mcp.exe`）。客户端配置：

```json
"mcpServers": {
  "brp": { "type": "stdio", "command": "bevy_brp_mcp", "args": [], "env": {} }
}
```

**版本对应**（务必对齐）：Bevy 0.19 ↔ `bevy_brp_mcp` 0.22.7 ↔ `bevy_brp_extras` 0.22.x。

### MCP 工具清单（实测 stdio 握手枚举，40+ 个）

MCP 服务器把 BRP 方法包装成带 JSON Schema 的工具，主要分四类：

| 类别 | 代表工具 | 作用 |
|---|---|---|
| **实体 / 组件** | `world_query`、`world_get_components`、`world_mutate_components`、`world_list_components`、`world_insert_components`、`world_spawn_entity`、`world_despawn_entity`、`world_reparent_entities` | 读改组件、增删实体 |
| **资源** | `world_get_resources`、`world_insert_resources`、`world_mutate_resources`、`world_list_resources`、`world_remove_resources` | 读改资源 |
| **监控** | `world_get_components_watch`、`world_list_components_watch`、`brp_list_active_watches`、`brp_stop_watch` | **订阅组件变化并写日志**，适合追数值变化 |
| **bevy_brp_extras** | `brp_extras_screenshot`、`brp_extras_send_keys`、`brp_extras_type_text`、`brp_extras_click_mouse`、`brp_extras_move_mouse`、`brp_extras_get_diagnostics`、`brp_extras_set_window_title`、`brp_extras_shutdown` | 截图、注入输入、FPS、优雅关闭 |
| **日志** | `brp_list_logs`、`brp_read_log`、`brp_delete_logs` | 读取 `cargo run` 的输出（MCP 启动的 app） |

> 重点：`brp_extras_screenshot` 是**验证画面内容**的唯一手段（见第 7 节的相机坑）。

### ⚠️ 配置加完之后必须重启会话

MCP 客户端在会话启动时读取 profile 配置。**会话中途添加 `mcp-brp` 不会让工具出现在当前
会话里**，需要重开对话（甚至重启 DSH）。判断方法：看当前可用工具里有没有 `brp_*` / `world_*`。

在重启前仍然可以直接打 HTTP（MCP 底层就是调它），效果等价：

```powershell
$body = '{"jsonrpc":"2.0","id":1,"method":"world.query","params":{"data":{"components":["voxelith_axiom::atoms::health::Health"]},"filter":{}}}'
Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post -Body $body -ContentType "application/json"
```

## 4. 跑起来

```powershell
cargo run -p voxelith-prime
```

启动后会生成一个名为 `debug-sample` 的演示实体（`Health` 100/100），让调试通道一启动就有东西可查。

## 5. 实测可用的 BRP 调用

以下均已在本项目**实测通过**。`$h` 为 Health 的完整类型路径：

```powershell
$h = "voxelith_axiom::atoms::health::Health"
$body = @{ jsonrpc="2.0"; id=1; method="world.query"; params=@{ data=@{ components=@($h) }; filter=@{} } } | ConvertTo-Json -Depth 8 -Compress
Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post -Body $body -ContentType "application/json"
```

实测响应：

```json
{"entity":4294967247,"components":{
  "voxelith_axiom::atoms::health::Health":{"current":100,"max":100}}}
```

| 方法 | 用途 | 参数要点 |
|---|---|---|
| `world.query` | **主力**：按组件筛选实体并读值 | `data.components` 传**完整类型路径** |
| `world.get_components` | 读指定实体的指定组件 | `entity` + `components` |
| `world.list_components` | 列出**某个实体**上已注册反射的组件 | **必须传 `entity`**，不是全局列表 |
| `world.mutate_components` | 改组件字段 | `entity` + `component` + `path`（如 `".current"`）+ `value` |
| `world.list_resources` | 列出已注册反射的资源 | 无参；返回 0 只说明没有 Reflect 资源 |
| `registry.schema` | 看所有可反射类型 | 无参 |
| `rpc.discover` | 列出全部可用方法 | 无参 |

实测写通道：`world.mutate_components` 把 `Health.current` 从 100 改成 42 成功，
重新读取确认 `{"current":42,"max":100}`。

实测方法总数：**39 个**（其中 `brp_extras/*` 16 个：截图、发按键、鼠标操作、优雅关闭等）。

### 三个实测踩过的坑

1. **类型路径必须是全路径**：传 `"Health"` 查不到，要传 `"voxelith_axiom::atoms::health::Health"`。
2. **`world.list_components` 需要 `entity`**：不传报 `missing field 'entity'`；
   它列的是"这个实体上的组件"，不是"全局组件表"。
3. **未注册反射的组件静默消失**：不报错，只是查不到（见第 6 节）。

## 6. 关键前提：组件必须可反射

BRP 用 `ReflectSerializer` 序列化组件。**未注册反射的组件不会报错，只会被跳过**。

好消息：Bevy 默认启用 `reflect_auto_register`，会自动注册所有 `#[derive(Reflect)]` 类型
**及其 type data**（含 `#[reflect(Component)]`），**不需要手写注册**。实测 `Health`
仅凭 `#[derive(Component, Reflect)]` + `#[reflect(Component)]` 即可被 `world.query` 读到。

防漏写靠一条守卫测试：
[`crates/voxelith-prime/tests/debug_visibility.rs`](../crates/voxelith-prime/tests/debug_visibility.rs)
断言组件的注册里带 `ReflectComponent`。新增可观察组件时照抄该测试。

## 7. egui 世界检查器（人用）

Bevy **没有**内置世界检查器（`bevy_dev_tools` 只有 `ci_testing` / `frame_time_graph` /
`schedule_data`），所以用 `bevy-inspector-egui 0.37`（对应 Bevy 0.19 + egui 0.34）：

```rust
app.add_plugins(EguiPlugin::default());        // 必须先
app.add_plugins(WorldInspectorPlugin::new());  // 后，顺序颠倒会 panic
```

它枚举世界里**已注册反射**的组件/资源并逐个展开编辑。

### ⚠️ 必须有相机，否则窗口纯黑且不报错

这是本项目**实际踩到**的坑，也是最容易漏的一个：egui 的 UI 要**经由相机**渲染到窗口。
场景里没有相机时，窗口是纯黑、检查器完全不显示，**进程正常、日志无警告**。

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

## 8. Windows 链接问题（`LNK1102`）

Bevy 全量 debug 构建 + egui 会让 MSVC 的 `link.exe` 撞
`LINK : fatal error LNK1102: out of memory`。这**不是物理内存不足**，
而是 `link.exe` 的 32 位地址空间限制（本机 27.6 GB 内存、21 GB 空闲仍会触发）。

两处配置已解决：

| 文件 | 配置 | 作用 |
|---|---|---|
| [`.cargo/config.toml`](../.cargo/config.toml) | `linker = "rust-lld.exe"` | 换用 Rust 自带 lld，绕开地址空间限制 |
| [`Cargo.toml`](../Cargo.toml) | `[profile.dev.package."*"] debug = "line-tables-only"` | 依赖只留行号级调试信息，链接压力骤降 |

## 9. 一页速查

| 我想做的 | 做法 |
|---|---|
| 让 MCP 读到组件 | `BrpExtrasPlugin`（一行，在 `DefaultPlugins` 之后） |
| 项目里的入口 | `main.rs` → `DefaultPlugins` + `VoxelithPlugin::new()` |
| 换 BRP 端口 | `VoxelithPlugin::new().with_brp_port(9000)` |
| 读组件值 | `world.query` + `data.components = ["完整::类型::路径"]` |
| 改组件值 | `world.mutate_components` + `path: ".字段"` |
| 人肉浏览组件 | egui 检查器（`EguiPlugin` → `WorldInspectorPlugin`）**+ 一个相机** |
| 窗口一片黑 | 检查有没有相机：egui 需要相机才渲染，且**不报错** |
| 组件查不到 | 检查是否 `#[derive(Reflect)]` + `#[reflect(Component)]` |
| 组件名是 `FEATURE_DISABLED` | 开 bevy 的 `debug` feature |
| 链接 OOM | rust-lld + line-tables-only |
| 想确认画面内容 | `brp_extras/screenshot` 抓 PNG（需传 `path`） |

### 截图报 `requires a primary window` 时看窗口尺寸

实测遇到：进程正常、BRP 可查询、`PrimaryWindow` 实体存在，但截图报
`Screenshot capture requires a primary window`。

根因不是"没有窗口"，而是**窗口尺寸为零**。`bevy_brp_extras` 的判定链是：

```rust
// live_target_size：窗口尺寸必须严格大于 0，否则视为不可截图
size.cmpgt(UVec2::ZERO).all().then_some(size)
```

实测读到的 `Window` 组件：

```json
{"position":{"At":[-32000,-32000]},
 "resolution":{"physical_width":0,"physical_height":0},
 "focused":false,"visible":true}
```

`physical_width/height = 0` + 位置 `-32000,-32000`（Windows 最小化窗口的典型坐标）
说明窗口处于**最小化/未真正显示**状态。恢复窗口（或把窗口前置）后截图即可成功。

用 BRP 自查窗口状态：

```powershell
# 找到 PrimaryWindow 实体后读它的 Window 组件
$body = '{"jsonrpc":"2.0","id":1,"method":"world.get_components","params":{"entity":4294967230,"components":["bevy_window::window::Window"]}}'
Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post -Body $body -ContentType "application/json"
```

> 注意：`Renderer` 相关的截图（`brp_extras/screenshot`）走的是主窗口渲染目标，
> 与"组件能否被查询"完全独立 —— 组件照样可读，只是截不到图。