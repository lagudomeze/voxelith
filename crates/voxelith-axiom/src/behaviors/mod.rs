//! L1 行为层（behaviors）：**需要多个组件/资源同时在场才生效**的逻辑（**R11**）。
//!
//! - 允许依赖 L0（**R12**）。
//! - 修改 L0 数据**必须通过消息**，不直接写（**R13**）。
//! - 禁止依赖 L2、禁止生成渲染实体（**R14**）。
//!
//! 领域地图（对应 `docs/combat-mechanics.md`）：
//!
//! | 模块 | 职责 | 当前状态 |
//! |---|---|---|
//! | [`progression`] | 成长编排：经验 → 升级 → 发点数 | 已完成 |
//! | [`damage`] | 伤害类型定义 + 附加行为分派 | 组件/消息/注册表就位，分派待接内容层 |
//! | [`resistance`] | 抵抗（自成一体：数据 + 修饰符 + 视图）+ 确定性减伤公式 | 已完成 |
//! | [`rolls`] | 概率判定（命中 / 闪避 / 暴击）：**唯一随机点** | RNG 与消息就位，判定公式待接 |
//! | [`damage_pipeline`] | 固定四阶段伤害管线 → `DamageResolvedMessage` | 阶段槽位就位，阶段体待实现 |
//! | [`status`] | 状态定义 / 施加 / 结算 / 到期 / 免疫 / 净化 / 叠加 | 组件与注册表就位，判定待接 |
//! | [`combat`] | 战斗配置汇总（`CombatConfig`）+ 装配 | 已完成 |
//!
//! 注：修饰符（`Modifier` / `ModifierSet` / `evaluate`）是**纯数据 + 纯算法、没有组件**，
//! 因此放在 L0（[`crate::atoms::modifiers`]），供 `atoms::stats` 与 `behaviors::resistance` 共用。

pub mod combat;
pub mod damage;
pub mod damage_pipeline;
pub mod progression;
pub mod resistance;
pub mod rolls;
pub mod status;
