use crate::{GameEntity, Session, touch::TouchControls};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

#[derive(Component)]
pub(crate) struct Compass;
#[derive(Component)]
pub(crate) struct BuildHint;
pub(crate) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let heading = crate::ui::heading_font(&mut fonts);
    commands.spawn((
        GameEntity,
        Compass,
        Text::new("N"),
        TextFont::from_font_size(15.).with_font(heading),
        TextColor(Color::srgb(0.97, 0.86, 0.57)),
        TextLayout::no_wrap(),
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: percent(35),
            max_width: percent(30),
            overflow: Overflow::clip(),
            padding: UiRect::all(px(6)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        GlobalZIndex(6),
        BackgroundColor(Color::srgba(0.03, 0.09, 0.08, 0.72)),
    ));
    commands.spawn((
        GameEntity,
        BuildHint,
        Text::new(""),
        TextFont::from_font_size(14.).with_font(font),
        TextColor(Color::srgb(0.97, 0.86, 0.57)),
        Node {
            position_type: PositionType::Absolute,
            top: px(96),
            left: percent(25),
            width: percent(50),
            padding: UiRect::all(px(6)),
            border_radius: BorderRadius::all(px(8)),
            display: Display::None,
            ..default()
        },
        GlobalZIndex(6),
        BackgroundColor(Color::srgba(0.03, 0.09, 0.08, 0.85)),
    ));
}
pub(crate) fn heading(yaw: f32) -> (&'static str, u32) {
    let degrees = yaw.to_degrees().rem_euclid(360.);
    let labels = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    (
        labels[((degrees + 22.5) / 45.).floor() as usize % 8],
        degrees.round() as u32 % 360,
    )
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    session: Res<Session>,
    touch: Res<TouchControls>,
    pause: Res<crate::pause::PauseMenu>,
    map: Res<crate::world_map::WorldMap>,
    console: Res<crate::admin_console::AdminConsole>,
    market: Res<crate::market::MarketPanel>,
    dialog: Res<crate::airships::PilotConversation>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut texts: Query<(&mut Text, &mut Node, Has<BuildHint>), Or<(With<Compass>, With<BuildHint>)>>,
) {
    let visible = !pause.open
        && !map.open
        && !console.input_blocked
        && !market.open
        && !dialog.open()
        && !session.inventory.input_blocked;
    let (direction, degrees) = heading(session.yaw);
    let touch_width = window.as_ref().map(|window| {
        let scale = (window.width() / 840.)
            .min(window.height() / 400.)
            .clamp(0.5, 1.);
        // Leave room for the first touch action, including on compact windows.
        (window.width() * 0.65 - 328. * scale - 12.).max(48.)
    });
    let name_limit = if touch.enabled {
        touch_width.map_or(8, |width| ((width - 84.) / 14.).clamp(3., 12.) as usize)
    } else {
        20
    };
    let here = session
        .observer
        .as_ref()
        .map_or(session.body.position, |o| o.position.to_array());
    let mut others: Vec<_> = session
        .players
        .iter()
        .filter(|p| p.id != session.id)
        .collect();
    others.sort_by(|a, b| {
        rubblekin_core::gliders::horizontal_distance(here, a.body.position).total_cmp(
            &rubblekin_core::gliders::horizontal_distance(here, b.body.position),
        )
    });
    let mut value = format!("{direction}  {degrees}°");
    for p in others.into_iter().take(if touch.enabled { 1 } else { 2 }) {
        let bearing = (p.body.position[0] - here[0]).atan2(-(p.body.position[2] - here[2]));
        let relative = (bearing - session.yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let arrow = if relative.abs() < 0.55 {
            "^"
        } else if relative.abs() > 2.5 {
            "v"
        } else if relative > 0. {
            ">"
        } else {
            "<"
        };
        let d = rubblekin_core::gliders::horizontal_distance(here, p.body.position);
        let distance = if d >= 1000. {
            format!("{:.1} km", d / 1000.)
        } else {
            format!("{d:.0} m")
        };
        let name = if p.name.chars().count() > name_limit {
            format!(
                "{}…",
                p.name.chars().take(name_limit - 1).collect::<String>()
            )
        } else {
            p.name.clone()
        };
        value.push_str(&format!("\n{arrow} {distance}  {name}"));
    }
    for (mut text, mut node, building) in &mut texts {
        if building {
            node.top = px(if touch.enabled { 90. } else { 150. });
            node.left = percent(32.);
            node.width = percent(45.);
        } else {
            node.max_width = if touch.enabled {
                touch_width.map_or(percent(30), px)
            } else {
                percent(30)
            };
        }
        let value = if building {
            session
                .building
                .hint(session.hotbar[session.selected], touch.enabled)
        } else {
            value.clone()
        };
        node.display = if visible && !value.is_empty() {
            Display::Flex
        } else {
            Display::None
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compass_matches_north_up_map_and_wraps() {
        assert_eq!(heading(0.), ("N", 0));
        assert_eq!(heading(std::f32::consts::FRAC_PI_2), ("E", 90));
        assert_eq!(heading(-std::f32::consts::FRAC_PI_2), ("W", 270));
    }

    #[test]
    fn nearest_player_cues_keep_distance_ahead_of_shortened_names_and_restore_after_menus() {
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "test".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        session.body.position = [0., 20., 0.];
        session.yaw = std::f32::consts::FRAC_PI_2;
        for (id, name, position) in [
            (31, "AlexandraTheExplorer", [20., 20., 0.]),
            (32, "山谷を歩く友だち", [0., 20., 2000.]),
            (33, "Faraway", [0., 20., 3000.]),
        ] {
            let mut player = session.players[0].clone();
            player.id = id;
            player.name = name.into();
            player.body.position = position;
            session.players.push(player);
        }
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<TouchControls>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::market::MarketPanel>()
            .init_resource::<crate::airships::PilotConversation>()
            .add_systems(Update, update);
        app.world_mut().resource_mut::<TouchControls>().enabled = true;
        app.world_mut().spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(640, 360),
                ..default()
            },
            PrimaryWindow,
        ));
        let compass = app
            .world_mut()
            .spawn((Compass, Text::default(), Node::default()))
            .id();
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().get::<Text>(compass).unwrap().0,
            "E  90°\n^ 20 m  Alex…"
        );
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = true;
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().get::<Node>(compass).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = false;
        app.world_mut().resource_mut::<TouchControls>().enabled = false;
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().get::<Text>(compass).unwrap().0,
            "E  90°\n^ 20 m  AlexandraTheExplorer\n> 2.0 km  山谷を歩く友だち"
        );
        assert_eq!(
            app.world().get::<Node>(compass).unwrap().display,
            Display::Flex
        );
    }
}
