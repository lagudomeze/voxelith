# 命名规范（R28–R32、R111）

> 命名是架构的一部分，改名成本远低于长期忍受混淆的成本。（R111）

## 1. 组件：名词（R28）

| 好 | 坏 |
|---|---|
| `Resources` | `HasResources`、`ResourcesComponent` |
| `Stats` | `HasStats`、`StatData` |
| `Velocity` | `Moving`、`VelocityData` |
| `ChunkPos` | `ChunkPosition`（冗长）、`ChunkPosComp` |

规则：组件是"实体拥有的东西"，所以是名词。避免 `XxxComponent` 后缀——路径已经说明它是组件。

## 2. 事件：名词短语或过去式（R29）+ 后缀区分机制

**后缀即机制**（已确认）：Message 以 `Message` 结尾，Event 以 `Event` 结尾。

| 名字 | 机制 | 类型 |
|---|---|---|
| `CastRequest` | `Message`（请求型） | 名词短语（请求） |
| `DetachStatusMessage` | `Message` | 名词短语（指令） |
| `StatusTickMessage` | `Message` | 名词短语（状态变更） |
| `ChunkDirtyMessage` | `Message` | 名词短语（状态变更） |
| `DeathEvent` | `EntityEvent` | 名词短语（事实） |
| `VoxelClickedEvent` | `EntityEvent` | 名词短语（事实） |

规则：

- 广播型数据变更 → `Message` 后缀；语义是"请求某个模块处理"的 → `Request` 后缀（仍是 `Message`）。
- 绑定实体、走 observer 的 → `Event` 后缀。
- 已发生的事实可用过去式（`VoxelPlacedEvent`）。
- **不要**用动词原形当事件名（`DoAttack`、`ApplyCast` 是系统名，不是事件名）。
- **后缀写错 = 机制选错**：到底该用哪套机制，见 [bevy-events.md](bevy-events.md) §2 判定规则。

## 3. 系统：动词短语（R30）

| 好 | 坏 |
|---|---|
| `cast_requests` | `cast_system` |
| `apply_status_modifiers` | `modifiers` |
| `tick_actions` | `action_tick` |
| `purge_statuses` | `status_purge` |
| `resolve_status_ticks` | `status_processor` |

规则：系统是"每帧做的动作"，动词开头，全小写蛇形。

## 4. Plugin：`XxxPlugin`（R31）

`CombatPlugin`、`ActionPlugin`、`StatusPlugin`、`PhasePlugin`、`MonsterPlugin`、`VoxelRenderPlugin`。

拆不拆 Plugin 见 [events-and-plugins.md](events-and-plugins.md)（R39–R46）。

## 5. "造成一次伤害"的严格分工（R32）

现行设计里**没有 `Damage` 这个类型**：一次攻击就是一次 `CastRequest` → `Contest` → `Effect`。
名字分工仍然成立：

| 名字 | 语义 | 所在层 | 签名方向 |
|---|---|---|---|
| `resolve_actions` | 结算一次行动（含上下文/来源的完整动作） | L1 | 读 `ReadyToResolve` → 判定 → 执行 `Effect` |
| `modify_resource` | 纯数值增减（不关心为什么减） | L1 原语 | 只对 `Resources::modify` 出口做加减 |
| `resolve_status_ticks` / `resolve_status_detaches` | 跑状态队列里的时机效果 | L1 | 消费 `Message` 流，不做数值语义 |

判断口诀：**带原因的叫 resolve，纯数字的叫 modify/pool，遍历队列的叫 resolve_xxx_events 式的 `resolve_*`。**
（队列既可能是 `Message` 也可能是 `Event`，系统名不必带机制后缀。）

## 6. 其它命名约定（沿用规则精神）

| 对象 | 约定 | 例 |
|---|---|---|
| 模块 | 功能领域名词，蛇形 | `voxel_render`、`targeting` |
| crate | `voxelith-` 前缀，短横线 | `voxelith-axiom` |
| 资源 | 名词 | `VoxelWorld`、`ChunkStorage` |
| 布尔函数 | `is_` / `has_` / `can_` | `is_alive()`、`has_armor()` |
| 常量 | 大写下划线 | `CHUNK_SIZE`、`AIR` |
| 泛型/类型参数 | 单个大写字母或语义名 | `M: Message` |

## 7. 禁止的名字清单

- 模块名：`base`、`common`、`utils`、`helpers`、`misc`、`shared`（R21、R96）
- 拼写：`scense`（R104）、`seperate`、`recieve`
- 事件：`GameEvent` 大枚举（R35、R97）、`Event`、`Events` 这类无信息名
- 系统：`xxx_system` 后缀（Bevy 已说明是 system）、`handle_stuff`、`do_thing`
- 组件：`XxxData`、`XxxInfo`、`XxxComp`
