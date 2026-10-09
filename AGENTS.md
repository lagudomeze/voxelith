# AGENTS.md

本仓库的 AI agent 入口。**正文不在这里**——读完规则按链接走。

## 先读

1. [README.md](README.md) — 项目状态与文档地图
2. [docs/README.md](docs/README.md) — 文档职责边界
3. 任务相关目录：[需求](docs/requirements/README.md) / [设计](docs/design/README.md)

## 硬规则

- **需求是唯一上游**。只从[原始讨论](docs/requirements/00-original-discussion.md)与用户确认中提炼需求；不要用旧实现（`legacy`）或常识去补需求。
- **不确定就登记**。拿不准的点写进 [open-questions.md](docs/requirements/open-questions.md)，每条带 ID；不要在文档或对话里自问自答。
- **两级目录各管各的**。需求变更只动 `docs/requirements/`；设计后果只写进 `docs/design/<engine>/`。
- **薄文档**。想往 README/AGENTS 加内容时，先问能不能放进专门文档，再在这里加一条链接。
- **一次一步**。设计按分解出的子部分逐个执行，范围外的东西不顺手做。
- **`legacy` 只读**。不向它提交、不改写、不 rebase。
