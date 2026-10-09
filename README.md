# Voxelith

体素游戏项目。当前状态：**整体重构的起点**——`main` 是无历史的空分支，重构前的完整实现与历史保存在 `legacy` 分支。

## 找东西

| 我要… | 去 |
| --- | --- |
| 弄清项目到底要做什么 | [docs/requirements/](docs/requirements/README.md) |
| 看怎么做（Bevy / Godot 两条线） | [docs/design/](docs/design/README.md) |
| 知道 AI agent 在本仓库怎么干活 | [AGENTS.md](AGENTS.md) |
| 翻重构前的旧代码 | `git switch legacy`（只读） |

## 文档约定

- 正文只写在专门文档里；README 与 AGENTS 保持薄，只做导航。
- 中文正文，英文目录与文件名。
- 需求：先澄清 → 再确认 → 后冻结。没被 `docs/requirements/` 记录并确认的东西，不进设计。

## 背景

- `legacy`（末次提交 `42015be`）：重构前的 Bevy 实现，含分层与自研体素/ECS 拆分，只作参照，不是新设计的前提。
- 本分支从零重建，不做增量演进。
