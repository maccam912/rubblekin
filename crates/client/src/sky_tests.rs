use super::*;
use crate::{airships::AirshipClock, graphics::GraphicsQuality};

#[test]
fn day_and_night_each_last_twenty_minutes_and_repeat_on_old_worlds() {
    assert_eq!(CYCLE_SECONDS, 2400.0);
    let daylight_samples = (0..2400)
        .filter(|second| state(f64::from(*second) + 0.5).colors.sun_direction.y > 0.0)
        .count();
    assert_eq!(daylight_samples, 1200);
    for seconds in [0.0, 300.0, 875.5, 1500.0, 2399.0] {
        let initial = state(seconds);
        let old = state(seconds + CYCLE_SECONDS * 1_000_000.0);
        assert!(
            initial
                .colors
                .sun_direction
                .abs_diff_eq(old.colors.sun_direction, 0.00001)
        );
    }
}

#[test]
fn sunset_shadows_lengthen_and_light_tracks_the_visible_sun_without_a_zenith_flip() {
    let noon = state(300.0);
    let afternoon = state(600.0);
    let sunset = state(840.0);
    let shadow_length =
        |s: &SkyState| s.light_direction.xz().length() / s.light_direction.y.max(0.0001);
    assert!(shadow_length(&noon) < 0.001);
    assert!(shadow_length(&afternoon) > 0.9);
    assert!(shadow_length(&sunset) > 6.0);
    assert!(sunset.light_color.to_linear().green < afternoon.light_color.to_linear().green);
    for seconds in [0.0, 299.99, 300.0, 300.01, 600.0, 899.0] {
        let s = state(seconds);
        let transform = light_transform(s.light_direction);
        assert!(transform.rotation.is_finite());
        assert!(transform.forward().dot(-s.colors.sun_direction.truncate()) > 0.9999);
    }
}

#[test]
fn shadow_direction_holds_between_small_steps_while_the_sun_moves_smoothly() {
    for second in [0.0, 17.0, 299.0, 300.0, 840.0, 1499.0, 2399.0] {
        let start = state(second);
        let middle = state(second + 0.5);
        let end = state(second + 0.999);
        assert_eq!(start.light_direction, middle.light_direction);
        assert_eq!(start.light_direction, end.light_direction);
        assert_ne!(start.colors.sun_direction, middle.colors.sun_direction);
        let next = state(second + LIGHT_STEP_SECONDS);
        assert_ne!(start.light_direction, next.light_direction);
        // At most one tiny orbital step of error relative to the smooth sky.
        assert!(
            end.light_direction
                .dot(end.colors.sun_direction.truncate())
                .abs()
                > 0.99999
        );
        let old = state(second + 0.999 + CYCLE_SECONDS * 1_000_000.0);
        assert_eq!(end.light_direction, old.light_direction);
    }
    // The color/intensity ramps are not quantized along with shadow direction.
    let a = state(870.1);
    let b = state(870.5);
    assert_eq!(a.light_direction, b.light_direction);
    assert_ne!(a.colors.horizon, b.colors.horizon);
    assert_ne!(a.illuminance, b.illuminance);
}

#[test]
fn twilight_is_continuous_and_night_keeps_a_readable_light_floor() {
    let noon = state(300.0);
    let dusk = state(900.0);
    let night = state(1500.0);
    assert_eq!(noon.colors.sun_direction.w, 0.0, "no daytime stars");
    assert_eq!(night.colors.sun_direction.w, 1.0);
    assert!(dusk.colors.phase.x > 0.95, "warm horizon at sunset");
    assert!(night.colors.zenith.z > night.colors.zenith.x);
    assert!(noon.colors.horizon.x > noon.colors.zenith.x);
    assert!(night.colors.horizon.x > night.colors.zenith.x);
    assert_eq!(night.ambient_brightness, 400.0);
    assert_eq!(night.illuminance, 900.0);
    for second in 0..2400 {
        let a = state(f64::from(second));
        let b = state(f64::from(second) + 1.0);
        assert!((400.0..=600.0).contains(&a.ambient_brightness));
        assert!(a.colors.horizon.abs_diff_eq(b.colors.horizon, 0.012));
        assert!((a.colors.sun_direction.w - b.colors.sun_direction.w).abs() < 0.02);
        assert!((a.illuminance - b.illuminance).abs() < 200.0);
    }
}

fn session(world_seconds: f64) -> Session {
    let (_, mut session) = crate::join::session_from_welcome(
        crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
        "sky test".into(),
        GraphicsQuality::Balanced,
        0.0,
        rubblekin_core::protocol::SessionMode::Player,
    )
    .unwrap();
    // Authoritative packet time differs deliberately: presentation must share
    // the continuously advanced ship clock instead of stepping with packets.
    session.world_time = 300.0;
    session.airship_clock = AirshipClock::new(world_seconds, 0.0);
    session
}

#[test]
fn scene_uses_shared_presentation_time_updates_fog_and_reuses_assets_on_rejoin() {
    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<SkyMaterial>>()
        .init_resource::<SkyAssets>()
        .init_resource::<GlobalAmbientLight>()
        .init_resource::<ClearColor>()
        .insert_resource(GraphicsSettings::new(GraphicsQuality::Balanced))
        .insert_resource(session(1500.0))
        .add_systems(
            Update,
            (setup.run_if(resource_added::<Session>), update)
                .chain()
                .run_if(resource_exists::<Session>),
        );
    let position = Vec3::new(16300.0, 2200.0, -12000.0);
    app.world_mut().spawn((
        GameCamera,
        Transform::from_translation(position),
        DistanceFog::default(),
    ));
    app.update();
    let world = app.world_mut();
    let (transform, material) = world
        .query_filtered::<(&Transform, &MeshMaterial3d<SkyMaterial>), With<SkyDome>>()
        .single(world)
        .unwrap();
    assert_eq!(transform.translation, position);
    let material = material.0.clone();
    assert_eq!(
        world
            .resource::<Assets<SkyMaterial>>()
            .get(&material)
            .unwrap()
            .colors
            .sun_direction
            .w,
        1.0
    );
    let fog = world.query::<&DistanceFog>().single(world).unwrap();
    assert_eq!(fog.color, world.resource::<ClearColor>().0);
    assert_eq!(world.resource::<GlobalAmbientLight>().brightness, 400.0);
    let light = world.query::<&DirectionalLight>().single(world).unwrap();
    assert!(light.shadow_maps_enabled);
    assert_eq!(light.illuminance, 900.0);
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<GameEntity>>()
        .iter(world)
        .collect();
    for entity in entities {
        world.despawn(entity);
    }
    world.remove_resource::<Session>();
    // Track a complete frame without a session, as in the actual join menu.
    app.update();
    app.world_mut().insert_resource(session(300.0));
    app.world_mut()
        .resource_mut::<GraphicsSettings>()
        .set_quality(GraphicsQuality::Low);
    app.update();
    let world = app.world_mut();
    assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
    assert_eq!(world.resource::<Assets<SkyMaterial>>().len(), 1);
    assert_eq!(
        world
            .resource::<Assets<SkyMaterial>>()
            .get(&material)
            .unwrap()
            .colors
            .sun_direction
            .w,
        0.0
    );
    let light = world.query::<&DirectionalLight>().single(world).unwrap();
    assert!(!light.shadow_maps_enabled);
    assert_eq!(light.illuminance, 10_500.0);
}
