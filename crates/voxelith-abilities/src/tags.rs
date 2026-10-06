//! **技能类别**：内容是字符串（`["ATTACK", "SPELL"]`），引擎里是位掩码。
//!
//! 用途只有一个，但是关键的一个：**硬控**。旧内容的眩晕/沉默/定身全是
//! `blocks_tags: [ATTACK, MOVEMENT, COUNTER]`——"让对手某一类技能不可用"。
//! 新栈把它落在**释放门控**上：身上有封禁 `ATTACK` 的状态时，`ATTACK` 类技能放不出来。
//!
//! ## 为什么是掩码而不是 `Vec<SkillTag>`
//!
//! 判定发生在**每次释放请求**上，而且要对每个生效状态求交；掩码一行搞定：
//!
//! ```text
//! 被挡住 ⟺ (状态的 blocks 掩码) & (技能的 tags 掩码) != 0
//! ```
//!
//! ## 加一个类别的代价
//!
//! [`SkillTag::ALL`] 里加一行 + 内容里用新名字。**类别集合是编译期固定的**
//! （与池方案 D 同一个取舍：类型安全换"加内容不改代码"）——所以新增类别要改 Rust，
//! 但**给已有类别打标签、封禁已有类别**都只改 `.ron`。
//!
//! ⚠️ 只支持 **32 个**类别（`u32` 掩码）。到 33 个时该换 `u64` 或换成集合——别硬撑。

/// 技能类别（决定"哪一类技能会被硬控封掉"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillTag {
    /// 普通攻击类。
    Attack,
    /// 位移类。
    Movement,
    /// 反制类。
    Counter,
    /// 法术类。
    Spell,
    /// 防御 / 格挡类。
    Guard,
    /// 瞬发类。
    Instant,
}

impl SkillTag {
    /// 全部类别（顺序稳定，供遍历与报错列举）。
    pub const ALL: [SkillTag; 6] = [
        SkillTag::Attack,
        SkillTag::Movement,
        SkillTag::Counter,
        SkillTag::Spell,
        SkillTag::Guard,
        SkillTag::Instant,
    ];

    /// 内容里写的名字（大写）。
    pub const fn name(self) -> &'static str {
        match self {
            SkillTag::Attack => "ATTACK",
            SkillTag::Movement => "MOVEMENT",
            SkillTag::Counter => "COUNTER",
            SkillTag::Spell => "SPELL",
            SkillTag::Guard => "GUARD",
            SkillTag::Instant => "INSTANT",
        }
    }

    /// 它的位。
    pub const fn bit(self) -> u32 {
        match self {
            SkillTag::Attack => 1 << 0,
            SkillTag::Movement => 1 << 1,
            SkillTag::Counter => 1 << 2,
            SkillTag::Spell => 1 << 3,
            SkillTag::Guard => 1 << 4,
            SkillTag::Instant => 1 << 5,
        }
    }

    /// 从内容里的名字解析（大小写不敏感，允许前后空白）。
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|tag| tag.name().eq_ignore_ascii_case(name))
    }
}

/// 一组技能类别。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkillTagMask(pub u32);

impl SkillTagMask {
    /// 空集（谁都不封 / 没有类别）。
    pub const NONE: Self = Self(0);

    /// 由若干类别组成。
    pub fn of(tags: &[SkillTag]) -> Self {
        Self(tags.iter().fold(0, |mask, tag| mask | tag.bit()))
    }

    /// 含某个类别吗。
    pub const fn contains(self, tag: SkillTag) -> bool {
        self.0 & tag.bit() != 0
    }

    /// 两个掩码有交集吗（**硬控判定就是这一行**）。
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// 是否为空。
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 从内容里的名字列表解析；**遇到不认识的类别返回那个名字**（加载期报错用）。
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, String> {
        let mut mask = Self::NONE;
        for name in names {
            let Some(tag) = SkillTag::parse(name) else {
                return Err(name.trim().to_string());
            };
            mask.0 |= tag.bit();
        }
        Ok(mask)
    }

    /// 列出里面有哪些类别（报错与 UI 用）。
    pub fn names(self) -> Vec<&'static str> {
        SkillTag::ALL
            .into_iter()
            .filter(|tag| self.contains(*tag))
            .map(SkillTag::name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tag_has_a_unique_bit() {
        let mut seen = 0u32;
        for tag in SkillTag::ALL {
            assert_eq!(seen & tag.bit(), 0, "`{}` 的位与别人重复", tag.name());
            seen |= tag.bit();
        }
        assert_eq!(seen.count_ones(), SkillTag::ALL.len() as u32);
    }

    #[test]
    fn names_round_trip_and_are_case_insensitive() {
        for tag in SkillTag::ALL {
            assert_eq!(SkillTag::parse(tag.name()), Some(tag));
            assert_eq!(SkillTag::parse(&tag.name().to_lowercase()), Some(tag));
            assert_eq!(SkillTag::parse(&format!(" {} ", tag.name())), Some(tag));
        }
        assert_eq!(SkillTag::parse("STUNNED"), None, "类别不是状态名");
    }

    #[test]
    fn blocking_is_a_bitmask_intersection() {
        let attack = SkillTagMask::of(&[SkillTag::Attack, SkillTag::Counter]);
        let spell = SkillTagMask::of(&[SkillTag::Spell]);

        assert!(attack.intersects(SkillTagMask::of(&[SkillTag::Attack])));
        assert!(
            !attack.intersects(spell),
            "封的是攻击与反制，法术不该被挡住"
        );
        assert!(!attack.intersects(SkillTagMask::NONE));
        assert!(!SkillTagMask::NONE.intersects(attack));
    }

    #[test]
    fn an_unknown_name_is_reported_verbatim() {
        let error = SkillTagMask::from_names(["ATTACK", "Stuned"]).expect_err("该报错");
        assert_eq!(error, "Stuned", "把写错的那个名字原样带回来");
    }

    #[test]
    fn names_lists_what_is_inside() {
        let mask = SkillTagMask::of(&[SkillTag::Movement, SkillTag::Guard]);
        assert_eq!(mask.names(), vec!["MOVEMENT", "GUARD"]);
        assert_eq!(SkillTagMask::NONE.names(), Vec::<&str>::new());
    }
}
