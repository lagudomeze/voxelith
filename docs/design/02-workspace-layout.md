# 工作区与 crate 划分

**分 crate 只有一个正当理由：依赖方向。** 说得出"谁依赖谁"，才分；说不清就先放在同一个 crate 里——**切早了就是自顶向下设计**。

## 现在有三个

| crate | 放什么 | 依赖 |
| --- | --- | --- |
| `voxelith_core` | 与引擎无关的模型（目前只有[属性与资源](01-attributes-and-resources.md)） | **只有 `bevy_ecs`** |
| `voxelith_app` | 装配层：把模型挂到一个 Bevy App 上 | `bevy_app` / `bevy_asset` / `bevy_scene` + core |
| `voxelith_rules` | **规则层**：有效值缓存的重算 + 量之间的依赖（派生）、内容参数的加载 | core + `bevy_ecs` / `bevy_app` / `bevy_asset` / `serde` / `ron` |

两条由此得到的硬性质：

- **core 不依赖 App、资产、渲染** → 逻辑可以无渲染测试（对应 [decisions.md](../decisions.md) 里"开发期需要的能力"的第一条）。
- **core 不依赖 `bevy_scene`**：BSN 只在测试里用（`dev-dependencies`）。"组件能被 BSN 表达"是**测试要证明的事**，不是运行时要背的依赖。

## `voxelith_core` 内部的模块

边界是“**机制**”与“**具体有哪些量**”：

| 模块 | 管什么 |
| --- | --- |
| `value` | **可修正的量**这一层的基础代码：修正 `Modifier`（平加 + 比例，以及它贡献多少）、**唯一的量接口 `Value`**（`raw` / `raw_mut`）、上限 `Max<Q>`（本身就是个 `Value`）、有效值缓存 `Total<T>`。**求和与写缓存、修正怎么挂，这里都不管** |
| `attribute` | 六项标准属性——每项一个 newtype（自己就是自己的标记）+ 修正别名 |
| `resource` | 资源（血量、精力、法力）——**当前值组件 + `Max<资源>` 别名 + 回复速率** + 修正别名 |
| `change` | 改值：当前值的唯一写入通路（`Adjust` 消息 + 落地系统） |

**资源为什么拆成两个组件**：上限本身是可修正的量，把它做成独立的 `Value`（`Max<Health>`），重算就只认一种输入——`recompute::<Max<Health>, L>` 与 `recompute::<Strength, L>` 是同一条代码路径，不需要第二个 trait。当前值 `#[require(Max<Self>)]` 绑上限。\n\n**`Total<T>` 为什么在 core**：它是"有效值"这个概念的落点（`SY-58`：面板上的数就是进公式的数），每个量一出生就带一份（`#[require]`）。**写它的人**（知道修正怎么挂的重算系统）在 `voxelith_rules` —— core 只提供格子与"一条修正贡献多少"。

**`value` 为什么是一个模块**：它放的是“可修正的量”这一层本身——修正怎么改值、值怎么读出来，这是一件事的两半，分开放反而要互相引用。属性与资源都只是它的实现者；“有哪几项属性、哪几条资源”分别在 `attribute` 与 `resource` 里。

**`voxelith_rules` 为什么不并进 core**：它要读配置资产（`Handle` / `AssetLoader`），于是依赖 `bevy_asset`；而 core 刻意只有 `bevy_ecs`，保持“无资产、无渲染也能测”。规则层依赖 core 是单向的 ✓。

## `voxelith_rules` 里有什么

| 模块 | 管什么 |
| --- | --- |
| `recalc` | **重算**：按项目给的挂法求出有效值写进 `Total`（`recompute` / `recompute_max`），并把一帧分成三段：属性 → 派生 → 回复速率与资源上限 |
| `derived` | **派生**：`Formula<From, To>`（声明 + 配置 handle）+ 每条规则的 `apply` |
| `config` | 规则参数资产（`RuleConfig`：一份文件一条规则） |

对外只有一个入口：[`RulesPlugin<L>`](../design/bevy/02-derived-rules.md)——`L` 就是**修正怎么挂**（项目自己的 relationship）。

## 为什么现在只有这些

- 需求侧刚走完，能落地的模型只有"属性与资源 + 规则"这一块；
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
## 内容放哪

**内容与代码分开放：资产根是仓库根的 `assets/`**（`assets/rules/*.ron` 是规则参数）。

- 运行时：应用把 `AssetPlugin.file_path` 指到它——用**绝对路径**钉住（`{CARGO_MANIFEST_DIR}/../../assets`），免得受工作目录影响；
- 测试：`voxelith_rules` 的测试也指同一个根，所以**测试读的就是真实内容**，另有一份 fixture 就等着两边跑偏；
- 将来内容变多也还是这一个根（贴图、关卡、数据都从它下面分目录），不按 crate 分散。
