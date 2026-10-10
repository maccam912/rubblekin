//! One pictured invitation to the existing authoritative work offer.
use crate::{GameEntity, Session, VoxelWorld, market::MarketPanel, touch::TouchControls};
use bevy::{prelude::*, window::PrimaryWindow};
use rubblekin_core::economy::{WorkKind, WorkOffer, WorkReward};

#[derive(Resource)]
pub(crate) struct Pictures([Handle<Image>; 3]);
#[derive(Component)]
pub(crate) struct Card;
#[derive(Component)]
pub(crate) struct Tool;
#[derive(Component)]
pub(crate) struct Caption;
#[derive(Component)]
pub(crate) struct Progress;

fn bounds(size: Vec2, touch: bool, teaching: Option<Rect>) -> Rect {
    let left = if touch {
        let scale = (size.x / 840.).min(size.y / 400.).clamp(0.5, 1.);
        (size.x * 0.36).min(size.x - 328. * scale - 218.).max(8.)
    } else {
        size.x * 0.36
    };
    let mut card = Rect::from_corners(
        Vec2::new(left, if touch { 70. } else { 114. }),
        Vec2::new(left + 210., if touch { 176. } else { 220. }),
    );
    if let Some(lesson) = teaching
        && !card.intersect(lesson).is_empty()
    {
        card = Rect::from_corners(
            Vec2::new(left, lesson.max.y + 8.),
            Vec2::new(left + 210., lesson.max.y + 114.),
        );
    }
    card
}

enum Cue<'a> {
    Work(&'a WorkOffer, Option<f32>),
    Reward(WorkReward),
}

fn cue<'a>(
    panel: &'a MarketPanel,
    session: &Session,
    world: &VoxelWorld,
    now: f64,
) -> Option<Cue<'a>> {
    if session.help
        || session.inspector
        || session.observer.is_some()
        || session.inventory.input_blocked
        || panel.open
        || panel.input_blocked
    {
        return None;
    }
    if let Some(active) = panel.active_work() {
        return crate::work_animation::can_pose(session, active).then_some(Cue::Work(
            &active.offer,
            Some((active.elapsed_seconds / active.offer.duration_seconds.max(0.01)).clamp(0., 1.)),
        ));
    }
    if let Some(reward) = panel.recent_reward(now) {
        return Some(Cue::Reward(reward));
    }
    if crate::activities::touch_opportunity(session) {
        return None;
    }
    panel
        .contextual_work(session, world, now)
        .map(|offer| Cue::Work(offer, None))
}

pub(crate) fn panel_at(
    position: Vec2,
    size: Vec2,
    session: &Session,
    panel: &MarketPanel,
    world: &VoxelWorld,
    now: f64,
    teaching: Option<Rect>,
) -> bool {
    cue(panel, session, world, now).is_some() && bounds(size, true, teaching).contains(position)
}

pub(crate) fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    commands.insert_resource(Pictures(std::array::from_fn(|i| {
        images.add(crate::work_tools::picture(i))
    })));
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            Card,
            Node {
                position_type: PositionType::Absolute,
                width: px(210.),
                height: px(106.),
                padding: UiRect::all(px(8.)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.),
                display: Display::None,
                border_radius: BorderRadius::all(px(8.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.10, 0.09, 0.93)),
        ))
        .with_children(|root| {
            root.spawn(Node {
                column_gap: px(8.),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Tool,
                    ImageNode::default(),
                    Node {
                        width: px(52.),
                        height: px(52.),
                        flex_shrink: 0.,
                        ..default()
                    },
                ));
                row.spawn((
                    Caption,
                    Text::new(""),
                    TextFont::from_font_size(16.).with_font(font),
                    TextColor(Color::srgb(0.94, 0.88, 0.72)),
                ));
            });
            root.spawn((
                Node {
                    width: percent(100.),
                    height: px(12.),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.15, 0.23, 0.20)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Progress,
                    Node {
                        width: percent(0.),
                        height: percent(100.),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.92, 0.73, 0.31)),
                ));
            });
        });
}

fn title(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::TendField => "Tend field",
        WorkKind::WorkshopMaintenance => "Workshop",
        WorkKind::HarvestField => "Harvest",
        WorkKind::QuarryStone => "Collect stone",
        WorkKind::GatherForage => "Gather food",
        WorkKind::Salvage => "Salvage",
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    session: Res<Session>,
    world: Res<VoxelWorld>,
    panel: Res<MarketPanel>,
    pictures: Res<Pictures>,
    trade: Res<crate::trade_pictures::TradePictures>,
    time: Res<Time>,
    touch: Res<TouchControls>,
    tutorials: Res<crate::tutorials::Tutorials>,
    windows: Query<&Window, With<PrimaryWindow>>,
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::world_map::WorldMap>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::airships::PilotConversation>,
    ),
    mut roots: Query<&mut Node, (With<Card>, Without<Progress>)>,
    mut progress: Query<&mut Node, (With<Progress>, Without<Card>)>,
    mut tools: Query<&mut ImageNode, With<Tool>>,
    mut captions: Query<&mut Text, With<Caption>>,
    mut gizmos: Gizmos,
) {
    let (pause, map, console, travel) = modals;
    let window = windows.iter().next();
    let visible = window
        .filter(|w| w.focused)
        .filter(|_| {
            !touch.suspended
                && !touch.menu_open
                && !pause.open
                && !pause.input_blocked
                && !map.open
                && !map.input_blocked
                && !console.input_blocked
                && !travel.open()
                && !travel.input_blocked
        })
        .and_then(|_| cue(&panel, &session, &world, time.elapsed_secs_f64()));
    for mut root in &mut roots {
        root.display = if visible.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(w) = window {
            let rect = bounds(
                Vec2::new(w.width(), w.height()),
                touch.enabled,
                tutorials.world_rect,
            );
            root.left = px(rect.min.x);
            root.top = px(rect.min.y);
        }
    }
    let Some(visible) = visible else {
        return;
    };
    let (picture, caption, fraction, target) = match visible {
        Cue::Reward(reward) => {
            let (picture, caption) = match reward {
                WorkReward::Coins(coins) => (trade.coins.clone(), format!("Earned {coins} coins")),
                WorkReward::Cargo { kind, amount } => (
                    trade.resource(kind),
                    format!("+{amount} {}\nSell at a market", kind.name()),
                ),
            };
            (picture, caption, Some(1.), None)
        }
        Cue::Work(offer, fraction) => {
            let reward = match offer.reward {
                WorkReward::Coins(n) => format!("{n} coins"),
                WorkReward::Cargo { kind, amount } => format!("{amount} {}", kind.name()),
            };
            let caption = format!(
                "{}\n{} · {}",
                title(offer.site.kind),
                if fraction.is_some() {
                    "Stay here"
                } else if touch.enabled {
                    "Work"
                } else {
                    "T: Work"
                },
                reward
            );
            (
                pictures.0[crate::work_tools::tool_index(offer.site.kind)].clone(),
                caption,
                fraction,
                Some(offer.position),
            )
        }
    };
    for mut image in &mut tools {
        image.image = picture.clone();
    }
    for mut text in &mut captions {
        if text.0 != caption {
            text.0.clone_from(&caption);
        }
    }
    for mut bar in &mut progress {
        bar.width = percent(fraction.unwrap_or(0.) * 100.);
    }
    // Point out the actual authoritative target; no speculative distant job marker.
    if let Some(position) = target {
        gizmos.cube(
            Transform::from_translation(Vec3::from_array(position) + Vec3::Y * 0.3)
                .with_scale(Vec3::new(0.7, 0.6, 0.7)),
            Color::srgb(0.95, 0.76, 0.30),
        );
    }
}
