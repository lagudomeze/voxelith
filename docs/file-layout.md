# 模块与文件组织（R20–R27、R106）

## 1. 模块命名

- 模块按**功能领域**命名，不按技术角色命名。（R20、R115）
  - 好：`combat`、`health`、`movement`、`targeting`、`skills`、`world`、`voxel_render`、`interaction`、`presentation`
  - 坏：`components`、`systems`、`events`、`managers`、`handlers`（在顶层按角色切分）
- 禁止 `base` / `common` / `utils` / `helpers` 作为模块名。（R21、R96）
  - 需要共享时，把类型**下沉到它真正的归属领域**（例：`Lifetime` → 通用实体生命周期领域，R61、R63）。
- 不使用 `scense` 命名，统一用 `presentation`，避免与 Bevy `Scene` 混淆。（R22、R104）
  - 若确实要指"场景文件/关卡"，用 `levels`、`worlds`、`map` 等不与 Bevy 类型冲突的词。

## 2. 文件树形态

- 尽量扁平，**不为了分类而创建空文件夹**。（R24、R106）
- 需要模块时再建目录；目录里至少要有真实内容。
- 组件定义不强制放 `components.rs`：小模块直接放 `mod.rs` 更合适。（R25）

## 3. 文件规模与拆分阈值

| 规模 | 组织方式 | 规则 |
|---|---|---|
| < 150 行 | 单文件 | R23、R26 |
| 150 ~ 500 行 | `mod.rs` + `systems.rs` | R23 |
| > 500 行 | `components.rs` / `events.rs` / `systems.rs` | R23 |
| 单文件 > 500 行 | **必须**拆分 | R26、R93、R110 |
| 单 crate > 5000 行 | **考虑**拆 crate | R27、R110 |

拆 crate 的判据见 [architecture.md](architecture.md#4-什么时候拆第三个-crate)。

## 4. 示例结构

小模块（< 150 行）：

```
crates/voxelith-axiom/src/atoms/
    mod.rs          # pub mod health; 以及必要的小组件定义
    health.rs       # Health 组件 + ModifyHealthMessage + apply 系统
```

中等模块（150 ~ 500 行）：

```
crates/voxelith-axiom/src/behaviors/combat/
    mod.rs          # 模块说明、re-export、CombatPlugin
    systems.rs      # 公式系统、事件转发系统
```

大模块（> 500 行）：

```
crates/voxelith-axiom/src/behaviors/combat/
    mod.rs          # 对外 API、re-export、CombatPlugin
    components.rs   # DamageType、Armor 等组件
    events.rs       # DamageRequest、ModifyHealthMessage 转发定义
    systems.rs      # 命中/闪避/减伤公式系统
```

> 注意：R36 禁止"集中在一个 `events.rs` 注册所有事件"。这里 `events.rs` 只是**同一个模块内部**的文件切分，注册仍在 `CombatPlugin::build` 里。（见 [events-and-plugins.md](events-and-plugins.md)）

## 5. lib.rs 体积控制

`lib.rs` 只做三件事：写规则注释（R95）、声明模块、按需 `pub use`。**不要把业务代码写进 `lib.rs`。**

## 6. 拆分决策流程

```
文件 > 500 行？
├─ 否 → 结束
└─ 是 → 文件里是否混了多种角色？
        ├─ 是 → 按 components / events / systems 切分（R23）
        └─ 否 → 说明是"单一职责但太大"，考虑按子领域再拆模块
                 → 整个 crate 是否 > 5000 行？
                    ├─ 是 → 考虑拆 crate（R27）
                    └─ 否 → 继续拆模块
```
