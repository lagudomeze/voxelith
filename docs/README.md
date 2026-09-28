# Voxelith 设计文档

本目录是 Voxelith 的项目规则全集，来源是 117 条讨论提炼规则。每条规则都能用编号 `R<n>` 追溯到具体文档，方便代码注释与 review 引用（例：`// R49`）。

入口：[`../AGENTS.md`](../AGENTS.md)。

## 规则编号 → 文档映射

| 规则编号 | 主题 | 文档 |
|---|---|---|
| 1–7 | Crate 分层 | [architecture.md](architecture.md) |
| 8–19 | 三层架构职责 | [layers.md](layers.md) |
| 20–27 | 模块与文件组织 | [file-layout.md](file-layout.md) |
| 28–32 | 命名规范 | [naming.md](naming.md) |
| 33–38 | 事件通信 | [events-and-plugins.md](events-and-plugins.md) |
| 39–46 | Plugin 使用 | [events-and-plugins.md](events-and-plugins.md) |
| 47–63 | 战斗系统设计 | [combat.md](combat.md) |
| 64–77 | 体素世界设计 | [voxel-world.md](voxel-world.md) |
| 78–82 | 玩家/怪物/角色 | [entities-and-rendering.md](entities-and-rendering.md) |
| 83–89 | 渲染与 UI 边界 | [entities-and-rendering.md](entities-and-rendering.md) |
| 90–95 | 架构守卫 | [architecture-guard.md](architecture-guard.md) |
| 96–107 | 反模式清单 | [anti-patterns.md](anti-patterns.md) |
| 108–111 | 工作流 | [workflow.md](workflow.md) |
| 112–117 | 总则 | [layers.md](layers.md)（112–115）、[architecture.md](architecture.md)（116–117） |

## 技术参考（非规则）

| 文档 | 内容 |
|---|---|
| [bevy-events.md](bevy-events.md) | **Message vs Event 的选择依据**：两套机制对比、决策树、本项目逐场景判定、代码模板、陷阱 |
| [debug-mcp.md](debug-mcp.md) | **调试通道**：BRP / MCP 读改组件的实测用法、egui 世界检查器、无头模式、`LNK1102` 链接问题 |

## 状态说明

- 文档中的规则**默认全部生效**，除非 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) 中标为"待确认"。
- 若某条规则与 Bevy 0.19 实际 API 冲突（例如 `add_event` 已更名），以"规则意图"为准，API 名称按 [events-and-plugins.md](events-and-plugins.md) 的写法。
- 新增规则：先在 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) 记录，确认后再落到对应文档并分配编号。

## 阅读顺序建议

1. 新人 / 新会话 AI：[architecture.md](architecture.md) → [layers.md](layers.md) → [anti-patterns.md](anti-patterns.md)
2. 写战斗相关代码：[combat.md](combat.md) → [events-and-plugins.md](events-and-plugins.md)
3. 写世界/渲染相关代码：[voxel-world.md](voxel-world.md) → [entities-and-rendering.md](entities-and-rendering.md)
4. 提交前：[architecture-guard.md](architecture-guard.md)
