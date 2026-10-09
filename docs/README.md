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
| 定下来的需求条目 | `requirements/requirements.md` |
| 定下来的流程 / 结构决定 | [decisions.md](decisions.md) |
| 引擎无关的模型（组件 / 系统 / 消息 / 数值） | `design/ecs-model.md`、`design/atoms.md` |
| 某个引擎下怎么实现 | `design/<engine>/NN-主题.md` |
| 引擎选型结论（定下来才写） | `design/engine-decision.md` |

判断标准很简单：**回答"是什么 / 要什么"进 requirements，回答"怎么做"进 design。**

## 两种东西不进仓库

1. **还没定的问题。** 未决事项在对话里定，定完按上面那张表落文件——仓库不是讨论区。
2. **过程记录。** 谁在哪一轮说了什么、为什么改主意；除非它解释了某个决定，那属于 [decisions.md](decisions.md)。
