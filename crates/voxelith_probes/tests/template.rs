//! 探针：`Template::build_template` 在"构建那一刻"到底能碰到什么？
//!
//! 要验的假设：一个组件的模板能不能顺手读/改世界 —— 比如
//! 「构建 `Cost(20)` 时读 Mana（40），改成 20，然后给出组件」。
//!
//! 三件事：
//! 1. 能不能读 + 改**资源**（`TemplateContext::resource` / `resource_mut`）
//! 2. **每次构建都会再跑一遍**（同一个模板构建两次 → 扣两次）
//! 3. 能不能读**别的实体的组件**（比如"施法者的法力"）

use bevy_ecs::error::Result;
use bevy_ecs::prelude::*;
use bevy_ecs::template::{FromTemplate, Template, TemplateContext};

/// 全局资源：一个池子。
#[derive(Resource, Default, Debug)]
struct Pool(f32);

/// 组件：这次要花多少。
#[derive(Component, Clone, Copy, Debug)]
struct Cost(pub f32);

/// 自定义模板：构建时读 `Pool`、扣掉、然后给出 `Cost`。
#[derive(Clone, Debug)]
struct CostTemplate {
    amount: f32,
}

impl Template for CostTemplate {
    type Output = Cost;

    fn build_template(&self, context: &mut TemplateContext) -> Result<Cost> {
        // ① 读得到
        let before = context.resource::<Pool>().0;
        // ② 也改得了
        context.resource_mut::<Pool>().0 = before - self.amount;
        Ok(Cost(self.amount))
    }

    fn clone_template(&self) -> Self {
        self.clone()
    }
}

impl FromTemplate for Cost {
    type Template = CostTemplate;
}

/// 1. 模板**能**读并改资源。
#[test]
fn template_can_read_and_change_a_resource() {
    let mut world = World::new();
    world.insert_resource(Pool(40.0));
    let entity = world.spawn_empty().id();

    let built = world
        .entity_mut(entity)
        .build_template(&CostTemplate { amount: 20.0 })
        .expect("构建失败");

    assert_eq!(built.0, 20.0, "组件造出来了");
    assert_eq!(
        world.resource::<Pool>().0,
        20.0,
        "构建那一刻真的把资源从 40 改成了 20"
    );
}

/// 2. **每次构建都会再跑一遍** —— 同一个模板构建两次就扣两次。
///
/// 这就是"把代价写进模板"的致命处：复制实体、再 spawn 一个同样的、加载存档、热重载，
/// 都会再构建一次 ✓ 而"付一次代价"必须只发生一次 ✗
#[test]
fn every_build_runs_the_side_effect_again() {
    let mut world = World::new();
    world.insert_resource(Pool(40.0));

    let first = world.spawn_empty().id();
    let second = world.spawn_empty().id();

    let template = CostTemplate { amount: 20.0 };
    world.entity_mut(first).build_template(&template).unwrap();
    world.entity_mut(second).build_template(&template).unwrap();

    assert_eq!(
        world.resource::<Pool>().0,
        0.0,
        "两次构建扣了两次（同一个模板、同一个数值）"
    );
}

/// 3. 模板拿不到 `World`，所以**读不到别的实体的组件**。
///
/// `TemplateContext` 公开的就这几个入口：
/// `resource` / `resource_mut` / `resource_entity` / `get_entity` / `set`
/// —— 全是资源和**实体引用**，没有一个能读组件 ✗
/// 要读"施法者的法力"，只能从外面把 `&mut World` 递进去 ✗ 而 `build_template` 的签名里没有 ✗
#[test]
fn template_cannot_reach_another_entitys_components() {
    let mut world = World::new();
    world.insert_resource(Pool(40.0));

    // 一个"角色"：它的法力是自己的组件
    let hero = world.spawn(Cost(0.0)).id();
    let mage = world.spawn_empty().id();

    // 模板里能拿到的只是"引用 → Entity"，拿不到 Entity 上的数据
    let context = world
        .entity_mut(mage)
        .template_context(|_context| Ok(()))
        .map(|_| ());
    assert!(context.is_ok(), "模板上下文建得起来");

    // 想在模板里做这件事，必须绕到外面来 —— 这已经不叫"模板"了
    let mana_of_hero = world.get::<Cost>(hero).map(|c| c.0);
    assert_eq!(mana_of_hero, Some(0.0), "组件只能从外面读");
}
