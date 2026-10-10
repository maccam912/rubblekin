//! Discover existing activities without accepting a task or predicting its outcome.
use crate::{
    Session, VoxelWorld,
    world_map::{MapAction, WorldMap},
};
use bevy::{
    picking::hover::Hovered,
    prelude::*,
    ui_widgets::{ActivateOnPress, Button},
    window::PrimaryWindow,
};
use rubblekin_core::activities::{ActivityKind, ActivitySnapshot, MAX_PLANS, PropState};

#[derive(Component)]
pub(crate) struct TownContent;
#[derive(Component)]
pub(crate) struct ActivityContent;
#[derive(Component)]
pub(crate) struct ActivityRow(usize);
#[derive(Component)]
pub(crate) struct ActivityLabel(usize);
#[derive(Component)]
pub(crate) struct ActivityPicture(usize);
#[derive(Component)]
pub(crate) struct ActivityMarker(usize);
#[derive(Component)]
pub(crate) struct Empty;
#[derive(Component)]
pub(crate) struct Tab;

fn label(text: &str, font: &Handle<Font>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from_font_size(size).with_font(font.clone()),
        TextColor(Color::srgb(0.91, 0.94, 0.88)),
    )
}

pub(crate) fn tabs(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    parent
        .spawn(Node {
            column_gap: px(6),
            ..default()
        })
        .with_children(|tabs| {
            for (action, title) in [
                (MapAction::Towns, "Towns"),
                (MapAction::Activities, "Activities"),
            ] {
                tabs.spawn((
                    Tab,
                    action,
                    Button,
                    ActivateOnPress,
                    Hovered::default(),
                    Node {
                        min_height: px(44),
                        padding: UiRect::axes(px(10), px(6)),
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.16, 0.29, 0.28)),
                    BorderColor::all(Color::NONE),
                ))
                .with_child(label(title, font, 15.));
            }
        });
}

pub(crate) fn sidebar(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    parent
        .spawn((
            ActivityContent,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            },
        ))
        .with_children(|body| {
            body.spawn(label("NEAREST ACTIVITIES", font, 15.));
            body.spawn(label("Choose a picture to see its start.", font, 13.));
            for i in 0..3 {
                body.spawn((
                    ActivityRow(i),
                    MapAction::Activity(0),
                    Button,
                    ActivateOnPress,
                    Hovered::default(),
                    Node {
                        min_height: px(56),
                        width: percent(100),
                        padding: UiRect::all(px(6)),
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.24, 0.22)),
                    BorderColor::all(Color::NONE),
                ))
                .with_children(|row| {
                    row.spawn((
                        ActivityPicture(i),
                        ImageNode::default(),
                        Node {
                            width: px(36),
                            height: px(36),
                            flex_shrink: 0.,
                            ..default()
                        },
                    ));
                    row.spawn((ActivityLabel(i), label("", font, 14.)));
                });
            }
            body.spawn((
                Empty,
                label("No available activities in this world.", font, 14.),
            ));
        });
}

pub(crate) fn markers(parent: &mut ChildSpawnerCommands) {
    for i in 0..MAX_PLANS {
        parent.spawn((
            ActivityMarker(i),
            ZIndex(3),
            ImageNode::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: px(24),
                height: px(24),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                padding: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.24, 0.22)),
            BorderColor::all(Color::srgb(0.91, 0.94, 0.88)),
        ));
    }
}

fn nearest(session: &Session) -> Vec<&ActivitySnapshot> {
    let you = crate::world_map::position(session);
    let mut activities: Vec<_> = session.activities.iter().filter(|a| a.available).collect();
    activities.sort_by(|a, b| {
        let distance = |a: &ActivitySnapshot| {
            (a.plan.objects[0][0] - you[0]).hypot(a.plan.objects[0][2] - you[2])
        };
        distance(a)
            .total_cmp(&distance(b))
            .then(a.plan.id.cmp(&b.plan.id))
    });
    activities.truncate(MAX_PLANS);
    activities
}

fn description(
    activity: &ActivitySnapshot,
    world: &rubblekin_core::world::World,
    you: [f32; 3],
) -> String {
    let (title, progress) = match activity.plan.kind {
        ActivityKind::SpilledSupplies => (
            "Return supplies",
            activity
                .props
                .iter()
                .filter(|p| **p == PropState::Placed)
                .count(),
        ),
        ActivityKind::ShapeStones => (
            "Match stones",
            activity
                .faces
                .iter()
                .zip(activity.plan.answer)
                .filter(|(face, answer)| **face == *answer)
                .count(),
        ),
        ActivityKind::CartRepair => (
            "Repair cart",
            activity
                .props
                .iter()
                .filter(|p| **p == PropState::Placed)
                .count(),
        ),
    };
    let place = world
        .settlements()
        .and_then(|p| {
            p.composed_sites
                .iter()
                .find(|s| Some(s.id) == activity.plan.site_id)
        })
        .map(|s| s.arrangement.name());
    let distance =
        (activity.plan.objects[0][0] - you[0]).hypot(activity.plan.objects[0][2] - you[2]);
    let distance = if distance >= 1000. {
        format!("{:.1} km", distance / 1000.)
    } else {
        format!("{distance:.0} m")
    };
    format!(
        "{title}\n{} · {distance} · {}",
        place.unwrap_or("Activity"),
        if activity.complete {
            "Done".into()
        } else {
            format!("{progress}/3")
        }
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn refresh(
    map: Res<WorldMap>,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    window: Single<&Window, With<PrimaryWindow>>,
    scene: Option<Res<crate::activities::Scene>>,
    tutorials: Option<Res<crate::tutorials::Tutorials>>,
    mut content: Query<
        (&mut Node, Has<ActivityContent>),
        Or<(With<TownContent>, With<ActivityContent>)>,
    >,
    mut rows: Query<
        (&ActivityRow, &mut Node, &mut MapAction, &mut BorderColor),
        (
            Without<TownContent>,
            Without<ActivityContent>,
            Without<ActivityMarker>,
        ),
    >,
    mut labels: Query<(&ActivityLabel, &mut Text)>,
    mut pictures: Query<(&ActivityPicture, &mut ImageNode), Without<ActivityMarker>>,
    mut markers: Query<
        (&ActivityMarker, &mut Node, &mut ImageNode, &mut BorderColor),
        (
            Without<TownContent>,
            Without<ActivityContent>,
            Without<ActivityRow>,
        ),
    >,
    mut empty: Query<
        &mut Node,
        (
            With<Empty>,
            Without<TownContent>,
            Without<ActivityContent>,
            Without<ActivityRow>,
            Without<ActivityMarker>,
        ),
    >,
    mut tabs: Query<
        (&MapAction, &mut BorderColor),
        (With<Tab>, Without<ActivityRow>, Without<ActivityMarker>),
    >,
) {
    for (mut node, activities) in &mut content {
        node.display = if map.activities == activities {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (action, mut border) in &mut tabs {
        let selected = matches!(action, MapAction::Activities) == map.activities;
        *border = BorderColor::all(if selected {
            Color::srgb(0.95, 0.76, 0.30)
        } else {
            Color::NONE
        });
    }
    if !map.open {
        return;
    }
    let activities = nearest(&session);
    for mut node in &mut empty {
        node.display = if activities.is_empty() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (row, mut node, mut action, mut border) in &mut rows {
        let Some(a) = activities.get(row.0) else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        *action = MapAction::Activity(a.plan.id);
        *border = BorderColor::all(if map.selected_activity == Some(a.plan.id) {
            Color::srgb(0.95, 0.76, 0.30)
        } else {
            Color::NONE
        });
    }
    for (slot, mut text) in &mut labels {
        text.0 = activities.get(slot.0).map_or_else(String::new, |a| {
            description(a, &world.0, crate::world_map::position(&session))
        });
    }
    let Some(scene) = scene else {
        return;
    };
    for (slot, mut image) in &mut pictures {
        if let Some(a) = activities.get(slot.0) {
            image.image = scene.map_picture(a.plan.kind);
        }
    }
    let (mut side, _) = crate::world_map::layout(&window);
    if tutorials.is_some_and(|t| t.incomplete(crate::tutorials::Lesson::Map)) {
        side = (side - 24.).max(64.);
    }
    for (slot, mut node, mut image, mut border) in &mut markers {
        let entry = activities
            .get(slot.0)
            .filter(|_| map.activities)
            .and_then(|a| {
                map.point(
                    crate::world_map_image::map_uv(&world.0, a.plan.objects[0]),
                    side,
                )
                .map(|point| (*a, point))
            });
        let Some((a, point)) = entry else {
            node.display = Display::None;
            continue;
        };
        let selected = map.selected_activity == Some(a.plan.id);
        let size = if selected || map.tutorial_view().0 >= 3. {
            32.
        } else {
            24.
        };
        node.display = Display::Flex;
        node.left = px(point.x - size / 2.);
        node.top = px(point.y - size / 2.);
        node.width = px(size);
        node.height = px(size);
        node.border = UiRect::all(px(if selected { 3. } else { 1. }));
        image.image = scene.map_picture(a.plan.kind);
        *border = BorderColor::all(if selected {
            Color::srgb(0.95, 0.76, 0.30)
        } else {
            Color::srgb(0.91, 0.94, 0.88)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{activities, protocol::SessionMode};

    #[test]
    fn map_uses_available_shared_snapshots_and_reports_real_progress_without_answers() {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "map activities".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        let plan = activities::plans(&world).remove(0);
        let snapshot = ActivitySnapshot {
            plan,
            revision: 1,
            props: [PropState::Home; 3],
            faces: [0; 3],
            complete: false,
            available: true,
            repair: None,
        };
        for (id, distance, available) in [
            (9, 20., true),
            (4, 20., true),
            (1, 1., false),
            (8, 30., true),
        ] {
            let mut a = snapshot.clone();
            a.plan.id = id;
            a.plan.objects[0] = [
                session.body.position[0] + distance,
                0.,
                session.body.position[2],
            ];
            a.available = available;
            session.activities.push(a);
        }
        assert_eq!(
            nearest(&session)
                .iter()
                .map(|a| a.plan.id)
                .collect::<Vec<_>>(),
            [4, 9, 8]
        );
        session.activities.reverse();
        assert_eq!(
            nearest(&session)
                .iter()
                .map(|a| a.plan.id)
                .collect::<Vec<_>>(),
            [4, 9, 8]
        );
        let mut a = snapshot;
        a.plan.kind = ActivityKind::SpilledSupplies;
        a.props[0] = PropState::Placed;
        assert!(description(&a, &world, session.body.position).ends_with("1/3"));
        a.complete = true;
        assert!(description(&a, &world, session.body.position).ends_with("Done"));
        a.complete = false;
        a.plan.kind = ActivityKind::ShapeStones;
        a.plan.answer = [0, 1, 2];
        a.faces = [0, 2, 2];
        assert!(description(&a, &world, session.body.position).ends_with("2/3"));
    }
}
