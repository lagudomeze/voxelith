# BRP HTTP 接口参考

[`SKILL.md`](SKILL.md) 是用法入口；本文件是细节参考（方法全表、返回形状、错误码、坑）。

## 1. 传输约定

- **JSON-RPC 2.0**，`POST` 到根路径（`http://127.0.0.1:15702`），**无路径后缀**
- 请求体：`{"jsonrpc":"2.0","id":1,"method":"...","params":{...}}`
- `params` 省略 = 无参方法
- 端口：`15702` 主 app；`15703` 渲染子 app

## 2. 方法全表（实测本项目：主 app 39 个）

### 读取

| 方法 | params | 返回 |
|---|---|---|
| `world.query` | `{data:{components:[全路径]},filter:{},strict:bool}` | 实体数组，每项 `{entity, components:{...}}` |
| `world.get_components` | `{entity:数字, components:[全路径]}` | `{components:{...}, errors:{...}}` |
| `world.list_components` | `{entity:数字}` | **裸字符串数组**（见下方坑 3） |
| `world.list_resources` | 无 | 资源类型全路径数组 |
| `world.get_resources` | `{resource:全路径}` | 资源值 |
| `registry.schema` | 无 | 对象，键 = 已注册反射类型全路径（本项目实测 1048 个） |
| `rpc.discover` | 无 | `{methods:[{name,...}]}` |
| `schedule.list` / `schedule.graph` | 无 / `{label}` | 调度信息 |

### ⚠️ `+watch` 方法不要用简单 HTTP 客户端调

`rpc.discover` 里还有三个 watching 方法：

```text
world.get_components+watch    world.list_components+watch    world.observe+watch
```

实测：它们会**保持长连接流式推送**，不是一问一答。用 `Invoke-RestMethod` / `Invoke-WebRequest`
调用会**一直挂住**（实测 `-TimeoutSec 5` 也无法中断，180 秒仍在运行），必须手动杀进程。

**要观察变化，改用轮询**：

```powershell
# 每 500ms 读一次，自己比对
while ($true) {
  & $s -Query "voxelith_axiom::atoms::health::Health" 6>&1 | Select-String 'current'
  Start-Sleep -Milliseconds 500
}
```

> 长连接方案（如 MCP 客户端）能正确处理 streaming，但本项目已统一走 HTTP 直连，
> 因此**用轮询代替 watch**。

### 写入

| 方法 | params | 说明 |
|---|---|---|
| `world.mutate_components` | `{entity, component, path, value}` | `path` 形如 `".current"`；空串表示整体替换 |
| `world.insert_components` | `{entity, components:{...}}` | 覆盖式插入 |
| `world.remove_components` | `{entity, components:[...]}` | 移除 |
| `world.spawn_entity` | `{components:{全路径:值}}` | 返回新实体 ID |
| `world.despawn_entity` | `{entity}` | 销毁 |
| `world.reparent_entities` | `{entities:[...], parent}` | 改父子 |
| `world.mutate_resources` | `{resource, path, value}` | 改资源字段 |
| `world.insert_resources` / `world.remove_resources` | `{resource, value}` / `{resource}` | 增删资源 |
| `world.trigger_event` | `{event:全路径, value}` | 触发**已注册反射**的 `Event` |
| `world.write_message` | `{message:全路径, value}` | 写入**已注册反射**的 `Message` |

### bevy_brp_extras（16 个，需 `BrpExtrasPlugin`）

| 方法 | params | 说明 |
|---|---|---|
| `brp_extras/screenshot` | `{path:"绝对路径"}` | 存 PNG；**path 必填** |
| `brp_extras/send_keys` / `type_text` | `{keys}` / `{text}` | 注入键盘 |
| `brp_extras/click_mouse` / `move_mouse` / `drag_mouse` / `scroll_mouse` | 见 `bevy_brp_extras` 的方法定义 | 注入鼠标 |
| `brp_extras/get_diagnostics` | 无 | FPS / 帧时间 |
| `brp_extras/set_window_title` | `{title}` | 改窗口标题 |
| `brp_extras/shutdown` | 无 | 优雅退出 |
| `brp_extras/agent_tools` | 无 | 面向 agent 的工具目录 |

## 3. 实测踩过的坑（含 PowerShell 侧）

### 坑 1：类型路径必须是全路径

`"Health"` → 报 `Component 'Health' isn't registered or used in the world`。
必须 `"voxelith_axiom::atoms::health::Health"`。用 `registry.schema` 查准确名字。

### 坑 2：`world.query` 默认静默返回空

不带 `strict` 时，类型名写错 → `{"result":[]}`，**不报错**。极易误判成"世界里没有"。
**建议一律加 `strict: true`**，会得到精确错误：

```json
{"error":{"code":-23402,"message":"Component `Health` isn't registered or used in the world"}}
```

### 坑 3：两个方法的返回形状不同（容易写错解析）

| 方法 | 返回 |
|---|---|
| `world.query` | `{"result":[{entity, components}]}` |
| `world.list_components` | `{"result":["类型A","类型B"]}` ← **裸数组**，没有 `components` 包装 |

### 坑 4：PowerShell 会把空数组展开成 `$null`

`Invoke-RestMethod` 对 `{"result":[]}` 返回的是**被展开**的空数组，`$null -ne $r` 判断会失效，
无法区分"0 条结果"与"调用失败"。`brp.ps1` 里用 `return , $resp.result` 抵消展开。

### 坑 5：PowerShell 局部变量名与开关参数撞名

变量名**大小写不敏感**。写 `$schema = ...` 会命中 `-Schema` 开关参数，
抛 `无法将 PSCustomObject 转换为 SwitchParameter`。
命名局部变量时避开 `$Status` / `$Discover` / `$Schema` / `$Query` / `$Mutate`。

### 坑 6：`world.list_components` 必须传 `entity`

不传 → `missing field 'entity'`。它列的是"这个实体上的组件"，不是全局组件表。

### 坑 7：截图报 `requires a primary window`

多半不是"没窗口"，而是**窗口尺寸为 0**（最小化）。
`bevy_brp_extras` 的判定是 `size.cmpgt(UVec2::ZERO).all()`。
读 `Window` 组件确认：`resolution.physical_width/height` 是否为 0、`position` 是否 `-32000,-32000`。
恢复窗口显示即可。不影响组件查询。

## 4. 错误码

| 码 | 含义 |
|---|---|
| `-32700` | JSON 解析失败 |
| `-32600` | 请求非法 |
| `-32601` | 方法不存在（先 `rpc.discover`） |
| `-32602` | 参数非法（如缺 `entity`） |
| `-32603` | 内部错误（如截图无主窗口） |
| `-23401` | 实体不存在 |
| `-23402` | 组件错误（未注册反射 / 类型名错） |
| `-23403` | 组件不存在于该实体 |
| `-23404` | 自我 reparent |
| `-23501` | 资源错误 |
| `-23502` | 资源不存在 |

## 5. 实体 ID

形如 `4294966862`（`Entity::to_bits`）。**当不透明数字原样传回**即可，实测 round-trip 正常。
不要做算术、不要自己拼。

要稳定定位实体，挂 `Name` 组件后用 `world.query` 按 `bevy_ecs::name::Name` 筛。

## 6. 一次性探测命令（不依赖 helper）

```powershell
# 服务在不在
Get-NetTCPConnection -LocalPort 15702 -State Listen

# 读所有 Health
$b = '{"jsonrpc":"2.0","id":1,"method":"world.query","params":{"data":{"components":["voxelith_axiom::atoms::health::Health"]},"filter":{},"strict":true}}'
Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post -Body $b -ContentType "application/json" | ConvertTo-Json -Depth 10

# 所有方法名
$b = '{"jsonrpc":"2.0","id":1,"method":"rpc.discover","params":{}}'
(Invoke-RestMethod -Uri "http://127.0.0.1:15702" -Method Post -Body $b -ContentType "application/json").result.methods.name
```