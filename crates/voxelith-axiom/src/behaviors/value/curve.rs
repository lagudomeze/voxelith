//! **数值曲线**：让成长"看起来一直在涨、实际越来越慢"。
//!
//! 这两个原语是整个数值体系的骨干。设计取自 ToME4
//! （`t-engine4/game/modules/tome/class/interface/Combat.lua`），**只取机制不抄实现**。
//!
//! | 原语 | 作用 | ToME4 对应 |
//! |---|---|---|
//! | [`Scale`] | 过两个锚点拟合一条曲线，可带渐近上限 | `combatTalentScale` / `combatTalentLimit` |
//! | [`Rescale`] | 分段线性压缩，压低高端的边际收益 | `rescaleCombatStats` |
//!
//! ## 为什么两者都要
//!
//! 它们解决**不同**的问题，不能互相替代：
//!
//! - [`Scale`] 回答"**技能升到 5 级该有多强**"——锚点是设计意图，形状是曲线。
//! - [`Rescale`] 回答"**属性堆到 300 之后还有多少用**"——它只压缩，不设定目标值。
//!
//! 一个技能常常**两者都用**：先用 `Scale` 从技能等级算出基础值，
//! 再用 `Rescale` 把它接进全局的数值尺度。

use super::Value;

/// 分段线性压缩：让原始数值"看起来一直在涨"，而参与公式的有效值增长越来越慢。
///
/// 完整说明见模块文档。
#[derive(Debug, Clone, PartialEq)]
pub struct Rescale {
    /// 原始值。
    pub input: Value,
    /// 每一段的跨度（ToME4 默认 `20`）。
    pub interval: f32,
}

impl Rescale {
    /// 造一条压缩曲线（用 ToME4 的默认段宽 `20`）。
    pub fn new(input: Value) -> Self {
        Self {
            input,
            interval: 20.0,
        }
    }

    /// 换段宽（比如属性总和用 `45`）。
    pub fn with_interval(mut self, interval: f32) -> Self {
        self.interval = interval;
        self
    }

    /// 求值；`interval <= 0` 时返回 `None`（无法定义分段）。
    ///
    /// `x <= 0` 时**原样返回**：负的护甲/抗性不该被"压缩"，
    /// 否则一个减益会莫名其妙变小。这一条与 ToME4 只在正区间压缩的做法一致。
    pub fn eval(&self, x: f32) -> Option<f32> {
        if !self.interval.is_finite() || self.interval <= 0.0 {
            return None;
        }
        if x <= 0.0 {
            return Some(x);
        }
        // 第 k 段：起点 raw = interval * k(k-1)/2，起点有效值 = interval * (k-1)，
        // 斜率 = 1/k。逐段求值取最小 —— 也就是"下包络"。
        //
        // 例（interval = 20）：k=1 → 起点 0、有效 0、斜率 1/1；
        //                     k=2 → 起点 20、有效 20、斜率 1/2；
        //                     k=3 → 起点 60、有效 40、斜率 1/3。
        // 与 ToME4 的换段点（20、60、120、200…）完全一致。
        let mut best = x;
        let mut k = 2_u32;
        loop {
            let span_index = (k * (k - 1)) as f32 / 2.0;
            let start_raw = self.interval * span_index;
            if start_raw > x {
                break;
            }
            let start_effective = self.interval * (k - 1) as f32;
            let candidate = start_effective + (x - start_raw) / k as f32;
            if candidate < best {
                best = candidate;
            }
            k += 1;
            // 段数上限：`f32` 精度下再多也没有意义（raw 会先溢出）。
            if k > 64 {
                break;
            }
        }
        Some(best)
    }
}

/// 伸缩曲线：把输入 `x` 映射到 `low..high` 区间（含可选的**渐近上限**）。
///
/// ## 出处与取舍
///
/// 设计取自 ToME4 的 `combatTalentScale` / `combatStatScale` / `combatTalentLimit`
/// （`t-engine4/game/modules/tome/class/interface/Combat.lua:1286-1363`）。
/// **只取机制，不抄实现**——那边是 Lua 的"给两个锚点、拟合一条曲线"，
/// 这里重写成 Rust 的纯函数，并统一成**一个**原语（那边是三个函数，两两之间只差参数）。
///
/// ## 数学
///
/// 把输入先做一次**单调变换** `t(x)`，然后在变换后的空间里做**线性插值**：
///
/// ```text
/// m = (high - low) / (t(anchor_high) - t(anchor_low))
/// b = low - m · t(anchor_low)
/// 结果 = m · t(x) + b
/// ```
///
/// 两端是**精确锚点**：`x = anchor_low` 处得到 `low`，`x = anchor_high` 处得到 `high`。
/// 所以**作者只需要说清两端想要多少**，中间的形状由 `power` 决定。
///
/// ## 锚点必须成对选（这是 ToME4 的一个关键分工）
///
/// | 曲线喂什么 | 锚点 | 理由 |
/// |---|---|---|
/// | **等级 / 技能点** | [`Anchors::TALENT`]`= 1..5` | 天赋 1 级与 5 级 |
/// | **属性值** | [`Anchors::STAT`]`= 10..100` | 属性的起点是 10、上限是 100 |
///
/// 两者不能混用：把 `1..5` 的锚点套到属性上，`power` 会把整条曲线压得几乎没有增长
/// （因为属性从 10 到 100 的变化远大于 1 到 5）。这是移植时最容易错的一处。
///
/// ## 三种模式
///
/// | 模式 | `t(x)` | 用途 |
/// |---|---|---|
/// | [`ScalePower::Power`] | `x^power` | 最常见的成长曲线 |
/// | [`ScalePower::Log`] | `log10(x)` | 极早熟：前期就接近上限 |
/// | [`ScalePower::Limit`] | 见下 | **有渐近上限**，永远不会越过 `limit` |
///
/// `Limit` 模式求的是"过两个锚点、且以 `limit` 为渐近线的指数曲线"：
///
/// ```text
/// 结果 = limit + (low - limit) · exp(a · (t(x) - t(anchor_low)))
/// a = ln( (high - limit) / (low - limit) ) / (t(anchor_high) - t(anchor_low))
/// ```
///
/// 要求 `low`、`high`、`limit` **单调**（同增或同减），否则曲线会拐弯。
#[derive(Debug, Clone, PartialEq)]
pub struct Scale {
    /// 输入：通常是等级，也可以是属性 / 技能强度 / 资源。
    ///
    /// 用 [`Value`] 而不是裸 `f32`，是为了让"这条曲线吃什么"也成为配置的一部分
    /// （比如"按力量成长"就是 `CasterStat(strength)`）。
    pub input: Value,
    /// 输入为 `anchor_low` 时的目标值。
    pub low: f32,
    /// 输入为 `anchor_high` 时的目标值。
    pub high: f32,
    /// 变换的幂次（或对数 / 渐近上限）。
    pub power: ScalePower,
    /// 两个锚点。
    pub anchors: Anchors,
    /// 在输入上加的偏移（默认 `0`）。
    pub shift: f32,
    /// 在结果上加的偏移（默认 `0`）。
    pub add: f32,
}

/// [`Scale`] 的变换方式。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalePower {
    /// `t(x) = (x + shift)^power`。
    ///
    /// `power < 1` ⇒ 前期涨得快、后期趋缓（**减益**）；
    /// `power = 1` ⇒ 线性；`power > 1` ⇒ 后期加速。
    ///
    /// ToME4 的默认是 `0.5`。
    Power(f32),
    /// `t(x) = log10(x + shift)`：极早熟，前期就接近 `high`。
    Log,
    /// 以 `limit` 为渐近上限（见 [`Scale`] 的公式）。**永远不会越过 `limit`**。
    Limit(f32),
}

/// 曲线的两个锚点：`x = low` 处得到 `low` 值，`x = high` 处得到 `high` 值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchors {
    /// 下锚点的输入值。
    pub low: f32,
    /// 上锚点的输入值。
    pub high: f32,
}

impl Anchors {
    /// 等级 / 技能点：`1..5`。对应 ToME4 的 `combatTalentScale`。
    pub const TALENT: Self = Self {
        low: 1.0,
        high: 5.0,
    };
    /// 属性值：`10..100`。对应 ToME4 的 `combatStatScale`。
    pub const STAT: Self = Self {
        low: 10.0,
        high: 100.0,
    };
}

impl Default for Anchors {
    fn default() -> Self {
        // 默认为等级锚点：绝大多数成长曲线喂的是技能等级。
        Self::TALENT
    }
}

impl Scale {
    /// 造一条 `x^power` 曲线（锚点默认 [`Anchors::TALENT`]）。
    pub fn power(input: Value, low: f32, high: f32, power: f32) -> Self {
        Self {
            input,
            low,
            high,
            power: ScalePower::Power(power),
            anchors: Anchors::default(),
            shift: 0.0,
            add: 0.0,
        }
    }

    /// 造一条对数曲线（锚点默认 [`Anchors::TALENT`]）。
    pub fn log(input: Value, low: f32, high: f32) -> Self {
        Self {
            input,
            low,
            high,
            power: ScalePower::Log,
            anchors: Anchors::default(),
            shift: 0.0,
            add: 0.0,
        }
    }

    /// 造一条以 `limit` 为渐近上限的曲线。
    pub fn limit(input: Value, low: f32, high: f32, limit: f32) -> Self {
        Self {
            input,
            low,
            high,
            power: ScalePower::Limit(limit),
            anchors: Anchors::default(),
            shift: 0.0,
            add: 0.0,
        }
    }

    /// 换一组锚点（比如从"等级"改成"属性"）。
    ///
    /// **必须成对选**：`Anchors::TALENT` 配等级，`Anchors::STAT` 配属性。
    /// 混用会让曲线几乎没有增长（见类型文档）。
    pub fn with_anchors(mut self, anchors: Anchors) -> Self {
        self.anchors = anchors;
        self
    }

    /// 输入加偏移。
    pub fn shifted(mut self, shift: f32) -> Self {
        self.shift = shift;
        self
    }

    /// 结果加偏移。
    pub fn plus(mut self, add: f32) -> Self {
        self.add = add;
        self
    }

    /// `x` 的单调变换 `t(x)`。
    fn transform(&self, x: f32) -> f32 {
        let shifted = x + self.shift;
        match self.power {
            // 定义域保护：负数的非整数次幂是 NaN。夹到 0 以上再算。
            ScalePower::Power(power) => shifted.max(0.0).powf(power),
            // log 的定义域：`x + shift <= 0` 会得到 -inf / NaN。
            ScalePower::Log => (shifted.max(f32::MIN_POSITIVE)).log10(),
            ScalePower::Limit(_) => shifted,
        }
    }

    /// 求值（**纯函数**；参数不合法时返回 `None`）。
    ///
    /// 不合法的情况：锚点算出的斜率非有限（比如 `log` 模式下 `shift` 把两个锚点
    /// 挤到一起）、或者 `Limit` 模式的三个值不单调。
    pub fn eval(&self, x: f32) -> Option<f32> {
        let t = self.transform(x);
        match self.power {
            ScalePower::Power(_) | ScalePower::Log => {
                let t_low = self.transform(self.anchors.low);
                let t_high = self.transform(self.anchors.high);
                let span = t_high - t_low;
                if !span.is_finite() || span.abs() < f32::EPSILON {
                    return None;
                }
                let m = (self.high - self.low) / span;
                let b = self.low - m * t_low;
                Some(m * t + b + self.add)
            }
            ScalePower::Limit(limit) => {
                let t_low = self.transform(self.anchors.low);
                let t_high = self.transform(self.anchors.high);
                let span = t_high - t_low;
                if !span.is_finite() || span.abs() < f32::EPSILON {
                    return None;
                }
                let low_delta = self.low - limit;
                let high_delta = self.high - limit;
                // 三个值必须单调，且两端都要在同一侧（同为正或同为负），
                // 否则 `ln` 无定义或曲线会拐弯。
                if low_delta == 0.0 || high_delta == 0.0 {
                    return None;
                }
                if low_delta.signum() != high_delta.signum() {
                    return None;
                }
                let a = (high_delta / low_delta).ln() / span;
                let value = limit + low_delta * (a * (t - t_low)).exp();
                Some(value + self.add)
            }
        }
    }
}

/// 单元测试（单独成文件；见 `curve_tests.rs`）。
#[cfg(test)]
#[path = "curve_tests.rs"]
mod curve_tests;
