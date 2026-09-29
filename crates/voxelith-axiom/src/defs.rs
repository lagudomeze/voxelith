//! 定义工具：让"新增一种属性 / 资源 / 伤害类型 / 状态"**只改一处**。
//!
//! [`voxelith_defs!`] 由一份变体列表生成：
//!
//! | 生成项 | 用途 |
//! |---|---|
//! | 枚举本身 | 唯一定义点 |
//! | `COUNT` | 按变体定宽的数组列数（属性值表、抵抗表自动跟随） |
//! | `ALL` | 遍历顺序 = 定义顺序 |
//! | `index()` | 紧凑下标，用于数组索引 |
//! | `name()` | 稳定标识串（存档 / 配置 / 日志） |
//!
//! 因此新增一个变体后：
//!
//! 1. 所有 `[T; X::COUNT]` 的定宽数组**自动扩列**，不会漏项；
//! 2. 任何别处的穷尽 `match` 立即**编译期报错**——这是"遗漏定义"的第一道兜底。
//!
//! 第二道兜底是各域注册表的 `build()`（必须有内容的项缺失 → 加载期报错），
//! 见 `docs/combat-mechanics.md` §3.3。

/// 由变体列表生成"唯一定义点"枚举。用法见 [模块文档](self)。
#[macro_export]
macro_rules! voxelith_defs {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident = $label:literal),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $($variant),*
        }

        impl $name {
            /// 变体数量：由定义列表算出，新增变体自动 +1。
            pub const COUNT: usize = 0 $(+ { let _ = stringify!($variant); 1 })*;

            /// 全部变体，顺序 = 定义顺序（也是 `index()` 的取值顺序）。
            pub const ALL: [Self; Self::COUNT] = [$(Self::$variant),*];

            /// 紧凑下标：用于按变体索引的定宽数组。
            pub const fn index(self) -> usize {
                self as usize
            }

            /// 稳定标识串：改字面量即改对外契约（存档 / 配置依赖它）。
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),*
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    voxelith_defs! {
        /// 测试用定义：验证宏只依赖变体列表。
        enum Sample {
            Alpha = "alpha",
            Beta = "beta",
            Gamma = "gamma",
        }
    }

    #[test]
    fn count_and_all_follow_the_definition_list() {
        assert_eq!(Sample::COUNT, 3);
        assert_eq!(Sample::ALL, [Sample::Alpha, Sample::Beta, Sample::Gamma]);
    }

    #[test]
    fn index_is_definition_order() {
        assert_eq!(Sample::Alpha.index(), 0);
        assert_eq!(Sample::Beta.index(), 1);
        assert_eq!(Sample::Gamma.index(), 2);
    }

    #[test]
    fn all_is_indexed_by_index() {
        for id in Sample::ALL {
            assert_eq!(Sample::ALL[id.index()], id);
        }
    }

    #[test]
    fn name_returns_the_declared_label() {
        assert_eq!(Sample::Beta.name(), "beta");
    }
}
