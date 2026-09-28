# 架构守卫（R90–R95）

守卫的作用：把"靠自觉"变成"靠命令"。每次提交前跑一遍，越界立刻暴露。

## 1. 一键运行

```powershell
pwsh ./scripts/arch-guard.ps1
```

脚本位于 [`../scripts/arch-guard.ps1`](../scripts/arch-guard.ps1)。退出码 `0` = 全部通过，非 `0` = 有失败项。

## 2. 检查项与规则对应

| # | 规则 | 检查内容 | 失败含义 |
|---|---|---|---|
| 1 | R90 | `cargo check --workspace` | 编译失败 |
| 2 | — | `cargo test --workspace` | 有测试失败（含 `atoms/health` 的 Message/Event 用法样板测试） |
| 3 | R91 | `cargo tree -p voxelith-axiom` 不含 `bevy_render` / `bevy_ui` / `bevy_sprite` / `bevy_pbr` | L0/L1 被渲染 crate 污染 |
| 4 | R92 | `cargo tree -p voxelith-axiom` 不含 `voxelith-prime` | 反向依赖，依赖方向被破坏 |
| 5 | R93 | `tokei --sort code`，无文件 > 500 行 | 需要拆分文件（R26） |
| 6 | R5/R95 | `voxelith-axiom/Cargo.toml` 不出现 `bevy =` | 用了完整 Bevy |
| 7 | R95 | 两个 crate 的 `lib.rs`/`main.rs` 顶部含规则注释 | 新 crate 忘记写规则注释 |
| 8 | R21/R96 | 源码中不出现 `mod base` / `common` / `utils` / `helpers` | 违规模块名 |
| 9 | R104 | 源码中不出现 `scense` | 违规拼写 |
| 10 | — | `cargo fmt --all -- --check` | 格式不一致 |

## 3. 手工等价命令

```powershell
# R90
cargo check --workspace

# R91 / R92
cargo tree -p voxelith-axiom

# R93（先安装：cargo install tokei）
tokei --sort code
```

Linux/macOS 下规则 91、92 的原始写法（规则原文）：

```bash
cargo tree -p voxelith-axiom | grep -E "bevy_render|bevy_ui|bevy_sprite|bevy_pbr"   # 输出必须为空
cargo tree -p voxelith-axiom | grep voxelith-prime                                   # 输出必须为空
```

## 4. 为什么用 Cargo 依赖做物理隔离（R94）

L0 纯净性不是"约定"，而是**依赖图事实**：

- `axiom` 的依赖里没有 `bevy` → `Sprite`、`Handle<Image>` 这些类型**在 `axiom` 里根本不存在**，写不出来。
- `axiom` 不依赖 `prime` → 反向依赖直接编译失败。
- 编译器成为第一道守卫，脚本是第二道，人是第三道。

因此：**修复越界问题的正确方式是把类型下沉/上移，而不是加例外依赖。**

## 5. 规则注释（R95）

每个 crate 的入口文件（`lib.rs` 或 `main.rs`）顶部写该 crate 的硬约束，提醒 AI 与开发者。模板见 [architecture.md](architecture.md#6-规则注释模板r95)。

新增 crate 时，守卫脚本第 6 项会检查注释是否存在。

## 6. 建议的接入方式

| 时机 | 命令 |
|---|---|
| 本地提交前 | `pwsh ./scripts/arch-guard.ps1` |
| CI | `cargo check --workspace` + 同样的 grep 断言，失败即红 |
| 让 AI 改完大块代码后 | 强制要求它贴出守卫输出 |

> 约定：向 AI 提需求时，可以要求"完成后必须给出 `arch-guard.ps1` 的输出"。没有输出 = 没做完。
