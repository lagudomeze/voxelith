# TODO

> 当前任务：按 [docs/combat-mechanics.md](../docs/combat-mechanics.md)（设计稿）逐步落地战斗机制 ECS。
> 已拍板：Q18 **推迟资源池**（保留 `Health`）、Q21 **放宽 R5，允许 `bevy_time`**。
> 仍未拍板：Q17、Q19、Q20、Q22（见 [OPEN-QUESTIONS.md](../docs/OPEN-QUESTIONS.md)）。

## 已完成

- [x] **M1 定义工具**：`src/defs.rs` 的 `voxelith_defs!`（枚举 → `COUNT` / `ALL` / `index` / `name`）+ 单测
- [x] **M2 属性 L0 收敛**：`atoms/attribute/`（`defs.rs` / `systems.rs` / `mod.rs`）；
      唯一写入口齐备（加点 / 成长 / 洗点 / 存最终值），L0 内不再有任何多组件查询
- [x] **M3 修饰符与属性 L1**：`behaviors/modifiers`（`Modifier` / `ModifierSet` / `evaluate` + 上限）
      + `behaviors/attributes`（脏消息 → 重算 → `AttributeFinalMessage` → L0 缓存；`revision` 防回退）
- [x] 修复原先**无法编译**的 `atoms/attribute.rs`（已删除，改为 `atoms/attribute/` 目录）
- [x] 架构守卫升级：R5 从"禁止 `bevy =`"升级为**依赖白名单**，并做过反向验证
- [x] 记录陷阱：`axiom` 测试不能用 `MinimalPlugins`（属 `bevy` facade），见 [layers.md](../docs/layers.md) §8

## 待办

- [ ] **M4 资源池（已推迟，Q18）**：等法力/耐力有真实需求时再做，届时一并处理 Q2（`Health` 字段私有化）
- [ ] **M5 伤害链路**：`behaviors::{damage, resistance, rolls, damage_pipeline}` +
      `DamageResolvedMessage` 接线 `ModifyHealthMessage`
- [ ] **M6 状态**：`behaviors::status`（定义 / 注册表 / 判定 / 每回合结算 / 到期 / 免疫 / 净化 / 叠加），
      计时用 `bevy_time`（R5 已放宽）
- [ ] **M7 L2 接线**：`content`（伤害附加行为、状态定义、技能表）+ `presentation`
      （监听 `DamageResolvedMessage` / `StatusResolvedMessage`，不算伤害）
- [ ] 拍板 Q17 / Q19 / Q20 / Q22

## 每步收尾

- [ ] `pwsh ./scripts/arch-guard.ps1`（当前 **10/10 通过、38 个测试**）
- [ ] 单文件 < 500 行（R26）、新事件在定义它的模块注册（R34）
