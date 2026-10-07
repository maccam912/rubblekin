use crate::{PALETTE, Session};
use bevy::prelude::*;

#[derive(Component)]
pub struct StatusText;
#[derive(Component)]
pub struct NpcText;
#[derive(Component)]
pub struct NoticeText;
#[derive(Component)]
pub struct NoticePanel;
#[derive(Component)]
pub struct HelpPanel;
#[derive(Component)]
pub struct NpcPanel;
#[derive(Component)]
pub struct PaletteSlot(pub usize);
#[derive(Component)]
pub struct ModeText;

fn ink() -> Color {
    Color::srgb(0.89, 0.92, 0.85)
}
fn panel() -> Color {
    Color::srgba(0.055, 0.10, 0.10, 0.87)
}

pub fn setup_ui(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    session: Res<Session>,
    touch: Option<Res<crate::touch::TouchControls>>,
) {
    let observing = session.observer.is_some();
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    if touch.is_some_and(|touch| touch.enabled) {
        setup_touch_ui(&mut commands, font, observing);
        return;
    }
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(26),
            left: px(30),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
        children![
            (
                Text::new("R U B B L E K I N"),
                TextFont::from_font_size(28.0).with_font(font.clone()),
                TextColor(ink()),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.6)
                }
            ),
            (
                Text::new("MOUNTAINS & VALLEYS  /  EARLY PROTOTYPE"),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(Color::srgb(0.86, 0.76, 0.52)),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.6)
                }
            ),
        ],
    ));
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(24),
            right: px(28),
            padding: UiRect::all(px(15)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(panel()),
        children![(
            Text::new("Preparing the landscape…"),
            TextFont::from_font_size(16.0).with_font(font.clone()),
            TextColor(ink()),
            StatusText
        )],
    ));
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(128),
            right: px(28),
            width: px(340),
            padding: UiRect::all(px(18)),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(panel()),
        NpcPanel,
        children![
            (
                Text::new("INSPECTOR"),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(Color::srgb(0.90, 0.73, 0.42))
            ),
            (
                Text::new("Aim at a character, block, or farm plot…"),
                TextFont::from_font_size(18.0).with_font(font.clone()),
                TextColor(ink()),
                NpcText
            ),
            (
                Text::new("Tab  close · reopen to inspect aim"),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(Color::srgb(0.57, 0.69, 0.65))
            ),
        ],
    ));
    commands.spawn((
        crate::GameEntity,
        Node { position_type: PositionType::Absolute, bottom: px(28), left: px(28), padding: UiRect::all(px(17)), flex_direction: FlexDirection::Column, row_gap: px(8), border_radius: BorderRadius::all(px(8)), ..default() },
        BackgroundColor(panel()), HelpPanel,
        children![
            (Text::new(if observing { "OBSERVE THE WORLD" } else { "MAKE YOURSELF AT HOME" }), TextFont::from_font_size(14.0).with_font(font.clone()), TextColor(Color::srgb(0.90, 0.73, 0.42))),
            (Text::new(format!("{}{}", if observing {
                "W A S D   fly     •     mouse / arrows   look\nQ / E   descend / ascend     •     Shift   5× speed\nScroll   adjust speed     •     R / Home   return to spawn\nTab   inspect aimed target     •     F2   graphics\nV   visit next village     •     M   world map\nEsc   pause menu     •     H   hide controls\nF10   leave world / choose another server\nRead-only camera · no avatar or editing"
            } else {
                "W A S D   move     •     mouse / arrows   look\nSpace   jump     •     Shift   sprint\nLeft click   dig     •     Right click   build\n1–6   materials     •     F   creative flight\nQ / E   descend / ascend     •     scroll   zoom\nTab   inspect aimed target\nG   talk to airship pilot     •     M   world map\nB   cargo, markets & delivery work\nEsc   pause menu     •     H   hide controls\nF10   leave world / choose another server"
            }, if session.can_admin { "\n` / ~   admin commands (help lists commands)" } else { "" })), TextFont::from_font_size(16.0).with_font(font.clone()), TextColor(ink())),
        ],
    ));
    let palette_font = font.clone();
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(28),
            left: percent(35),
            right: percent(22),
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            row_gap: px(9),
            ..default()
        },
        children![
            (
                Text::new("CREATIVE  /  UNLIMITED MATERIALS"),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(ink()),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.6)
                },
                ModeText
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    display: if observing {
                        Display::None
                    } else {
                        Display::Flex
                    },
                    ..default()
                },
                Children::spawn(SpawnIter(PALETTE.into_iter().enumerate().map(
                    move |(index, block)| {
                        let [r, g, b, _] = block.color();
                        (
                            Node {
                                width: px(76),
                                height: px(70),
                                padding: UiRect::all(px(6)),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::all(px(7)),
                                ..default()
                            },
                            BackgroundColor(panel()),
                            BorderColor::all(Color::srgba(0.6, 0.7, 0.6, 0.2)),
                            PaletteSlot(index),
                            children![
                                (
                                    Node {
                                        width: px(24),
                                        height: px(20),
                                        border_radius: BorderRadius::all(px(3)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(r, g, b))
                                ),
                                (
                                    Text::new(format!("{} {}", index + 1, block.name())),
                                    TextFont::from_font_size(13.0).with_font(palette_font.clone()),
                                    TextColor(ink())
                                ),
                            ],
                        )
                    }
                )))
            ),
        ],
    ));
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: percent(50),
            left: percent(50),
            width: px(4),
            height: px(4),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 0.95, 0.78, 0.9)),
    ));
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(92),
            left: px(30),
            max_width: px(600),
            padding: UiRect::all(px(10)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.70)),
        NoticePanel,
        children![(
            Text::new(""),
            TextFont::from_font_size(16.0).with_font(font.clone()),
            TextColor(ink()),
            NoticeText
        )],
    ));
}

fn setup_touch_ui(commands: &mut Commands, font: Handle<Font>, observing: bool) {
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            max_width: percent(62),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            ..default()
        },
        children![
            (
                Text::new("RUBBLEKIN"),
                TextFont::from_font_size(18.).with_font(font.clone()),
                TextColor(ink()),
                TextShadow::default()
            ),
            (
                Text::new("Preparing the landscape…"),
                TextFont::from_font_size(14.).with_font(font.clone()),
                TextColor(ink()),
                StatusText,
                TextShadow::default()
            ),
        ],
    ));
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: px(58),
            left: px(12),
            max_width: percent(58),
            padding: UiRect::all(px(6)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.70)),
        NoticePanel,
        children![(
            Text::new(""),
            TextFont::from_font_size(14.).with_font(font.clone()),
            TextColor(ink()),
            NoticeText
        )],
    ));
    for help in [false, true] {
        let entity = commands.spawn((
            crate::GameEntity,
            GlobalZIndex(24),
            Node {
                position_type: PositionType::Absolute,
                top: px(74), bottom: px(156), left: percent(25), width: percent(50),
                display: Display::None,
                padding: UiRect::all(px(12)), flex_direction: FlexDirection::Column,
                row_gap: px(8), border_radius: BorderRadius::all(px(8)),
                overflow: Overflow::scroll_y(), ..default()
            },
            ScrollPosition::default(), BackgroundColor(panel()),
        )).with_children(|panel| {
            panel.spawn((
                Text::new(if help { "CONTROLS · tap Controls in Menu to close" } else { "INSPECTOR · tap Inspect to close" }),
                TextFont::from_font_size(14.).with_font(font.clone()), TextColor(Color::srgb(0.90, 0.73, 0.42)),
                Node { flex_shrink: 0., ..default() },
            ));
            if help {
                panel.spawn((
                    Text::new(if observing {
                        "Left stick: fly · swipe the world: look\nRise / Fall: vertical flight · Sprint: boost\n+ / −: camera speed\nMenu: graphics, return to spawn, next village, servers\nInspect: aimed character, block, or plot · swipe panel to scroll\nRead-only observer: no avatar or editing"
                    } else {
                        "Left stick: move · swipe the world: look\nJump: hop · Sprint: run · Fly: creative flight\nRise / Fall: vertical flight · + / −: camera distance\nDig / Build: change the block under the center dot\nTap a material tile to choose a building block\nInspect: aimed character, block, or plot · swipe panel to scroll\nWalk or jump onto a landed airship to ride\nMove / jump normally aboard · Pilot asks the route\nCargo: coins, goods and delivery work · trade at market entrances\nMenu: graphics, controls, and leave world"
                    }),
                    TextFont::from_font_size(16.).with_font(font.clone()), TextColor(ink()),
                    Node { flex_shrink: 0., ..default() },
                ));
            } else {
                panel.spawn((Text::new("Aim at a character, block, or farm plot…"), TextFont::from_font_size(16.).with_font(font.clone()), TextColor(ink()), NpcText, Node { flex_shrink: 0., ..default() }));
            }
        }).id();
        if help {
            commands.entity(entity).insert(HelpPanel);
        } else {
            commands.entity(entity).insert(NpcPanel);
        }
    }
    commands.spawn((
        crate::GameEntity,
        Node {
            position_type: PositionType::Absolute,
            top: percent(50),
            left: percent(50),
            width: px(4),
            height: px(4),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::srgba(1., 0.95, 0.78, 0.9)),
    ));
}

/// The touch router and rendered inspector/help panels share these exact bounds.
pub(crate) fn touch_panel_at(position: Vec2, size: Vec2, session: &Session) -> bool {
    (session.inspector || session.help)
        && Rect::from_corners(
            Vec2::new(size.x * 0.25, 74.),
            Vec2::new(size.x * 0.75, size.y - 156.),
        )
        .contains(position)
}

#[derive(Default)]
pub(super) struct PanelScroll {
    finger: Option<(u64, f32)>,
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn scroll_panels(
    touch: Option<Res<crate::touch::TouchControls>>,
    map: Option<Res<crate::world_map::WorldMap>>,
    session: Res<Session>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    touches: Res<Touches>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut drag: Local<PanelScroll>,
    mut panels: Query<
        (&Node, &ComputedNode, &mut ScrollPosition),
        Or<(With<NpcPanel>, With<HelpPanel>)>,
    >,
) {
    if map.is_some_and(|map| map.open || map.input_blocked)
        || !touch.is_some_and(|touch| touch.enabled && !touch.menu_open)
    {
        drag.finger = None;
        return;
    }
    let size = Vec2::new(window.width(), window.height());
    if drag.finger.is_none()
        && let Some(finger) = touches
            .iter_just_pressed()
            .find(|finger| touch_panel_at(finger.position(), size, &session))
    {
        drag.finger = Some((finger.id(), finger.position().y));
    }
    let mut delta = 0.;
    if let Some((id, previous)) = drag.finger {
        if let Some(finger) = touches.get_pressed(id) {
            delta = previous - finger.position().y;
            drag.finger = Some((id, finger.position().y));
        } else {
            drag.finger = None;
        }
    }
    if window
        .cursor_position()
        .is_some_and(|position| touch_panel_at(position, size, &session))
    {
        delta -= wheel.delta.y * 28.;
    }
    for (node, computed, mut scroll) in &mut panels {
        if node.display != Display::None {
            let max = ((computed.content_size.y - computed.size.y) * computed.inverse_scale_factor)
                .max(0.);
            scroll.0.y = (scroll.0.y + delta).clamp(0., max);
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_ui(
    session: Res<Session>,
    world: Res<crate::VoxelWorld>,
    time: Res<Time>,
    touch: Option<Res<crate::touch::TouchControls>>,
    pause: Option<Res<crate::pause::PauseMenu>>,
    console: Option<Res<crate::admin_console::AdminConsole>>,
    map: Option<Res<crate::world_map::WorldMap>>,
    conversation: Option<Res<crate::airships::PilotConversation>>,
    market: Option<Res<crate::market::MarketPanel>>,
    mut refresh: Local<f32>,
    mut texts: ParamSet<(
        Query<&mut Text, With<StatusText>>,
        Query<&mut Text, With<NpcText>>,
        Query<&mut Text, With<NoticeText>>,
        Query<&mut Text, With<ModeText>>,
    )>,
    mut panels: Query<(
        &mut Node,
        Option<&HelpPanel>,
        Option<&NpcPanel>,
        Option<&NoticePanel>,
    )>,
    mut slots: Query<(&PaletteSlot, &mut BorderColor, &mut BackgroundColor)>,
) {
    *refresh += time.delta_secs();
    if *refresh < 0.1 {
        return;
    }
    *refresh = 0.0;
    let touch_enabled = touch.as_ref().is_some_and(|touch| touch.enabled);
    let menu_open = pause.as_ref().is_some_and(|pause| pause.open)
        || console.as_ref().is_some_and(|console| console.open)
        || map.as_ref().is_some_and(|map| map.open)
        || market.as_ref().is_some_and(|market| market.open)
        || conversation
            .as_ref()
            .is_some_and(|conversation| conversation.open());
    let minutes = (session.world_time / 60.0) as u64;
    for mut text in &mut texts.p0() {
        set_text(
            &mut text,
            if touch_enabled {
                format!(
                    "{} explorer{} · {:02}:{:02} · {:.0} fps · {}",
                    session.players.len(),
                    if session.players.len() == 1 { "" } else { "s" },
                    minutes / 60,
                    minutes % 60,
                    session.fps,
                    session.graphics.label()
                )
            } else {
                format!(
                    "{} explorer{}  ·  world {:02}:{:02}\n{:.0} fps  ·  {}  [F2]\n{} saved terrain changes",
                    session.players.len(),
                    if session.players.len() == 1 { "" } else { "s" },
                    minutes / 60,
                    minutes % 60,
                    session.fps,
                    session.graphics.label(),
                    session.edits
                )
            },
        );
    }
    for mut text in &mut texts.p1() {
        let value = crate::inspection_details::text(&world.0, &session);
        set_text(&mut text, value);
    }
    let mut notice = notice_text(&session, &world, time.elapsed_secs_f64(), touch_enabled);
    if let Some(market) = &market {
        let cargo = crate::market::hud_text(market, &session, &world, touch_enabled);
        if !cargo.is_empty() {
            if !notice.is_empty() {
                notice.push('\n');
            }
            notice.push_str(&cargo);
        }
    }
    for mut text in &mut texts.p2() {
        set_text(&mut text, notice.clone());
    }
    for mut text in &mut texts.p3() {
        let value = if let Some(observer) = &session.observer {
            format!(
                "OBSERVER  /  READ ONLY\n{:.0} m/s  ·  Shift {:.0} m/s",
                observer.speed,
                observer.speed(true)
            )
        } else {
            format!(
                "CREATIVE  /  {}",
                if session.flying {
                    "FLIGHT ENABLED"
                } else {
                    "UNLIMITED MATERIALS"
                }
            )
        };
        set_text(&mut text, value);
    }
    for (mut node, help, npc, notice_panel) in &mut panels {
        if notice_panel.is_some() {
            node.display = if !notice.is_empty() && !menu_open {
                Display::Flex
            } else {
                Display::None
            };
        }
        if help.is_some() {
            node.display = if session.help && !menu_open {
                Display::Flex
            } else {
                Display::None
            };
        }
        if npc.is_some() {
            node.display = if session.inspector && !menu_open && (!touch_enabled || !session.help) {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    for (slot, mut border, mut background) in &mut slots {
        let active = slot.0 == session.selected;
        *border = BorderColor::all(if active {
            Color::srgb(0.95, 0.75, 0.35)
        } else {
            Color::srgba(0.6, 0.7, 0.6, 0.2)
        });
        *background = BackgroundColor(if active {
            Color::srgba(0.16, 0.21, 0.16, 0.96)
        } else {
            panel()
        });
    }
}

fn notice_text(session: &Session, world: &crate::VoxelWorld, now: f64, touch: bool) -> String {
    if now < session.status_until {
        return session.status.clone();
    }
    if session.ride.is_some() {
        return crate::airships::travel_hint(session, world, touch).unwrap_or_default();
    }
    if touch {
        if session.observer.is_some() {
            "Observer · swipe to look · Menu for travel and servers".into()
        } else if session.target.is_none() {
            String::new()
        } else {
            format!(
                "{} · {}",
                PALETTE[session.selected].name(),
                if session.flying {
                    "creative flight"
                } else {
                    "Dig / Build changes the aimed block"
                }
            )
        }
    } else if !session.captured {
        if session.observer.is_some() {
            "Click to fly the camera  ·  read-only observation".into()
        } else {
            "Click to explore  ·  changes are saved automatically".into()
        }
    } else if let Some(observer) = &session.observer {
        format!(
            "Camera  {:.0}, {:.0}, {:.0} m  ·  R or Home returns to spawn",
            observer.position.x, observer.position.y, observer.position.z
        )
    } else if session.target.is_none() {
        String::new()
    } else {
        format!(
            "{}  ·  0.5 m blocks  ·  hold Ctrl + click to repeat",
            PALETTE[session.selected].name()
        )
    }
}

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VoxelWorld, graphics::GraphicsQuality, join::session_from_welcome};
    use rubblekin_core::{
        airships::AirshipRide,
        protocol::{ServerMessage, SessionMode},
        world::{BlockPos, WorldGeneration},
    };

    fn fixture(generation: WorldGeneration) -> (VoxelWorld, Session) {
        let mut welcome = crate::join::tests::welcome(SessionMode::Player);
        if let ServerMessage::Welcome {
            generation: actual, ..
        } = &mut welcome
        {
            *actual = generation;
        }
        let (world, mut session) = session_from_welcome(
            welcome,
            "test".into(),
            GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.status_until = 0.;
        session.captured = true;
        (VoxelWorld(world), session)
    }

    #[test]
    fn idle_no_target_has_no_build_warning_but_ground_targets_keep_direct_hints() {
        let (world, mut session) = fixture(WorldGeneration::ValleyV1);
        for touch in [false, true] {
            assert!(notice_text(&session, &world, 10., touch).is_empty());
            session.target = Some((BlockPos::new(0, 10, 0), BlockPos::new(0, 11, 0)));
            let text = notice_text(&session, &world, 10., touch);
            assert!(text.contains(PALETTE[session.selected].name()));
            assert!(text.contains(if touch { "Dig / Build" } else { "Ctrl + click" }));
            session.target = None;
        }
        session.status = "Move closer to reach a block".into();
        session.status_until = 12.;
        assert!(notice_text(&session, &world, 10., false).contains("Move closer"));
        assert!(notice_text(&session, &world, 13., false).is_empty());
        session.captured = false;
        assert!(notice_text(&session, &world, 13., false).contains("Click to explore"));
    }

    #[test]
    fn riding_notice_uses_actual_destination_and_live_departure_or_arrival() {
        let (world, mut session) = fixture(WorldGeneration::GeographyV3);
        let ship = session
            .airships
            .ships(0.)
            .into_iter()
            .find(|ship| ship.docked_at.is_some())
            .unwrap();
        session.ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: 0,
        });
        let destination = world
            .0
            .settlements()
            .unwrap()
            .villages
            .iter()
            .find(|village| village.id == ship.next_village)
            .unwrap()
            .name
            .clone();
        for touch in [false, true] {
            session.airship_clock.time = 0.;
            let docked = notice_text(&session, &world, 20., touch);
            assert!(docked.contains(&destination), "{docked}");
            assert!(docked.contains("departs"), "{docked}");
            session.airship_clock.time = f64::from(ship.departure_in) + 1.;
            let underway = notice_text(&session, &world, 20., touch);
            assert!(underway.contains(&destination), "{underway}");
            assert!(underway.contains("arrives"), "{underway}");
            assert!(!underway.contains("block"), "{underway}");
        }
    }

    #[test]
    fn missing_target_warns_only_on_ground_edit_attempt_and_never_on_modal_close() {
        use crate::network::Connection;
        use bevy::gizmos::{AppGizmoBuilder, config::DefaultGizmoConfigGroup};
        use rubblekin_core::{protocol::ClientMessage, world::Block};
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            time::Duration,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let client = std::thread::spawn(move || {
            Connection::connect(&address, "Tester".into(), SessionMode::Player).unwrap()
        });
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut peer = BufReader::new(socket);
        peer.read_line(&mut String::new()).unwrap();
        serde_json::to_writer(
            peer.get_mut(),
            &crate::join::tests::welcome(SessionMode::Player),
        )
        .unwrap();
        peer.get_mut().write_all(b"\n").unwrap();
        let (connection, _) = client.join().unwrap();
        let (world, mut session) = fixture(WorldGeneration::ValleyV1);
        session.body.position = [0.25, 76., 0.25];
        session.status.clear();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(10));
        let mut app = App::new();
        app.insert_resource(world)
            .insert_resource(session)
            .insert_resource(connection)
            .insert_resource(time)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<crate::touch::TouchControls>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::airships::PilotConversation>()
            .init_resource::<crate::market::MarketPanel>()
            .init_gizmo_group::<DefaultGizmoConfigGroup>()
            .add_systems(Update, crate::edit_blocks);
        app.world_mut().spawn((
            crate::GameCamera,
            Transform::from_xyz(0.25, 77.25, 0.25).looking_to(Vec3::NEG_Z, Vec3::Y),
        ));
        app.world_mut().run_schedule(Update);
        assert!(app.world().resource::<Session>().status.is_empty());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.world_mut().run_schedule(Update);
        assert!(
            app.world()
                .resource::<Session>()
                .status
                .contains("Move closer")
        );
        assert_eq!(app.world().resource::<Session>().status_until, 13.);
        assert_eq!(app.world().resource::<Session>().next_request, 1);
        for modal in 0..7 {
            *app.world_mut().resource_mut::<crate::pause::PauseMenu>() = default();
            *app.world_mut().resource_mut::<crate::world_map::WorldMap>() = default();
            *app.world_mut()
                .resource_mut::<crate::admin_console::AdminConsole>() = default();
            *app.world_mut()
                .resource_mut::<crate::airships::PilotConversation>() = default();
            app.world_mut()
                .resource_mut::<crate::market::MarketPanel>()
                .clear();
            {
                let mut session = app.world_mut().resource_mut::<Session>();
                session.status = "existing notice".into();
                session.edit_clock = 0.;
                session.ride = None;
            }
            match modal {
                0 => {
                    app.world_mut()
                        .resource_mut::<crate::pause::PauseMenu>()
                        .input_blocked = true
                }
                1 => {
                    app.world_mut()
                        .resource_mut::<crate::world_map::WorldMap>()
                        .input_blocked = true
                }
                2 => {
                    app.world_mut()
                        .resource_mut::<crate::admin_console::AdminConsole>()
                        .input_blocked = true
                }
                3 => {
                    app.world_mut()
                        .resource_mut::<crate::airships::PilotConversation>()
                        .input_blocked = true
                }
                4 => {
                    app.world_mut().resource_mut::<Session>().ride = Some(AirshipRide {
                        ship_id: 1,
                        seat: 0,
                    })
                }
                5 => {
                    app.world_mut()
                        .resource_mut::<crate::market::MarketPanel>()
                        .open = true
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<crate::market::MarketPanel>()
                        .input_blocked = true
                }
            }
            app.world_mut().run_schedule(Update);
            assert_eq!(
                app.world().resource::<Session>().status,
                "existing notice",
                "modal {modal}"
            );
            assert_eq!(app.world().resource::<Session>().next_request, 1);
        }
        app.world_mut().resource_mut::<Session>().ride = None;
        app.world_mut()
            .resource_mut::<crate::market::MarketPanel>()
            .clear();
        let block = BlockPos::new(0, 154, -8);
        app.world_mut()
            .resource_mut::<VoxelWorld>()
            .0
            .set_block(block, Block::Wood)
            .unwrap();
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Session>().next_request, 2);
        let mut line = String::new();
        peer.read_line(&mut line).unwrap();
        assert!(
            matches!(
                serde_json::from_str::<ClientMessage>(&line).unwrap(),
                ClientMessage::Edit { request_id: 1, .. }
            ),
            "{line}"
        );
        app.world_mut()
            .resource_mut::<Connection>()
            .send(ClientMessage::Ping);
        line.clear();
        peer.read_line(&mut line).unwrap();
        assert!(
            matches!(
                serde_json::from_str::<ClientMessage>(&line).unwrap(),
                ClientMessage::Ping
            ),
            "{line}"
        );
    }
}
