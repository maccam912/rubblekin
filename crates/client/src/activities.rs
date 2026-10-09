//! The same models supply world props, socket silhouettes and picture cards.
use crate::{
    GameEntity, Session, VoxelWorld, network::Connection, terrain::Geometry, touch::TouchControls,
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{activities::*, protocol::ClientMessage};
use std::collections::HashMap;

type Part = (Vec3, Vec3, [f32; 4]);
const GOLD: [f32; 4] = [0.95, 0.76, 0.30, 1.];
const WOOD: [f32; 4] = [0.47, 0.29, 0.13, 1.];
const CREAM: [f32; 4] = [0.94, 0.90, 0.73, 1.];
fn supply_parts(i: usize) -> Vec<Part> {
    let mut out = Vec::new();
    let mut add =
        |p: [f32; 3], s: [f32; 3], c| out.push((Vec3::from_array(p), Vec3::from_array(s), c));
    match i {
        0 => {
            // Round basket, with a round rim and a tall handle.
            for k in -5_i32..=5 {
                let x = k as f32 * 0.065;
                let z = (0.35_f32.powi(2) - x * x).max(0.).sqrt();
                add([x, 0.25, 0.], [0.065, 0.4, z * 2.], WOOD);
            }
            add([-0.26, 0.63, 0.], [0.055, 0.45, 0.07], GOLD);
            add([0.26, 0.63, 0.], [0.055, 0.45, 0.07], GOLD);
            add([0., 0.84, 0.], [0.56, 0.06, 0.07], GOLD);
        }
        1 => {
            for z in [-0.17, 0., 0.17] {
                add([0., 0.17, z], [1.1, 0.25, 0.12], WOOD);
            }
            for x in [-0.31, 0.31] {
                add([x, 0.17, 0.], [0.08, 0.29, 0.52], GOLD);
            }
        }
        _ => {
            add([0., 0.37, 0.], [0.65, 0.65, 0.55], WOOD);
            for x in [-0.26, 0.26] {
                add([x, 0.37, -0.29], [0.075, 0.65, 0.03], GOLD);
            }
            for y in [0.1, 0.65] {
                add([0., y, -0.3], [0.65, 0.075, 0.03], GOLD);
            }
        }
    }
    out
}
fn silhouette_parts(i: usize) -> Vec<Part> {
    let mut out = Vec::new();
    for (p, size, _) in supply_parts(i) {
        for x in [-1., 1.] {
            out.push((
                Vec3::new(p.x + x * size.x / 2., p.y, -0.32),
                Vec3::new(0.025, size.y, 0.025),
                CREAM,
            ));
        }
        for y in [-1., 1.] {
            out.push((
                Vec3::new(p.x, p.y + y * size.y / 2., -0.32),
                Vec3::new(size.x, 0.025, 0.025),
                CREAM,
            ));
        }
    }
    out
}
fn base_parts(stones: bool) -> Vec<Part> {
    let mut out = vec![(Vec3::new(0., 0.07, 0.), Vec3::new(1.25, 0.14, 1.05), WOOD)];
    for x in [-0.58, 0.58] {
        out.push((Vec3::new(x, 0.18, 0.), Vec3::new(0.07, 0.16, 1.05), GOLD));
    }
    if stones {
        for x in [-0.42, 0.42] {
            out.push((Vec3::new(x, 0.95, 0.18), Vec3::new(0.065, 1.9, 0.065), WOOD));
        }
        out.push((Vec3::new(0., 1.75, 0.22), Vec3::new(0.9, 0.8, 0.1), WOOD));
        out.push((Vec3::new(0., 0.60, 0.06), Vec3::new(0.85, 0.8, 0.15), WOOD));
    }
    out
}
fn symbol_parts(i: usize) -> Vec<Part> {
    let mut out = Vec::new();
    let mut add = |x, y, w, h| out.push((Vec3::new(x, y, 0.), Vec3::new(w, h, 0.40), CREAM));
    match i {
        0 => {
            add(0., 0.19, 0.09, 0.35);
            add(0., 0.37, 0.53, 0.13);
            add(0., 0.49, 0.37, 0.12);
            add(0., 0.60, 0.19, 0.12);
        }
        1 => {
            add(-0.05, 0.37, 0.42, 0.24);
            add(0.22, 0.37, 0.12, 0.37);
            add(-0.22, 0.37, 0.12, 0.10);
        }
        _ => {
            for row in 0..5 {
                add(0., 0.16 + row as f32 * 0.1, 0.62 - row as f32 * 0.12, 0.1);
            }
        }
    }
    out
}
fn picture(parts: &[Part]) -> Image {
    let mut pixels = vec![0_u8; 64 * 64 * 4];
    for (p, s, c) in parts {
        let left = ((p.x - s.x / 2.) * 45. + 32.).clamp(0., 63.) as usize;
        let right = ((p.x + s.x / 2.) * 45. + 32.).clamp(0., 64.) as usize;
        let top = (60. - (p.y + s.y / 2.) * 57.).clamp(0., 63.) as usize;
        let bottom = (60. - (p.y - s.y / 2.) * 57.).clamp(0., 64.) as usize;
        for y in top..bottom {
            for x in left..right {
                let at = (y * 64 + x) * 4;
                for n in 0..4 {
                    pixels[at + n] = (c[n] * 255.) as u8;
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
fn mesh(parts: &[Part]) -> Mesh {
    let mut g = Geometry::default();
    for (p, s, c) in parts {
        g.cuboid(*p, *s, *c);
    }
    g.into_mesh()
}
#[derive(Resource)]
pub(crate) struct Scene {
    supplies: [Handle<Mesh>; 3],
    silhouettes: [Handle<Mesh>; 3],
    symbols: [Handle<Mesh>; 3],
    supply_pictures: [Handle<Image>; 3],
    symbol_pictures: [Handle<Image>; 3],
    material: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
    base: [Handle<Mesh>; 2],
    base_material: Handle<StandardMaterial>,
    objects: HashMap<(u64, u8, u8), Entity>,
    pub pending: Option<(u64, f64)>,
    hint_until: f64,
}
#[derive(Component)]
pub(crate) struct Card;
#[derive(Component)]
pub(crate) struct CardTitle;
#[derive(Component)]
pub(crate) struct CardPicture(usize);
#[derive(Clone, Copy, Debug)]
struct Target {
    id: u64,
    revision: u64,
    action: ActivityAction,
    position: [f32; 3],
}
fn carried(session: &Session) -> Option<(&ActivitySnapshot, usize)> {
    session.activities.iter().find_map(|a| {
        a.props
            .iter()
            .position(|s| *s == PropState::Held(session.id))
            .map(|i| (a, i))
    })
}
fn target(session: &Session, world: &rubblekin_core::world::World) -> Option<Target> {
    let hold = carried(session);
    session
        .activities
        .iter()
        .filter(|a| {
            !a.complete && (hold.is_none() || hold.is_some_and(|(h, _)| h.plan.id == a.plan.id))
        })
        .flat_map(|a| {
            (0..3).filter_map(move |i| {
                let (action, p) = match a.plan.kind {
                    ActivityKind::SpilledSupplies if hold.is_some() => {
                        (ActivityAction::Place(i as u8), a.plan.sockets[i])
                    }
                    ActivityKind::SpilledSupplies if a.props[i] == PropState::Home => {
                        (ActivityAction::Take(i as u8), a.plan.objects[i])
                    }
                    ActivityKind::ShapeStones => (ActivityAction::Turn(i as u8), a.plan.sockets[i]),
                    _ => return None,
                };
                can_interact(world, session.body.position, p).then_some(Target {
                    id: a.plan.id,
                    revision: a.revision,
                    action,
                    position: p,
                })
            })
        })
        .min_by(|a, b| {
            distance(session.body.position, a.position)
                .total_cmp(&distance(session.body.position, b.position))
        })
}
pub(crate) fn touch_opportunity(session: &Session) -> bool {
    session.observer.is_none()
        && session.activities.iter().any(|a| {
            !a.complete
                && a.plan
                    .objects
                    .iter()
                    .chain(a.plan.sockets.iter())
                    .any(|p| distance(session.body.position, *p) < 12.)
        })
}
pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
    touch: Res<TouchControls>,
) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands.insert_resource(Scene {
        supplies: std::array::from_fn(|i| meshes.add(mesh(&supply_parts(i)))),
        silhouettes: std::array::from_fn(|i| meshes.add(mesh(&silhouette_parts(i)))),
        symbols: std::array::from_fn(|i| meshes.add(mesh(&symbol_parts(i)))),
        supply_pictures: std::array::from_fn(|i| images.add(picture(&supply_parts(i)))),
        symbol_pictures: std::array::from_fn(|i| images.add(picture(&symbol_parts(i)))),
        material: materials.add(StandardMaterial {
            perceptual_roughness: 1.,
            unlit: true,
            ..default()
        }),
        ghost: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        }),
        base: std::array::from_fn(|i| meshes.add(mesh(&base_parts(i == 1)))),
        base_material: materials.add(StandardMaterial {
            unlit: true,
            ..default()
        }),
        objects: HashMap::new(),
        pending: None,
        hint_until: 0.,
    });
    commands
        .spawn((
            GameEntity,
            Card,
            Node {
                position_type: PositionType::Absolute,
                top: px(if touch.enabled { 70. } else { 114. }),
                left: percent(36.),
                width: px(if touch.enabled { 210. } else { 250. }),
                padding: UiRect::all(px(8.)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.),
                display: Display::None,
                border_radius: BorderRadius::all(px(8.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.10, 0.09, 0.93)),
        ))
        .with_children(|p| {
            p.spawn((
                CardTitle,
                Text::new(""),
                TextFont::from_font_size(16.).with_font(font),
                TextColor(Color::srgb(0.94, 0.88, 0.72)),
            ));
            p.spawn(Node {
                column_gap: px(8.),
                ..default()
            })
            .with_children(|row| {
                for i in 0..3 {
                    row.spawn((
                        CardPicture(i),
                        ImageNode::default(),
                        Node {
                            width: px(52.),
                            height: px(52.),
                            border: UiRect::all(px(3.)),
                            ..default()
                        },
                        BorderColor::all(Color::srgb(0.7, 0.58, 0.28)),
                    ));
                }
            });
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn read(
    mut session: ResMut<Session>,
    world: Res<VoxelWorld>,
    mut scene: ResMut<Scene>,
    mut connection: ResMut<Connection>,
    mut touch: ResMut<TouchControls>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    windows: Query<&Window>,
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::world_map::WorldMap>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::market::MarketPanel>,
        Res<crate::airships::PilotConversation>,
    ),
) {
    let use_now = keys.just_pressed(KeyCode::KeyT) || touch.activity;
    let return_now = keys.just_pressed(KeyCode::Backspace) || touch.activity_return;
    let help_now = keys.just_pressed(KeyCode::KeyY) || touch.activity_hint;
    touch.activity = false;
    touch.activity_return = false;
    touch.activity_hint = false;
    let (pause, map, console, market, travel) = modals;
    if session.observer.is_some()
        || !windows.iter().any(|w| w.focused)
        || touch.suspended
        || connection.error.is_some()
        || session.inventory.input_blocked
        || pause.open
        || pause.input_blocked
        || map.open
        || map.input_blocked
        || console.input_blocked
        || market.open
        || market.input_blocked
        || travel.open()
        || travel.input_blocked
    {
        return;
    }
    if help_now {
        scene.hint_until = time.elapsed_secs_f64() + 8.;
    }
    if scene
        .pending
        .is_some_and(|(_, at)| time.elapsed_secs_f64() - at > 5.)
    {
        scene.pending = None;
    }
    if scene.pending.is_some() {
        return;
    }
    let chosen = if return_now {
        carried(&session).map(|(a, _)| Target {
            id: a.plan.id,
            revision: a.revision,
            action: ActivityAction::Return,
            position: session.body.position,
        })
    } else if use_now {
        target(&session, &world.0)
    } else {
        None
    };
    if let Some(t) = chosen {
        let request_id = session.next_request;
        session.next_request += 1;
        connection.send(ClientMessage::Activity {
            request_id,
            activity_id: t.id,
            revision: t.revision,
            action: t.action,
        });
        scene.pending = Some((request_id, time.elapsed_secs_f64()));
    }
}
fn entity(
    commands: &mut Commands,
    scene: &Scene,
    handle: Handle<Mesh>,
    ghost: bool,
    position: Vec3,
) -> Entity {
    commands
        .spawn((
            GameEntity,
            Mesh3d(handle),
            MeshMaterial3d(if ghost {
                scene.ghost.clone()
            } else {
                scene.material.clone()
            }),
            Transform::from_translation(position),
        ))
        .id()
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    mut commands: Commands,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    mut scene: ResMut<Scene>,
    mut transforms: Query<(
        &mut Transform,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut gizmos: Gizmos,
    time: Res<Time>,
    mut card: Query<&mut Node, With<Card>>,
    mut title: Query<&mut Text, With<CardTitle>>,
    mut pictures: Query<(&CardPicture, &mut ImageNode, &mut BorderColor)>,
    touch: Res<TouchControls>,
) {
    let eye = session
        .observer
        .as_ref()
        .map_or(Vec3::from_array(session.body.position), |c| c.position);
    // Leaving and rejoining removes GameEntity objects; discard stale cache entries.
    scene.objects.retain(|_, e| transforms.contains(*e));
    let mut wanted = Vec::new();
    for a in &session.activities {
        if distance(eye.to_array(), a.plan.sockets[1]) > 100. {
            continue;
        }
        for i in 0..3 {
            let base = Vec3::from_array(a.plan.sockets[i]);
            let base_key = (a.plan.id, i as u8, 0);
            wanted.push(base_key);
            if !scene.objects.contains_key(&base_key) {
                let e = commands
                    .spawn((
                        GameEntity,
                        Mesh3d(
                            scene.base[usize::from(a.plan.kind == ActivityKind::ShapeStones)]
                                .clone(),
                        ),
                        MeshMaterial3d(scene.base_material.clone()),
                        Transform::from_translation(base),
                    ))
                    .id();
                scene.objects.insert(base_key, e);
            }
            let key = (a.plan.id, i as u8, 1);
            wanted.push(key);
            let (handle, p, ghost) = if a.plan.kind == ActivityKind::SpilledSupplies {
                let (p, ghost) = match a.props[i] {
                    PropState::Home => (Vec3::from_array(a.plan.objects[i]), false),
                    PropState::Placed => (base + Vec3::Y * 0.14, false),
                    PropState::Held(id) => {
                        let (position, yaw) = if id == session.id {
                            (session.body.position, session.yaw)
                        } else {
                            session
                                .players
                                .iter()
                                .find(|p| p.id == id)
                                .map_or((a.plan.objects[i], 0.), |p| (p.body.position, p.yaw))
                        };
                        (
                            Vec3::from_array(position)
                                + Vec3::new(-yaw.sin() * 0.55, 0.75, -yaw.cos() * 0.55),
                            false,
                        )
                    }
                };
                (scene.supplies[i].clone(), p, ghost)
            } else {
                (
                    scene.symbols[a.faces[i] as usize].clone(),
                    base + Vec3::Y * 0.24,
                    false,
                )
            };
            if let Some(e) = scene.objects.get(&key).copied() {
                if let Ok((mut t, mut m, mut mat)) = transforms.get_mut(e) {
                    *t = Transform::from_translation(p);
                    if a.complete && a.plan.kind == ActivityKind::ShapeStones {
                        t.rotation = Quat::from_rotation_y(time.elapsed_secs() * 1.2);
                    }
                    m.0 = handle;
                    mat.0 = if ghost {
                        scene.ghost.clone()
                    } else {
                        scene.material.clone()
                    };
                }
            } else {
                let e = entity(&mut commands, &scene, handle, ghost, p);
                scene.objects.insert(key, e);
            }
            let key = (a.plan.id, i as u8, 2);
            if a.plan.kind == ActivityKind::ShapeStones || a.props[i] != PropState::Placed {
                wanted.push(key);
                let (handle, p) = if a.plan.kind == ActivityKind::SpilledSupplies {
                    (scene.silhouettes[i].clone(), base + Vec3::Y * 0.14)
                } else {
                    (
                        scene.symbols[a.plan.answer[i] as usize].clone(),
                        base + Vec3::new(0., 1.35, 0.10),
                    )
                };
                if !scene.objects.contains_key(&key) {
                    let e = entity(&mut commands, &scene, handle, true, p);
                    scene.objects.insert(key, e);
                }
            }
            let finished = if a.plan.kind == ActivityKind::SpilledSupplies {
                a.props[i] == PropState::Placed
            } else {
                a.faces[i] == a.plan.answer[i]
            };
            if finished {
                gizmos.cube(
                    Transform::from_translation(base + Vec3::Y * 0.5)
                        .with_scale(Vec3::new(1.1, 1.1, 0.9)),
                    Color::srgb(0.95, 0.80, 0.40),
                );
            }
        }
        // Filled trays remain; solved stones gently turn in their existing racks.
    }

    scene.objects.retain(|k, e| {
        if wanted.contains(k) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    let hold = carried(&session);
    let next_target = target(&session, &world.0);
    let focused = hold
        .map(|(a, _)| a)
        .or_else(|| next_target.and_then(|t| session.activities.iter().find(|a| a.plan.id == t.id)))
        .or_else(|| {
            session
                .activities
                .iter()
                .filter(|a| distance(session.body.position, a.plan.sockets[1]) < 12.)
                .min_by(|a, b| {
                    distance(session.body.position, a.plan.sockets[1])
                        .total_cmp(&distance(session.body.position, b.plan.sockets[1]))
                })
        });
    let blocked = session.observer.is_some() || session.inventory.open || touch.menu_open;
    for mut n in &mut card {
        n.display = if focused.is_some() && !blocked {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(a) = focused {
        for mut text in &mut title {
            let s = if a.complete {
                "All in place!"
            } else if !a.available {
                "Ground changed · return your supply"
            } else if scene.pending.is_some() {
                "…"
            } else if hold.is_some() && touch.enabled {
                "Use: place · Return: put back"
            } else if hold.is_some() {
                "T: place · Backspace: return"
            } else if touch.enabled && a.plan.kind == ActivityKind::ShapeStones {
                "Use: turn · Hint: next piece"
            } else if touch.enabled {
                "Use: take / place · Hint"
            } else if a.plan.kind == ActivityKind::ShapeStones {
                "T: turn · Y: hint"
            } else {
                "T: take / place · Y: hint"
            };
            if text.0 != s {
                text.0 = s.into();
            }
        }
        for (index, mut img, mut border) in &mut pictures {
            let i = index.0;
            img.image = if a.plan.kind == ActivityKind::SpilledSupplies {
                scene.supply_pictures[i].clone()
            } else {
                scene.symbol_pictures[a.plan.answer[i] as usize].clone()
            };
            let done = if a.plan.kind == ActivityKind::SpilledSupplies {
                a.props[i] == PropState::Placed
            } else {
                a.faces[i] == a.plan.answer[i]
            };
            img.color = if done {
                Color::WHITE
            } else {
                Color::srgba(1., 1., 1., 0.75)
            };
            *border = BorderColor::all(if done {
                Color::srgb(0.95, 0.80, 0.40)
            } else {
                Color::srgb(0.36, 0.42, 0.37)
            });
        }
    }
    if !blocked {
        if let Some(t) = target(&session, &world.0) {
            gizmos.cube(
                Transform::from_translation(Vec3::from_array(t.position) + Vec3::Y * 0.5)
                    .with_scale(Vec3::new(1.25, 1.15, 1.1)),
                Color::WHITE,
            );
        }
        if scene.hint_until > time.elapsed_secs_f64()
            && let Some(a) = focused.filter(|a| !a.complete)
        {
            let p = if let Some((_, i)) = hold {
                Some(a.plan.sockets[i])
            } else if a.plan.kind == ActivityKind::SpilledSupplies {
                a.props
                    .iter()
                    .position(|s| *s == PropState::Home)
                    .map(|i| a.plan.objects[i])
            } else {
                a.faces
                    .iter()
                    .zip(a.plan.answer)
                    .position(|(f, t)| *f != t)
                    .map(|i| a.plan.sockets[i])
            };
            if let Some(p) = p {
                let p = Vec3::from_array(p);
                gizmos.line(
                    eye + Vec3::Y,
                    p + Vec3::Y * 1.2,
                    Color::srgb(1., 0.82, 0.36),
                );
                gizmos.cube(
                    Transform::from_translation(p + Vec3::Y * 0.8).with_scale(Vec3::splat(1.5)),
                    Color::srgb(1., 0.82, 0.36),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pictures_use_distinct_model_silhouettes_and_have_visible_pixels() {
        let supplies: Vec<_> = (0..3)
            .map(|i| picture(&supply_parts(i)).data.unwrap())
            .collect();
        let symbols: Vec<_> = (0..3)
            .map(|i| picture(&symbol_parts(i)).data.unwrap())
            .collect();
        for set in [supplies, symbols] {
            for i in 0..3 {
                assert!(
                    set[i]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|p| p[3] > 0)
                        .count()
                        > 100
                );
                for j in i + 1..3 {
                    assert_ne!(set[i], set[j]);
                }
            }
        }
    }
    #[test]
    fn carried_prop_focus_keeps_the_matching_activity_and_other_holders_cannot_take_it() {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "activities".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let plan = review_plans(&world)[0].clone();
        session.body.position = plan.objects[0];
        session.activities = vec![ActivitySnapshot {
            plan: plan.clone(),
            revision: 1,
            props: [
                PropState::Held(session.id + 10),
                PropState::Home,
                PropState::Home,
            ],
            faces: [0; 3],
            complete: false,
            available: true,
        }];
        assert!(!matches!(
            target(&session, &world).map(|t| t.action),
            Some(ActivityAction::Take(0))
        ));
        session.activities[0].props[0] = PropState::Held(session.id);
        session.body.position = plan.sockets[0];
        assert!(matches!(
            target(&session, &world).map(|t| t.action),
            Some(ActivityAction::Place(0))
        ));
        assert_eq!(carried(&session).unwrap().1, 0);
    }
}
