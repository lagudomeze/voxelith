# Voxelith

体素游戏项目。当前状态：**整体重构的起点**——`main` 是无历史的空分支，重构前的完整实现与历史保存在 `legacy` 分支。

## 找东西

| 我要… | 去 |
| --- | --- |
| 弄清项目到底要做什么 | [docs/requirements/](docs/requirements/README.md) |
| 看怎么做（Bevy / Godot 两条线） | [docs/design/](docs/design/README.md) |
| 知道 AI agent 在本仓库怎么干活 | [AGENTS.md](AGENTS.md) |
| 看还有什么要做（执行任务） | [TODO.md](TODO.md) |
| 查素材出处与署名 | [docs/assets.md](docs/assets.md) |
| 翻重构前的旧代码 | `git switch legacy`（只读） |

## 代码

Cargo 工作区：crate 在 `crates/`，**内容（配置等）在根目录 `assets/`**（划分理由见 [docs/design/02-workspace-layout.md](docs/design/02-workspace-layout.md)）：

| crate | 放什么 | 依赖 |
| --- | --- | --- |
| `voxelith_core` | 与引擎无关的模型：可修正的量、六项属性、资源 | 只有 `bevy_ecs` |
| `voxelith_rules` | 规则层：有效值缓存的重算、派生（量之间的依赖）、内容参数加载（一个无泛型的 `RulesPlugin`） | core + `bevy_app` / `bevy_asset` / `serde` / `ron` |
| `voxelith_probes` | 探针与实验（验证引擎行为/设计假设，不属于游戏） |
| `voxelith_app` | 装配层：把模型挂上 Bevy App（含加载闸门） | `bevy_app` / `bevy_asset` / `bevy_scene` + core + rules |

```sh
cargo test                  # 全部测试
cargo run -p voxelith_app   # 自检：造一个英雄，打印最终属性与血量上限
```

## 文档约定

- 正文只写在专门文档里；README 与 AGENTS 保持薄，只做导航。
- 中文正文，英文目录与文件名。
- 需求：先澄清 → 再确认 → 后冻结。没被 `docs/requirements/` 记录并确认的东西，不进设计。

## 背景

- `legacy`（末次提交 `42015be`）：重构前的 Bevy 实现，含分层与自研体素/ECS 拆分，只作参照，不是新设计的前提。
- 本分支从零重建，不做增量演进。
