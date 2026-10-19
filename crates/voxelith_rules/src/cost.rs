//! 付费：代价的**检查与扣减**。
//!
//! 三条定下来的事（见 [`docs/design/04-time-and-actions.md`](../../../docs/design/04-time-and-actions.md)）：
//!
//! 1. **同步**：读 → 查 → 扣全在同一个函数里 ✓ 中间没有消息 ✓
//!    所以同一帧里两条请求天然按顺序处理，**不会超扣** ✓（消息是"谁先读到取决于系统顺序" ✗
//!    同一个系统里看不到自己刚写的那条）
//! 2. **一处把资源列全**（甲）：换来"**跨资源全不或全**" ✓
//!    加一条资源＝[`Costs`] 加一个字段 + [`try_pay`] 加一段 ✓
//! 3. **拒绝而非夹取**：有一条付不起 → **一条都不扣** ✓ 并说清是哪条 ✓
//!    （`GP-26` 的"先扣"只在付得起时才成立 ✓ 夹到 0 仍释放是错的 ✗ 允许负数也是错的 ✗）
//!
//! 扣减走 [`voxelith_core::change`]（唯一通路）✓ 于是留痕与夹取将来只加在一处 ✓

use bevy_ecs::prelude::*;
use voxelith_core::prelude::*;

/// 一次释放要花的账。
///
/// **中间形态**：以后技能模板上挂的是 `Cost<Q>` 组件 ✓ 由"收集"那一步汇总成它 ✓
/// 于是"减耗"这类修正也加在汇总那一步 ✓ 付费只看这张账单 ✓
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Costs {
    /// 扣多少法力。
    pub mana: f32,
    /// 扣多少精力（`GP-44`：冲刺也扣精力）✓
    pub stamina: f32,
}

/// 哪条量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    /// 法力。
    Mana,
    /// 精力。
    Stamina,
}

/// 为什么付不起。**游戏内正常失败** ✗ 不是编程错误 ✓
///
/// 编程错误（参数形状不对、引用了没有的技能模板）该 panic ✓ 这个不该 ✓
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 某条量不够。
    NotEnough(ResourceKind),
}

/// 检查 + 扣减。**同步、原子**：全够 → 全扣 ✓ 有一条不够 → 一条都不扣 ✓
///
/// 没有这条量的实体视作**不够** ✓（有些角色就是没有法力 ✓ 不 panic）✓
pub fn try_pay(
    actor: Entity,
    costs: &Costs,
    mana: &mut Query<&mut Mana>,
    stamina: &mut Query<&mut Stamina>,
) -> Result<(), RejectReason> {
    // ① 全查。只读，不碰 `&mut`，所以不会误标变更。
    if costs.mana > 0.0 {
        match mana.get(actor) {
            Ok(value) if value.raw() >= costs.mana => {}
            _ => return Err(RejectReason::NotEnough(ResourceKind::Mana)),
        }
    }
    if costs.stamina > 0.0 {
        match stamina.get(actor) {
            Ok(value) if value.raw() >= costs.stamina => {}
            _ => return Err(RejectReason::NotEnough(ResourceKind::Stamina)),
        }
    }

    // ② 全扣。到这里一定都够，中间没有消息、没有 await。
    if costs.mana > 0.0 {
        change(mana, actor, -costs.mana);
    }
    if costs.stamina > 0.0 {
        change(stamina, actor, -costs.stamina);
    }
    Ok(())
}
