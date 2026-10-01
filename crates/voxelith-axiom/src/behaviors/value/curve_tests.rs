//! [`super`] 里两条曲线的单元测试。
//!
//! 平铺成**一个**模块（不嵌套 `mod`）：用 `#[path]` 挂进来时本文件**就是**
//! `curve::curve_tests`，再嵌一层会让 `super` 指错地方。

use super::*;
// `EvalContext` / `eval` 在**父模块**（`value`）里，`curve` 自己不用它们。
use super::super::{EvalContext, eval};

fn curve(low: f32, high: f32, power: f32) -> Scale {
    Scale::power(Value::Literal(0.0), low, high, power)
}

#[test]
fn anchors_are_exact() {
    // **这是整个原语的契约**：输入 1 得到 low、输入 5 得到 high。
    // 作者只要说清"1 级和 5 级各想要多少"，中间形状交给 power。
    let s = curve(10.0, 50.0, 0.5);
    assert!((s.eval(1.0).unwrap() - 10.0).abs() < 1e-3, "x=1 该得 low");
    assert!((s.eval(5.0).unwrap() - 50.0).abs() < 1e-3, "x=5 该得 high");
}

#[test]
fn power_below_one_front_loads_the_growth() {
    // `power < 1` = 前期涨得快、后期变慢。这是绝大多数减益曲线的形状。
    let s = curve(0.0, 100.0, 0.5);
    let at_three = s.eval(3.0).unwrap();
    let linear_at_three = 50.0; // 线性的话 x=3 该是一半
    assert!(
        at_three > linear_at_three,
        "power=0.5 时 x=3 该超过线性的一半（实际 {at_three}）"
    );
}

#[test]
fn power_above_one_back_loads_the_growth() {
    let s = curve(0.0, 100.0, 2.0);
    let at_three = s.eval(3.0).unwrap();
    assert!(
        at_three < 50.0,
        "power=2 时 x=3 该低于线性的一半（实际 {at_three}）"
    );
}

#[test]
fn limit_mode_never_crosses_the_limit() {
    // `combatTalentLimit` 的存在意义：**永远不越过上限**。
    // 这条守住"渐近"这个性质，而不是某个具体数值。
    let s = Scale::limit(Value::Literal(0.0), 10.0, 40.0, 50.0);
    assert!((s.eval(1.0).unwrap() - 10.0).abs() < 1e-3, "x=1 该得 low");
    assert!((s.eval(5.0).unwrap() - 40.0).abs() < 1e-3, "x=5 该得 high");
    for x in [1.0, 2.0, 5.0, 20.0, 100.0, 1000.0] {
        let v = s.eval(x).unwrap();
        // **渐近是"趋近"而不是"严格小于"**：浮点上会正好落在上限处。
        // 断言写成 `<=` 才对，否则会把一条正确的曲线判成错的（踩过）。
        assert!(v <= 50.0 + 1e-3, "x={x} 时越过了上限 50（实际 {v}）");
    }
    // 单调递增。
    let mut last = f32::MIN;
    for step in 1..40 {
        let v = s.eval(step as f32).unwrap();
        assert!(v > last, "该单调递增：x={step} 得 {v}，上一个 {last}");
        last = v;
    }
}

#[test]
fn limit_mode_rejects_non_monotone_parameters() {
    // low / high / limit 必须单调，否则曲线会拐弯 —— 这种配置应当被拒绝
    // （返回 None，而不是画出一条怪曲线）。
    // limit 夹在 low 与 high 之间 ⇒ 非法。
    let bad = Scale::limit(Value::Literal(0.0), 10.0, 60.0, 40.0);
    assert!(bad.eval(3.0).is_none(), "非单调的参数该被拒绝");
}

#[test]
fn log_mode_is_monotone_and_starts_above_the_power_curve() {
    // 对数模式"极早熟"：前期就接近 high。
    let log = Scale::log(Value::Literal(0.0), 0.0, 100.0);
    let pow = curve(0.0, 100.0, 0.5);
    assert!(
        log.eval(3.0).unwrap() > pow.eval(3.0).unwrap(),
        "对数模式前期该比 power=0.5 更接近上限"
    );
    let mut last = f32::MIN;
    for step in 1..30 {
        let v = log.eval(step as f32).unwrap();
        assert!(v > last, "对数模式该单调");
        last = v;
    }
}

#[test]
fn shift_and_add_move_the_anchors_as_documented() {
    let s = curve(10.0, 50.0, 0.5).plus(100.0);
    assert!((s.eval(1.0).unwrap() - 110.0).abs() < 1e-3, "add 该抬整体");
    // shift 改的是输入侧，锚点仍然落在 x=1 / x=5 上（因为变换里加了 shift，
    // 而锚点的 t 也用同一个 shift）—— 所以 low/high 依然精确。
    let s2 = curve(10.0, 50.0, 0.5).shifted(3.0);
    assert!((s2.eval(1.0).unwrap() - 10.0).abs() < 1e-3);
    assert!((s2.eval(5.0).unwrap() - 50.0).abs() < 1e-3);
}

#[test]
fn evaluation_is_deterministic_and_pure() {
    // 曲线必须是纯函数：同样的输入永远同样的输出（否则战斗无法复现）。
    let s = Scale::limit(Value::Literal(0.0), 0.5, 2.5, 3.0);
    let first = s.eval(7.0).unwrap();
    for _ in 0..100 {
        assert_eq!(s.eval(7.0).unwrap(), first);
    }
}

#[test]
fn the_curve_composes_with_other_expressions() {
    // 真实用法：`2 * Scale(...)` —— 曲线只是表达式树里的一个节点。
    let ctx = EvalContext {
        skill_power: 0.0,
        caster_resources: None,
        caster_stats: None,
        target_resources: None,
        target_stats: None,
    };
    let s = curve(1.0, 5.0, 1.0); // 线性：x=1→1, x=5→5
    let composed = Value::Mul(
        Box::new(Value::Literal(2.0)),
        Box::new(Value::Scale(Box::new(s))),
    );
    // 输入是字面量 0 ⇒ 走 eval 时曲线拿到 0；锚点外推得到 1 - 1 = 0。
    // 这里只断言"能算出来且是有限值"，组合性才是重点。
    let value = eval(&composed, &ctx);
    assert!(value.is_finite(), "组合后该得到有限值，实际 {value}");
}

/// **这组断言直接取自 ToME4 的换段点**（`Combat.lua:1219-1238`）。
///
/// 值本身不重要，重要的是**形状**：每一段还在涨、但斜率逐段变缓。
/// 如果哪天有人"顺手优化"成 `ln(x)`，这些断言会立刻失败——
/// 而对数曲线在低端太陡、高端太平，且玩家无法心算，是有意不选的。
#[test]
fn reproduces_tome4_breakpoints() {
    let r = Rescale::new(Value::Literal(0.0));
    for (raw, expected) in [
        (20.0, 20.0),
        (40.0, 30.0),
        (60.0, 40.0),
        (120.0, 60.0),
        (200.0, 80.0),
        (300.0, 100.0),
    ] {
        let got = r.eval(raw).unwrap();
        assert!(
            (got - expected).abs() < 1e-3,
            "raw {raw} 该压成 {expected}，实际 {got}"
        );
    }
}

#[test]
fn is_identity_below_the_first_breakpoint() {
    // 第一段之前是**恒等**：前期每一点加成都是实打实的。
    // 这正是"玩家看到 +1 就是 +1"的那部分，不能被压缩吃掉。
    let r = Rescale::new(Value::Literal(0.0));
    for raw in [1.0, 5.0, 10.0, 19.0, 20.0] {
        assert_eq!(r.eval(raw).unwrap(), raw, "raw {raw} 该原样通过");
    }
}

#[test]
fn is_monotone_and_concave() {
    let r = Rescale::new(Value::Literal(0.0));
    let mut last = f32::MIN;
    let mut last_gain = f32::MAX;
    for step in 0..400 {
        let x = step as f32;
        let v = r.eval(x).unwrap();
        assert!(v >= last, "该单调不减：x={x} 得 {v}，上一个 {last}");
        if step > 0 {
            let gain = v - last;
            // **凹性**：增长速度只会变慢，不会变快。
            assert!(
                gain <= last_gain + 1e-4,
                "该凹（增速递减）：x={x} 这步涨了 {gain}，上一步涨了 {last_gain}"
            );
            last_gain = gain;
        }
        last = v;
    }
}

#[test]
fn never_increases_a_value() {
    // 压缩永远不该**放大**数值 —— 那会让"压缩"变成"增益"，
    // 反过来把平衡推向失控。
    let r = Rescale::new(Value::Literal(0.0));
    for step in 1..500 {
        let x = step as f32 * 2.0;
        assert!(r.eval(x).unwrap() <= x, "x={x} 被放大了");
    }
}

#[test]
fn non_positive_passes_through_untouched() {
    // 负的护甲 / 负的抗性不该被"压缩"。ToME4 也只在正区间压缩。
    // 否则一个减益会莫名其妙变小，玩家完全看不懂。
    let r = Rescale::new(Value::Literal(0.0));
    for raw in [-100.0, -20.0, -1.0, 0.0] {
        assert_eq!(r.eval(raw).unwrap(), raw, "{raw} 该原样通过");
    }
}

#[test]
fn interval_changes_the_breakpoints() {
    // 段宽可配：ToME4 给"属性总和"用了 45 而不是 20，
    // 因为属性总和天然比力量/护甲小 —— 用 20 会压得太狠。
    let r = Rescale::new(Value::Literal(0.0)).with_interval(45.0);
    assert!(
        (r.eval(45.0).unwrap() - 45.0).abs() < 1e-3,
        "45 该是第一个换段点"
    );
    // 45 之前恒等。
    assert!((r.eval(30.0).unwrap() - 30.0).abs() < 1e-3);
    // 换段点之后开始变缓。
    assert!(r.eval(90.0).unwrap() < 90.0);
}

#[test]
fn rejects_a_degenerate_interval() {
    // 段宽为 0 或负数无法定义分段 —— 返回 None 而不是死循环/panic。
    for bad in [0.0, -20.0, f32::NAN, f32::INFINITY] {
        let r = Rescale::new(Value::Literal(0.0)).with_interval(bad);
        assert!(r.eval(50.0).is_none(), "段宽 {bad} 该被拒绝");
    }
}
