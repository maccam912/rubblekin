//! A small, modal command console. The server owns permissions and command results.
use std::collections::VecDeque;

use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{keyboard::Key, mouse::AccumulatedMouseScroll},
    prelude::*,
    window::PrimaryWindow,
};
use rubblekin_core::{admin_commands::MAX_ADMIN_COMMAND_BYTES, protocol::ClientMessage};

use crate::{
    GameEntity, Session, airships::PilotConversation, join::MenuKey, network::Connection,
    pause::PauseMenu, touch::TouchControls,
};

const MAX_OUTPUT_LINES: usize = 80;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_HISTORY: usize = 32;

#[derive(Resource)]
pub(crate) struct AdminConsole {
    pub open: bool,
    /// Covers opening and closing frames as well as the open panel.
    pub input_blocked: bool,
    pub just_closed: bool,
    input: String,
    cursor: usize,
    output: VecDeque<String>,
    history: VecDeque<String>,
    history_index: Option<usize>,
    history_draft: String,
    pending_paste: Option<ClipboardRead>,
    follow_output: u8,
}

impl Default for AdminConsole {
    fn default() -> Self {
        let mut console = Self {
            open: false,
            input_blocked: false,
            just_closed: false,
            input: String::new(),
            cursor: 0,
            output: VecDeque::new(),
            history: VecDeque::new(),
            history_index: None,
            history_draft: String::new(),
            pending_paste: None,
            follow_output: 0,
        };
        console.reply("Type help to see all available commands.");
        console
    }
}

impl AdminConsole {
    pub fn reply(&mut self, text: impl AsRef<str>) {
        // Bound both replies and retained scrollback, including untrusted server text.
        let text = text.as_ref();
        let end = floor_char_boundary(text, MAX_OUTPUT_BYTES.min(text.len()));
        for line in text[..end].lines() {
            self.output
                .push_back(line.chars().filter(|ch| !ch.is_control()).collect());
        }
        while self.output.len() > MAX_OUTPUT_LINES
            || self.output.iter().map(String::len).sum::<usize>() > MAX_OUTPUT_BYTES
        {
            self.output.pop_front();
        }
        // UI measurement follows this system; give it two further layout frames.
        self.follow_output = 3;
    }

    fn insert(&mut self, text: &str) {
        // A multiline paste becomes one editable command, never several submissions.
        let text: String = text
            .chars()
            .filter_map(|ch| {
                if ch.is_control() {
                    ch.is_whitespace().then_some(' ')
                } else {
                    Some(ch)
                }
            })
            .collect();
        if self.input.len() + text.len() > MAX_ADMIN_COMMAND_BYTES {
            self.reply(format!(
                "Command is too long (maximum {MAX_ADMIN_COMMAND_BYTES} bytes)."
            ));
            return;
        }
        self.input.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.history_index = None;
    }

    fn previous_boundary(&self) -> usize {
        self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self) -> usize {
        self.input[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
    }

    fn history(&mut self, previous: bool) {
        if self.history.is_empty() {
            return;
        }
        if previous {
            self.history_index = Some(match self.history_index {
                Some(index) => index.saturating_sub(1),
                None => {
                    self.history_draft = self.input.clone();
                    self.history.len() - 1
                }
            });
        } else if let Some(index) = self.history_index {
            self.history_index = (index + 1 < self.history.len()).then_some(index + 1);
        } else {
            return;
        }
        self.input = self.history_index.map_or_else(
            || self.history_draft.clone(),
            |index| self.history[index].clone(),
        );
        self.cursor = self.input.len();
    }

    fn submit(&mut self) -> Option<String> {
        let command = self.input.trim().to_owned();
        if command.is_empty() {
            return None;
        }
        if self.history.back() != Some(&command) {
            self.history.push_back(command.clone());
            if self.history.len() > MAX_HISTORY {
                self.history.pop_front();
            }
        }
        self.reply(format!("> {command}"));
        self.input.clear();
        self.cursor = 0;
        self.history_index = None;
        self.history_draft.clear();
        Some(command)
    }

    fn close(&mut self) {
        self.open = false;
        self.just_closed = true;
        self.pending_paste = None;
    }

    fn poll_paste(&mut self) {
        if let Some(paste) = self
            .pending_paste
            .as_mut()
            .and_then(ClipboardRead::poll_result)
        {
            self.pending_paste = None;
            match paste {
                Ok(text) => self.insert(&text),
                Err(error) => self.reply(format!("Could not paste: {error}")),
            }
        }
    }
}

fn floor_char_boundary(text: &str, mut end: usize) -> usize {
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[derive(Component)]
pub(crate) struct ConsoleRoot;
#[derive(Component)]
pub(crate) struct ConsolePanel;
#[derive(Component)]
pub(crate) struct ConsoleOutput;
#[derive(Component)]
pub(crate) struct ConsoleScroll;
#[derive(Component)]
pub(crate) struct ConsoleInput;

pub(crate) fn setup(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut console: ResMut<AdminConsole>,
) {
    *console = AdminConsole::default();
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let ink = Color::srgb(0.89, 0.92, 0.85);
    commands
        .spawn((
            GameEntity,
            ConsoleRoot,
            GlobalZIndex(200),
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.04, 0.04, 0.65)),
        ))
        .with_children(|root| {
            root.spawn((
                ConsolePanel,
                Node {
                    width: px(1000),
                    max_width: percent(100),
                    height: percent(72),
                    max_height: px(560),
                    min_height: px(200),
                    padding: UiRect::all(px(16)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.055, 0.10, 0.10)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("ADMIN COMMANDS"),
                    TextFont::from_font_size(20.).with_font(font.clone()),
                    TextColor(Color::srgb(0.90, 0.73, 0.42)),
                ));
                panel
                    .spawn((
                        ConsoleScroll,
                        Node {
                            flex_grow: 1.,
                            min_height: px(0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    ))
                    .with_child((
                        ConsoleOutput,
                        Text::new(""),
                        TextFont::from_font_size(18.).with_font(font.clone()),
                        TextColor(ink),
                        Node {
                            flex_shrink: 0.,
                            width: percent(100),
                            ..default()
                        },
                    ));
                panel
                    .spawn((
                        Node {
                            min_height: px(44),
                            flex_shrink: 0.,
                            padding: UiRect::all(px(10)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.10, 0.18, 0.17)),
                        BorderColor::all(Color::srgb(0.40, 0.57, 0.48)),
                    ))
                    .with_child((
                        ConsoleInput,
                        Text::new("> |"),
                        TextFont::from_font_size(18.).with_font(font.clone()),
                        TextColor(ink),
                    ));
                panel.spawn((
                    Text::new(
                        "Enter  run · Up / Down  history · Ctrl/Cmd+V  paste · Esc / ~  close",
                    ),
                    TextFont::from_font_size(14.).with_font(font),
                    TextColor(Color::srgb(0.62, 0.74, 0.69)),
                ));
            });
        });
}

fn backquote(key: &MenuKey) -> bool {
    key.input.key_code == KeyCode::Backquote
        || matches!(&key.input.logical_key, Key::Character(value) if value == "`" || value == "~")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read(
    mut console: ResMut<AdminConsole>,
    session: Option<Res<Session>>,
    pause: Option<Res<PauseMenu>>,
    conversation: Option<Res<PilotConversation>>,
    map: Option<Res<crate::world_map::WorldMap>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<MenuKey>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut connection: Option<ResMut<Connection>>,
    mut touch: Option<ResMut<TouchControls>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    console.just_closed = false;
    let was_open = console.open;
    console.input_blocked = was_open;
    if !session.is_some_and(|session| session.can_admin && session.observer.is_none()) {
        if was_open {
            console.close();
        }
        keyboard.clear();
        return;
    }
    let events: Vec<_> = keyboard.read().collect();
    if windows.iter().all(|window| !window.focused) {
        return;
    }
    let toggle = keys.just_pressed(KeyCode::Backquote)
        || events
            .iter()
            .any(|key| key.input.state.is_pressed() && !key.input.repeat && backquote(key));
    let escape = keys.just_pressed(KeyCode::Escape)
        || events.iter().any(|key| {
            key.input.state.is_pressed()
                && !key.input.repeat
                && key.input.logical_key == Key::Escape
        });
    if was_open && (toggle || escape) {
        console.close();
    } else if !was_open
        && toggle
        && !pause.is_some_and(|pause| pause.open || pause.input_blocked)
        && !conversation.is_some_and(|dialog| dialog.open() || dialog.input_blocked)
        && !map.is_some_and(|map| map.open || map.input_blocked)
    {
        console.open = true;
        console.follow_output = 3;
    }
    console.input_blocked = was_open || console.open;
    if was_open != console.open {
        if let Some(touch) = touch.as_mut() {
            touch.reset();
        }
        return;
    }
    if !console.open {
        return;
    }
    console.poll_paste();
    let native_enter = events
        .iter()
        .any(|key| key.input.state.is_pressed() && key.input.logical_key == Key::Enter);
    for key in events {
        let event = &key.input;
        if !event.state.is_pressed() || backquote(key) {
            continue;
        }
        let shortcut = key.modifiers.control_key() || key.modifiers.super_key();
        if shortcut {
            match event.key_code {
                KeyCode::KeyV => {
                    if let Some(clipboard) = clipboard.as_mut() {
                        console.pending_paste = Some(clipboard.fetch_text());
                        // Native clipboard reads are ready now. Apply them before
                        // any Enter event that arrived later in this same frame.
                        console.poll_paste();
                    }
                }
                KeyCode::KeyU => {
                    console.input.clear();
                    console.cursor = 0;
                    console.history_index = None;
                }
                _ => {}
            }
            continue;
        }
        match event.logical_key {
            Key::Enter => {
                if !event.repeat {
                    send_command(&mut console, &mut connection);
                }
            }
            Key::ArrowUp => console.history(true),
            Key::ArrowDown => console.history(false),
            Key::ArrowLeft => console.cursor = console.previous_boundary(),
            Key::ArrowRight => console.cursor = console.next_boundary(),
            Key::Home => console.cursor = 0,
            Key::End => console.cursor = console.input.len(),
            Key::Backspace => {
                let start = console.previous_boundary();
                let end = console.cursor;
                console.input.replace_range(start..end, "");
                console.cursor = start;
                console.history_index = None;
            }
            Key::Delete => {
                let start = console.cursor;
                let end = console.next_boundary();
                console.input.replace_range(start..end, "");
                console.history_index = None;
            }
            _ => {
                if let Some(text) = &event.text {
                    console.insert(text);
                }
            }
        }
    }
    if !native_enter && keys.just_pressed(KeyCode::Enter) {
        send_command(&mut console, &mut connection);
    }
}

fn send_command(console: &mut AdminConsole, connection: &mut Option<ResMut<Connection>>) {
    let Some(command) = console.submit() else {
        return;
    };
    if let Some(connection) = connection {
        if let Some(error) = &connection.error {
            console.reply(format!("Command not sent: {error}"));
        } else {
            connection.send(ClientMessage::AdminCommand { command });
            if let Some(error) = &connection.error {
                console.reply(format!("Could not send command: {error}"));
            }
        }
    } else {
        console.reply("Command not sent: no server connection.");
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn refresh(
    mut console: ResMut<AdminConsole>,
    wheel: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut roots: Query<&mut Node, With<ConsoleRoot>>,
    mut panels: Query<&mut Node, (With<ConsolePanel>, Without<ConsoleRoot>)>,
    mut text: Query<(&mut Text, Option<&ConsoleInput>, Option<&ConsoleOutput>)>,
    mut scrolls: Query<(&ComputedNode, &mut ScrollPosition), With<ConsoleScroll>>,
) {
    for mut root in &mut roots {
        root.display = if console.open {
            Display::Flex
        } else {
            Display::None
        };
        root.padding = UiRect::all(px(if window.height() < 500. { 8. } else { 16. }));
    }
    for mut panel in &mut panels {
        panel.height = percent(if window.height() < 500. { 100 } else { 72 });
        panel.padding = UiRect::all(px(if window.width() < 620. { 10 } else { 16 }));
    }
    if !console.open {
        return;
    }
    for (mut text, input, output) in &mut text {
        if input.is_some() {
            text.0 = format!(
                "> {}|{}",
                &console.input[..console.cursor],
                &console.input[console.cursor..]
            );
        } else if output.is_some() {
            text.0 = console
                .output
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    if wheel.delta.y != 0. {
        console.follow_output = 0;
    }
    for (computed, mut scroll) in &mut scrolls {
        let max =
            ((computed.content_size.y - computed.size.y) * computed.inverse_scale_factor).max(0.);
        scroll.0.y = if console.follow_output > 0 {
            max
        } else {
            (scroll.0.y - wheel.delta.y * 28.).clamp(0., max)
        };
    }
    console.follow_output = console.follow_output.saturating_sub(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, keyboard::KeyboardInput};
    use rubblekin_core::protocol::SessionMode;
    use winit::keyboard::ModifiersState;

    fn console_app(admin: bool, mode: SessionMode) -> (App, Entity) {
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(mode),
            "console test".into(),
            crate::graphics::GraphicsQuality::Balanced,
            0.,
            mode,
        )
        .unwrap();
        session.can_admin = admin;
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<AdminConsole>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<MenuKey>()
            .add_systems(Update, read);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        (app, window)
    }

    fn key(app: &mut App, window: Entity, key_code: KeyCode, logical_key: Key, text: Option<&str>) {
        app.world_mut().write_message(MenuKey {
            input: KeyboardInput {
                key_code,
                logical_key,
                text: text.map(Into::into),
                state: ButtonState::Pressed,
                repeat: false,
                window,
            },
            modifiers: ModifiersState::empty(),
        });
    }

    #[test]
    fn tilde_is_modal_and_never_enters_the_command() {
        let (mut app, window) = console_app(true, SessionMode::Player);
        key(
            &mut app,
            window,
            KeyCode::Backquote,
            Key::Character("~".into()),
            Some("~"),
        );
        app.update();
        let console = app.world().resource::<AdminConsole>();
        assert!(console.open && console.input_blocked);
        assert!(console.input.is_empty());
        key(
            &mut app,
            window,
            KeyCode::KeyH,
            Key::Character("h".into()),
            Some("help"),
        );
        app.update();
        assert_eq!(app.world().resource::<AdminConsole>().input, "help");
        key(&mut app, window, KeyCode::Escape, Key::Escape, None);
        app.update();
        let console = app.world().resource::<AdminConsole>();
        assert!(!console.open && console.input_blocked && console.just_closed);
        app.update();
        assert!(!app.world().resource::<AdminConsole>().input_blocked);
    }

    #[test]
    fn observers_guests_and_other_modal_panels_cannot_open_console() {
        for (admin, mode) in [(false, SessionMode::Player), (true, SessionMode::Observer)] {
            let (mut app, window) = console_app(admin, mode);
            key(
                &mut app,
                window,
                KeyCode::Backquote,
                Key::Character("`".into()),
                Some("`"),
            );
            app.update();
            assert!(!app.world().resource::<AdminConsole>().open);
        }
        let (mut app, window) = console_app(true, SessionMode::Player);
        let mut pause = PauseMenu::default();
        pause.open = true;
        app.insert_resource(pause);
        key(
            &mut app,
            window,
            KeyCode::Backquote,
            Key::Character("`".into()),
            Some("`"),
        );
        app.update();
        assert!(!app.world().resource::<AdminConsole>().open);
    }

    #[test]
    fn map_open_and_closing_frames_block_console_shortcut() {
        for open in [false, true] {
            let (mut app, window) = console_app(true, SessionMode::Player);
            let mut map = crate::world_map::WorldMap::default();
            map.open = open;
            map.input_blocked = true;
            app.insert_resource(map);
            key(
                &mut app,
                window,
                KeyCode::Backquote,
                Key::Character("`".into()),
                Some("`"),
            );
            app.update();
            assert!(!app.world().resource::<AdminConsole>().open);
        }
    }

    #[test]
    fn typing_before_enter_submits_once_and_history_restores_a_draft() {
        let (mut app, window) = console_app(true, SessionMode::Player);
        app.world_mut().resource_mut::<AdminConsole>().open = true;
        key(
            &mut app,
            window,
            KeyCode::KeyH,
            Key::Character("h".into()),
            Some("help"),
        );
        key(&mut app, window, KeyCode::Enter, Key::Enter, Some("\r"));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let console = app.world().resource::<AdminConsole>();
        assert!(console.input.is_empty());
        assert_eq!(console.history.len(), 1);
        assert_eq!(
            console
                .output
                .iter()
                .filter(|line| *line == "> help")
                .count(),
            1
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        key(
            &mut app,
            window,
            KeyCode::KeyT,
            Key::Character("t".into()),
            Some("teleport "),
        );
        key(&mut app, window, KeyCode::ArrowUp, Key::ArrowUp, None);
        app.update();
        assert_eq!(app.world().resource::<AdminConsole>().input, "help");
        key(&mut app, window, KeyCode::ArrowDown, Key::ArrowDown, None);
        app.update();
        assert_eq!(app.world().resource::<AdminConsole>().input, "teleport ");
    }

    #[test]
    fn ready_paste_is_applied_before_enter_and_repeat_enter_does_not_run_again() {
        let (mut app, window) = console_app(true, SessionMode::Player);
        {
            let mut console = app.world_mut().resource_mut::<AdminConsole>();
            console.open = true;
            console.pending_paste = Some(ClipboardRead::Ready(Ok("teleport Ian Violet".into())));
        }
        key(&mut app, window, KeyCode::Enter, Key::Enter, Some("\r"));
        app.update();
        assert_eq!(
            app.world()
                .resource::<AdminConsole>()
                .history
                .back()
                .map(String::as_str),
            Some("teleport Ian Violet")
        );
        key(
            &mut app,
            window,
            KeyCode::KeyH,
            Key::Character("h".into()),
            Some("help"),
        );
        app.world_mut().write_message(MenuKey {
            input: KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                text: Some("\r".into()),
                repeat: true,
                window,
            },
            modifiers: ModifiersState::empty(),
        });
        app.update();
        let console = app.world().resource::<AdminConsole>();
        assert_eq!(console.input, "help");
        assert_eq!(console.history.len(), 1);
    }

    #[test]
    fn editing_and_limits_preserve_utf8_and_paste_never_executes() {
        let (mut app, window) = console_app(true, SessionMode::Player);
        app.world_mut().resource_mut::<AdminConsole>().open = true;
        key(
            &mut app,
            window,
            KeyCode::KeyA,
            Key::Character("Ian雪".into()),
            Some("Ian雪"),
        );
        key(&mut app, window, KeyCode::ArrowLeft, Key::ArrowLeft, None);
        key(&mut app, window, KeyCode::Backspace, Key::Backspace, None);
        key(&mut app, window, KeyCode::Delete, Key::Delete, None);
        app.update();
        assert_eq!(app.world().resource::<AdminConsole>().input, "Ia");
        let mut console = app.world_mut().resource_mut::<AdminConsole>();
        console.insert("\nhelp\r\nteleport Ian Violet\t");
        assert_eq!(console.input, "Ia help  teleport Ian Violet ");
        assert!(console.history.is_empty());
        let before = console.input.clone();
        console.insert(&"雪".repeat(MAX_ADMIN_COMMAND_BYTES));
        assert_eq!(console.input, before);
        for _ in 0..MAX_OUTPUT_LINES + 10 {
            console.reply("reply");
        }
        assert_eq!(console.output.len(), MAX_OUTPUT_LINES);
        console.reply("雪".repeat(MAX_OUTPUT_BYTES));
        assert!(console.output.iter().map(String::len).sum::<usize>() <= MAX_OUTPUT_BYTES);
    }
}
