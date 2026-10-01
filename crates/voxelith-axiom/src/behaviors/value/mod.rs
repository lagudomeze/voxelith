//! `Value`：**受控闭集**的表达式原语（引擎能力，不随内容增长）。
//!
//! 它只做算术与查表，不认识"伤害""治疗"这类语义——那些由 [`Effect`](crate::behaviors::effect::Effect)
//! 组合出来。求值**绝不 panic**：缺池 / 缺属性一律回退 `0.0`（RON 是内容，内容写错不能炸运行时）。
//!
//! 数值曲线的两个原语（[`Scale`] / [`Rescale`]）在 [`curve`] 子模块里。

mod curve;

pub use curve::{Anchors, Rescale, Scale, ScalePower};

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryFilter;

use crate::atoms::actor::{Resources, Stats};
use crate::behaviors::content::{ResourceId, StatId};

/// 表达式里的"谁"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Who {
    /// 施加者（技能释放者 / 状态的来源）。
    Caster,
    /// 承受者（技能目标 / 状态宿主）。
    Target,
}

/// 表达式：数值来源。
///
/// **不派生 `Deserialize`**：RON 里的引用是名字（`CasterStat("strength")`），
/// 由 [`ValueRon`](crate::behaviors::content::ValueRon) + 加载器翻译成 ID。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 字面量。
    Literal(f32),
    /// 本次结算的强度：对抗的差值 / 比值（见 [docs/combat-design.md](../../../../docs/combat-design.md) §5）。
    ///
    /// 不在对抗里（比如状态 tick）时按 `0.0` 算。
    SkillPower,
    /// 施法者的属性最终值。
    CasterStat(StatId),
    /// 目标的属性最终值。
    TargetStat(StatId),
    /// 某一方的资源池当前值。
    Resource {
        /// 哪个池。
        pool: ResourceId,
        /// 哪一方。
        who: Who,
    },
    /// 求和。
    Sum(Vec<Value>),
    /// 求积。
    Mul(Box<Value>, Box<Value>),
    /// 取负。
    Neg(Box<Value>),
    /// **伸缩曲线**：把一个输入（通常是等级）映射到一个"经过设计"的数值。
    ///
    /// 见 [`Scale`]。这是整套数值设计的骨干——**所有成长曲线都走它**，
    /// 而不是在每个技能里各写一段 `a * level + b`。
    Scale(Box<Scale>),
    /// **分段线性压缩**：让原始数值"看起来一直在涨"，而参与公式的有效值增长越来越慢。
    ///
    /// 见 [`Rescale`]。这是 ToME4 数值观的核心：**玩家看到的数字线性增长，
    /// 而真正进公式的数字被压平**。两个好处同时拿到——升级有快感，平衡不爆炸。
    Rescale(Box<Rescale>),
}

/// 分段线性压缩（ToME4 的 `rescaleCombatStats`）。
///
/// ## 它在解决什么
///
/// 装备与等级会把"力量""护甲"这类原始值推到几百。如果这些数**直接**进公式，
/// 后期每一点加成的收益会和前期一样大，数值必然失控。
///
/// 但简单乘以一个系数（线性缩放）又很糟：玩家看到 `+100 力量` 只变成 `+20 有效值`，
/// 会觉得加点没意义。**取对数**更糟：前期每一项都值很多，后期几乎无感，玩家的
/// 直觉完全对不上。
///
/// 所以 ToME4 用一条**分段线性凸包**：
///
/// ```text
/// 有效 = min(
///     x,
///     20 + (x - 20) / 2,      // 20 之后：每 2 点原始值换 1 点有效值
///     40 + (x - 60) / 3,      // 60 之后：每 3 点换 1 点
///     60 + (x - 120) / 4,     // 120 之后：每 4 点换 1 点
///     ...
/// )
/// ```
///
/// 结果是**一段段直线的下包络**：每一段都还在涨（玩家看得见收益），
/// 但斜率逐段变缓（平衡可控）。ToME4 源码里的注释把它讲得很直白：
/// *"the first twenty ranks cost 1 point each, the second twenty cost two each"*。
///
/// ## 为什么比"对数"好
///
/// 对数曲线在低端**极陡**（前几点收益巨大），在高端**极平**（几乎无收益），
/// 而且玩家无法心算。分段线性每一段的兑换率都是**整数比**——
/// "现在每 4 点换 1 点"是能说清楚、能在 UI 上展示的。
///
/// ## 换段点
///
/// `interval` 是每一段的原始值跨度（ToME4 默认 `20`）。
/// ToME4 还给"属性总和"用了另一套（`interval = 45`），因为属性总和天然比
/// 力量/护甲小。所以这里也做成参数。
#[derive(Debug, Clone, PartialEq)]
pub struct EvalContext<'a> {
    /// 施法者的池。
    pub caster_resources: Option<&'a Resources>,
    /// 施法者的属性。
    pub caster_stats: Option<&'a Stats>,
    /// 目标的池。
    pub target_resources: Option<&'a Resources>,
    /// 目标的属性。
    pub target_stats: Option<&'a Stats>,
    /// 本次结算的强度（对抗差值 / 比值）。
    pub skill_power: f32,
}

impl Default for EvalContext<'_> {
    fn default() -> Self {
        Self {
            caster_resources: None,
            caster_stats: None,
            target_resources: None,
            target_stats: None,
            skill_power: 0.0,
        }
    }
}

impl EvalContext<'_> {
    /// 某一方的池。
    pub fn resources(&self, who: Who) -> Option<&Resources> {
        match who {
            Who::Caster => self.caster_resources,
            Who::Target => self.target_resources,
        }
    }

    /// 某一方的属性。
    pub fn stats(&self, who: Who) -> Option<&Stats> {
        match who {
            Who::Caster => self.caster_stats,
            Who::Target => self.target_stats,
        }
    }
}

/// 求值；缺数据一律 `0.0`。
pub fn eval(value: &Value, ctx: &EvalContext) -> f32 {
    match value {
        Value::Literal(number) => *number,
        Value::SkillPower => ctx.skill_power,
        Value::CasterStat(stat) => ctx.stats(Who::Caster).map_or(0.0, |s| s.get(*stat)),
        Value::TargetStat(stat) => ctx.stats(Who::Target).map_or(0.0, |s| s.get(*stat)),
        Value::Resource { pool, who } => ctx.resources(*who).map_or(0.0, |r| r.current(*pool)),
        Value::Sum(items) => items.iter().map(|item| eval(item, ctx)).sum(),
        Value::Mul(left, right) => eval(left, ctx) * eval(right, ctx),
        Value::Neg(inner) => -eval(inner, ctx),
        // 曲线参数不合法时按 `0.0` 处理（与"缺数据一律 0.0"的约定一致），
        // 并且**不 panic**：内容配错不该让一局游戏崩掉。
        Value::Scale(scale) => scale.eval(eval(&scale.input, ctx)).unwrap_or(0.0),
        Value::Rescale(rescale) => rescale.eval(eval(&rescale.input, ctx)).unwrap_or(0.0),
    }
}

/// 从世界里搭一份求值快照。
///
/// 只在执行效果的瞬间借用（借完即还），所以**不会与随后的可变写入口打架**。
/// 具名只读数据来源：**按实体取一份只读数据**。
///
/// 实现方式决定"数据从哪来"：`Query<T>`（世界查询）或 `&T`（调用方手里已经有值）。
/// 效果执行器只需要它，不必知道对方究竟是哪一种。
pub trait Provider<T: ?Sized> {
    /// 取某个实体的数据。
    fn provide(&self, entity: Entity) -> Option<&T>;
}

impl<T: Component, F: QueryFilter> Provider<T> for Query<'_, '_, &T, F> {
    fn provide(&self, entity: Entity) -> Option<&T> {
        self.get(entity).ok()
    }
}

/// 借来的查询同样可用（调用方常常只持有 `&Query`）。
impl<T: Component, F: QueryFilter> Provider<T> for &Query<'_, '_, &T, F> {
    fn provide(&self, entity: Entity) -> Option<&T> {
        self.get(entity).ok()
    }
}

impl<T: ?Sized> Provider<T> for &T {
    fn provide(&self, entity: Entity) -> Option<&T> {
        if entity == Entity::PLACEHOLDER {
            None
        } else {
            Some(self)
        }
    }
}

/// `None` = 这个系统不提供这类数据（相关效果自然什么都不做，也不会 panic）。
impl<T: ?Sized> Provider<T> for Option<&T> {
    fn provide(&self, _entity: Entity) -> Option<&T> {
        *self
    }
}

/// 求值快照：一次解析期间看到的双方数据 + 强度。
///
/// 它**按值借入**数据来源（不借用查询本身），所以在哪里构造都不会和可变借用打架。

pub struct EvalTargets {
    /// 施法者。
    pub caster: Entity,
    /// 目标（可能与施法者相同）。
    pub target: Option<Entity>,
    /// 本次结算的强度。
    pub skill_power: f32,
}

/// 用数据来源构造快照（池与属性各给一个 [`Provider`]）。
pub fn snapshot<'a, P, S>(targets: &EvalTargets, pools: &'a P, stats: &'a S) -> EvalContext<'a>
where
    P: Provider<Resources>,
    S: Provider<Stats>,
{
    let caster_resources = pools.provide(targets.caster);
    let caster_stats = stats.provide(targets.caster);
    let (target_resources, target_stats) = match targets.target {
        Some(target) => (pools.provide(target), stats.provide(target)),
        None => (None, None),
    };
    EvalContext {
        caster_resources,
        caster_stats,
        target_resources,
        target_stats,
        skill_power: targets.skill_power,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(caster_resources: &'a Resources, caster_stats: &'a Stats) -> EvalContext<'a> {
        EvalContext {
            caster_resources: Some(caster_resources),
            caster_stats: Some(caster_stats),
            skill_power: 7.0,
            ..Default::default()
        }
    }

    #[test]
    fn arithmetic_follows_the_tree() {
        let resources = Resources::default();
        let stats = Stats::from_base([(StatId(0), 4.0)]);
        let context = ctx(&resources, &stats);

        let value = Value::Sum(vec![
            Value::CasterStat(StatId(0)),
            Value::Literal(5.0),
            Value::Neg(Box::new(Value::Literal(2.0))),
        ]);
        assert_eq!(eval(&value, &context), 7.0);

        let product = Value::Mul(Box::new(Value::Literal(3.0)), Box::new(value));
        assert_eq!(eval(&product, &context), 21.0);
    }

    #[test]
    fn skill_power_flows_into_effects() {
        let resources = Resources::default();
        let stats = Stats::from_base([]);
        let context = ctx(&resources, &stats);
        assert_eq!(eval(&Value::SkillPower, &context), 7.0);
    }

    #[test]
    fn missing_data_reads_as_zero_instead_of_panicking() {
        let context = EvalContext::default();
        assert_eq!(eval(&Value::SkillPower, &context), 0.0);
        assert_eq!(eval(&Value::CasterStat(StatId(3)), &context), 0.0);
        assert_eq!(eval(&Value::TargetStat(StatId(3)), &context), 0.0);
        assert_eq!(
            eval(
                &Value::Resource {
                    pool: ResourceId(1),
                    who: Who::Target
                },
                &context
            ),
            0.0
        );
    }

    #[test]
    fn each_side_reads_its_own_data() {
        use crate::atoms::actor::Pool;

        let mut caster_resources = Resources::default();
        caster_resources.define(ResourceId(0), Pool::full(30.0));
        let caster_stats = Stats::from_base([(StatId(0), 1.0)]);
        let mut target_resources = Resources::default();
        target_resources.define(ResourceId(0), Pool::full(9.0));
        let target_stats = Stats::from_base([(StatId(0), 2.0)]);

        let context = EvalContext {
            caster_resources: Some(&caster_resources),
            caster_stats: Some(&caster_stats),
            target_resources: Some(&target_resources),
            target_stats: Some(&target_stats),
            skill_power: 0.0,
        };

        assert_eq!(
            eval(
                &Value::Resource {
                    pool: ResourceId(0),
                    who: Who::Caster
                },
                &context
            ),
            30.0
        );
        assert_eq!(
            eval(
                &Value::Resource {
                    pool: ResourceId(0),
                    who: Who::Target
                },
                &context
            ),
            9.0
        );
        assert_eq!(eval(&Value::TargetStat(StatId(0)), &context), 2.0);
    }
}
