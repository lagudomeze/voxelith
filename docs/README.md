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
| 47–63 | 战斗系统的分层与命名 | [combat.md](combat.md)、[combat-design.md](combat-design.md)（**结构以后者为准**） |
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
| [combat-design.md](combat-design.md) | **半即时战斗（唯一权威）**：设计原则、三大数据类别、引擎原语（`Value` / `Contest` / `Effect`）、定义与实例、`CombatPhase` 时间控制、反制机制、RON 配置、扩展手册、与旧方案的差异 |
| [combat-mechanics.md](combat-mechanics.md) | 旧战斗机制设计稿（**已被取代**）：不变量 I1–I6、伤害管线、抵抗、判定层、里程碑。保留取舍过程 |
| [numbers.md](numbers.md) | **数值设计（ToME4 参考）**：两个曲线原语（`Scale` / `Rescale`）、为什么是分段线性而不是对数、抗性/护甲/判定/状态/资源的设计取舍、落地优先级 |
| [bevy-events.md](bevy-events.md) | **Message vs Event 的选择依据**：两套机制对比、决策树、本项目逐场景判定、代码模板、陷阱 |
| [debugging.md](debugging.md) | **调试通道（项目侧）**：两条通道（BRP HTTP / egui）、代码接线、相机前提、`LNK1102`。查询配方见 skill |

## 状态说明

- 文档中的规则**默认全部生效**，除非 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) 中标为"待确认"。
- 若某条规则与 Bevy 0.19 实际 API 冲突（例如 `add_event` 已更名），以"规则意图"为准，API 名称按 [events-and-plugins.md](events-and-plugins.md) 的写法。
- 新增规则：先在 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) 记录，确认后再落到对应文档并分配编号。
- **战斗相关以 [combat-design.md](combat-design.md) 为准**；[combat.md](combat.md)（R47–R63）的分层与命名原则仍然有效，
  但其中"伤害管线 / 抵抗 / 判定层"的具体结构已被取代。

## 阅读顺序建议

1. 新人 / 新会话 AI：[architecture.md](architecture.md) → [layers.md](layers.md) → [anti-patterns.md](anti-patterns.md)
2. 写战斗相关代码：[combat-design.md](combat-design.md) → [bevy-events.md](bevy-events.md)
3. 写世界/渲染相关代码：[voxel-world.md](voxel-world.md) → [entities-and-rendering.md](entities-and-rendering.md)
4. 提交前：[architecture-guard.md](architecture-guard.md)
