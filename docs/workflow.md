# 工作流（R108–R111）

## 1. 新增功能（R108）

固定五步，顺序不要颠倒：

```
① 判断层级   → ② 按领域建模块 → ③ 定义组件/事件 → ④ 编写系统 → ⑤ 在 Plugin 注册 → ⑥ 跑架构守卫
```

### ① 判断层级

| 问题 | 答案 | 层级 |
|---|---|---|
| 这是纯数据 / 只碰自己的逻辑吗？ | 是 | L0 `atoms` |
| 是否需要多个组件同时存在才生效？ | 是 | L1 `behaviors` |
| 是否是表现、UI、渲染、输入、AI？ | 是 | L2 `prime` |

判断口诀：**数据归数据，公式归公式，执行归执行，表现归表现。**（R112）

### ② 按领域建模块

- 名字必须是**功能领域**（`combat`、`voxel_render`、`targeting`），不是技术角色（R20）。
- 不要建 `base`/`common`/`utils`/`helpers`（R21、R96）。
- 不要为空文件夹建目录（R24、R106）。
- 组件少就直接放 `mod.rs`（R25）。

### ③ 定义组件 / 事件

- 组件用名词（R28）：`Threat`、`Armor`。
- 事件用名词短语或过去式（R29）：`CastRequest`、`AggroChanged`。
- 事件只含数据，不含渲染句柄（R9）。
- 事件定义在**发出它的模块**（R33）。
- **先按 [bevy-events.md](bevy-events.md) §2 判定用 Message 还是 Event，再定名字**：
  - 用 Message → 后缀 `Message`（语义是请求 → `Request`）。
  - 用 observer 事件 → 后缀 `Event`。
  - 后缀写错说明机制选错了；**核心数据变化（血量/位置/体素）一律 Message**。

### ④ 编写系统

- 系统用动词短语（R30）：`apply_damage`、`update_aggro`。
- L0 系统不跨组件查询（R8）；L1 改 L0 必须发事件（R13）。
- L2 不写公式（R17），不改核心数据（R16）。

### ⑤ 在 Plugin 注册

- 谁定义事件谁注册（R34）。
- 只有需要独立配置/生命周期/对外发布才建 Plugin（R41）；否则用元组（R44）或直接挂在父 Plugin。
- `main.rs` 的 `add_plugins` 保持简洁（R43）。

### ⑥ 跑架构守卫

```powershell
pwsh ./scripts/arch-guard.ps1
```

见 [architecture-guard.md](architecture-guard.md)。

## 2. 修改代码（R109）

```
① 确认不违反三层架构
② 新事件在对应模块定义并注册
③ L2 监听新事件更新表现
```

修改时的额外检查：

- 我改的数据属于哪一层？有没有越权写？
- 我加的 import 会不会把上层 crate 拖到下层？
- 我新增的事件，注册点是否在它自己的模块？
- 改动后文件是否 > 500 行（R26）？crate 是否 > 5000 行（R27）？

## 3. 拆分触发线（R110）

| 触发线 | 动作 |
|---|---|
| 单文件 > 500 行 | **必须**拆分：`components.rs` / `events.rs` / `systems.rs`（R23、R26） |
| 单 crate > 5000 行 | **考虑**拆 crate（R27） |

决策流程见 [file-layout.md](file-layout.md#6-拆分决策流程)。

## 4. 命名与重构（R111）

> 命名是架构的一部分，改名成本远低于长期忍受混淆的成本。

- 名字不对就现在改，不要"以后统一改"。
- 重命名时同步更新：模块名、Plugin 名、事件名、文档、守卫脚本的检查项。
- 发现规则歧义 → 写入 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)，不要自行发明约定。

## 5. 与 AI 协作的固定要求

给 AI 下任务时，把下面这段附在需求里，可以显著降低越界概率：

```
先读 AGENTS.md，再读与本任务相关的 docs/*.md。
产出要求：
1) 说明本改动属于哪一层、放哪个模块、为什么；
2) 新增/修改的事件必须写清定义位置与注册位置；
3) 完成后给出 `pwsh ./scripts/arch-guard.ps1` 的完整输出；
4) 有架构歧义时不要猜，写入 docs/OPEN-QUESTIONS.md 并询问。
```

## 6. 提交前检查清单

- [ ] `cargo check --workspace` 通过（R90）
- [ ] `cargo test --workspace` 通过
- [ ] `cargo tree -p voxelith-axiom` 无渲染 crate、无 `voxelith-prime`（R91、R92）
- [ ] `tokei --sort code` 无文件 > 500 行（R93）
- [ ] 新事件在用自己的模块定义并注册（R33、R34）
- [ ] 机制选对了：核心数据变化用 `Message`，实体级立即反应用 `EntityEvent`（[bevy-events.md](bevy-events.md)）
- [ ] 后缀与机制一致：`...Message` / `...Request` 对 Message，`...Event` 对 observer 事件
- [ ] 没有新增 `GameEvent` 枚举、没有中心 `events.rs` 注册（R35、R36）
- [ ] 没有空壳 Plugin（R46、R102）
- [ ] 文档中相关条目是否需要同步更新
