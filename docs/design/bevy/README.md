# Bevy 线

用 Bevy 实现 Voxelith 的设计，**只放真的取决于 Bevy 的取舍**；与引擎无关的模型在上层目录。

版本：**Bevy 0.19.1**。凡是写进这里的技术结论，都要**编过跑过**——不写"看起来应该行"。

## 现状

- 重构前的实现（`legacy`）走的就是 Bevy 路线，可以拿来对照与避坑，但**不作为新设计的前提**。
- 已经落地的代码：`voxelith_core`（属性与资源 + 修正）、`voxelith_rules`（派生 + 配置资产）、`voxelith_app`（装配自检）。

## 文档

| 文档 | 内容 |
| --- | --- |
| [01-attributes-and-resources.md](01-attributes-and-resources.md) | 属性与资源：命名禁忌、组件形状、读出接口、修正怎么挂（交给应用侧）、在 `bsn!` 里写"角色 + 修正" |
| [02-derived-rules.md](02-derived-rules.md) | 派生：配置 handle 写进 `bsn!`、加载出错策略、每条 (属性→量) 一条 system，以及那个"`&mut Mut<T>` 一取就脏"的坑 |
