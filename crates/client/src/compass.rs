use crate::{GameEntity, Session, touch::TouchControls};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Compass;
#[derive(Component)]
pub(crate) struct BuildHint;
pub(crate) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands.spawn((
        GameEntity,
        Compass,
        Text::new("N"),
        TextFont::from_font_size(15.).with_font(font.clone()),
        TextColor(Color::srgb(0.97, 0.86, 0.57)),
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: percent(35),
            width: percent(30),
            padding: UiRect::all(px(6)),
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
    mut texts: Query<(&mut Text, &mut Node, Has<BuildHint>), Or<(With<Compass>, With<BuildHint>)>>,
) {
    let visible = !pause.open
        && !map.open
        && !console.input_blocked
        && !market.open
        && !dialog.open()
        && !session.inventory.input_blocked;
    let (direction, degrees) = heading(session.yaw);
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
        value.push_str(&format!("\n{arrow} {}  {distance}", p.name));
    }
    for (mut text, mut node, building) in &mut texts {
        if building {
            node.top = px(if touch.enabled { 90. } else { 150. });
            node.left = percent(32.);
            node.width = percent(45.);
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
}
