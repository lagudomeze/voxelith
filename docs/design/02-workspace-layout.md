# 工作区与 crate 划分

**分 crate 只有一个正当理由：依赖方向。** 说得出"谁依赖谁"，才分；说不清就先放在同一个 crate 里——**切早了就是自顶向下设计**。

## 现在有两个

| crate | 放什么 | 依赖 |
| --- | --- | --- |
| `voxelith_core` | 与引擎无关的模型（目前只有[属性与资源](01-attributes-and-resources.md)） | **只有 `bevy_ecs`** |
| `voxelith_app` | 装配层：把模型挂到一个 Bevy App 上 | `bevy_app` / `bevy_asset` / `bevy_scene` + core |

两条由此得到的硬性质：

- **core 不依赖 App、资产、渲染** → 逻辑可以无渲染测试（对应 [decisions.md](../decisions.md) 里"开发期需要的能力"的第一条）。
- **core 不依赖 `bevy_scene`**：BSN 只在测试里用（`dev-dependencies`）。"组件能被 BSN 表达"是**测试要证明的事**，不是运行时要背的依赖。

## `voxelith_core` 内部的模块

边界是“**机制**”与“**具体有哪些量**”：

| 模块 | 管什么 |
| --- | --- |
| `modifier` | 修正：一条 `Modifier`（枚举，平加 / 比例）、指回主人的关系、遍历修正的 `modifiers()`、**唯一的公式** `resolve_with` |
| `value` | 读出：`Resolvable`（单值型）/ `Resourceable`（资源，只到上限）/ 上限的标记 `Max<T>` |
| `attribute` | 六项标准属性——每项一个 newtype（自己就是自己的标记）+ 修正别名 |
| `resource` | 资源池（血量、精力）——当前值 + 基础上限，上限有独立标记 + 修正别名 |

**`value` 为什么是一个模块**：它放的是“读出”这件事本身（规则只有一条），属性与资源都只是它的实现者；而“有哪几项属性、哪几条资源”分别在 `attribute` 与 `resource` 里。

## 为什么现在只有两个

- 需求侧刚走完，能落地的模型只有"属性与资源"这一块；
- 其余（体素与存档、内容加载、2D 等距呈现、时间与行动）**还没设计**。等它们的依赖方向清楚了再切。候选名字先记着，**不当承诺**：
  - `voxelith_world`：体素地形、区块、生成、存档
  - `voxelith_content`：内容数据与热重载
  - `voxelith_render`：2D 等距呈现
  - `voxelith_gameplay`：时间推进、行动、反制、移动

## 引擎依赖怎么引

`[workspace.dependencies]` 里**只引需要的 bevy 子 crate**（`bevy_app` / `bevy_asset` / `bevy_scene` / `bevy_ecs`），**不拉整个 `bevy`**——省编译时间。要画东西时再加渲染那一层。

`bevy_ecs` 开了 `debug` feature：让"重复组件"这类报错能打出组件名。

## 跑什么

| 想要 | 命令 |
| --- | --- |
| 自检（造一个英雄、打印最终属性） | `cargo run -p voxelith_app` |
| 全部测试 | `cargo test` |
