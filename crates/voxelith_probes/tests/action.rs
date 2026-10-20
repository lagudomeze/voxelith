use std::{collections::HashMap, fmt::Debug};

use bevy_asset::prelude::*;
use bevy_ecs::prelude::*;
use bevy_math::prelude::*;

enum TargetType {
    // 自己
    This,
    // 地面位置
    Cell,
    // 某个单位
    Unit,
}

enum Targeter {
    This,
    Cell(IVec2),
    Unit(Entity),
}

trait Action: Default + Send + Sync + 'static {
    const NAME: &'static str;

    const TARGET_TYPE: TargetType;

    type RequiredResouce: Component + Debug;

    // Resouce/Asset ？
    type Config: Asset;

    type Trigger: Message;

    fn build(
        assets: &Res<Assets<Self::Config>>,
        caller: Entity,
        targeter: Targeter,
    ) -> Result<Self::Trigger>;
}

#[derive(Debug, Clone, Copy, Default, Component)]
#[component(storage = "SparseSet")]
struct SpawnAction<T: Action> {
    _mark: std::marker::PhantomData<T>,
}

// for all ActionComponent define a system？
fn spawn<T: Action>(
    caller: Query<(Entity, &T::RequiredResouce), With<SpawnAction<T>>>,
    mut writer: MessageWriter<T::Trigger>,
    assets: Res<Assets<T::Config>>,
) {
    for (caller, req_res) in caller.iter() {
        // do some cost resouce;
        // maybe add some method in action trait or RequiredResouce
        println!("want {:?}", req_res);

        // here
        // for npc/monstor is resolve by ai
        // for pc is resolve by another step
        //    maybe define some global resouce
        let targeter = Targeter::This;

        if let Ok(e) = T::build(&assets, caller, targeter) {
            writer.write(e);
        } else {
            println!("some error");
        }
    }
}
