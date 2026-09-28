---
name: brp-http
description: Read or change a running Voxelith Bevy app's live ECS state by POSTing JSON-RPC to the Bevy Remote Protocol HTTP endpoint (default 127.0.0.1:15702) — query component values, list components/resources, mutate or spawn, grab a screenshot. Use when you must inspect or verify what is actually in the running world instead of reading source. Works with a plain HTTP client, no extra tooling or network lookup.
whenToUse: The user asks what a live entity's component values are, wants to verify runtime state, mentions BRP or remote inspection, or you need ground truth about a running app.
---

# 通过 HTTP 直连 BRP 抓运行时数据

运行中的 app 会开一个 JSON-RPC 服务。**直接 POST 即可，不需要额外客户端、不需要联网查文档。**

## 1. 起 app 并确认在跑

```powershell
cargo run -p voxelith-prime              # 窗口模式（含 egui 检查器）
Get-NetTCPConnection -LocalPort 15702 -State Listen   # 有输出 = 服务已就绪
```

| 端口 | 用途 |
|---|---|
| `15702` | 主 app（组件/资源/截图，日常用这个） |
| `15703` | 渲染子 app（渲染线程内部状态，少用） |

窗口模式下 `cargo run` 会占用终端；要后台跑就 `Start-Process target\debug\voxelith-prime.exe`。

## 2. 请求形状（最小可复制模板）

JSON-RPC 2.0，POST 到根路径，**无路径后缀**：

```powershell
$body = @{
  jsonrpc = "2.0"; id = 1; method = "world.query"
  params = @{
    data   = @{ components = @("voxelith_axiom::atoms::health::Health") }
    filter = @{}
    strict = $true          # 关键：类型名写错时直接报错，而不是静默返回空
  }
} | ConvertTo-Json -Depth 10 -Compress

Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post `
  -Body $body -ContentType "application/json" -TimeoutSec 20
```

实测返回：

```json
{"entity":4294966862,"components":{
  "bevy_ecs::name::Name":"debug-sample",
  "voxelith_axiom::atoms::health::Health":{"current":100,"max":100}}}
```

失败会返回 `{"error":{"code":-32602,"message":"..."}}`——**先看 message，它通常直接说明原因**。

## 3. 常用方法

| 方法 | 用途 | params 要点 |
|---|---|---|
| `world.query` | **主力**：按组件筛实体并读值 | `data.components=[全路径]`，`filter={}` |
| `world.get_components` | 读某个实体的指定组件 | `entity` + `components` |
| `world.list_components` | 列出**某实体**上的组件 | **必须传 `entity`** |
| `world.mutate_components` | 改组件字段 | `entity`+`component`+`path`（如 `".current"`）+`value` |
| `world.spawn_entity` | 生成实体 | `components={ "全路径": {...} }` |
| `world.list_resources` | 列出可反射资源 | 无参 |
| `world.get_resources` | 读资源 | `resource`（全路径） |
| `registry.schema` | 列出所有可反射类型 | 无参 |
| `rpc.discover` | 列出所有可用方法（含 `brp_extras/*`） | 无参 |
| `brp_extras/screenshot` | 截图存 PNG | **必须传 `path`**（绝对路径） |

完整方法表、JSON 形状、错误码见同目录 [`reference.md`](reference.md)。

## 4. 实测踩过的坑

1. **类型路径必须是全路径**。传 `"Health"` 报
   `Component 'Health' isn't registered or used in the world`，
   要传 `"voxelith_axiom::atoms::health::Health"`。不确定就查 `registry.schema`。
2. **`world.query` 默认静默返回空**。不带 `strict` 时类型名写错 → `result: []`，
   **不报错**，极易误判成"世界里没有"。**一律加 `strict: true`**，错误会精确指出是哪个类型。
3. **`world.list_components` 返回裸数组**（`["类型A","类型B"]`），
   不像 `world.query` 那样有 `components` 包装；且**必须传 `entity`**（否则 `missing field 'entity'`）。
4. **未注册反射的组件静默消失**。BRP 用 `ReflectSerializer`，没注册反射的组件查不到。
   项目侧只需 `#[derive(Reflect)]` + `#[reflect(Component)]`
   （Bevy 默认的 `reflect_auto_register` 会自动注册，不必手写）。
5. **截图报 `requires a primary window` 时看窗口尺寸**。多半不是"没窗口"，而是窗口最小化：
   `Window.resolution.physical_width/height = 0`（坐标常为 `-32000,-32000`）。
   恢复窗口显示即可。这不影响组件查询。

## 5. helper 脚本（省掉 JSON 转义）

同目录 [`brp.ps1`](brp.ps1) 封装了常用操作，避免手写 `ConvertTo-Json` 出错。
以下命令**全部实测通过**：

```powershell
$s = ".dsh/skills/brp-http/brp.ps1"     # 相对项目根

& $s -Status                                                      # 服务在不在 + 注册的 Voxelith 类型
& $s -Discover                                                    # 列出所有方法（39 个）
& $s -Schema                                                      # 列出可反射类型（1048 个）
& $s -Query "voxelith_axiom::atoms::health::Health"               # 读所有 Health
& $s -Query "bevy_ecs::name::Name","voxelith_axiom::atoms::health::Health"
& $s -Entity 4294966862 -Components "bevy_ecs::name::Name"        # 读单实体的指定组件
& $s -ListComponents -Entity 4294966862                           # 列该实体的全部组件
& $s -Mutate -Entity 4294966862 `
      -Component "voxelith_axiom::atoms::health::Health" -Path ".current" -Value 42
& $s -Method "world.list_resources" -Params "{}"                  # 任意未封装方法的逃生口
```

`-Mutate` 会**自动回读确认**，改完立刻看到新值。查询默认带 `strict`，类型写错会直接报错并提示正确写法。

## 6. 实体 ID 当不透明数字

`world.query` 返回的 `entity` 形如 `4294966862`。**原样传回即可**（实测 round-trip 正常），
不要对它做算术、不要自己拼 ID。要定位"某个"实体，用 `Name` 组件筛：

```powershell
& $s -Query "bevy_ecs::name::Name"      # 列出带名字的实体（debug-sample / egui-camera / ...）
```

## 7. 相关文档

- [`docs/debugging.md`](../../../docs/debugging.md) —— **项目侧**接线：BRP 怎么接进代码、
  egui 检查器与相机前提、`LNK1102`
- [`reference.md`](reference.md) —— 本 skill 的细节：方法全表、返回形状、错误码、PowerShell 坑
- [`docs/bevy-events.md`](../../../docs/bevy-events.md) —— 用 BRP 观察 Message / Event 行为时的背景