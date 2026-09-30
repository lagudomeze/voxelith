# TODO

> 当前任务：按 [docs/combat-mechanics.md](../docs/combat-mechanics.md)（全景稿）推进战斗机制 ECS。
> 已拍板并落地：Q17–Q25（其中 Q19 / Q22 有修订，见 [OPEN-QUESTIONS.md](../docs/OPEN-QUESTIONS.md)）。

## 已完成

- [x] **`Stat` 自成一体**：`base`（初始值）/ `allocated`（分配与成长点数）/ `modifiers`（槽位）/ `cached`（视图），
      收到加点、发点数、洗点、修饰符增删消息后**自己 `refresh`**；删除了 `StatBaseChangedMessage` /
      `StatFinalMessage` / `behaviors::stats` 模块，`StatStage` 简化为 `Intake → Apply`
- [x] **`Resistance` 同样自成一体**：`base` + `modifiers` + `cached`；删除 `ResistanceFinalMessage` /
      `ResistanceModifiersChangedMessage` / `recompute_resistance` / `apply_resistance_final`
- [x] **去掉所有 `defs.rs` 与 `voxelith_defs!` 宏**：枚举的辅助项（`COUNT` / `ALL` / `index` / `name`）
      写在枚举旁边；属性定义并入 `atoms/stats/stat.rs`，状态定义并入 `behaviors/status/mod.rs`
- [x] **修饰符下移到 L0**（`atoms::modifiers`，纯数据 + 纯算法）；永久 / 临时（`remaining` + `lasting`）+
      按来源清理 + 两个域的计时系统
- [x] 全景骨架：`damage` / `resistance` / `rolls` / `damage_pipeline` / `status` / `combat` + 配置全走 Resource
- [x] 守卫与测试全绿：`10/10`、**73 个测试**

## 留了接口（TODO(M5)）

- [ ] `stage_attacker_bonus`：攻击方属性缩放 + 增伤规则表（规则顺序 = 注册顺序）
- [ ] `dispatch_damage_behaviors`：`DamageBehaviorId → 具体请求消息` 的映射（需要内容层行为表）
- [ ] 状态 `on_apply` / `on_tick` / `on_expire` 行为表（内容层注册）

## 待办

- [ ] **M7 L2 接线**：`content`（`StatusDef` 表、附加行为绑定、技能表）+ `presentation`
      （监听 `DamageResolvedMessage` / `StatusResolvedMessage`，不算伤害）
- [ ] **技能与目标选择**：`behaviors::skills`（技能数值与释放）+ `behaviors::targeting`（寻找与判定）
- [ ] **M4 资源池（已推迟，Q18）**：等法力 / 耐力有真实需求时再做，届时一并处理 Q2（`Health` 字段私有化）

## 每步收尾

- [ ] `pwsh ./scripts/arch-guard.ps1`（当前 **10/10 通过、73 个测试**）
- [ ] 单文件 < 500 行（R26）、新事件在定义它的模块注册（R34）、新参数进 `XxxConfig`（I6）
