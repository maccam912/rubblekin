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
#[derive(Component)]
pub(crate) struct ProgressTrack;
#[derive(Component)]
pub(crate) struct RewardRow;
#[derive(Component)]
pub(crate) struct RewardPicture;
#[derive(Component)]
pub(crate) struct RewardAmount;

const CARD_HEIGHT: f32 = 80.;

fn bounds(size: Vec2, touch: bool, teaching: Option<Rect>) -> Rect {
    let left = if touch { 12. } else { size.x * 0.36 };
    let top = if touch { 94. } else { 114. };
    let mut card = Rect::from_corners(
        Vec2::new(left, top),
        Vec2::new(left + 210., top + CARD_HEIGHT),
    );
    if let Some(lesson) = teaching
        && !card.intersect(lesson).is_empty()
    {
        card = Rect::from_corners(
            Vec2::new(left, lesson.max.y + 8.),
            Vec2::new(left + 210., lesson.max.y + 8. + CARD_HEIGHT),
        );
    }
    card
}

pub(crate) fn has_cue(
    panel: &MarketPanel,
    session: &Session,
    world: &VoxelWorld,
    now: f64,
) -> bool {
    cue(panel, session, world, now).is_some()
}

fn interrupted(session: &Session, now: f64) -> bool {
    session.prediction.waiting() || (now < session.status_until && !session.status.is_empty())
}

enum Cue<'a> {
    Work(&'a WorkOffer, Option<f32>),
    Reward(WorkReward),
    Unavailable(&'a WorkOffer),
    CargoFull,
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
        || session.flying
        || session.gliding
        || session.ride.is_some()
        || session.glider_ride.is_some()
        || session.vehicle.is_some()
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
    if panel.full_cargo_at_work_site(session, world, now) {
        return Some(Cue::CargoFull);
    }
    panel
        .contextual_work_offer(session, world, now)
        .map(|offer| {
            if offer.unavailable_reason.is_some() {
                Cue::Unavailable(offer)
            } else {
                Cue::Work(offer, None)
            }
        })
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
    has_cue(panel, session, world, now)
        && !interrupted(session, now)
        && bounds(size, true, teaching).contains(position)
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
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Bold.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            Card,
            Node {
                position_type: PositionType::Absolute,
                width: px(210.),
                height: px(CARD_HEIGHT),
                padding: UiRect::all(px(6.)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.),
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
                        width: px(44.),
                        height: px(44.),
                        flex_shrink: 0.,
                        ..default()
                    },
                ));
                row.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2.),
                    min_width: px(0.),
                    flex_grow: 1.,
                    ..default()
                })
                .with_children(|content| {
                    content.spawn((
                        Caption,
                        Text::new(""),
                        TextFont::from_font_size(14.).with_font(font.clone()),
                        TextColor(Color::srgb(0.94, 0.88, 0.72)),
                    ));
                    content
                        .spawn((
                            RewardRow,
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(4.),
                                ..default()
                            },
                        ))
                        .with_children(|reward| {
                            reward.spawn((
                                RewardPicture,
                                ImageNode::default(),
                                Node {
                                    width: px(24.),
                                    height: px(24.),
                                    flex_shrink: 0.,
                                    ..default()
                                },
                            ));
                            reward.spawn((
                                RewardAmount,
                                Text::new(""),
                                TextFont::from_font_size(16.).with_font(font.clone()),
                                TextColor(Color::srgb(0.94, 0.88, 0.72)),
                            ));
                        });
                });
            });
            root.spawn((
                ProgressTrack,
                Node {
                    width: percent(100.),
                    height: px(6.),
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
    mut feedback: Query<
        (
            &mut Node,
            Option<&Progress>,
            Option<&ProgressTrack>,
            Option<&RewardRow>,
        ),
        (
            Without<Card>,
            Or<(With<Progress>, With<ProgressTrack>, With<RewardRow>)>,
        ),
    >,
    mut images: Query<(&mut ImageNode, Option<&Tool>), Or<(With<Tool>, With<RewardPicture>)>>,
    mut texts: Query<(&mut Text, Option<&Caption>), Or<(With<Caption>, With<RewardAmount>)>>,
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
                && !interrupted(&session, time.elapsed_secs_f64())
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
    let (picture, caption, reward, fraction, target) = match visible {
        Cue::CargoFull => (
            trade.coins.clone(),
            "Cargo full\nSell at a market".into(),
            None,
            None,
            None,
        ),
        Cue::Unavailable(offer) => {
            let needs_space = match offer.reward {
                WorkReward::Cargo { amount, .. } => panel.ledger.as_ref().is_some_and(|l| {
                    l.cargo_total().saturating_add(amount) > rubblekin_core::economy::CARGO_CAPACITY
                }),
                WorkReward::Coins(_) => false,
            };
            let caption = if needs_space {
                "Make room in cargo\nSell at a market".into()
            } else if touch.enabled {
                "Work unavailable\nCargo: details".into()
            } else {
                "Work unavailable\nB: Cargo for details".into()
            };
            (
                if needs_space {
                    trade.coins.clone()
                } else {
                    pictures.0[crate::work_tools::tool_index(offer.site.kind)].clone()
                },
                caption,
                None,
                None,
                Some(offer.position),
            )
        }
        Cue::Reward(reward) => {
            let (picture, caption) = match reward {
                WorkReward::Coins(coins) => (trade.coins.clone(), format!("Earned {coins} coins")),
                WorkReward::Cargo { kind, amount } => (
                    trade.resource(kind),
                    format!("+{amount} {}\nSell at a market", kind.name()),
                ),
            };
            (picture, caption, None, None, None)
        }
        Cue::Work(offer, fraction) => {
            let reward = match offer.reward {
                WorkReward::Coins(n) => (trade.coins.clone(), n),
                WorkReward::Cargo { kind, amount } => (trade.resource(kind), u64::from(amount)),
            };
            let action = if let Some(fraction) = fraction {
                format!(
                    "Stay · {:.1}/{:.0}s",
                    fraction * offer.duration_seconds,
                    offer.duration_seconds
                )
            } else {
                format!(
                    "{} · {:.0}s",
                    if touch.enabled { "Work" } else { "T: Work" },
                    offer.duration_seconds
                )
            };
            let caption = format!("{}\n{}", title(offer.site.kind), action);
            (
                pictures.0[crate::work_tools::tool_index(offer.site.kind)].clone(),
                caption,
                Some(reward),
                fraction,
                Some(offer.position),
            )
        }
    };
    for (mut image, tool) in &mut images {
        if tool.is_some() {
            image.image = picture.clone();
        } else if let Some((picture, _)) = &reward {
            image.image = picture.clone();
        }
    }
    for (mut text, title) in &mut texts {
        if title.is_some() {
            if text.0 != caption {
                text.0.clone_from(&caption);
            }
        } else if let Some((_, amount)) = &reward {
            crate::ui::set_text(&mut text, format!("+{amount}"));
        }
    }
    for (mut node, bar, track, row) in &mut feedback {
        if bar.is_some() {
            node.width = percent(fraction.unwrap_or(0.) * 100.);
        } else if track.is_some() {
            node.display = if fraction.is_some() {
                Display::Flex
            } else {
                Display::None
            };
        } else if row.is_some() {
            node.display = if reward.is_some() {
                Display::Flex
            } else {
                Display::None
            };
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::gizmos::{AppGizmoBuilder, config::DefaultGizmoConfigGroup};
    use rubblekin_core::economy::{PlayerEconomy, WorkProgress, WorkSite, WorkState};

    #[test]
    fn confirmed_work_pictures_follow_progress_and_share_their_real_touch_bounds() {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "work receipt".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        session.help = false;
        session.inspector = false;
        session.body.on_ground = true;
        session.status_until = 0.;
        let offer = WorkOffer {
            site: WorkSite {
                village_id: 1,
                kind: WorkKind::Salvage,
                index: 0,
            },
            position: session.body.position,
            label: "Collect timber".into(),
            reward: WorkReward::Coins(2),
            duration_seconds: 6.,
            unavailable_reason: None,
        };
        let mut panel = MarketPanel::default();
        panel.work_reply(
            1,
            WorkState {
                offer: None,
                active: Some(WorkProgress {
                    offer,
                    elapsed_seconds: 3.,
                }),
            },
            PlayerEconomy::default(),
            String::new(),
            true,
        );
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(VoxelWorld(world))
            .insert_resource(panel)
            .init_resource::<Time>()
            .init_resource::<TouchControls>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Font>>()
            .init_resource::<crate::block_textures::BlockIcons>()
            .init_resource::<crate::trade_pictures::TradePictures>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::airships::PilotConversation>()
            .init_resource::<crate::tutorials::Tutorials>()
            .init_gizmo_group::<DefaultGizmoConfigGroup>()
            .add_systems(Startup, setup)
            .add_systems(Update, update);
        app.world_mut().resource_mut::<TouchControls>().enabled = true;
        app.world_mut().spawn((
            Window {
                focused: true,
                resolution: (640, 360).into(),
                ..default()
            },
            PrimaryWindow,
        ));
        app.world_mut().run_schedule(Startup);
        app.world_mut().run_schedule(Update);
        let mut card = app.world_mut().query_filtered::<&Node, With<Card>>();
        let mut track = app
            .world_mut()
            .query_filtered::<&Node, With<ProgressTrack>>();
        let mut bar = app.world_mut().query_filtered::<&Node, With<Progress>>();
        let mut amount = app
            .world_mut()
            .query_filtered::<&Text, With<RewardAmount>>();
        let mut caption = app.world_mut().query_filtered::<&Text, With<Caption>>();
        assert_eq!(bar.single(app.world()).unwrap().width, percent(50.));
        assert_eq!(track.single(app.world()).unwrap().display, Display::Flex);
        assert_eq!(amount.single(app.world()).unwrap().0, "+2");
        assert!(
            caption
                .single(app.world())
                .unwrap()
                .0
                .contains("Stay · 3.0/6s")
        );
        assert_eq!(
            app.world().resource::<MarketPanel>().ledger,
            Some(PlayerEconomy::default())
        );
        let owns = |app: &App, point| {
            panel_at(
                point,
                Vec2::new(640., 360.),
                app.world().resource::<Session>(),
                app.world().resource::<MarketPanel>(),
                app.world().resource::<VoxelWorld>(),
                0.,
                app.world()
                    .resource::<crate::tutorials::Tutorials>()
                    .world_rect,
            )
        };
        assert!(owns(&app, Vec2::new(20., 100.)));
        assert!(!owns(&app, Vec2::new(280., 140.)));
        app.world_mut()
            .resource_mut::<crate::tutorials::Tutorials>()
            .world_rect = Some(Rect::from_corners(
            Vec2::new(12., 58.),
            Vec2::new(370., 132.),
        ));
        app.world_mut().run_schedule(Update);
        assert_eq!(card.single(app.world()).unwrap().top, px(140.));
        assert!(!owns(&app, Vec2::new(20., 100.)));
        assert!(owns(&app, Vec2::new(20., 150.)));
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.status = "Work cancelled because you moved away.".into();
            session.status_until = 5.;
        }
        app.world_mut().run_schedule(Update);
        assert_eq!(card.single(app.world()).unwrap().display, Display::None);
        assert!(!owns(&app, Vec2::new(20., 150.)));
        app.world_mut().resource_mut::<Session>().status_until = 0.;
        app.world_mut().run_schedule(Update);
        assert_eq!(card.single(app.world()).unwrap().display, Display::Flex);
        app.world_mut().resource_mut::<Session>().flying = true;
        app.world_mut().run_schedule(Update);
        assert_eq!(card.single(app.world()).unwrap().display, Display::None);
        assert!(!owns(&app, Vec2::new(20., 150.)));
    }
}
