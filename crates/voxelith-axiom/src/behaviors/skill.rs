//! 技能**定义**（Skill）与它的标签。
//!
//! `Skill` 是全局共享的模板实体（一个技能一个实体），运行时实例是
//! [`Action`](crate::behaviors::action::Action)。
//!
//! 语义用**标记**表达，不用 `enum ActionKind`：占不占行动槽只由 [`Skill::duration`] 决定，
//! 是不是反制只由 [`SkillTags::COUNTER`] 决定（见 [docs/combat-design.md](../../../../docs/combat-design.md) §0）。

use bevy_ecs::prelude::*;

use crate::behaviors::content::SkillId;
use crate::behaviors::effect::Effect;
use crate::behaviors::requirement::{Cost, Requirement, Targeting};

/// 技能标签（手写位标志：不引入 `bitflags`，见 [docs/combat-design.md](../../../../docs/combat-design.md) §12）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SkillTags(pub u32);

impl SkillTags {
    /// 无标签。
    pub const NONE: Self = Self(0);
    /// 攻击：会被 `blocks_tags` 里的它拦住。
    pub const ATTACK: Self = Self(1 << 0);
    /// 移动。
    pub const MOVEMENT: Self = Self(1 << 1);
    /// 反制：`update_phase` 靠它判断"有没有反制可用"。
    pub const COUNTER: Self = Self(1 << 2);
    /// 防守。
    pub const GUARD: Self = Self(1 << 3);
    /// 法术。
    pub const SPELL: Self = Self(1 << 4);
    /// 治疗。
    pub const HEAL: Self = Self(1 << 5);
    /// 瞬发（与 `duration == 0` 一致，供内容自查）。
    pub const INSTANT: Self = Self(1 << 6);

    /// 是否包含 `other` 的全部位。
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 是否与 `other` 有任意交集（状态门控用）。
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// 并上 `other`。
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// 两个标签位求并（内容层 / 测试组合标签用）。
    pub fn union(left: SkillTags, right: SkillTags) -> SkillTags {
        SkillTags(left.0 | right.0)
    }

    /// 单个标签 → 位。
    pub fn from_tag(tag: SkillTag) -> Self {
        match tag {
            SkillTag::ATTACK => Self::ATTACK,
            SkillTag::MOVEMENT => Self::MOVEMENT,
            SkillTag::COUNTER => Self::COUNTER,
            SkillTag::GUARD => Self::GUARD,
            SkillTag::SPELL => Self::SPELL,
            SkillTag::HEAL => Self::HEAL,
            SkillTag::INSTANT => Self::INSTANT,
        }
    }
}

/// 单个技能标签：RON 里写成 `[ATTACK, COUNTER]` 这样的名字列表。
///
/// 每个变体都显式写 serde(rename)：SCREAMING_SNAKE_CASE 会把 ATTACK 拆成 A_T_T_A_C_K，
/// 而 UPPERCASE 又依赖命名约定，所以这里逐个写死。
/// 而 UPPERCASE 又依赖命名约定。显式写死最直白，加多词标签时也不会踩坑。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum SkillTag {
    /// [`SkillTags::ATTACK`]
    #[serde(rename = "ATTACK")]
    ATTACK,
    /// [`SkillTags::MOVEMENT`]
    #[serde(rename = "MOVEMENT")]
    MOVEMENT,
    /// [`SkillTags::COUNTER`]
    #[serde(rename = "COUNTER")]
    COUNTER,
    /// [`SkillTags::GUARD`]
    #[serde(rename = "GUARD")]
    GUARD,
    /// [`SkillTags::SPELL`]
    #[serde(rename = "SPELL")]
    SPELL,
    /// [`SkillTags::HEAL`]
    #[serde(rename = "HEAL")]
    HEAL,
    /// [`SkillTags::INSTANT`]
    #[serde(rename = "INSTANT")]
    INSTANT,
}

impl FromIterator<SkillTag> for SkillTags {
    fn from_iter<T: IntoIterator<Item = SkillTag>>(iter: T) -> Self {
        let mut tags = Self::NONE;
        for tag in iter {
            tags.insert(Self::from_tag(tag));
        }
        tags
    }
}

/// 技能定义（组件，全局共享的**模板**）。
///
/// 字段语义与 RON 一一对应；字符串在加载期已解析成词汇 ID（[`SkillId`] 等）。
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Skill {
    /// 技能标识。
    pub id: SkillId,
    /// 显示名。
    pub name: String,
    /// 图标名（L2 自己解释，引擎不碰资源）。
    pub icon: String,
    /// 标签。
    pub tags: SkillTags,
    /// **谁会这一招**：空 = 谁都能用；否则只有这些引擎角色能用。
    ///
    /// 它是"技能栏里该不该出现这一招"的判据，与"能不能放得出来"（`requirements` / `costs`）分开：
    /// 前者是**归属**，后者是**当下条件**。混在一起的后果见 `descriptor::SkillRon::roles`。
    pub roles: Vec<crate::behaviors::content::descriptor::RoleRon>,
    /// 释放时长（秒）：`0.0` = **瞬发**，同帧结算且从不占行动槽。
    pub duration: f32,
    /// 后摇时长（秒）：**释放点之后仍然占着槽**的时间（`0.0` = 没有后摇）。
    ///
    /// 后摇的语义是"僵直 / 冷却"，**不是定身**：后摇期间可以移动，但不能再出手。
    /// 它只在行动实体上生效（`Action::enter_recovery`），不进 `Cooldowns`。
    pub recovery: f32,
    /// 释放前的需求（全部满足才可用）。
    pub requirements: Vec<Requirement>,
    /// 消耗（全部付得起才可用）。
    pub costs: Vec<Cost>,
    /// 目标如何解析。
    pub targeting: Targeting,
    /// 结算时执行的效果。
    pub effects: Vec<Effect>,
}

impl Skill {
    /// 是否是瞬发（同帧结算）。
    pub fn is_instant(&self) -> bool {
        self.duration <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 组合两个标签位（测试里用；引擎内部只用 `insert` / `contains` / `intersects`）。
    fn union(left: SkillTags, right: SkillTags) -> SkillTags {
        SkillTags(left.0 | right.0)
    }

    #[test]
    fn tags_combine_with_or() {
        let tags: SkillTags = [SkillTag::ATTACK, SkillTag::COUNTER].into_iter().collect();
        assert!(tags.contains(SkillTags::ATTACK));
        assert!(tags.contains(SkillTags::COUNTER));
        assert!(!tags.contains(SkillTags::SPELL));
        assert!(tags.intersects(union(SkillTags::COUNTER, SkillTags::HEAL)));
        assert!(!tags.intersects(union(SkillTags::SPELL, SkillTags::MOVEMENT)));
    }

    #[test]
    fn empty_tags_are_contained_by_everything() {
        let tags = SkillTags::ATTACK;
        assert!(tags.contains(SkillTags::NONE));
        assert!(!SkillTags::NONE.intersects(SkillTags::ATTACK));
    }
}
