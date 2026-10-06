//! A north-up map of the actual world. Only player controls pause.
use crate::world_map_image::map_uv;
use crate::{GameEntity, Session, VoxelWorld, join::MenuKey, touch::TouchControls};
use bevy::{
    input::{
        keyboard::Key,
        mouse::AccumulatedMouseScroll,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    window::PrimaryWindow,
};

#[derive(Resource)]
pub(crate) struct WorldMap {
    pub open: bool,
    pub input_blocked: bool,
    pub just_closed: bool,
    pub requested: bool,
    zoom: f32,
    center: Vec2,
    drag: Option<Vec2>,
}
impl Default for WorldMap {
    fn default() -> Self {
        Self {
            open: false,
            input_blocked: false,
            just_closed: false,
            requested: false,
            zoom: 1.,
            center: Vec2::splat(0.5),
            drag: None,
        }
    }
}
impl WorldMap {
    fn view(&self) -> Rect {
        let half = 0.5 / self.zoom;
        let center = self.center.clamp(Vec2::splat(half), Vec2::splat(1. - half));
        Rect::from_center_size(center, Vec2::splat(2. * half))
    }
    fn point(&self, uv: Vec2, side: f32) -> Option<Vec2> {
        let view = self.view();
        view.contains(uv)
            .then_some((uv - view.min) / view.size() * side)
    }
}

#[derive(Component)]
pub(super) struct MapRoot;
#[derive(Component)]
pub(super) struct MapCanvas;
#[derive(Component)]
pub(super) struct MapLabel(usize);
#[derive(Component)]
pub(super) enum MapMarker {
    Town(usize),
    You,
    Spawn,
}
#[derive(Component)]
pub(super) struct MapPosition;
#[derive(Component)]
pub(super) struct MapTownDistance(usize);
#[derive(Component)]
pub(super) struct MapScale;
#[derive(Component)]
pub(super) struct MapClose;
#[derive(Component)]
pub(super) struct MapSidebar;

fn ink() -> Color {
    Color::srgb(0.91, 0.94, 0.88)
}
fn label(value: impl Into<String>, font: &Handle<Font>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont::from_font_size(size).with_font(font.clone()),
        TextColor(ink()),
    )
}
fn layout(window: &Window) -> (f32, bool) {
    let compact = window.height() < 550.;
    (
        (window.height() - if compact { 112. } else { 144. })
            .min(window.width() - 300.)
            .max(64.),
        compact,
    )
}
fn position(session: &Session) -> [f32; 3] {
    session
        .observer
        .as_ref()
        .map_or(session.body.position, |camera| camera.position.to_array())
}

pub(crate) fn setup(
    mut commands: Commands,
    mut map: ResMut<WorldMap>,
    world: Res<VoxelWorld>,
    scene: Res<crate::terrain::TerrainScene>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    *map = WorldMap::default();
    let image = scene
        .map_image
        .clone()
        .unwrap_or_else(|| images.add(crate::world_map_image::valley_image(&world.0)));
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let towns = world
        .0
        .settlements()
        .map(|plan| plan.villages.as_slice())
        .unwrap_or(&[]);
    commands
        .spawn((
            GameEntity,
            MapRoot,
            GlobalZIndex(110),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(8),
                ..default()
            },
            BackgroundColor(Color::srgb(0.035, 0.065, 0.07)),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|header| {
                header.spawn(label("WORLD MAP  /  N at top", &font, 23.));
                header
                    .spawn((
                        Button,
                        MapClose,
                        Node {
                            padding: UiRect::axes(px(14), px(8)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.16, 0.29, 0.28)),
                    ))
                    .with_child(label("Return  [M / Esc]", &font, 16.));
            });
            root.spawn((Node {
                column_gap: px(20),
                align_items: AlignItems::Center,
                ..default()
            },))
                .with_children(|body| {
                    body.spawn((
                        MapCanvas,
                        ImageNode::new(image),
                        Node {
                            width: px(700),
                            height: px(700),
                            flex_shrink: 0.,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                    ))
                    .with_children(|canvas| {
                        for (index, town) in towns.iter().enumerate() {
                            canvas
                                .spawn((
                                    MapMarker::Town(index),
                                    ZIndex(2),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        width: px(20),
                                        height: px(20),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border_radius: BorderRadius::MAX,
                                        border: UiRect::all(px(1)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.22, 0.14, 0.07)),
                                    BorderColor::all(Color::srgb(0.98, 0.82, 0.48)),
                                ))
                                .with_child(label((index + 1).to_string(), &font, 12.));
                            canvas.spawn((
                                MapLabel(index),
                                ZIndex(1),
                                label(&town.name, &font, 14.),
                                Node {
                                    position_type: PositionType::Absolute,
                                    padding: UiRect::axes(px(4), px(2)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.03, 0.06, 0.06, 0.87)),
                            ));
                        }
                        canvas
                            .spawn((
                                MapMarker::Spawn,
                                Node {
                                    position_type: PositionType::Absolute,
                                    width: px(12),
                                    height: px(12),
                                    border_radius: BorderRadius::MAX,
                                    border: UiRect::all(px(2)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgb(1., 0.92, 0.65)),
                            ))
                            .with_child((
                                label("Spawn", &font, 14.),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(16),
                                    top: px(-5),
                                    ..default()
                                },
                            ));
                        canvas
                            .spawn((
                                MapMarker::You,
                                ZIndex(3),
                                Node {
                                    position_type: PositionType::Absolute,
                                    width: px(16),
                                    height: px(16),
                                    border_radius: BorderRadius::MAX,
                                    border: UiRect::all(px(3)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.01, 0.12, 0.17)),
                                BorderColor::all(Color::srgb(0.42, 0.96, 1.)),
                            ))
                            .with_child((
                                label("You", &font, 16.),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(16),
                                    top: px(-6),
                                    padding: UiRect::all(px(2)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.02, 0.10, 0.13, 0.92)),
                            ));
                    });
                    body.spawn((
                        MapSidebar,
                        Node {
                            width: px(240),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(7),
                            ..default()
                        },
                    ))
                    .with_children(|sidebar| {
                        sidebar.spawn(label("TOWNS", &font, 20.));
                        if towns.is_empty() {
                            sidebar.spawn(label("No towns in this world.", &font, 16.));
                        }
                        for index in 0..towns.len() {
                            sidebar.spawn((MapTownDistance(index), label("", &font, 16.)));
                        }
                        sidebar.spawn((
                            MapPosition,
                            label("", &font, 15.),
                            Node {
                                margin: UiRect::top(px(8)),
                                ..default()
                            },
                        ));
                    });
                });
            root.spawn((MapScale, label("", &font, 14.)));
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read(
    mut map: ResMut<WorldMap>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    mut touch: ResMut<TouchControls>,
    pause: Res<crate::pause::PauseMenu>,
    console: Res<crate::admin_console::AdminConsole>,
    dialog: Res<crate::airships::PilotConversation>,
    mut native: MessageReader<MenuKey>,
    mut fingers: MessageReader<TouchInput>,
    buttons: Query<&Interaction, (With<MapClose>, Changed<Interaction>)>,
    close_targets: Query<(&ComputedNode, &UiGlobalTransform), With<MapClose>>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<MapCanvas>>,
) {
    let was_open = map.open;
    map.just_closed = false;
    map.input_blocked = was_open;
    let back = native
        .read()
        .any(|key| key.input.state.is_pressed() && key.input.logical_key == Key::BrowserBack);
    let close_touch = fingers.read().any(|finger| {
        was_open
            && finger.phase == TouchPhase::Started
            && close_targets.iter().any(|(node, transform)| {
                node.contains_point(*transform, finger.position * window.scale_factor())
            })
    });
    if !window.focused || touch.suspended {
        map.drag = None;
        return;
    }
    let requested = std::mem::take(&mut map.requested);
    let other_modal = console.input_blocked
        || dialog.open()
        || dialog.input_blocked
        || pause.open
        || pause.input_blocked;
    if !was_open && (requested || (!other_modal && keys.just_pressed(KeyCode::KeyM))) {
        map.open = true;
    } else if was_open
        && (keys.just_pressed(KeyCode::KeyM)
            || keys.just_pressed(KeyCode::Escape)
            || back
            || close_touch
            || buttons
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed))
    {
        map.open = false;
    }
    map.input_blocked = was_open || map.open;
    map.just_closed = was_open && !map.open;
    if was_open != map.open {
        touch.reset();
        map.drag = None;
    }
    if !map.open || !was_open {
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        map.zoom = 1.;
        map.center = Vec2::splat(0.5);
    }
    if keys.just_pressed(KeyCode::KeyC) {
        map.center = map_uv(&world.0, position(&session));
    }
    if let Some(cursor) = window.cursor_position() {
        let over_map = canvas.iter().any(|(node, transform)| {
            node.contains_point(*transform, cursor * window.scale_factor())
        });
        if over_map && wheel.delta.y != 0. {
            // Zoom toward the cursor, preserving the land beneath it.
            let view = map.view();
            let pointer = canvas
                .iter()
                .next()
                .map_or(Vec2::splat(0.5), |(node, transform)| {
                    ((cursor * window.scale_factor() - transform.translation) / node.size()
                        + Vec2::splat(0.5))
                    .clamp(Vec2::ZERO, Vec2::ONE)
                });
            let anchor = view.min + pointer * view.size();
            map.zoom = (map.zoom * (wheel.delta.y * 0.16).exp()).clamp(1., 8.);
            map.center = anchor + (Vec2::splat(0.5) - pointer) / map.zoom;
        }
        if over_map && mouse.just_pressed(MouseButton::Left) {
            map.drag = Some(cursor);
        }
        if mouse.pressed(MouseButton::Left) {
            if let Some(previous) = map.drag {
                let (side, _) = layout(&window);
                let zoom = map.zoom;
                map.center -= (cursor - previous) / (side * zoom);
                map.drag = Some(cursor);
            }
        } else {
            map.drag = None;
        }
        map.center = map.view().center();
    } else {
        map.drag = None;
    }
}

// Lay out town names with bounded offsets instead of piling text on nearby towns.
fn town_labels(
    points: &[Option<Vec2>],
    names: &[&str],
    side: f32,
    reserved: &[Rect],
) -> Vec<Option<Rect>> {
    let mut placed: Vec<Option<Rect>> = Vec::new();
    for (point, name) in points.iter().zip(names) {
        let Some(point) = point else {
            placed.push(None);
            continue;
        };
        let size = Vec2::new((name.chars().count() as f32 * 7.5 + 8.).min(side), 23.);
        let mut best = None;
        let mut best_overlap = f32::INFINITY;
        for offset in [0., -26., 26., -52., 52., -78., 78., -104., 104.] {
            for dx in [14., -size.x - 14.] {
                let min = (*point + Vec2::new(dx, offset - 11.))
                    .clamp(Vec2::ZERO, Vec2::splat(side) - size);
                let rect = Rect::from_corners(min, min + size);
                let overlap = placed
                    .iter()
                    .flatten()
                    .chain(reserved.iter())
                    .map(|other| {
                        let extent =
                            (rect.max.min(other.max) - rect.min.max(other.min)).max(Vec2::ZERO);
                        extent.x * extent.y
                    })
                    .sum::<f32>();
                if overlap < best_overlap {
                    best = Some(rect);
                    best_overlap = overlap;
                }
                if overlap == 0. {
                    break;
                }
            }
            if best_overlap == 0. {
                break;
            }
        }
        placed.push(best);
    }
    placed
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn refresh(
    map: Res<WorldMap>,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    window: Single<&Window, With<PrimaryWindow>>,
    images: Res<Assets<Image>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&MapRoot>,
            Option<&MapCanvas>,
            Option<&MapMarker>,
            Option<&MapLabel>,
            Option<&MapSidebar>,
        ),
        Or<(
            With<MapRoot>,
            With<MapCanvas>,
            With<MapMarker>,
            With<MapLabel>,
            With<MapSidebar>,
        )>,
    >,
    mut atlas: Query<&mut ImageNode, With<MapCanvas>>,
    mut texts: Query<
        (
            &mut Text,
            &mut TextFont,
            Option<&MapPosition>,
            Option<&MapTownDistance>,
            Option<&MapScale>,
        ),
        Or<(With<MapPosition>, With<MapTownDistance>, With<MapScale>)>,
    >,
) {
    let (side, compact) = layout(&window);
    let towns = world
        .0
        .settlements()
        .map(|plan| plan.villages.as_slice())
        .unwrap_or(&[]);
    let you = position(&session);
    let you_point = map.point(map_uv(&world.0, you), side);
    let points: Vec<_> = towns
        .iter()
        .map(|town| map.point(map_uv(&world.0, town.center), side))
        .collect();
    let names: Vec<_> = towns.iter().map(|town| town.name.as_str()).collect();
    let reserved: Vec<_> = you_point
        .into_iter()
        .map(|point| Rect::from_corners(point + Vec2::new(-10., -14.), point + Vec2::new(58., 16.)))
        .collect();
    let labels = town_labels(&points, &names, side, &reserved);
    for (mut node, root, canvas, marker, label, sidebar) in &mut nodes {
        if root.is_some() {
            node.display = if map.open {
                Display::Flex
            } else {
                Display::None
            };
        }
        if !map.open {
            continue;
        }
        if canvas.is_some() {
            node.width = px(side);
            node.height = px(side);
        }
        if sidebar.is_some() {
            node.row_gap = px(if side < 280. {
                2.
            } else if compact {
                4.
            } else {
                7.
            });
        }
        if let Some(marker) = marker {
            let (point, radius) = match marker {
                MapMarker::Town(index) => (points[*index], 10.),
                MapMarker::You => (you_point, 8.),
                MapMarker::Spawn => {
                    let spawn = map.point(map_uv(&world.0, world.0.spawn_position()), side);
                    (
                        spawn.filter(|p| you_point.is_none_or(|you| you.distance(*p) > 30.)),
                        6.,
                    )
                }
            };
            node.display = if point.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if let Some(point) = point {
                node.left = px(point.x - radius);
                node.top = px(point.y - radius);
            }
        }
        if let Some(label) = label {
            node.display = if side >= 400. && labels[label.0].is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if let Some(rect) = labels[label.0] {
                node.left = px(rect.min.x);
                node.top = px(rect.min.y);
            }
        }
    }
    if !map.open {
        return;
    }
    for mut image in &mut atlas {
        if let Some(asset) = images.get(&image.image) {
            let size = asset.size().as_vec2();
            let view = map.view();
            image.rect = Some(Rect::from_corners(view.min * size, view.max * size));
        }
    }
    for (mut text, mut font, location, town, scale) in &mut texts {
        if location.is_some() {
            font.font_size = (if side < 280. { 13. } else { 15. }).into();
            text.0 = format!(
                "{}\nX {:.0}  Y {:.0}  Z {:.0}",
                if session.observer.is_some() {
                    "Your camera"
                } else {
                    "Your position"
                },
                you[0],
                you[1],
                you[2]
            );
        }
        if let Some(index) = town {
            font.font_size = (if side < 280. {
                13.
            } else if compact {
                14.
            } else {
                16.
            })
            .into();
            let town = &towns[index.0];
            let delta = Vec2::new(town.center[0] - you[0], town.center[2] - you[2]);
            let distance = if delta.length() >= 1000. {
                format!("{:.1} km", delta.length() / 1000.)
            } else {
                format!("{:.0} m", delta.length())
            };
            text.0 = format!("{}. {}  ·  {}", index.0 + 1, town.name, distance);
        }
        if scale.is_some() {
            let meters =
                world.0.radius_cells() as f32 * 2. * rubblekin_core::world::CELL_SIZE / map.zoom;
            let span = if meters >= 1000. {
                format!("{:.1} km across", meters / 1000.)
            } else {
                format!("{meters:.0} m across")
            };
            text.0 = if compact {
                format!("{span}  ·  Cyan: you  ·  Gold: towns  ·  M / Back: return")
            } else {
                format!(
                    "{span}  ·  Cyan: you  ·  Gold: towns  ·  Ring: spawn  |  Scroll: zoom  ·  Drag: pan  ·  C: center on you  ·  R: whole world"
                )
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, keyboard::KeyboardInput};
    use rubblekin_core::protocol::SessionMode;
    use winit::keyboard::ModifiersState;

    struct Fixture {
        app: App,
        window: Entity,
        canvas: Entity,
        marker: Entity,
        location: Entity,
        root: Entity,
        close: Entity,
    }

    fn fixture() -> Fixture {
        let (world, session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "map test".into(),
            crate::graphics::GraphicsQuality::Balanced,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(VoxelWorld(world))
            .insert_resource(session)
            .init_resource::<WorldMap>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::airships::PilotConversation>()
            .init_resource::<TouchControls>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Assets<Image>>()
            .add_message::<MenuKey>()
            .add_message::<TouchInput>()
            .add_systems(Update, (read, refresh).chain());
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(1000, 700),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let image = crate::world_map_image::valley_image(&app.world().resource::<VoxelWorld>().0);
        let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        let canvas = app
            .world_mut()
            .spawn((MapCanvas, Node::default(), ImageNode::new(handle)))
            .id();
        let marker = app
            .world_mut()
            .spawn((MapMarker::You, Node::default()))
            .id();
        let location = app
            .world_mut()
            .spawn((MapPosition, Text::new(""), TextFont::from_font_size(15.)))
            .id();
        app.world_mut()
            .spawn((MapScale, Text::new(""), TextFont::from_font_size(14.)));
        let root = app.world_mut().spawn((MapRoot, Node::default())).id();
        let close = app
            .world_mut()
            .spawn((
                MapClose,
                Interaction::None,
                Node::default(),
                ComputedNode {
                    size: Vec2::new(120., 60.),
                    ..default()
                },
                UiGlobalTransform::from_xy(400., 100.),
            ))
            .id();
        Fixture {
            app,
            window,
            canvas,
            marker,
            location,
            root,
            close,
        }
    }

    fn press(fixture: &mut Fixture, key: KeyCode) {
        {
            let mut keys = fixture
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
        }
        fixture.app.update();
    }

    fn clear_keys(fixture: &mut Fixture) {
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        fixture.app.update();
    }

    #[test]
    fn m_escape_and_return_close_leave_one_blocked_frame_then_resume() {
        let mut fixture = fixture();
        for closer in [Some(KeyCode::KeyM), Some(KeyCode::Escape), None] {
            press(&mut fixture, KeyCode::KeyM);
            let map = fixture.app.world().resource::<WorldMap>();
            assert!(map.open && map.input_blocked && !map.just_closed);
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<Node>(fixture.root)
                    .unwrap()
                    .display,
                Display::Flex
            );
            clear_keys(&mut fixture);
            if let Some(key) = closer {
                press(&mut fixture, key);
            } else {
                fixture
                    .app
                    .world_mut()
                    .entity_mut(fixture.close)
                    .insert(Interaction::Pressed);
                fixture.app.update();
            }
            let map = fixture.app.world().resource::<WorldMap>();
            assert!(!map.open && map.input_blocked && map.just_closed);
            assert_eq!(
                fixture
                    .app
                    .world()
                    .get::<Node>(fixture.root)
                    .unwrap()
                    .display,
                Display::None
            );
            clear_keys(&mut fixture);
            assert!(!fixture.app.world().resource::<WorldMap>().input_blocked);
        }
    }

    #[test]
    fn m_respects_other_modals_and_pause_map_request_can_transition_after_closing() {
        let mut fixture = fixture();
        for modal in 0..5 {
            {
                let world = fixture.app.world_mut();
                *world.resource_mut::<crate::pause::PauseMenu>() = default();
                *world.resource_mut::<crate::admin_console::AdminConsole>() = default();
                *world.resource_mut::<crate::airships::PilotConversation>() = default();
                match modal {
                    0 => world.resource_mut::<crate::pause::PauseMenu>().open = true,
                    1 => {
                        world
                            .resource_mut::<crate::pause::PauseMenu>()
                            .input_blocked = true
                    }
                    2 => {
                        world
                            .resource_mut::<crate::admin_console::AdminConsole>()
                            .input_blocked = true
                    }
                    3 => {
                        world
                            .resource_mut::<crate::airships::PilotConversation>()
                            .ship_id = Some(7)
                    }
                    _ => {
                        world
                            .resource_mut::<crate::airships::PilotConversation>()
                            .input_blocked = true
                    }
                }
            }
            press(&mut fixture, KeyCode::KeyM);
            assert!(!fixture.app.world().resource::<WorldMap>().open);
            clear_keys(&mut fixture);
        }
        {
            let world = fixture.app.world_mut();
            *world.resource_mut::<crate::airships::PilotConversation>() = default();
            world
                .resource_mut::<crate::pause::PauseMenu>()
                .input_blocked = true;
            world.resource_mut::<WorldMap>().requested = true;
        }
        fixture.app.update();
        let map = fixture.app.world().resource::<WorldMap>();
        assert!(map.open && map.input_blocked && !map.requested);
    }

    #[test]
    fn touch_return_uses_physical_ui_bounds_and_native_back_closes_the_map() {
        let mut fixture = fixture();
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(2.));
        press(&mut fixture, KeyCode::KeyM);
        clear_keys(&mut fixture);
        fixture.app.world_mut().write_message(TouchInput {
            id: 1,
            phase: TouchPhase::Started,
            position: Vec2::new(200., 50.),
            window: fixture.window,
            force: None,
        });
        fixture.app.update();
        assert!(fixture.app.world().resource::<WorldMap>().just_closed);
        clear_keys(&mut fixture);
        press(&mut fixture, KeyCode::KeyM);
        clear_keys(&mut fixture);
        fixture.app.world_mut().write_message(MenuKey {
            input: KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::BrowserBack,
                text: None,
                state: ButtonState::Pressed,
                repeat: false,
                window: fixture.window,
            },
            modifiers: ModifiersState::empty(),
        });
        fixture.app.update();
        let map = fixture.app.world().resource::<WorldMap>();
        assert!(!map.open && map.just_closed && map.input_blocked);
    }

    #[test]
    fn refresh_tracks_live_player_and_observer_positions_and_the_atlas_crop() {
        let mut fixture = fixture();
        {
            let world = fixture.app.world_mut();
            {
                let mut map = world.resource_mut::<WorldMap>();
                map.open = true;
                map.zoom = 2.;
            }
            world.resource_mut::<Session>().body.position = [20., 17., -20.];
        }
        fixture.app.update();
        let (side, _) = layout(fixture.app.world().get::<Window>(fixture.window).unwrap());
        let marker = fixture.app.world().get::<Node>(fixture.marker).unwrap();
        assert_eq!(marker.left, px(side * 0.75 - 8.));
        assert_eq!(marker.top, px(side * 0.25 - 8.));
        assert!(
            fixture
                .app
                .world()
                .get::<Text>(fixture.location)
                .unwrap()
                .0
                .contains("Your position\nX 20  Y 17  Z -20")
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<ImageNode>(fixture.canvas)
                .unwrap()
                .rect,
            Some(Rect::from_corners(Vec2::splat(80.), Vec2::splat(240.)))
        );
        {
            let mut session = fixture.app.world_mut().resource_mut::<Session>();
            let mut camera = crate::observer::ObserverCamera::new([0.; 3]);
            camera.position = Vec3::new(-20., 70., 20.);
            session.observer = Some(camera);
            session.body.position = [70.; 3];
        }
        fixture.app.update();
        let marker = fixture.app.world().get::<Node>(fixture.marker).unwrap();
        assert_eq!(marker.left, px(side * 0.25 - 8.));
        assert_eq!(marker.top, px(side * 0.75 - 8.));
        assert!(
            fixture
                .app
                .world()
                .get::<Text>(fixture.location)
                .unwrap()
                .0
                .contains("Your camera\nX -20  Y 70  Z 20")
        );
        fixture
            .app
            .world_mut()
            .resource_mut::<Session>()
            .observer
            .as_mut()
            .unwrap()
            .position = Vec3::new(70., 60., 70.);
        fixture.app.update();
        assert_eq!(
            fixture
                .app
                .world()
                .get::<Node>(fixture.marker)
                .unwrap()
                .display,
            Display::None,
            "a live camera outside the cropped map hides its marker"
        );
    }

    #[test]
    fn cursor_zoom_keeps_the_same_atlas_texel_under_a_scaled_ui_pointer() {
        let mut fixture = fixture();
        {
            let mut window = fixture
                .app
                .world_mut()
                .get_mut::<Window>(fixture.window)
                .unwrap();
            window.resolution.set_scale_factor_override(Some(2.));
            window.set_cursor_position(Some(Vec2::new(180., 120.)));
        }
        fixture.app.world_mut().entity_mut(fixture.canvas).insert((
            ComputedNode {
                size: Vec2::splat(400.),
                inverse_scale_factor: 0.5,
                ..default()
            },
            UiGlobalTransform::from_xy(300., 300.),
        ));
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture
            .app
            .world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0., 4.);
        fixture.app.update();
        let map = fixture.app.world().resource::<WorldMap>();
        assert!(map.zoom > 1.);
        let pointer = Vec2::new(0.65, 0.35);
        let view = map.view();
        let atlas_uv = view.min + pointer * view.size();
        assert!((atlas_uv - pointer).length() < 0.00001);
        let crop = fixture
            .app
            .world()
            .get::<ImageNode>(fixture.canvas)
            .unwrap()
            .rect
            .unwrap();
        let atlas_texel = crop.min + pointer * crop.size();
        assert!((atlas_texel - pointer * 320.).length() < 0.001);
    }
}
