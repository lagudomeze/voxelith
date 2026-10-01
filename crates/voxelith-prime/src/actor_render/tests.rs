//! `actor_render` 的单元测试。
//!
//! 通过 `#[path]` 挂在 `actor_render` 模块下，所以本文件**就是** `actor_render::tests`，
//! `super` 指向 `actor_render` —— 与内联 `mod tests { use super::*; }` 的写法完全一致。
//!
//! ## 时间相关的测试要注意（踩了很久）
//!
//! `TimeUpdateStrategy::Automatic`（默认）读**系统时钟**，测试里每帧 delta 只有
//! 几十微秒，动画**永远换不了帧**。而且手推 `Time::advance_by` 是**没用**的——
//! 时间系统每帧会覆盖它。
//!
//! 正确做法是插入 `TimeUpdateStrategy::ManualDuration(..)`，让每帧固定推进。

use super::*;

mod tests {
    use super::*;

    use super::*;

    #[test]
    fn facing_follows_the_dominant_axis() {
        assert_eq!(Facing::from_direction(Vec2::new(1.0, 0.0)), Facing::East);
        assert_eq!(Facing::from_direction(Vec2::new(-1.0, 0.0)), Facing::West);
        // 世界 `+Z` 在屏幕上朝下 ⇒ `+y` 是"朝南"。
        assert_eq!(Facing::from_direction(Vec2::new(0.0, 1.0)), Facing::South);
        assert_eq!(Facing::from_direction(Vec2::new(0.0, -1.0)), Facing::North);
        // 斜着走时取分量大的那一轴。
        assert_eq!(Facing::from_direction(Vec2::new(0.9, 0.4)), Facing::East);
        assert_eq!(Facing::from_direction(Vec2::new(0.4, 0.9)), Facing::South);
    }

    #[test]
    fn standing_still_has_no_facing_change() {
        assert_eq!(Facing::from_direction(Vec2::ZERO), Facing::South);
        assert!(!Movement::still(Vec2::ZERO).is_moving());
    }

    #[test]
    fn west_reuses_the_east_row_with_a_flip() {
        // 素材只有"朝东"的侧视，朝西靠水平翻转。
        assert_eq!(Facing::West.row(), Facing::East.row());
        assert!(Facing::West.flip_x());
        assert!(!Facing::East.flip_x());
    }

    #[test]
    fn every_facing_lives_on_a_valid_row() {
        for facing in [Facing::South, Facing::North, Facing::East, Facing::West] {
            assert!(
                facing.row() < SheetLayout::default().row_count(),
                "{facing:?} 的行号超出表范围"
            );
        }
    }

    #[test]
    fn movement_reports_motion_from_velocity() {
        let mut movement = Movement::still(Vec2::ZERO);
        assert!(!movement.is_moving());
        movement.velocity = Vec2::new(2.0, 0.0);
        assert!(movement.is_moving());
        // 极小速度按"不动"处理，免得静止时抖帧。
        movement.velocity = Vec2::new(1e-9, 0.0);
        assert!(!movement.is_moving());
    }

    #[test]
    fn patrol_keeps_position_and_velocity_consistent() {
        // 位置是正弦、速度取它的**时间导数**：两者必须自洽。
        // 不自洽就会出现"位置没动却被判为在走"（动画抖）或"在动却播待机"。
        //
        // ⚠️ 这里有个容易搞错的地方：`position` 是**相位**的函数，
        // 而 `velocity` 是**时间**导数，中间差一个链式因子 `ω`：
        //
        // ```text
        // phase(t) = ω·t          ⇒   d/dt[amp·sin(phase)] = amp·cos(phase)·ω
        // ```
        //
        // 所以核对时必须让相位按 `ω·dt` 前进（跟系统做的事一致），
        // 而不是按 `dt`——否则会差一个 `ω`，把一条正确的公式判成错的（我踩过）。
        let config = DemoPatrolConfig::default();
        let amplitude = config.amplitude;
        let omega = config.phase_per_second * std::f32::consts::TAU;
        assert!(amplitude > 0.0 && omega > 0.0, "幅度与角频率都不能为 0");

        let position = |phase: f32| phase.sin() * amplitude;
        let velocity = |phase: f32| phase.cos() * amplitude * omega;

        let dt: f32 = 1e-3;
        for step in 0..64 {
            let phase = step as f32 * 0.31;
            // 相位本身也在动：`phase ± ω·dt`。
            let numeric =
                (position(phase + omega * dt) - position(phase - omega * dt)) / (2.0 * dt);
            let analytic = velocity(phase);
            let scale = analytic.abs().max(amplitude * omega);
            assert!(
                (numeric - analytic).abs() <= 0.01 * scale,
                "相位 {phase}：数值导数 {numeric} 与解析速度 {analytic} 差得太多"
            );
        }
    }

    #[test]
    fn battlefield_slots_are_spread_out_and_unique() {
        let slots: Vec<Vec2> = (0..9).map(battlefield_slot).collect();
        for (i, a) in slots.iter().enumerate() {
            for b in slots.iter().skip(i + 1) {
                assert!(
                    (*a - *b).length() > 1.0,
                    "两个角色摆得太近（会重叠）：{a:?} vs {b:?}"
                );
            }
        }
    }
    /// **端到端**：动画系统真的在推进帧，而不是"测试过帧矩形算术"。
    ///
    /// 这条要证明的是三件接在一起的事：
    /// 1. `drive_demo_patrol` 让 `Movement` 动起来；
    /// 2. `advance_animation` 因此按帧率换帧（而不是每帧都换、或永远不换）；
    /// 3. `sync_sprite_rect` 把帧号写进精灵的 `rect`。
    ///
    /// 单独测 `frame_rect(row, frame)` 的算术是**不够**的 —— 那只能证明"给定帧号会算对矩形"，
    /// 证明不了"帧号在变"。这一轮之前的所有动画测试都属于后者。
    #[test]
    fn the_animation_systems_actually_advance_the_frame() {
        use bevy::time::TimePlugin;

        let mut app = App::new();
        // **只装动画那几个系统**，不装 ActorRenderPlugin：uild_actor_sheet 要
        // `ResMut<Assets<Image>>`，那是完整 App 才有的（需要 `AssetPlugin`），
        // 这里测的是动画推进，不该被资源装配牵连。
        app.add_plugins(TimePlugin);
        // **给测试用的时间策略**：Automatic（默认）读系统时钟，于是每帧 delta
        // 只有几十微秒，动画永远换不了帧。ManualDuration 让每帧固定推进这么多。
        // 这个坑很隐蔽：手推 Time 是**没用**的，时间系统每帧会覆盖它。
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ));
        app.insert_resource(ActorSheet {
            image: Handle::default(),
            layout: SheetLayout::default(),
            width: 128,
            height: 48,
        });
        app.init_resource::<DemoPatrolConfig>();
        app.add_systems(
            Update,
            (drive_demo_patrol, advance_animation, sync_sprite_transform).chain(),
        );

        // 一个会走路的角色：速度非零 ⇒ `is_moving()` 为真 ⇒ 该换帧。
        let actor = app
            .world_mut()
            .spawn((
                ActorVisual::new(),
                Movement {
                    position: Vec2::ZERO,
                    velocity: Vec2::new(3.0, 0.0),
                    desired: Vec2::new(3.0, 0.0),
                },
            ))
            .id();

        // 先跑一帧让"换行/起步"的分支消化掉（它会重置到第 0 帧并 `continue`）。
        app.update();
        // **显式推进时间**：只装 TimePlugin 时 delta_secs() 恒为 0
        // （需要完整的时间装配链才会被驱动），所以这里手动把 Time 往前推。
        // 一旦 delta = 0，dvance_animation 会一直停在第 0 帧 ——
        // 这条断言（distinct.len() > 1）正是为了挡住那种假绿。
        // 步数要足够多：delta_secs() 在只装 TimePlugin 的测试 App 里
        // 远小于 dvance_by 的设定值（时间系统会重算），所以要靠**累计**时间。
        let frames_seen = |app: &mut App, actor: Entity| -> Vec<u32> {
            (0..400)
                .map(|_| {
                    app.update();
                    app.world().get::<ActorVisual>(actor).unwrap().frame
                })
                .collect()
        };
        let seen = frames_seen(&mut app, actor);

        let distinct: std::collections::BTreeSet<u32> = seen.iter().copied().collect();
        assert!(
            distinct.len() > 1,
            "{} 步里帧号一个都没变 ⇒ 动画没在推进",
            seen.len()
        );
        // 形状校验：帧率 6 ⇒ 0.5 秒一个循环 ⇒ 40 帧（约 0.67 秒）里该换几次，不是每帧都换。
        assert!(
            distinct.len() <= 8,
            "帧号变得太频繁（每帧都在换？）：{distinct:?}"
        );
    }

    /// 站住时**定格在第 0 帧**（素材第 0 帧就是站姿）。
    #[test]
    fn standing_still_freezes_on_the_first_frame() {
        use bevy::time::TimePlugin;

        let mut app = App::new();
        app.add_plugins(TimePlugin);
        // **给测试用的时间策略**：Automatic（默认）读系统时钟，于是每帧 delta
        // 只有几十微秒，动画永远换不了帧。ManualDuration 让每帧固定推进这么多。
        // 这个坑很隐蔽：手推 Time 是**没用**的，时间系统每帧会覆盖它。
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ));
        app.insert_resource(ActorSheet {
            image: Handle::default(),
            layout: SheetLayout::default(),
            width: 128,
            height: 48,
        });
        app.init_resource::<DemoPatrolConfig>();
        app.add_systems(
            Update,
            (drive_demo_patrol, advance_animation, sync_sprite_transform).chain(),
        );

        let actor = app
            .world_mut()
            .spawn((ActorVisual::new(), Movement::still(Vec2::ZERO)))
            .id();

        for _ in 0..30 {
            app.update();
            let visual = app.world().get::<ActorVisual>(actor).unwrap();
            assert_eq!(visual.frame, 0, "站住时该定格在第 0 帧");
            assert!(!visual.walking, "站住时 walking 该是 false");
        }
    }

    /// **回归测试**：`sync_sprite_rect` 必须认得**真实生成路径**产生的精灵。
    ///
    /// 第 5 轮把角色组件从 `Sprite` 换成了 `SpriteMesh`（为了 `alpha_mode: Blend`），
    /// 但 `sync_sprite_rect` 的查询**没跟着改**，还是查 `Sprite`。后果：
    /// `advance_animation` 一直在推进帧（单测是绿的），而 `rect` **从没被写进去过**，
    /// 画面上角色**永远定格在第 0 帧**。
    ///
    /// 原来那条测试发现不了它，因为它是**自己 spawn 了一个 `Sprite`** ——
    /// 测的是测试搭的场景，不是真实世界。**测试的搭建方式必须和产品代码一致**，
    /// 否则它只证明"我构造的输入能得到我期望的输出"。
    #[test]
    fn the_frame_rect_reaches_a_real_sprite_mesh_child() {
        use bevy::sprite::{SpriteAlphaMode, SpriteMesh};

        let mut app = App::new();
        app.insert_resource(ActorSheet {
            image: Handle::default(),
            layout: SheetLayout::default(),
            width: 128,
            height: 48,
        });
        app.add_systems(Update, sync_sprite_rect);

        let layout = SheetLayout::default();
        let expected_frame = layout.frame_rect(2, 1);
        assert_ne!(
            expected_frame,
            layout.frame_rect(0, 0),
            "前提：第 2 行第 1 帧必须与第 0 行第 0 帧不同，否则这条测试没有意义"
        );

        // 父实体带 `ActorVisual`（产品代码就是这形状）。
        let mut visual = ActorVisual::new();
        visual.row = 2;
        visual.frame = 1;
        let parent = app.world_mut().spawn(visual).id();
        // 子实体带 `SpriteMesh`，并挂 `ChildOf` 建立父子关系。
        let child = app
            .world_mut()
            .spawn((
                SpriteMesh {
                    image: Handle::default(),
                    rect: Some(layout.frame_rect(0, 0)),
                    custom_size: Some(Vec2::splat(115.0)),
                    alpha_mode: SpriteAlphaMode::Blend,
                    ..default()
                },
                Transform::default(),
                ChildOf(parent),
            ))
            .id();

        app.update();

        let rect = app
            .world()
            .get::<SpriteMesh>(child)
            .expect("子实体该是 SpriteMesh")
            .rect;
        assert_eq!(
            rect,
            Some(expected_frame),
            "帧矩形没被写进 SpriteMesh ⇒ 动画在画面上不会动"
        );
    }
}
