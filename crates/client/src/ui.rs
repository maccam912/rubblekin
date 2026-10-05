use crate::{PALETTE, Session};
use bevy::prelude::*;

#[derive(Component)]
pub struct StatusText;
#[derive(Component)]
pub struct NpcText;
#[derive(Component)]
pub struct NoticeText;
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
                Text::new("A LIFE OF THEIR OWN"),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(Color::srgb(0.90, 0.73, 0.42))
            ),
            (
                Text::new("Meeting the forager…"),
                TextFont::from_font_size(18.0).with_font(font.clone()),
                TextColor(ink()),
                NpcText
            ),
            (
                Text::new("Tab  hide inspector"),
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
            (Text::new(if observing {
                "W A S D   fly     •     mouse / arrows   look\nQ / E   descend / ascend     •     Shift   5× speed\nScroll   adjust speed     •     R / Home   return to spawn\nTab   inspect village / forager     •     F2   graphics\nV   visit next village\nEsc   pause menu     •     H   hide controls\nF10   leave world / choose another server\nRead-only camera · no avatar or editing"
            } else {
                "W A S D   move     •     mouse / arrows   look\nSpace   jump     •     Shift   sprint\nLeft click   dig     •     Right click   build\n1–6   materials     •     F   creative flight\nQ / E   descend / ascend     •     scroll   zoom\nEsc   pause menu     •     H   hide controls\nF10   leave world / choose another server"
            }), TextFont::from_font_size(16.0).with_font(font.clone()), TextColor(ink())),
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
                        "Left stick: fly · swipe the world: look\nRise / Fall: vertical flight · Sprint: boost\n+ / −: camera speed\nMenu: graphics, return to spawn, next village, servers\nInspect: village and resident details · swipe panel to scroll\nRead-only observer: no avatar or editing"
                    } else {
                        "Left stick: move · swipe the world: look\nJump: hop · Sprint: run · Fly: creative flight\nRise / Fall: vertical flight · + / −: camera distance\nDig / Build: change the block under the center dot\nTap a material tile to choose a building block\nInspect: village and resident details · swipe panel to scroll\nMenu: graphics, controls, and leave world"
                    }),
                    TextFont::from_font_size(16.).with_font(font.clone()), TextColor(ink()),
                    Node { flex_shrink: 0., ..default() },
                ));
            } else {
                panel.spawn((Text::new("Meeting the residents…"), TextFont::from_font_size(16.).with_font(font.clone()), TextColor(ink()), NpcText, Node { flex_shrink: 0., ..default() }));
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
pub fn scroll_panels(
    touch: Option<Res<crate::touch::TouchControls>>,
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
    if !touch.is_some_and(|touch| touch.enabled && !touch.menu_open) {
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
    mut refresh: Local<f32>,
    mut texts: ParamSet<(
        Query<&mut Text, With<StatusText>>,
        Query<&mut Text, With<NpcText>>,
        Query<&mut Text, With<NoticeText>>,
        Query<&mut Text, With<ModeText>>,
    )>,
    mut panels: Query<(&mut Node, Option<&HelpPanel>, Option<&NpcPanel>)>,
    mut slots: Query<(&PaletteSlot, &mut BorderColor, &mut BackgroundColor)>,
) {
    *refresh += time.delta_secs();
    if *refresh < 0.1 {
        return;
    }
    *refresh = 0.0;
    let touch_enabled = touch.as_ref().is_some_and(|touch| touch.enabled);
    let menu_open = pause.as_ref().is_some_and(|pause| pause.open);
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
        let position = session
            .observer
            .as_ref()
            .map_or(session.body.position, |camera| camera.position.to_array());
        let village = world.0.settlements().and_then(|plan| {
            plan.villages.iter().min_by(|a, b| {
                let distance =
                    |p: [f32; 3]| (p[0] - position[0]).powi(2) + (p[2] - position[2]).powi(2);
                distance(a.center).total_cmp(&distance(b.center))
            })
        });
        let value = if let Some(village) =
            village.filter(|v| (v.center[0] - position[0]).hypot(v.center[2] - position[2]) < 300.0)
        {
            let stores = session.villages.iter().find(|v| v.id == village.id);
            let resident = session
                .residents
                .iter()
                .filter(|r| r.village_id == village.id)
                .min_by(|a, b| {
                    let d = |r: &rubblekin_core::protocol::ResidentSnapshot| {
                        (r.position[0] - position[0]).powi(2)
                            + (r.position[2] - position[2]).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                });
            let mut value = format!(
                "{}  ·  {:?}\n\nFreshwater  {:.0} m away\nLand  {:.0}% · Timber  {:.0}%\nStone {:.0}% · Clay {:.0}% · Iron {:.0}%",
                village.name,
                village.kind,
                village.freshwater_distance,
                village.resources.farming * 100.0,
                village.resources.timber * 100.0,
                village.resources.stone * 100.0,
                village.resources.clay * 100.0,
                village.resources.iron * 100.0
            );
            if let Some(s) = stores {
                value.push_str(&format!("\n\n{} residents · housing for {}\nFood {:.0} · Timber {:.0}\nStone {:.0} · Clay {:.0} · Iron {:.0}\nCrop growth {:.0}%", s.population, s.housing_capacity, s.food, s.timber, s.stone, s.clay, s.iron, s.crop_growth * 100.0));
            }
            if let Some(r) = resident {
                value.push_str(&format!(
                    "\n\n{} · {}\n{}\nHunger   {:3.0} / 100\nEnergy    {:3.0} / 100\n\n{}",
                    r.name,
                    r.role.label(),
                    r.action.label(),
                    r.hunger,
                    r.energy,
                    r.reason
                ));
            }
            value
        } else {
            format!(
                "{}  ·  {}{}\n\nHunger   {:3.0} / 100\nEnergy    {:3.0} / 100\nBerries gathered   {}\n\n{}{}",
                session.npc.name,
                session.npc.action.label(),
                if session.npc.forced {
                    " [override]"
                } else {
                    ""
                },
                session.npc.hunger,
                session.npc.energy,
                session.npc.berries,
                session.npc.reason,
                if session.can_admin {
                    "\n\nF6 forage · F7 rest · F8 autonomous\nF9 set needs · [ favor rest · ] reset weights"
                } else {
                    ""
                }
            )
        };
        set_text(&mut text, value);
    }
    for mut text in &mut texts.p2() {
        let value = if time.elapsed_secs_f64() < session.status_until {
            session.status.clone()
        } else if touch_enabled {
            if session.observer.is_some() {
                "Observer · swipe to look · Menu for travel and servers".into()
            } else if session.target.is_none() {
                "Aim near the center dot to reach a block".into()
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
            "Move closer to reach a block  ·  aim down to build nearby".into()
        } else {
            format!(
                "{}  ·  0.5 m blocks  ·  hold Ctrl + click to repeat",
                PALETTE[session.selected].name()
            )
        };
        set_text(&mut text, value);
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
    for (mut node, help, npc) in &mut panels {
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

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}
