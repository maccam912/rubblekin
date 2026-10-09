//! Creative catalog: choose a block, then assign it to one of six hotbar slots.
use crate::{GameEntity, Session, block_textures::BlockIcons, join::MenuKey, palette::QUICK_SLOTS};
use bevy::{
    input::{
        keyboard::Key,
        touch::{TouchInput, TouchPhase},
    },
    picking::hover::Hovered,
    prelude::*,
    ui_widgets::{ActivateOnPress, Button},
    window::PrimaryWindow,
};
use rubblekin_core::{blocks::BlockCategory, world::Block};
#[derive(Default)]
pub struct Inventory {
    pub open: bool,
    pub input_blocked: bool,
    pub just_closed: bool,
    category: Option<BlockCategory>,
    search: String,
    searching: bool,
    page: usize,
    pending: Option<Block>,
    finger: Option<(u64, Action, Vec2)>,
}
impl Inventory {
    fn matches(&self) -> Vec<Block> {
        let search = self.search.to_lowercase();
        Block::ALL
            .iter()
            .copied()
            .filter(|b| {
                self.category.is_none_or(|c| b.category() == c)
                    && b.name().to_lowercase().contains(&search)
            })
            .collect()
    }
    fn choose(
        &mut self,
        action: Action,
        hotbar: &mut [Block; QUICK_SLOTS],
        selected: &mut usize,
        size: usize,
    ) -> bool {
        self.searching = action == Action::Search;
        match action {
            Action::Close => self.open = false,
            Action::Category(c) => {
                self.category = c;
                self.page = 0;
            }
            Action::Previous => self.page = self.page.saturating_sub(1),
            Action::Next => {
                self.page = (self.page + 1).min(self.matches().len().saturating_sub(1) / size)
            }
            Action::Entry(index) => {
                self.pending = self.matches().get(self.page * size + index).copied()
            }
            Action::Slot(index) if index < QUICK_SLOTS => {
                *selected = index;
                if let Some(block) = self.pending {
                    hotbar[index] = block;
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}
#[derive(Component)]
pub(super) struct Root;
#[derive(Component)]
pub(super) struct Grid;
#[derive(Component)]
pub(super) struct Heading;
#[derive(Component)]
pub(super) struct SearchText;
#[derive(Component)]
pub(super) struct PageText;
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) enum Action {
    Close,
    Category(Option<BlockCategory>),
    Search,
    Previous,
    Next,
    Entry(usize),
    Slot(usize),
}
#[derive(Component)]
pub(super) struct CardImage;
#[derive(Component)]
pub(super) struct CardLabel;
fn label(value: impl Into<String>, font: &Handle<Font>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont::from_font_size(size).with_font(font.clone()),
        TextColor(Color::srgb(0.91, 0.94, 0.88)),
    )
}
fn button(action: Action) -> impl Bundle {
    (
        (Button, ActivateOnPress, Hovered::default()),
        action,
        Node {
            padding: UiRect::axes(px(10), px(6)),
            min_height: px(32),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.13, 0.24, 0.22)),
        BorderColor::all(Color::NONE),
    )
}
pub fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            Root,
            GlobalZIndex(110),
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.035, 0.035, 0.80)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(96),
                    max_width: px(920),
                    height: percent(94),
                    padding: UiRect::all(px(12)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.055, 0.10, 0.10)),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((label("CREATIVE INVENTORY", &font, 22.), Heading));
                        row.spawn(button(Action::Close)).with_child(label(
                            "Done · I / Esc",
                            &font,
                            16.,
                        ));
                    });
                panel
                    .spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(4),
                        row_gap: px(4),
                        ..default()
                    })
                    .with_children(|row| {
                        for category in
                            std::iter::once(None).chain(BlockCategory::ALL.into_iter().map(Some))
                        {
                            row.spawn(button(Action::Category(category)))
                                .with_child(label(
                                    category.map_or("All", BlockCategory::name),
                                    &font,
                                    14.,
                                ));
                        }
                    });
                if !cfg!(target_os = "android") {
                    panel
                        .spawn(button(Action::Search))
                        .with_child((label("Search blocks…", &font, 15.), SearchText));
                }
                panel
                    .spawn((
                        Node {
                            flex_grow: 1.,
                            min_height: px(0),
                            overflow: Overflow::clip(),
                            flex_wrap: FlexWrap::Wrap,
                            align_content: AlignContent::FlexStart,
                            ..default()
                        },
                        Grid,
                    ))
                    .with_children(|grid| {
                        for index in 0..24 {
                            grid.spawn(button(Action::Entry(index)))
                                .insert(Node {
                                    width: percent(16.666),
                                    height: percent(25),
                                    padding: UiRect::all(px(3)),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(2)),
                                    ..default()
                                })
                                .with_children(|card| {
                                    card.spawn((
                                        ImageNode::default(),
                                        Node {
                                            width: px(38),
                                            height: px(38),
                                            ..default()
                                        },
                                        CardImage,
                                    ));
                                    card.spawn((label("", &font, 13.), CardLabel));
                                });
                        }
                    });
                panel
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(button(Action::Previous)).with_child(label(
                            "‹ Previous",
                            &font,
                            14.,
                        ));
                        row.spawn((label("", &font, 14.), PageText));
                        row.spawn(button(Action::Next))
                            .with_child(label("Next ›", &font, 14.));
                    });
                panel
                    .spawn(Node {
                        column_gap: px(4),
                        ..default()
                    })
                    .with_children(|row| {
                        for index in 0..QUICK_SLOTS {
                            row.spawn(button(Action::Slot(index)))
                                .insert(Node {
                                    width: percent(16.666),
                                    height: px(58),
                                    padding: UiRect::all(px(2)),
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(2)),
                                    ..default()
                                })
                                .with_children(|card| {
                                    card.spawn((
                                        ImageNode::default(),
                                        Node {
                                            width: px(28),
                                            height: px(28),
                                            ..default()
                                        },
                                        CardImage,
                                    ));
                                    card.spawn((label("", &font, 12.), CardLabel));
                                });
                        }
                    });
            });
        });
}
fn page_size(window: &Window) -> usize {
    if window.height() < 550. { 12 } else { 24 }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn read(
    mut session: ResMut<Session>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut native: MessageReader<MenuKey>,
    mut fingers: MessageReader<TouchInput>,
    actions: Query<&Action, Changed<crate::ui::Activated>>,
    targets: Query<(&Action, &ComputedNode, &UiGlobalTransform, &Node)>,
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::world_map::WorldMap>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::airships::PilotConversation>,
        Res<crate::market::MarketPanel>,
    ),
    mut touch: ResMut<crate::touch::TouchControls>,
) {
    let (pause, map, console, pilot, market) = modals;
    let other = pause.open
        || pause.input_blocked
        || map.open
        || map.input_blocked
        || console.input_blocked
        || pilot.open()
        || pilot.input_blocked
        || market.open
        || market.input_blocked;
    let events: Vec<_> = native.read().collect();
    let s = &mut *session;
    let inv = &mut s.inventory;
    let was_open = inv.open;
    inv.just_closed = false;
    inv.input_blocked = was_open;
    if !window.focused || s.observer.is_some() || other {
        inv.finger = None;
        fingers.clear();
        return;
    }
    let back = events
        .iter()
        .any(|e| e.input.state.is_pressed() && e.input.logical_key == Key::Escape);
    if (!inv.searching && (keys.just_pressed(KeyCode::KeyI) || keys.just_pressed(KeyCode::KeyC)))
        || touch.palette_page
        || (was_open && (keys.just_pressed(KeyCode::Escape) || back))
    {
        inv.open = !inv.open;
        inv.searching = false;
    }
    inv.input_blocked = was_open || inv.open;
    if was_open != inv.open {
        touch.reset();
        inv.finger = None;
        fingers.clear();
    }
    inv.just_closed = was_open && !inv.open;
    if !was_open || !inv.open {
        fingers.clear();
        return;
    }
    if inv.searching {
        for event in events.iter().filter(|e| e.input.state.is_pressed()) {
            match &event.input.logical_key {
                Key::Backspace => {
                    inv.search.pop();
                }
                Key::Enter => inv.searching = false,
                Key::Character(value)
                    if !event.modifiers.control_key() && !event.modifiers.super_key() =>
                {
                    for c in value.chars().filter(|c| !c.is_control()) {
                        if inv.search.chars().count() < 48 {
                            inv.search.push(c);
                        }
                    }
                }
                _ => {}
            }
        }
        inv.page = 0;
    }
    inv.page = inv
        .page
        .min(inv.matches().len().saturating_sub(1) / page_size(&window));
    let mut chosen = None;
    let mut native_touch = inv.finger.is_some();
    for finger in fingers.read() {
        native_touch = true;
        let point = finger.position * window.scale_factor();
        let at = targets
            .iter()
            .find(|(_, node, transform, style)| {
                style.display != Display::None && node.contains_point(**transform, point)
            })
            .map(|(a, ..)| *a);
        match finger.phase {
            TouchPhase::Started if inv.finger.is_none() => {
                if let Some(a) = at {
                    inv.finger = Some((finger.id, a, point));
                }
            }
            TouchPhase::Ended => {
                if let Some((id, action, start)) = inv.finger
                    && id == finger.id
                {
                    if at == Some(action) && start.distance(point) < 24. {
                        chosen = Some(action);
                    }
                    inv.finger = None;
                }
            }
            TouchPhase::Canceled => inv.finger = None,
            _ => {}
        }
    }
    if !native_touch {
        chosen = actions.iter().next().copied();
    }
    if !inv.searching {
        for (i, key) in [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
        ]
        .iter()
        .enumerate()
        {
            if keys.just_pressed(*key) {
                chosen = Some(Action::Slot(i));
            }
        }
    }
    if let Some(action) = chosen {
        if inv.choose(action, &mut s.hotbar, &mut s.selected, page_size(&window))
            && let Err(error) = crate::palette::save(&s.hotbar)
        {
            warn!("Could not save creative hotbar: {error}");
            s.status = "Hotbar changed; could not save preferences".into();
        }
        inv.just_closed = was_open && !inv.open;
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn refresh(
    session: Res<Session>,
    window: Single<&Window, With<PrimaryWindow>>,
    icons: Res<BlockIcons>,
    mut root: Query<&mut Node, (With<Root>, Without<Action>)>,
    mut cards: Query<(&Action, &mut Node, &mut BorderColor, &Children), Without<Root>>,
    mut images: Query<&mut ImageNode, With<CardImage>>,
    mut text: ParamSet<(
        Query<&mut Text, With<CardLabel>>,
        Query<&mut Text, With<Heading>>,
        Query<&mut Text, With<SearchText>>,
        Query<&mut Text, With<PageText>>,
    )>,
) {
    let inv = &session.inventory;
    root.single_mut().unwrap().display = if inv.open {
        Display::Flex
    } else {
        Display::None
    };
    if !inv.open {
        return;
    }
    let entries = inv.matches();
    let size = page_size(&window);
    let pages = entries.len().div_ceil(size).max(1);
    let page = inv.page.min(pages - 1);
    for mut label in &mut text.p1() {
        label.0 = inv.pending.map_or_else(
            || format!("{} blocks · pick a block, then a slot", Block::ALL.len()),
            |b| format!("{} · choose a hotbar slot", b.name()),
        );
    }
    for mut label in &mut text.p2() {
        label.0 = format!(
            "{}{}",
            if inv.searching {
                "Search (typing): "
            } else {
                "Search: "
            },
            if inv.search.is_empty() {
                "click here to type"
            } else {
                &inv.search
            }
        );
    }
    for mut label in &mut text.p3() {
        label.0 = format!("{} matches · page {} / {}", entries.len(), page + 1, pages);
    }
    for (action, mut node, mut border, children) in &mut cards {
        let (block, active) = match *action {
            Action::Entry(i) => {
                let block = if i < size {
                    entries.get(page * size + i).copied()
                } else {
                    None
                };
                node.display = if block.is_some() {
                    Display::Flex
                } else {
                    Display::None
                };
                node.height = percent(if size == 12 { 50. } else { 25. });
                (block, block.is_some() && block == inv.pending)
            }
            Action::Slot(i) => (Some(session.hotbar[i]), session.selected == i),
            Action::Category(c) => (None, inv.category == c),
            _ => (None, false),
        };
        *border = BorderColor::all(if active {
            Color::srgb(0.95, 0.75, 0.35)
        } else {
            Color::NONE
        });
        if let Some(block) = block {
            for child in children {
                if let Ok(mut img) = images.get_mut(*child) {
                    img.image = icons.0[block.catalog_index().unwrap()].clone();
                }
                if let Ok(mut label) = text.p0().get_mut(*child) {
                    label.0 = if let Action::Slot(i) = action {
                        format!("{} {}", i + 1, block.name())
                    } else {
                        block.name().into()
                    };
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_search_and_assignment_reach_the_whole_catalog() {
        let mut inv = Inventory::default();
        let mut hotbar = crate::palette::DEFAULT_HOTBAR;
        let mut selected = 0;
        for &block in Block::ALL {
            inv.category = Some(block.category());
            inv.search = block.name().into();
            inv.page = 0;
            let index = inv.matches().iter().position(|b| *b == block).unwrap();
            inv.choose(Action::Entry(index), &mut hotbar, &mut selected, 24);
            assert!(inv.choose(Action::Slot(5), &mut hotbar, &mut selected, 24));
            assert_eq!(hotbar[5], block);
            assert_eq!(selected, 5);
        }
        inv.search = "no matching blocks".into();
        inv.choose(Action::Entry(0), &mut hotbar, &mut selected, 24);
        assert_eq!(inv.pending, None);
    }
    #[test]
    fn rendered_inventory_opens_and_closes_without_leaking_the_closing_frame() {
        let (_, session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "inventory test".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<BlockIcons>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::airships::PilotConversation>()
            .init_resource::<crate::market::MarketPanel>()
            .init_resource::<crate::touch::TouchControls>()
            .add_message::<MenuKey>()
            .add_message::<TouchInput>()
            .add_systems(Startup, setup)
            .add_systems(Update, (read, refresh).chain());
        app.world_mut().spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
        ));
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        app.update();
        assert!(app.world().resource::<Session>().inventory.open);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let inv = &app.world().resource::<Session>().inventory;
        assert!(!inv.open && inv.input_blocked && inv.just_closed);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(!app.world().resource::<Session>().inventory.input_blocked);
        app.world_mut()
            .resource_mut::<crate::world_map::WorldMap>()
            .open = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        app.update();
        assert!(!app.world().resource::<Session>().inventory.open);
    }
}
