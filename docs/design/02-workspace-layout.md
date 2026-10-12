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
| `modifier` | 修正机制：平加 `Flat` / 比例 `Ratio` / 指回主人的关系 `ModifiesOf`+`Modifiers` / 收集 `collect` 与公式 `Totals::resolve` |
| `carrier` | 可修正的量的两种载体：单值型 `Value` / 资源池型 `Pool`，以及一步到位的读出 |
| `attribute` | 六项标准属性（标记 + 载体别名 + 修正别名） |
| `resource` | 资源池（血量、精力；标记 + 别名） |

**载体为什么单独一个模块**：`Value` 不只服务属性——护甲、移动速度、挖掘力将来都是单值；`Pool` 也不只服务血量。把它们塞进 `attribute` 里迟早名不副实。

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
