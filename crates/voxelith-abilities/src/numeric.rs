//! **数值层**：资源的上限与按秒自然回复。
//!
//! 旧引擎把这三件事写死在池模板里（`vocabulary.ron` 的 `max` / `regen` 字段）：
//!
//! ```text
//! 一个池 = 当前值 + 上限 + 每秒回复          （回复**全局按池名**生效，不分角色）
//! ```
//!
//! 新栈（gauge）里属性只是"名字 → 一个数"，**没有上限也没有回复**。所以这两件事
//! 落到内容与一条系统上：
//!
//! ```text
//! 上限   一条表达式属性：MaxHealth = Constitution * 10
//! 回复   attributes.ron 的 regen 规则 + 本模块的 regenerate 系统
//! ```
//!
//! ## 为什么把"这一帧该回多少"算成字面量再施加
//!
//! gauge 的 `InstantModifierSet` 只收**字面量**（或预先编译好的表达式），
//! 而"回多少"要读该角色自己的 `HealthRegen`——那是运行期才知道的数。
//! 所以这里自己算，再把结果作为**字面量**推给 gauge：
//!
//! ```text
//! amount = rate × 本帧时长          然后与"离上限还差多少"取小 ⇒ 封顶
//! ```
//!
//! 顺带解决了旧引擎的一个毛病：旧回复可能把池**顶过上限**（要靠别处再夹一次），
//! 这里"离上限还差多少"就是天然的上限约束。
//!
//! ## 这一层不管什么
//!
//! - **不生成**资源：当前值在 `attributes.ron` 的 `base` 里铺。
//! - **不管消耗**：扣费在 [`crate::casting`] 的门控与结算里。
//! - **不管上限的施加**：`MaxHealth` 现在就是一条**表达式属性**——角色定义迁过来以后，
//!   改成"装备/职业给 MaxHealth 加一条修饰符"即可，本模块不用动。

use bevy::prelude::*;
use bevy_gauge::prelude::{Attributes, AttributesMut, InstantExt, InstantModifierSet};

use crate::attributes::{AttributeSetRon, RegenRon};

/// 自然回复规则（从 `attributes.ron` 的内容来）。
#[derive(Resource, Debug, Clone, Default)]
pub struct RegenRules(pub Vec<RegenRon>);

impl RegenRules {
    /// 从一份属性定义里取。
    pub fn from_set(set: &AttributeSetRon) -> Self {
        Self(set.regen.clone())
    }

    /// 有几条。
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是不是空的。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 按规则回复所有角色的资源（**只往上长，封顶在上限**）。
///
/// 每帧一遍：规则少（个位数）、角色少，代价可以忽略；换来的是"回复速度天然是每秒"，
/// 与帧率无关（`rate × delta`）。
///
/// **为什么用 `ParamSet`**：`AttributesMut` 内部就是 `Query<&mut Attributes>`，
/// 与只读的 `Query<&Attributes>` 在同一个系统里会撞 B0001（本项目的老问题）。
/// `ParamSet` 让"先只读收集、再统一写"这两段互不重叠——顺带也保证了读到的
/// `Health` / `MaxHealth` / `HealthRegen` 是**同一帧的一致快照**。
pub fn regenerate(
    time: Res<Time>,
    rules: Res<RegenRules>,
    mut gauge: ParamSet<(Query<(Entity, &Attributes)>, AttributesMut)>,
) {
    if rules.is_empty() {
        return;
    }
    let delta = time.delta_secs();
    if delta <= 0.0 {
        return;
    }

    // ① 只读：算出每个角色这一帧该回哪些属性、各回多少。
    let mut plan: Vec<(Entity, Vec<(String, f32)>)> = Vec::new();
    {
        let readers = gauge.p0();
        for (entity, attributes) in readers.iter() {
            let mut pushes: Vec<(String, f32)> = Vec::new();
            for RegenRon {
                attribute,
                max,
                rate,
            } in &rules.0
            {
                let per_second = attributes.value(rate);
                if per_second <= 0.0 {
                    continue;
                }
                let current = attributes.value(attribute);
                let cap = attributes.value(max);
                // 已经满了（或上限没写）就别推：`<=` 而不是 `<`，免得在满值时反复标脏。
                let room = cap - current;
                if room <= 0.0 {
                    continue;
                }
                pushes.push((attribute.clone(), (per_second * delta).min(room)));
            }
            if !pushes.is_empty() {
                plan.push((entity, pushes));
            }
        }
    }

    // ② 写：一次性把这一帧的回复推给 gauge。
    let mut writer = gauge.p1();
    for (entity, pushes) in plan {
        let mut instant = InstantModifierSet::new();
        for (attribute, amount) in &pushes {
            instant.push_add(attribute, *amount);
        }
        writer.apply_instant(&instant, &[], entity);
    }
}

/// 注册数值层。
pub struct NumericPlugin;

impl Plugin for NumericPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RegenRules>()
            .add_systems(Update, regenerate);
    }
}
