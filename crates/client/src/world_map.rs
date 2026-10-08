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
use rubblekin_core::{settlement::Village, village_assets::BuildingKind};

#[derive(Resource)]
pub(crate) struct WorldMap {
    pub open: bool,
    pub input_blocked: bool,
    pub just_closed: bool,
    pub requested: bool,
    zoom: f32,
    center: Vec2,
    drag: Option<Vec2>,
    contacts: [Option<MapContact>; 2],
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
            contacts: [None; 2],
        }
    }
}
#[derive(Clone, Copy)]
struct MapContact {
    id: u64,
    pointer: Vec2,
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
    fn zoom_at(&mut self, pointer: Vec2, factor: f32) {
        let anchor = self.view().min + pointer / self.zoom;
        self.zoom = (self.zoom * factor).clamp(1., 8.);
        self.center = anchor + (Vec2::splat(0.5) - pointer) / self.zoom;
        self.center = self.view().center();
    }
    fn cancel_drag(&mut self) {
        self.drag = None;
        self.contacts = [None; 2];
    }
    fn action(&mut self, action: MapAction, player: Vec2) {
        self.cancel_drag();
        match action {
            MapAction::Close => self.open = false,
            MapAction::ZoomIn => self.zoom_at(Vec2::splat(0.5), 1.5),
            MapAction::ZoomOut => self.zoom_at(Vec2::splat(0.5), 1. / 1.5),
            MapAction::Center => {
                self.center = player;
                self.center = self.view().center();
            }
            MapAction::WholeWorld => {
                self.zoom = 1.;
                self.center = Vec2::splat(0.5);
            }
        }
    }
    fn finger(&mut self, id: u64, phase: TouchPhase, pointer: Vec2) {
        if !pointer.is_finite() && matches!(phase, TouchPhase::Started | TouchPhase::Moved) {
            return;
        }
        if phase == TouchPhase::Started {
            if Rect::from_corners(Vec2::ZERO, Vec2::ONE).contains(pointer)
                && !self
                    .contacts
                    .iter()
                    .flatten()
                    .any(|contact| contact.id == id)
                && let Some(slot) = self.contacts.iter_mut().find(|slot| slot.is_none())
            {
                *slot = Some(MapContact { id, pointer });
                self.drag = None;
            }
            return;
        }
        let Some(index) = self
            .contacts
            .iter()
            .position(|slot| slot.is_some_and(|c| c.id == id))
        else {
            return;
        };
        if phase == TouchPhase::Canceled {
            self.cancel_drag();
            return;
        }
        if !pointer.is_finite() {
            if phase == TouchPhase::Ended {
                self.contacts[index] = None;
            }
            return;
        }
        let previous = self.contacts[index].unwrap().pointer;
        self.contacts[index].as_mut().unwrap().pointer = pointer;
        if let Some(other) = self.contacts[1 - index] {
            let old_midpoint = (previous + other.pointer) * 0.5;
            let midpoint = (pointer + other.pointer) * 0.5;
            let anchor = self.view().min + old_midpoint / self.zoom;
            let old_span = previous.distance(other.pointer);
            let span = pointer.distance(other.pointer);
            // Ignore the ill-conditioned pinch scale while contacts overlap.
            if old_span > 0.02 && span > 0.02 {
                self.zoom = (self.zoom * span / old_span).clamp(1., 8.);
            }
            self.center = anchor + (Vec2::splat(0.5) - midpoint) / self.zoom;
        } else {
            self.center -= (pointer - previous) / self.zoom;
        }
        self.center = self.view().center();
        if phase == TouchPhase::Ended {
            self.contacts[index] = None;
        }
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
    Roadside(usize),
    You,
    Spawn,
}
#[derive(Component)]
pub(super) struct MapRoadsideGlyph(usize);
#[derive(Component)]
pub(super) struct MapPosition;
#[derive(Component)]
pub(super) struct MapTownDistance(usize);
#[derive(Component)]
pub(super) struct MapScale;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum MapAction {
    Close,
    ZoomIn,
    ZoomOut,
    Center,
    WholeWorld,
}
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
        (window.height() - if compact { 144. } else { 184. })
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

fn landmark_badge(town: &Village) -> &'static str {
    let windmill = town
        .buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Windmill);
    let lookout = town
        .buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Lookout);
    match (windmill, lookout) {
        (true, true) => " [W L]",
        (true, false) => " [W]",
        (false, true) => " [L]",
        _ => "",
    }
}

fn town_description(town: &Village) -> String {
    let homes = [
        (BuildingKind::Cottage, "Cottages"),
        (BuildingKind::TimberCabin, "Timber cabins"),
        (BuildingKind::MasonryCottage, "Stone cottages"),
        (BuildingKind::UplandHouse, "Upland homes"),
    ]
    .into_iter()
    .map(|(kind, label)| {
        (
            town.buildings
                .iter()
                .filter(|building| building.kind == kind)
                .count(),
            label,
        )
    })
    .filter(|(count, _)| *count > 0)
    .max_by_key(|(count, _)| *count)
    .map(|(_, label)| label);
    let specialty = town.kind.name().trim_end_matches(" village");
    homes.map_or_else(
        || specialty.into(),
        |homes| format!("{specialty} · {homes}"),
    )
}

fn town_row(town: &Village, index: usize, you: [f32; 3], detailed: bool) -> String {
    let distance = Vec2::new(town.center[0] - you[0], town.center[2] - you[2]).length();
    let distance = if distance >= 1000. {
        format!("{:.1} km", distance / 1000.)
    } else {
        format!("{distance:.0} m")
    };
    let mut text = format!(
        "{}. {} · {}{}",
        index + 1,
        town.name,
        distance,
        landmark_badge(town)
    );
    if detailed {
        text.push_str(&format!("\n{}", town_description(town)));
    }
    text
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
    let roadside = world
        .0
        .settlements()
        .map(|plan| plan.roadside_landmarks.as_slice())
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
                        MapAction::Close,
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
                        for (index, site) in roadside.iter().enumerate() {
                            canvas
                                .spawn((
                                    MapMarker::Roadside(index),
                                    ZIndex(1),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        width: px(16),
                                        height: px(16),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border_radius: BorderRadius::all(px(3)),
                                        border: UiRect::all(px(1)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.23, 0.15, 0.28)),
                                    BorderColor::all(Color::srgb(0.83, 0.65, 0.92)),
                                ))
                                .with_child((
                                    label(roadside_symbol(site.building.kind), &font, 11.),
                                    MapRoadsideGlyph(index),
                                ));
                        }
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
                        if towns.iter().any(|town| !landmark_badge(town).is_empty()) {
                            sidebar.spawn(label("TOWNS · W: windmill · L: lookout", &font, 14.));
                        } else {
                            sidebar.spawn(label("TOWNS", &font, 20.));
                        }
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
            root.spawn(Node {
                column_gap: px(8),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|controls| {
                for (action, text) in [
                    (MapAction::ZoomOut, "Zoom -"),
                    (MapAction::ZoomIn, "Zoom +"),
                    (MapAction::Center, "Center on you"),
                    (MapAction::WholeWorld, "Whole world"),
                ] {
                    controls
                        .spawn((
                            Button,
                            action,
                            Node {
                                padding: UiRect::axes(px(12), px(6)),
                                border_radius: BorderRadius::all(px(6)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.16, 0.29, 0.28)),
                        ))
                        .with_child(label(text, &font, 15.));
                }
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
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::airships::PilotConversation>,
        Option<Res<crate::market::MarketPanel>>,
    ),
    mut native: MessageReader<MenuKey>,
    mut fingers: MessageReader<TouchInput>,
    buttons: Query<(&MapAction, &Interaction), Changed<Interaction>>,
    targets: Query<(&MapAction, &ComputedNode, &UiGlobalTransform)>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<MapCanvas>>,
) {
    let (pause, console, dialog, market) = modals;
    let was_open = map.open;
    map.just_closed = false;
    map.input_blocked = was_open;
    let mut back = false;
    for key in native.read() {
        back |= key.input.state.is_pressed()
            && !key.input.repeat
            && key.input.logical_key == Key::BrowserBack;
    }
    let events: Vec<_> = fingers.read().copied().collect();
    if !window.focused || touch.suspended {
        map.cancel_drag();
        return;
    }
    let touch_action = events.iter().find_map(|finger| {
        (was_open && finger.phase == TouchPhase::Started)
            .then(|| {
                targets.iter().find_map(|(action, node, transform)| {
                    node.contains_point(*transform, finger.position * window.scale_factor())
                        .then_some(*action)
                })
            })
            .flatten()
    });
    // Native touch and Bevy's emulated click can describe the same press.
    let action = touch_action.or_else(|| {
        // Bevy can synthesize a touch click at a stale mouse cursor position.
        // Native touch hit testing owns these frames and active gestures.
        if !events.is_empty() || map.contacts.iter().any(Option::is_some) {
            return None;
        }
        buttons.iter().find_map(|(action, interaction)| {
            (*interaction == Interaction::Pressed).then_some(*action)
        })
    });
    let requested = std::mem::take(&mut map.requested);
    let other_modal = console.input_blocked
        || dialog.open()
        || dialog.input_blocked
        || pause.open
        || pause.input_blocked
        || market.is_some_and(|market| market.open || market.input_blocked);
    if !was_open && (requested || (!other_modal && keys.just_pressed(KeyCode::KeyM))) {
        map.open = true;
    } else if was_open
        && (keys.just_pressed(KeyCode::KeyM)
            || keys.just_pressed(KeyCode::Escape)
            || back
            || action == Some(MapAction::Close))
    {
        map.open = false;
    }
    map.input_blocked = was_open || map.open;
    map.just_closed = was_open && !map.open;
    if was_open != map.open {
        touch.reset();
        map.cancel_drag();
    }
    if !map.open || !was_open {
        return;
    }
    let player = map_uv(&world.0, position(&session));
    if let Some(action) = action {
        map.action(action, player);
    } else if keys.just_pressed(KeyCode::KeyR) {
        map.action(MapAction::WholeWorld, player);
    } else if keys.just_pressed(KeyCode::KeyC) {
        map.action(MapAction::Center, player);
    }
    let Some((node, transform)) = canvas.iter().next() else {
        map.cancel_drag();
        return;
    };
    let pointer = |position: Vec2| {
        (position * window.scale_factor() - transform.translation) / node.size() + Vec2::splat(0.5)
    };
    let had_contacts = map.contacts.iter().any(Option::is_some);
    if action.is_none() {
        for finger in &events {
            map.finger(finger.id, finger.phase, pointer(finger.position));
        }
    }
    // Never apply synthetic mouse motion on top of a touch gesture.
    if had_contacts || map.contacts.iter().any(Option::is_some) || !events.is_empty() {
        map.drag = None;
        return;
    }
    if let Some(cursor) = window.cursor_position() {
        let pointer = pointer(cursor);
        let over_map = Rect::from_corners(Vec2::ZERO, Vec2::ONE).contains(pointer);
        if over_map && wheel.delta.y != 0. {
            map.zoom_at(pointer, (wheel.delta.y * 0.16).exp());
        }
        if over_map && mouse.just_pressed(MouseButton::Left) {
            map.drag = Some(pointer);
        }
        if mouse.pressed(MouseButton::Left) {
            if let Some(previous) = map.drag {
                let zoom = map.zoom;
                map.center -= (pointer - previous) / zoom;
                map.drag = Some(pointer);
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
            Option<&MapRoadsideGlyph>,
        ),
        Or<(
            With<MapPosition>,
            With<MapTownDistance>,
            With<MapScale>,
            With<MapRoadsideGlyph>,
        )>,
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
    let roadside = world
        .0
        .settlements()
        .map(|plan| plan.roadside_landmarks.as_slice())
        .unwrap_or(&[]);
    let names: Vec<_> = towns.iter().map(|town| town.name.as_str()).collect();
    let mut reserved: Vec<_> = you_point
        .into_iter()
        .map(|point| Rect::from_corners(point + Vec2::new(-10., -14.), point + Vec2::new(58., 16.)))
        .collect();
    reserved.extend(roadside.iter().filter_map(|site| {
        map.point(map_uv(&world.0, site.building.entrance()), side)
            .map(|point| Rect::from_center_size(point, Vec2::splat(18.)))
    }));
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
                MapMarker::Roadside(index) => (
                    roadside.get(*index).and_then(|site| {
                        map.point(map_uv(&world.0, site.building.entrance()), side)
                    }),
                    if map.zoom < 3.0
                        && roadside
                            .get(*index)
                            .is_some_and(|s| s.building.kind.is_exploration_site())
                    {
                        3.0
                    } else {
                        8.0
                    },
                ),
                MapMarker::You => (you_point, 8.),
                MapMarker::Spawn => {
                    let spawn = map.point(map_uv(&world.0, world.0.spawn_position()), side);
                    (
                        spawn.filter(|p| you_point.is_none_or(|you| you.distance(*p) > 30.)),
                        6.,
                    )
                }
            };
            if matches!(marker, MapMarker::Roadside(_)) {
                node.width = px(radius * 2.0);
                node.height = px(radius * 2.0);
            }
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
    for (mut text, mut font, location, town, scale, glyph) in &mut texts {
        if let Some(glyph) = glyph {
            text.0 = roadside.get(glyph.0).map_or_else(String::new, |site| {
                if map.zoom < 3.0 && site.building.kind.is_exploration_site() {
                    String::new()
                } else {
                    roadside_symbol(site.building.kind).into()
                }
            });
        }
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
            if side >= 520.
                && let Some(site) = roadside.iter().min_by(|a, b| {
                    let distance = |p: [f32; 3]| (p[0] - you[0]).hypot(p[2] - you[2]);
                    distance(a.building.entrance()).total_cmp(&distance(b.building.entrance()))
                })
            {
                let p = site.building.entrance();
                let distance = (p[0] - you[0]).hypot(p[2] - you[2]);
                let name = roadside_name(site.building.kind);
                text.0
                    .push_str(&format!("\nNearest {name}: {:.1} km", distance / 1000.));
            }
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
            // Keep a single line per town on landscape phones. Taller maps
            // have room for local architecture and the village's specialty.
            text.0 = town_row(&towns[index.0], index.0, you, side >= 520.);
        }
        if scale.is_some() {
            let meters =
                world.0.radius_cells() as f32 * 2. * rubblekin_core::world::CELL_SIZE / map.zoom;
            let span = if meters >= 1000. {
                format!("{:.1} km across", meters / 1000.)
            } else {
                format!("{meters:.0} m across")
            };
            let extended = roadside.iter().any(|site| {
                matches!(
                    site.building.kind,
                    BuildingKind::TrailPavilion | BuildingKind::QuarryYard
                )
            });
            let discoveries = roadside
                .iter()
                .any(|site| site.building.kind.is_exploration_site());
            text.0 = if discoveries && compact {
                format!("{span} · Cyan: you · Gold: towns · Violet: places · Drag/pinch")
            } else if discoveries && map.zoom < 3.0 {
                format!(
                    "{span} · Cyan: you · Gold: towns · Violet: places (zoom for symbols) | Drag/scroll · C/R: center/world"
                )
            } else if discoveries {
                format!(
                    "{span} · A arch · S stone · F trunk · T camp · O tower · K kiln | R ruin · P shelter · Q quarry | Drag/scroll · C/R: center/world"
                )
            } else if extended && compact {
                format!("{span} · Cyan: you · Gold: towns · R/S/P/Q: places · Drag/pinch")
            } else if extended {
                format!(
                    "{span} · R ruin · S waystone · P shelter · Q quarry | Cyan: you · Gold: towns | Drag/scroll: pan/zoom · C/R: center/world"
                )
            } else if !roadside.is_empty() && compact {
                format!(
                    "{span} · Cyan: you · Gold: towns · Violet R/S: ruins/waystones · Drag/pinch"
                )
            } else if !roadside.is_empty() {
                format!(
                    "{span} · Cyan: you · Gold: towns · Violet R/S: ruins/waystones | Drag: pan · Scroll/pinch: zoom · C/R: center/world"
                )
            } else if compact {
                format!("{span}  ·  Drag: pan  ·  Pinch: zoom  ·  Cyan: you  ·  Gold: towns")
            } else {
                format!(
                    "{span}  ·  Cyan: you  ·  Gold: towns  ·  Ring: spawn  |  Scroll / pinch: zoom  ·  Drag: pan  ·  C: center on you  ·  R: whole world"
                )
            };
        }
    }
}

fn roadside_symbol(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::TrailRuin => "R",
        BuildingKind::Waystone => "S",
        BuildingKind::TrailPavilion => "P",
        BuildingKind::QuarryYard => "Q",
        BuildingKind::StoneArch => "A",
        BuildingKind::StandingStones => "S",
        BuildingKind::FallenGiant => "F",
        BuildingKind::TrailCamp => "T",
        BuildingKind::RuinedTower => "O",
        BuildingKind::AbandonedKiln => "K",
        _ => "?",
    }
}

fn roadside_name(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::TrailRuin => "trail ruin",
        BuildingKind::Waystone => "waystone",
        BuildingKind::TrailPavilion => "trail shelter",
        BuildingKind::QuarryYard => "quarry workyard",
        BuildingKind::StoneArch => "stone arch",
        BuildingKind::StandingStones => "standing stones",
        BuildingKind::FallenGiant => "fallen giant",
        BuildingKind::TrailCamp => "traveller camp",
        BuildingKind::RuinedTower => "ruined watchtower",
        BuildingKind::AbandonedKiln => "abandoned kiln",
        _ => "place",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, keyboard::KeyboardInput};
    use rubblekin_core::protocol::SessionMode;
    use winit::keyboard::ModifiersState;

    #[test]
    fn town_details_describe_generated_homes_and_only_badge_existing_landmarks() {
        use rubblekin_core::world::{World, WorldGeneration};
        let older = World::generate(42, WorldGeneration::GeographyV3);
        assert!(
            older
                .settlements()
                .unwrap()
                .villages
                .iter()
                .all(|town| landmark_badge(town).is_empty())
        );
        assert!(town_description(&older.settlements().unwrap().villages[0]).contains("Cottages"));
        let world = World::generate(42, WorldGeneration::GeographyV4);
        let towns = &world.settlements().unwrap().villages;
        assert!(towns.iter().any(|town| landmark_badge(town) == " [W]"));
        assert!(towns.iter().any(|town| landmark_badge(town) == " [L]"));
        for (index, town) in towns.iter().enumerate() {
            let compact = town_row(town, index, town.center, false);
            assert!(compact.contains(&town.name) && compact.contains("0 m"));
            assert!(
                !compact.contains('\n'),
                "compact town rows must stay one line"
            );
            let detailed = town_row(town, index, town.center, true);
            assert!(detailed.contains(town.kind.name().trim_end_matches(" village")));
            assert!(
                detailed.contains("Upland homes")
                    || detailed.contains("Stone cottages")
                    || detailed.contains("Timber cabins")
            );
            for (kind, badge) in [(BuildingKind::Windmill, "W"), (BuildingKind::Lookout, "L")] {
                assert_eq!(
                    landmark_badge(town).contains(badge),
                    town.buildings.iter().any(|building| building.kind == kind)
                );
            }
        }
        let mut no_landmark = towns[0].clone();
        no_landmark
            .buildings
            .retain(|building| !building.kind.is_landmark());
        assert!(landmark_badge(&no_landmark).is_empty());
    }

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
            .spawn((
                MapCanvas,
                Node::default(),
                ImageNode::new(handle),
                ComputedNode {
                    size: Vec2::splat(400.),
                    ..default()
                },
                UiGlobalTransform::from_xy(300., 300.),
            ))
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
                MapAction::Close,
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

    #[test]
    fn roadside_markers_follow_map_zoom_crop_and_do_not_invent_sites_in_old_worlds() {
        use rubblekin_core::world::{World, WorldGeneration};
        let mut fixture = fixture();
        let world = World::generate(42, WorldGeneration::GeographyV5);
        let entrance = world.settlements().unwrap().roadside_landmarks[0]
            .building
            .entrance();
        let uv = map_uv(&world, entrance);
        fixture.app.insert_resource(VoxelWorld(world));
        let site = fixture
            .app
            .world_mut()
            .spawn((MapMarker::Roadside(0), Node::default()))
            .id();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.update();
        let side = layout(fixture.app.world().get::<Window>(fixture.window).unwrap()).0;
        let marker = fixture.app.world().get::<Node>(site).unwrap();
        assert_eq!(marker.display, Display::Flex);
        assert_eq!(marker.left, px(uv.x * side - 8.));
        assert_eq!(marker.top, px(uv.y * side - 8.));
        {
            let mut map = fixture.app.world_mut().resource_mut::<WorldMap>();
            map.zoom = 8.;
            map.center = Vec2::splat(if uv.x > 0.5 { 0.0625 } else { 0.9375 });
        }
        fixture.app.update();
        assert_eq!(
            fixture.app.world().get::<Node>(site).unwrap().display,
            Display::None
        );
        fixture.app.world_mut().resource_mut::<WorldMap>().center = uv;
        fixture.app.update();
        assert_eq!(
            fixture.app.world().get::<Node>(site).unwrap().display,
            Display::Flex
        );
        fixture.app.insert_resource(VoxelWorld(World::generate(
            42,
            WorldGeneration::GeographyV4,
        )));
        fixture.app.update();
        assert_eq!(
            fixture.app.world().get::<Node>(site).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn frequent_discoveries_use_small_dots_until_zoom_reveals_their_symbols() {
        use rubblekin_core::world::{World, WorldGeneration};
        let mut fixture = fixture();
        let world = World::generate(42, WorldGeneration::GeographyV6);
        let sites = &world.settlements().unwrap().roadside_landmarks;
        let index = sites
            .iter()
            .position(|s| s.building.kind.is_exploration_site())
            .unwrap();
        let symbol = roadside_symbol(sites[index].building.kind);
        let uv = map_uv(&world, sites[index].building.entrance());
        fixture.app.insert_resource(VoxelWorld(world));
        let marker = fixture
            .app
            .world_mut()
            .spawn((MapMarker::Roadside(index), Node::default()))
            .id();
        let glyph = fixture
            .app
            .world_mut()
            .spawn((
                MapRoadsideGlyph(index),
                Text::new(symbol),
                TextFont::from_font_size(11.),
            ))
            .id();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.update();
        assert_eq!(
            fixture.app.world().get::<Node>(marker).unwrap().width,
            px(6.)
        );
        assert!(fixture.app.world().get::<Text>(glyph).unwrap().0.is_empty());
        {
            let mut map = fixture.app.world_mut().resource_mut::<WorldMap>();
            map.zoom = 4.;
            map.center = uv;
        }
        fixture.app.update();
        assert_eq!(
            fixture.app.world().get::<Node>(marker).unwrap().width,
            px(16.)
        );
        assert_eq!(fixture.app.world().get::<Text>(glyph).unwrap().0, symbol);
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
    fn android_back_repeat_does_not_close_map_and_fresh_press_does() {
        let mut fixture = fixture();
        press(&mut fixture, KeyCode::KeyM);
        clear_keys(&mut fixture);
        for repeat in [true, false] {
            fixture.app.world_mut().write_message(MenuKey {
                input: KeyboardInput {
                    key_code: KeyCode::Escape,
                    logical_key: Key::BrowserBack,
                    text: None,
                    state: ButtonState::Pressed,
                    repeat,
                    window: fixture.window,
                },
                modifiers: ModifiersState::empty(),
            });
            fixture.app.update();
            let map = fixture.app.world().resource::<WorldMap>();
            assert_eq!(map.open, repeat);
            assert!(map.input_blocked);
            assert_eq!(map.just_closed, !repeat);
        }
        fixture.app.update();
        assert!(!fixture.app.world().resource::<WorldMap>().input_blocked);
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

    fn finger(fixture: &mut Fixture, id: u64, phase: TouchPhase, position: Vec2) {
        fixture.app.world_mut().write_message(TouchInput {
            id,
            phase,
            position,
            window: fixture.window,
            force: None,
        });
    }

    #[test]
    fn touch_pan_and_pinch_keep_land_under_the_fingers_at_two_times_ui_scale() {
        let mut fixture = fixture();
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(2.));
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 2.;
        // Canvas is [50, 250] logical pixels on each axis.
        finger(&mut fixture, 7, TouchPhase::Started, Vec2::splat(150.));
        fixture.app.update();
        finger(&mut fixture, 7, TouchPhase::Moved, Vec2::new(170., 130.));
        fixture.app.update();
        assert!(
            (fixture.app.world().resource::<WorldMap>().center - Vec2::new(0.45, 0.55)).length()
                < 0.00001
        );
        finger(&mut fixture, 7, TouchPhase::Ended, Vec2::new(170., 130.));
        fixture.app.update();
        {
            let mut map = fixture.app.world_mut().resource_mut::<WorldMap>();
            map.center = Vec2::splat(0.5);
            map.zoom = 1.;
        }
        finger(&mut fixture, 10, TouchPhase::Started, Vec2::new(130., 150.));
        finger(&mut fixture, 11, TouchPhase::Started, Vec2::new(170., 150.));
        fixture.app.update();
        // Both move in one frame: the original 40px span doubles around its midpoint.
        finger(&mut fixture, 10, TouchPhase::Moved, Vec2::new(110., 150.));
        finger(&mut fixture, 11, TouchPhase::Moved, Vec2::new(190., 150.));
        fixture.app.update();
        let map = fixture.app.world().resource::<WorldMap>();
        assert!((map.zoom - 2.).abs() < 0.00001);
        assert!((map.center - Vec2::splat(0.5)).length() < 0.00001);
        // Lifting either finger rebases naturally to the remaining contact.
        finger(&mut fixture, 10, TouchPhase::Ended, Vec2::new(110., 150.));
        finger(&mut fixture, 11, TouchPhase::Moved, Vec2::new(210., 150.));
        fixture.app.update();
        assert!(
            (fixture.app.world().resource::<WorldMap>().center - Vec2::new(0.45, 0.5)).length()
                < 0.00001
        );
    }

    #[test]
    fn touch_gestures_ignore_third_contacts_and_cancel_on_focus_loss_or_map_close() {
        let mut fixture = fixture();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 2.;
        finger(&mut fixture, 1, TouchPhase::Started, Vec2::new(200., 300.));
        finger(&mut fixture, 2, TouchPhase::Started, Vec2::new(400., 300.));
        finger(&mut fixture, 3, TouchPhase::Started, Vec2::new(300., 200.));
        finger(&mut fixture, 3, TouchPhase::Moved, Vec2::new(600., 400.));
        fixture.app.update();
        assert_eq!(fixture.app.world().resource::<WorldMap>().zoom, 2.);
        assert_eq!(
            fixture.app.world().resource::<WorldMap>().center,
            Vec2::splat(0.5)
        );
        finger(&mut fixture, 1, TouchPhase::Canceled, Vec2::new(200., 300.));
        fixture.app.update();
        assert!(
            fixture
                .app
                .world()
                .resource::<WorldMap>()
                .contacts
                .iter()
                .all(Option::is_none)
        );
        finger(&mut fixture, 2, TouchPhase::Moved, Vec2::new(440., 300.));
        fixture.app.update();
        assert_eq!(
            fixture.app.world().resource::<WorldMap>().center,
            Vec2::splat(0.5)
        );
        for close in [false, true] {
            finger(&mut fixture, 4, TouchPhase::Started, Vec2::splat(300.));
            fixture.app.update();
            if close {
                press(&mut fixture, KeyCode::Escape);
            } else {
                fixture
                    .app
                    .world_mut()
                    .get_mut::<Window>(fixture.window)
                    .unwrap()
                    .focused = false;
                fixture.app.update();
                fixture
                    .app
                    .world_mut()
                    .get_mut::<Window>(fixture.window)
                    .unwrap()
                    .focused = true;
            }
            assert!(
                fixture
                    .app
                    .world()
                    .resource::<WorldMap>()
                    .contacts
                    .iter()
                    .all(Option::is_none)
            );
            finger(&mut fixture, 4, TouchPhase::Moved, Vec2::splat(450.));
            clear_keys(&mut fixture);
            assert_eq!(
                fixture.app.world().resource::<WorldMap>().center,
                Vec2::splat(0.5)
            );
        }
    }

    #[test]
    fn short_touch_swipe_applies_its_release_position_without_a_move_event() {
        let mut fixture = fixture();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 2.;
        finger(&mut fixture, 1, TouchPhase::Started, Vec2::splat(300.));
        finger(&mut fixture, 1, TouchPhase::Ended, Vec2::new(340., 320.));
        fixture.app.update();
        let map = fixture.app.world().resource::<WorldMap>();
        assert!((map.center - Vec2::new(0.45, 0.475)).length() < 0.00001);
        assert!(map.contacts.iter().all(Option::is_none));
    }

    #[test]
    fn a_canvas_finger_cannot_click_a_button_under_the_stale_mouse_cursor() {
        let mut fixture = fixture();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 2.;
        let button = fixture
            .app
            .world_mut()
            .spawn((
                MapAction::ZoomIn,
                Interaction::Pressed,
                ComputedNode {
                    size: Vec2::new(100., 40.),
                    ..default()
                },
                UiGlobalTransform::from_xy(700., 500.),
            ))
            .id();
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(700., 500.)));
        finger(&mut fixture, 1, TouchPhase::Started, Vec2::splat(300.));
        fixture.app.update();
        assert_eq!(fixture.app.world().resource::<WorldMap>().zoom, 2.);
        assert!(fixture.app.world().resource::<WorldMap>().contacts[0].is_some());
        // Even another emulated press during a stationary owned gesture is ignored.
        fixture
            .app
            .world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        fixture.app.update();
        assert_eq!(fixture.app.world().resource::<WorldMap>().zoom, 2.);
        finger(&mut fixture, 1, TouchPhase::Ended, Vec2::new(340., 320.));
        fixture.app.update();
        assert!(
            (fixture.app.world().resource::<WorldMap>().center - Vec2::new(0.45, 0.475)).length()
                < 0.00001
        );
    }

    #[test]
    fn mouse_drag_uses_actual_canvas_bounds_and_stops_after_release() {
        let mut fixture = fixture();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 2.;
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .set_cursor_position(Some(Vec2::splat(300.)));
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        fixture.app.update();
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(380., 340.)));
        fixture.app.update();
        assert!(
            (fixture.app.world().resource::<WorldMap>().center - Vec2::new(0.4, 0.45)).length()
                < 0.00001
        );
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        fixture
            .app
            .world_mut()
            .get_mut::<Window>(fixture.window)
            .unwrap()
            .set_cursor_position(Some(Vec2::splat(450.)));
        fixture.app.update();
        assert!(
            (fixture.app.world().resource::<WorldMap>().center - Vec2::new(0.4, 0.45)).length()
                < 0.00001
        );
    }

    #[test]
    fn map_gestures_clamp_at_edges_and_reject_nonfinite_positions() {
        let mut map = WorldMap {
            zoom: 4.,
            ..default()
        };
        map.finger(1, TouchPhase::Started, Vec2::splat(0.5));
        map.finger(1, TouchPhase::Moved, Vec2::splat(f32::NAN));
        assert_eq!(map.center, Vec2::splat(0.5));
        map.finger(1, TouchPhase::Moved, Vec2::splat(10.));
        assert_eq!(map.view().min, Vec2::ZERO);
        map.zoom_at(Vec2::splat(0.5), 100.);
        assert_eq!(map.zoom, 8.);
        map.zoom_at(Vec2::splat(0.5), 0.001);
        assert_eq!(map.zoom, 1.);
        assert_eq!(map.view(), Rect::from_corners(Vec2::ZERO, Vec2::ONE));
        map.finger(1, TouchPhase::Canceled, Vec2::splat(f32::NAN));
        assert!(map.contacts.iter().all(Option::is_none));
    }

    #[test]
    fn visible_map_actions_support_touch_without_double_applying_emulated_clicks() {
        let mut fixture = fixture();
        fixture.app.world_mut().resource_mut::<WorldMap>().open = true;
        let button = fixture
            .app
            .world_mut()
            .spawn((
                MapAction::ZoomIn,
                Interaction::Pressed,
                ComputedNode {
                    size: Vec2::new(100., 40.),
                    ..default()
                },
                UiGlobalTransform::from_xy(700., 500.),
            ))
            .id();
        finger(&mut fixture, 3, TouchPhase::Started, Vec2::new(700., 500.));
        fixture.app.update();
        assert_eq!(fixture.app.world().resource::<WorldMap>().zoom, 1.5);
        fixture
            .app
            .world_mut()
            .entity_mut(button)
            .insert(Interaction::None);
        fixture.app.update();
        for (action, zoom) in [
            (MapAction::ZoomIn, 2.25),
            (MapAction::ZoomOut, 1.5),
            (MapAction::WholeWorld, 1.),
        ] {
            fixture.app.world_mut().entity_mut(button).insert(action);
            finger(&mut fixture, 3, TouchPhase::Started, Vec2::new(700., 500.));
            fixture.app.update();
            assert_eq!(fixture.app.world().resource::<WorldMap>().zoom, zoom);
        }
        fixture.app.world_mut().resource_mut::<WorldMap>().zoom = 4.;
        fixture
            .app
            .world_mut()
            .resource_mut::<Session>()
            .body
            .position = [20., 0., -20.];
        fixture
            .app
            .world_mut()
            .entity_mut(button)
            .insert(MapAction::Center);
        finger(&mut fixture, 3, TouchPhase::Started, Vec2::new(700., 500.));
        fixture.app.update();
        assert_eq!(
            fixture.app.world().resource::<WorldMap>().center,
            Vec2::new(0.625, 0.375)
        );
        assert!(fixture.app.world().resource::<WorldMap>().input_blocked);
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
