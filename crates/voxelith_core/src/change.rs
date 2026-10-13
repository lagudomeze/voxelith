//! 改值：**当前值的唯一写入通路**。
//!
//! 资源的当前值只由一条消息改：治疗与扣血是同一条路，只靠正负号区分（`SY-17`）；
//! 消息上带着“谁改的”，给将来的战斗日志留着（`SY-05`）。
//!
//! 这里**不夹取、不判死**：
//!
//! - 夹到上限需要**算出来的上限**，而“修正怎么挂”是应用侧定的，core 不知道 ✗
//!   → 上限夹取、上限降低时的压缩，都留在拿到上限的那一层；
//! - 资源可以为负，负值意味着什么由各自的 system 决定（血量 ≤ 0 才谈死亡）。
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

/// 改一个量存着的那个数。\n///\n/// - 改**资源的当前值**：Adjust::<Health>::lose(...)（治疗与扣血同一条路，SY-17）\n/// - 改**属性的基础值**：Adjust::<Strength>::gain(...)（升级加点也走这条路）
#[derive(Message, Clone, Debug)]
pub struct Adjust<Q: Value> {
    /// 改谁。
    pub entity: Entity,
    /// 改多少（正 = 加，负 = 减）。
    pub amount: f32,
    /// 谁改的。环境伤害之类可以没有来源。
    pub cause: Option<Entity>,
    /// 把「改的是哪条资源」钉在类型上。
    ///
    /// 这里用 `PhantomData<R>` 而不是 `PhantomData<fn() -> R>`：`R: Resourceable` 已经蕴含
    /// `R: Send + Sync + 'static`（`Component` 的要求），所以两种写法在 auto trait 上等价，
    /// 直白的那种更好读。
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

/// 把 [`Adjust<R>`] 落到组件上。**这是当前值的唯一写入口。**
///
/// 用 `get_mut` 逐个取、而不是遍历所有资源：没事发生的实体不会被碰，
/// 也就不会被标成“变了”而去连锁触发下游的变更检测。
///
/// 每条资源注册一份：`apply_adjustments::<Health>`、`::<Stamina>`、`::<Mana>`…
pub fn apply_adjustments<Q: Value>(
    mut adjustments: MessageReader<Adjust<Q>>,
    mut values: Query<&mut Q>,
) {
    for adjustment in adjustments.read() {
        let Ok(mut value) = values.get_mut(adjustment.entity) else {
            continue; // 这个实体没有这个量——按 0 处理，不崩（`QL-04`）
        };
        *value.raw_mut() += adjustment.amount;
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

        app.world_mut().write_message(Adjust::<Health>::gain(e, 30.0, None));
        app.world_mut().write_message(Adjust::<Health>::lose(e, 11.0, None));
        app.update();

        assert_eq!(app.world().get::<Health>(e).unwrap().raw(), 19.0);
    }

    /// 资源可以为负：负值怎么解释由各自的 system 决定，core 不夹也不判死。
    #[test]
    fn values_can_go_negative() {
        let mut app = test_app();
        let e = app.world_mut().spawn(Health::default()).id();

        app.world_mut().write_message(Adjust::<Health>::lose(e, 5.0, None));
        app.update();

        assert_eq!(app.world().get::<Health>(e).unwrap().raw(), -5.0);
    }

    /// 没有这条资源的实体：按 0 处理，不崩（`QL-04`）。
    #[test]
    fn missing_resource_is_not_a_crash() {
        let mut app = test_app();
        let bare = app.world_mut().spawn_empty().id();

        app.world_mut().write_message(Adjust::<Health>::gain(bare, 10.0, None));
        app.update();

        assert!(app.world().get::<Health>(bare).is_none());
    }

    /// **没事发生的实体不该被标成“变了”**（否则下游变更检测被连锁触发）。
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

        // 预热一帧：`Changed<T>` 连“刚添加”也算，得让生成的 tick 先过去
        app.update();
        assert_eq!(app.world().resource::<Marks>().0, 2, "刚生成的两条都算“新”");

        app.world_mut().write_message(Adjust::<Health>::gain(touched, 3.0, None));
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
}
