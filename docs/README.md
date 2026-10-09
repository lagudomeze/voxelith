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

判断标准很简单：**回答"是什么/要什么"进 requirements，回答"怎么做"进 design**。放不进去的，说明它还是张 `Q-xxx`。
