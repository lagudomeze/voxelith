//! 改值：**唯一的一条窄门**。
//!
//! 改值只有一个函数：[`change`]。两条路最后都落到它：
//!
//! - **即时事务**：代价这类"必须当场知道够不够"，调用者直接调它 ✓
//! - **消息**：[`Adjust`] 用于伤害、治疗、持续每跳这类"不需要当场知道结果"的改动 ✓
//!   由 [`apply_adjustments`] 读消息后调它 ✓
//!
//! 治疗与扣血是同一条路，只靠正负号区分（`SY-17`）✓ 消息上带着"谁改的"给战斗日志留着（`SY-05`）✓
//!
//! 这里**不夹取、不判死**：
//!
//! - 夹到上限需要**算出来的上限** ✗ 那是拿到上限的那一层的事 ✓
//! - 资源可以为负 ✓ 负值意味着什么由各自的 system 决定（血量 ≤ 0 才谈死亡）✓
//!
//! 注册与调度由装配层做：
//!
//! ```ignore
//! app.add_message::<Adjust<Health>>()
//!    .add_systems(Update, apply_adjustments::<Health>);
//! ```

use crate::value::Value;
use bevy_ecs::prelude::*;
use core::marker::PhantomData;

/// 改值的**唯一通路**：**哪个实体**身上的**哪个量**，增减多少。
///
/// 返回改后的值 ✓ 这个实体身上没有这个量 → `None`（`QL-04`：按没有处理，不崩）✓
///
/// **留痕（`SY-05`）、夹取以后都加在这一个函数里** —— 一处就够 ✓
pub fn change<Q: Value>(values: &mut Query<&mut Q>, entity: Entity, delta: f32) -> Option<f32> {
    let mut value = values.get_mut(entity).ok()?;
    let raw = value.raw_mut();
    *raw += delta;
    Some(*raw)
}

/// 改一个量的意图。**它只是"意图"** —— 真正落地走 [`change`]（见模块文档的两条路）。
///
/// - 改**资源的当前值**：`Adjust::<Health>::lose(...)`（治疗与扣血同一条路，`SY-17`）
/// - 改**属性的基础值**：`Adjust::<Strength>::gain(...)`（升级加点也走这条路）
#[derive(Message, Clone, Debug)]
pub struct Adjust<Q: Value> {
    /// 改谁。
    pub entity: Entity,
    /// 改多少（正 = 加，负 = 减）。
    pub amount: f32,
    /// 谁改的。环境伤害之类可以没有来源。
    pub cause: Option<Entity>,
    /// 把「改的是哪条资源」钉在类型上。
    _marker: PhantomData<Q>,
}

impl<Q: Value> Adjust<Q> {
    /// 加（治疗、回复）。
    pub fn gain(entity: Entity, amount: f32, cause: Option<Entity>) -> Self {
        Self {
            entity,
            amount,
            cause,
            _marker: PhantomData,
        }
    }

    /// 减（扣血、消耗）。
    pub fn lose(entity: Entity, amount: f32, cause: Option<Entity>) -> Self {
        Self {
            entity,
            amount: -amount,
            cause,
            _marker: PhantomData,
        }
    }
}

/// 把 [`Adjust<Q>`] 落到组件上。**走 [`change`]** ✓
///
/// 用 `get_mut` 逐个取、而不是遍历所有资源：没事发生的实体不会被碰 ✓
/// 也就不会被标成"变了"而去连锁触发下游的变更检测 ✓
///
/// 每条资源注册一份：`apply_adjustments::<Health>`、`::<Stamina>`、`::<Mana>`…
pub fn apply_adjustments<Q: Value>(
    mut adjustments: MessageReader<Adjust<Q>>,
    mut values: Query<&mut Q>,
) {
    for adjustment in adjustments.read() {
        change(&mut values, adjustment.entity, adjustment.amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::Health;
    use bevy_app::prelude::*;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_message::<Adjust<Health>>()
            .add_systems(Update, apply_adjustments::<Health>);
        app
    }

    #[test]
    fn gain_and_lose_go_through_the_same_path() {
        let mut app = test_app();
        let e = app.world_mut().spawn(Health::default()).id();

        app.world_mut()
            .write_message(Adjust::<Health>::gain(e, 30.0, None));
        app.world_mut()
            .write_message(Adjust::<Health>::lose(e, 11.0, None));
        app.update();

        assert_eq!(app.world().get::<Health>(e).unwrap().raw(), 19.0);
    }

    /// 资源可以为负：负值怎么解释由各自的 system 决定，core 不夹也不判死。
    #[test]
    fn values_can_go_negative() {
        let mut app = test_app();
        let e = app.world_mut().spawn(Health::default()).id();

        app.world_mut()
            .write_message(Adjust::<Health>::lose(e, 5.0, None));
        app.update();

        assert_eq!(app.world().get::<Health>(e).unwrap().raw(), -5.0);
    }

    /// 没有这条资源的实体：按 0 处理，不崩（`QL-04`）。
    #[test]
    fn missing_resource_is_not_a_crash() {
        let mut app = test_app();
        let bare = app.world_mut().spawn_empty().id();

        app.world_mut()
            .write_message(Adjust::<Health>::gain(bare, 10.0, None));
        app.update();

        assert!(app.world().get::<Health>(bare).is_none());
    }

    /// **没事发生的实体不该被标成"变了"**（否则下游变更检测被连锁触发）。
    #[test]
    fn untouched_resources_are_not_marked_changed() {
        #[derive(Resource, Default)]
        struct Marks(u32);

        fn count(changed: Query<(), Changed<Health>>, mut n: ResMut<Marks>) {
            n.0 += changed.iter().count() as u32;
        }

        let mut app = test_app();
        app.init_resource::<Marks>();
        app.add_systems(PostUpdate, count); // 排在写入系统之后

        let touched = app.world_mut().spawn(Health::default()).id();
        app.world_mut().spawn(Health::default()); // 这条不该被碰

        // 预热一帧：`Changed<T>` 连"刚添加"也算，得让生成的 tick 先过去
        app.update();
        assert_eq!(app.world().resource::<Marks>().0, 2, "刚生成的两条都算“新”");

        app.world_mut()
            .write_message(Adjust::<Health>::gain(touched, 3.0, None));
        app.update();
        assert_eq!(
            app.world().resource::<Marks>().0,
            3,
            "这一帧只该多出被改的那一条"
        );

        app.update();
        assert_eq!(
            app.world().resource::<Marks>().0,
            3,
            "没人改的第二帧不该再冒出来"
        );
    }

    /// 唯一通路带实体：这条测试直接调它（不走消息）。
    #[test]
    fn change_is_the_narrow_gate() {
        #[derive(Resource, Default)]
        struct Outcome(Vec<Option<f32>>);

        #[derive(Resource)]
        struct Actor(Entity);

        fn call(mut values: Query<&mut Health>, actor: Res<Actor>, mut out: ResMut<Outcome>) {
            out.0.push(change(&mut values, actor.0, 5.0));
        }

        let mut app = test_app();
        app.init_resource::<Outcome>();
        let hero = app.world_mut().spawn(Health(7.0)).id();
        app.insert_resource(Actor(hero));
        app.add_systems(Update, call);

        // 有这个量 → 返回改后的值
        app.update();
        assert_eq!(app.world().resource::<Outcome>().0, vec![Some(12.0)]);
        assert_eq!(app.world().get::<Health>(hero).unwrap().raw(), 12.0);

        // 没有这个量 → None（不崩，`QL-04`）
        let bare = app.world_mut().spawn_empty().id();
        app.insert_resource(Actor(bare));
        app.update();
        assert_eq!(
            app.world().resource::<Outcome>().0,
            vec![Some(12.0), None],
            "没有这条量的实体返回 None"
        );
    }
}
