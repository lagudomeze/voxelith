# 文档地图

这里只放两类东西：**要做什么**、**怎么做**。其余都是这两者的过程记录。

| 目录 | 回答的问题 | 入口 |
| --- | --- | --- |
| [requirements/](requirements/README.md) | 要做什么、为什么、边界在哪 | 需求澄清与确认 |
| [design/](design/README.md) | 怎么做：结构、模块、数据流、取舍 | 编码设计，按引擎分线 |

## 内容放哪

| 手上是什么 | 写到 |
| --- | --- |
| 用户说过的话（原文） | `requirements/00-original-discussion.md` |
| 一个还答不上来的问题 | `requirements/open-questions.md` |
| 定下来的需求条目 | `requirements/requirements.md` |
| 引擎选型结论 | `design/engine-decision.md`（**待写**，见 `Q-001`） |
| 某个引擎下怎么实现 | `design/<engine>/NN-主题.md` |
| 引擎无关的机制设计 | 落点**未定**，见 [Q-002](requirements/open-questions.md) |

判断标准很简单：**回答"是什么/要什么"进 requirements，回答"怎么做"进 design**。放不进去的，说明它还是张 `Q-xxx`。

## 已定的结构决策

- 需求条目用 `REQ-xxx`；`legacy` 的 `R1`–`R117` 是旧项目的「规则」，`R<n>` 留给它引用，新体系不占用（[Q-003](requirements/open-questions.md)）。
- 旧项目的工程规范类内容（命名 / 文件组织 / 反模式 / 守卫 / 流程）**暂不迁移**：这里只保留「需求 + 设计」两类。将来真需要时再另立目录，**不许塞进 `design/<engine>/`**（[Q-004](requirements/open-questions.md)）。
