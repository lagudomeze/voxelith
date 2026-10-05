# Crate 分层（R1–R7、R116–R117）

## 1. 两个 crate

工作区用 Cargo Workspace 组织，**不发布到 crates.io**，所有 crate 名前缀 `voxelith-`。（R1）

| crate | 承载层级 | 允许依赖 | 对应规则 |
|---|---|---|---|
| `voxelith-axiom` | L0 `atoms` + L1 `behaviors` | `bevy_ecs`、`bevy_app`、`bevy_reflect`、`bevy_time`、`bevy_state` | R2、R5（已修订，见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q21、Q26） |
| `voxelith-prime` | L2 表现与游戏内容 | 完整 `bevy` 全特性 + `voxelith-axiom` + `ron` | R3、R6 |

## 2. 依赖方向

```
voxelith-prime ──► voxelith-axiom        ← 唯一允许的方向
voxelith-axiom ──✗──► voxelith-prime     ← 禁止（R4、R92）
```

- `voxelith-axiom` 的依赖分两类（**R5**，2024 修订）：
  - **bevy 家族**：严格白名单 —— `bevy_ecs` / `bevy_app` / `bevy_reflect` / `bevy_time` / `bevy_state`
    （`bevy_time` 见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q21；`bevy_state` 见 Q26）；
  - **非 bevy 工具 crate**：**登记制** —— 必须先在下面的登记表里登记，守卫脚本第 5 项对着表检查
    （见 [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) Q23）。
  - 禁止依赖完整 `bevy`：它会把 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr` 一起拖进来，
    破坏 L0/L1 纯净性。（R91、R99）
- `voxelith-prime` 可用完整 Bevy 及其全部特性。（R6）

### 非 bevy 依赖登记表（`axiom`）

| crate | 用途 | 为什么值得引入 |
|---|---|---|
| `exn` | 错误的上下文链 + `raise()` + `#[track_caller]` 定位帧 | 结构化错误要跨模块传递并定位来源，手写成本高于依赖成本 |
| `derive_more`（仅 `display` + `error`） | `Display` / `Error` 派生 | 纯样板消除，无运行时行为 |
| `serde`（仅 `derive` + `std`，无文件 IO） | 给 `behaviors::content` 的 RON 描述结构派生 `Deserialize` | 手写 RON 解析器不现实；`axiom` 只用派生，**自己不读文件**（R101 精神） |

> `voxelith-prime`（L2）额外用 `ron` 做"读文件 + 反序列化"；它本来就能用完整 Bevy 生态，
> 不进 `axiom` 的登记表。

> 新增非 bevy 依赖的流程：**先在本表登记**（写清用途与"为什么不能自己写"）→ 改 `Cargo.toml` → 跑守卫。
> 这样每一次依赖增加都必然出现在 review 里，而不是悄悄进树。

## 3. 为什么用 crate 边界而不是纪律

crate 边界是防止 AI（和人类）揉代码的**最强武器**：越界会直接编译失败，而不是靠 review 抓。（R117）

因此：

- L0 纯净性用 Cargo 依赖做**物理隔离**，编译器兜底。（R94）
- `axiom` 的 `Cargo.toml` 里**不要**加 `bevy`，也不要加 `bevy_render` 等任何渲染相关 crate 的例外。
- 需要共享类型时，类型下沉到 `axiom`；需要表现时，表现留在 `prime`。

## 4. 什么时候拆第三个 crate

未来如有第二个维度/世界，再考虑拆新 crate，**不提前拆**。（R7）

判断信号（与 R27、R110 一致）：

- 单个 crate 超过 5000 行。
- 出现两个互相独立的依赖子图，且其中一方不需要另一方的任何类型。

## 5. 目录与配置现状

```
Cargo.toml                      # workspace：members / workspace.package / workspace.dependencies
crates/voxelith-axiom/          # L0 + L1
    Cargo.toml                  # bevy_ecs, bevy_app, bevy_reflect, bevy_time
    src/lib.rs                  # 顶部写规则注释（R95）
    src/atoms/                  # L0：actor（角色数据）/ vocabulary（词汇）/ action / status /
                                #     decision / world（体素世界）
    src/behaviors/              # L1
crates/voxelith-prime/          # L2
    Cargo.toml                  # bevy（完整）, voxelith-axiom
    src/main.rs
```

> **顶层只有两个模块，名字就是层名。** 领域住在层里面（R115：按功能组织）——
> `atoms/actor`、`atoms/world`、`behaviors/action`……
>
> `atoms/world`（体素世界）曾经在顶层当 `atoms` 的兄弟，那是历史原因：它跟着体素玩法
> 长出来，而 `atoms/` 当时只服务战斗。它是**纯数据 + 纯计算、零系统**，是不折不扣的 L0，
> 所以收敛进来了。
>
> ⚠️ **Bevy 的 `TypePath` 跟着「定义所在模块」走，不跟 `pub use` 走**：`ChunkPos` /
> `VoxelStore` 这类组件在 BRP 查询里的路径是 `voxelith_axiom::atoms::world::…`。
> `voxelith_axiom::world::…` 仍然可用（`lib.rs` 里转出），但那是 Rust 路径，
> 不是 BRP 认的那个。

### 版本统一

所有 crate 通过 `workspace.package` 继承 `version` / `edition` / `license`；依赖通过 `workspace.dependencies` 统一版本，禁止在子 crate 里写具体版本号。

## 6. 规则注释模板（R95）

每个 crate 的 `lib.rs` 顶部必须写规则注释：

`crates/voxelith-axiom/src/lib.rs`

```rust
//! voxelith-axiom：核心机制层（L0 atoms + L1 behaviors）。
//!
//! 硬约束（违反即编译失败或架构守卫失败）：
//! - R5  只允许依赖 bevy_ecs / bevy_app / bevy_reflect / bevy_time，禁止完整 bevy。
//! - R8  L0 组件只依赖自己，系统只查询自己，不跨组件查询。
//! - R13 L1 修改 L0 数据必须通过事件，不直接改。
//! - R10 L0 禁止 Sprite / Text / Mesh / Transform / Handle<Image>。
//! - R14 L1 禁止生成渲染实体，禁止依赖渲染 crate。
```

`crates/voxelith-prime/src/main.rs`（或 `lib.rs`）

```rust
//! voxelith-prime：游戏内容层（L2 表现）。
//!
//! 硬约束：
//! - R15 L2 只读 L0/L1 数据，通过事件监听变化。
//! - R16 L2 禁止直接修改 Health / Velocity 等核心数据。
//! - R17 L2 禁止写战斗公式、伤害计算。
//! - R18 L2 允许使用完整 Bevy、渲染组件、UI。
```
