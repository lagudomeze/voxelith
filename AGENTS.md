# AGENTS.md — Voxelith 项目入口

> 本文件是**入口索引**，不是规则全集。完整规则在 [`docs/`](docs/README.md)。
> 改动代码前：先读本文件 → 按任务读对应文档 → 动手 → 跑架构守卫（`scripts/arch-guard.ps1`）。

## 一句话架构

两个 crate、三层、单向依赖、事件通信：

```
voxelith-prime (L2 表现/内容)  ──依赖──►  voxelith-axiom (L0 atoms + L1 behaviors)
      可依赖完整 bevy                          只允许 bevy_ecs / bevy_app / bevy_reflect
```

**数据归数据，公式归公式，执行归执行，表现归表现。**（规则 112）
L0 是砖块，L1 是搭砖规则，L2 是画出来的建筑。（规则 113）

## 目标技术栈

Rust edition 2024 · Cargo workspace（不发布 crates.io）· Bevy `0.19` · crate 前缀 `voxelith-`。

## 不可违反的硬约束（详情见链接文档）

| # | 约束 | 详见 |
|---|---|---|
| 1 | 依赖单向：`voxelith-prime → voxelith-axiom`，禁止反向 | [architecture.md](docs/architecture.md) |
| 2 | `voxelith-axiom` 不得依赖完整 `bevy`，不得出现 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr` | [layers.md](docs/layers.md) |
| 3 | L0 组件只依赖自己，系统只查询自己；禁止渲染类型（`Sprite`/`Mesh`/`Text`/`Transform`/`Handle<Image>`） | [layers.md](docs/layers.md) |
| 4 | L1 修改 L0 数据必须通过事件；L1 不得生成渲染实体 | [layers.md](docs/layers.md) |
| 5 | L2 只读 L0/L1 数据，禁止直接改 `Health`/`Velocity`，禁止写战斗公式 | [layers.md](docs/layers.md) |
| 6 | 模块名按功能领域；禁止 `base` / `common` / `utils` / `helpers` | [naming.md](docs/naming.md) |
| 7 | 禁用 `scense` 拼写，统一 `presentation` | [naming.md](docs/naming.md) |
| 8 | 单文件 > 500 行必须拆分；单 crate > 5000 行考虑拆 crate | [file-layout.md](docs/file-layout.md) |
| 9 | 事件定义在发出它的模块；谁定义谁注册；禁止全局 `GameEvent` 大枚举；**广播用 `Message`，实体级立即反应用 `EntityEvent`** | [events-and-plugins.md](docs/events-and-plugins.md)、[bevy-events.md](docs/bevy-events.md) |
| 10 | `health` 是纯数值执行器，唯一能改 `Health` 的地方；不认识 `DamageType` | [combat.md](docs/combat.md) |
| 11 | `world`（数据）不加载纹理、不碰 `AssetServer`；渲染层不得自己发明位置 | [voxel-world.md](docs/voxel-world.md) |
| 12 | 禁止空壳 Plugin；Plugin ≠ 文件夹，代码多只拆 mod | [events-and-plugins.md](docs/events-and-plugins.md) |

## 提交前必跑

```powershell
pwsh ./scripts/arch-guard.ps1
```

等价手工检查（规则 90–93）：

```powershell
cargo check --workspace
cargo tree -p voxelith-axiom            # 不得出现 bevy_render/bevy_ui/bevy_sprite/bevy_pbr/voxelith-prime
tokei --sort code                       # 无文件 > 500 行
```

## 文档地图

| 文档 | 内容 |
|---|---|
| [docs/README.md](docs/README.md) | 文档索引 + 规则编号 → 文档映射 |
| [docs/architecture.md](docs/architecture.md) | crate 分层与依赖方向（规则 1–7、117） |
| [docs/layers.md](docs/layers.md) | L0/L1/L2 职责边界（规则 8–19、112–115） |
| [docs/file-layout.md](docs/file-layout.md) | 模块与文件组织、拆分阈值（规则 20–27） |
| [docs/naming.md](docs/naming.md) | 命名规范（规则 28–32、111） |
| [docs/events-and-plugins.md](docs/events-and-plugins.md) | 事件通信 + Plugin 使用（规则 33–46） |
| [docs/bevy-events.md](docs/bevy-events.md) | **Message vs Event**：选择依据、决策树、代码模板 |
| [docs/combat.md](docs/combat.md) | 战斗系统设计（规则 47–63） |
| [docs/voxel-world.md](docs/voxel-world.md) | 体素世界与渲染分离（规则 64–77） |
| [docs/debug-mcp.md](docs/debug-mcp.md) | **调试通道**：BRP / MCP 读组件、egui 检查器、无头模式、链接问题 |
| [docs/entities-and-rendering.md](docs/entities-and-rendering.md) | 玩家/怪物组合、渲染与 UI 边界（规则 78–89） |
| [docs/architecture-guard.md](docs/architecture-guard.md) | 架构守卫与提交检查（规则 90–95） |
| [docs/anti-patterns.md](docs/anti-patterns.md) | 反模式清单（规则 96–107） |
| [docs/workflow.md](docs/workflow.md) | 新增/修改功能工作流（规则 108–111） |
| [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md) | **待人工确认**的规则歧义与冲突 |
| [work/TODO.md](work/TODO.md) | 当前任务清单（初始为空） |

## 给 AI 的固定动作

1. 新增功能：判断层级 → 按领域建模块 → 定义组件/事件 → 写系统 → 在 Plugin 注册 → 跑守卫。（规则 108）
2. 修改代码：确认不越层 → 新事件在对应模块定义并注册 → L2 监听新事件更新表现。（规则 109）
3. 拿不定的架构歧义：**不要猜**，写入 [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md) 并询问。
4. 命名是架构的一部分，改名越早越便宜。（规则 111）
