//! L1 行为层（behaviors）：**需要多个组件 / 资源同时在场才生效**的逻辑（**R11**）。
//!
//! - 允许依赖 L0（**R12**）。
//! - 修改 L0 数据**必须通过消息 / 效果执行器**，不直接写（**R13**）。
//! - 禁止依赖 L2、禁止生成渲染实体（**R14**）。
//!
//! 领域地图（对应 [docs/combat-design.md](../../../docs/combat-design.md) §9）：
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`content`] | 词汇表 + RON 描述 + 解析（字符串 → ID）+ 定义目录 |
//! | [`value`] | `Value`：受控闭集的表达式原语 |
//! | [`contest`] | `Contest`：对抗（唯一随机点 + 唯一强度来源） |
//! | [`effect`] | `Effect`：**唯一的世界突变原语**（技能与状态共用） |
//! | [`requirement`] | `Requirement` / `Condition`：门控是纯数据 |
//! | [`targeting`] | 目标解析（只做"寻找与判定"，R59） |
//! | [`skill`] | 技能**定义**与标签 |
//! | [`action`] | 行动**实例**：唯一入口（提交） / 推进 / 结算（成员之一即"行动槽"） |
//! | [`status`] | 状态**定义 + 实例 + 生命周期 + 派生修饰符** |
//! | [`threat`] | **威胁窗口**：未处理的、针对 PC 的威胁集合（相位暂停的依据） |
//! | [`monster`] | 怪物：能量 → **意图**（每帧重算，不落地） → 经唯一入口生成行动 |
//! | [`phase`] | `CombatPhase` 状态机 + 可用技能 + 战斗日志 |
//! | [`time_scale`] | 用 `Time<Virtual>` 倍率表达"冻结" |
//! | [`combat`] | 装配（`CombatConfig` + 固定系统顺序） |

pub mod action;
pub mod combat;
pub mod content;
pub mod contest;
pub mod effect;
pub mod monster;
pub mod phase;
pub mod requirement;
pub mod skill;
pub mod status;
pub mod targeting;
pub mod threat;
pub mod time_scale;
pub mod value;
