# 设计

**回答"怎么做"**：结构、模块、数据流、取舍。**不写**"要什么"（那属于 [requirements/](../requirements/README.md)）。

**不存草稿、不存提案**——设计先讨论，定了才写进来。

## 三个位置

| 位置 | 放什么 |
| --- | --- |
| 本目录顶层 | **与引擎无关**的模型：组件、系统、消息、数值。两条引擎线都必须实现它 |
| [bevy/](bevy/README.md) | 只有真的取决于 Bevy 的取舍 |
| [godot/](godot/README.md) | 只有真的取决于 Godot 的取舍 |

## 共同约束

- 每条设计都要引用它服务的需求条目（`C-xx` / `GP-xx` / `SY-xx` / `WD-xx` / `PR-xx` / `CT-xx` / `IT-xx` / `QL-xx`）；引不出来，说明这条设计没有上游。
- 引擎无关的内容写顶层，别下沉到某条线里。
- **一个主题一篇文档**，不写大而全的总设计；**不自顶向下设计**——先把小的组件定好，再用 BSN 组合成更复杂的效果（见[组件约定](00-component-conventions.md)）。

## 文档

| 文档 | 内容 |
| --- | --- |
| [00-component-conventions.md](00-component-conventions.md) | 组件约定：纯数据 + `Default + Clone`、字段具名、一件事一个组件、暂不派生 `Reflect` |
| [01-attributes-and-resources.md](01-attributes-and-resources.md) | 属性与资源：六维属性各自一个组件、修正是指回主人的实体、资源只有当前值 + 一串规则组件、变动留痕 |
| [02-workspace-layout.md](02-workspace-layout.md) | 工作区与 crate 划分：为什么现在只有 `voxelith_core` 与 `voxelith_app` |
