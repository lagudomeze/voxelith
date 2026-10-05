//! **词汇原子**：运行时把"名字"换成 `u16` 的那套基础设施。
//!
//! 这里只有三种东西，都是**纯数据、零领域知识**：
//!
//! | | 是什么 |
//! |---|---|
//! | [`ResourceId`] / [`StatId`] / [`StatusId`] / [`SkillId`] / [`ActorTagId`] | 四种内容词汇 + 一种角色特性的 ID（`u16` newtype） |
//! | [`NameTable`] | 「名字 → ID」表（按登记顺序发号） |
//! | [`UnknownName`] | 没登记过的名字（加载期错误） |
//!
//! ## 为什么它们在 L0，而不是在 `behaviors::content`
//!
//! 它们一开始是跟内容管线一起长出来的，所以住在 `behaviors/content/vocabulary.rs`。
//! 但那让**方向反了**——L0 的组件要拿它们当字段类型：
//!
//! ```text
//! atoms/actor  ──►  behaviors::content::{ResourceId, SkillId, StatId}   ✗ L0 依赖 L1
//! world        ──►  behaviors::content::{NameTable, UnknownName}        ✗ 同上
//! ```
//!
//! `Resources` 的键、`Cooldowns` 的键、`Stats` 的键都是这些 ID：
//! **砖块不该向搭砖规则要自己的尺寸**（R112、R113）。
//!
//! 判据（[docs/layers.md](../../../../docs/layers.md) §2）：把这个东西单独拿出来，
//! 它能不能只靠自己成立？——ID 是个 `u16`，`NameTable` 是个 map，都能。
//! 而 [`Vocab`](crate::behaviors::content::Vocab)（把五张表聚在一起、由加载器填充）
//! 留在 L1：它是**内容的**词汇表，不是原子。
//!
//! ## 为什么"特性"也是词汇 ID
//!
//! [`ActorTagId`] 与另外四个是同一类东西：它的取值**随游戏内容增长**
//! （亡灵 / 构装体 / 野兽 / 将来还有别的种族与类型）。
//! [docs/layers.md](../../../../docs/layers.md) §9 的判据是
//! **"这个集合会随游戏内容增长吗"** —— 会，所以它走 `.ron` + ID，
//! 而不是一个每加一种种族就要改一次代码的 Rust 枚举。
//!
//! 对照：[`Faction`](super::actor::Faction) 仍然是枚举。区别不在"是不是标签"，
//! 而在**它带不带规则**——`Faction::hostile_to` 是一条引擎级的敌对规则，
//! 而"亡灵"只是一个用来比较相等性的名字。

use std::collections::HashMap;

use bevy_reflect::Reflect;

/// 声明一种词汇 ID（`u16` newtype）。
///
/// `Reflect` 是为了**调试通道**（BRP）：没有它，`Resources` / `Stats` 这些组件的字段
/// 在 `world.query` 里读不出来（**静默跳过，不报错**）。
macro_rules! vocab_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
        pub struct $name(pub u16);
    };
}

vocab_id!(ResourceId, "资源池标识（HP / 法力 / 行动 / 反应……）。");
vocab_id!(StatId, "属性标识（力量 / 护甲 / 敏捷……）。");
vocab_id!(StatusId, "状态标识（格挡 / 眩晕 / 中毒 / 狂暴……）。");
vocab_id!(SkillId, "技能标识（按 `skills.ron` 的出现顺序编号）。");
vocab_id!(ActorTagId, "角色**特性**标识（亡灵 / 构装体 / 野兽……）。");

/// 一张「名字 → ID」表。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameTable {
    by_name: HashMap<String, u16>,
}

impl NameTable {
    /// 查 ID。
    pub fn id(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied()
    }

    /// 已登记的名字数量。
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// 按名字遍历（加载期报错时用来列清单）。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// 是否已经登记过这个名字。
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// 登记一个名字，返回分配到的下标（重复登记返回**已存在**的下标）。
    ///
    /// **按登记顺序发号**：所以"在配置中间插一条"会让它后面所有名字的下标后移一位。
    /// 热重载靠 `Vocab` 拿旧表垫底来避免这件事（见 `docs/combat-design.md` §8.2）。
    pub fn register(&mut self, name: &str) -> u16 {
        let next = self.by_name.len() as u16;
        *self.by_name.entry(name.to_owned()).or_insert(next)
    }
}

/// 词汇表里没有这个名字（加载期错误，见 [`crate::behaviors::content::LoaderError`]）。
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, derive_more::Error)]
#[display("unknown {kind} name `{name}` (not declared in vocabulary.ron)")]
pub struct UnknownName {
    /// 哪张表（resource / stat / status / skill / tag）。
    pub kind: &'static str,
    /// 未登记的名字。
    pub name: String,
}

impl UnknownName {
    /// 构造。
    pub fn new(kind: &'static str, name: &str) -> Self {
        Self {
            kind,
            name: name.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_assigned_in_registration_order() {
        let mut table = NameTable::default();
        assert_eq!(table.register("hp"), 0);
        assert_eq!(table.register("mana"), 1);
        assert_eq!(table.register("hp"), 0, "重复登记不新开槽");
        assert_eq!(table.len(), 2);
    }

    /// 每种词汇各自一张表，编号互不影响（`ResourceId(0)` 与 `StatId(0)` 是两件事）。
    #[test]
    fn two_tables_number_independently() {
        let mut resources = NameTable::default();
        let mut tags = NameTable::default();
        assert_eq!(resources.register("hp"), 0);
        assert_eq!(tags.register("undead"), 0);
        assert_eq!(resources.id("hp"), Some(0));
        assert_eq!(tags.id("undead"), Some(0));
    }

    #[test]
    fn unknown_names_report_which_table() {
        let error = UnknownName::new("tag", "ghost");
        assert_eq!(error.kind, "tag");
        assert!(error.to_string().contains("ghost"));
    }
}
