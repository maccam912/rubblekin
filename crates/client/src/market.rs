//! One private wallet and a physical village market; all changes await the server.
use bevy::{
    input::{
        keyboard::Key,
        mouse::AccumulatedMouseScroll,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    window::PrimaryWindow,
};
use rubblekin_core::{
    economy::{
        CARGO_CAPACITY, MarketAction, MarketView, PlayerEconomy, RESOURCES, can_reach_market,
        resource_index,
    },
    protocol::ClientMessage,
    settlement::ResourceKind,
};

use crate::{
    GameEntity, Session, VoxelWorld, join::MenuKey, network::Connection, touch::TouchControls,
};

#[derive(Resource)]
pub(crate) struct MarketPanel {
    pub open: bool,
    pub input_blocked: bool,
    pub just_closed: bool,
    pub ledger: Option<PlayerEconomy>,
    market: Option<MarketView>,
    notice: String,
    quantity: u32,
    pending: Option<(u64, Option<u32>, bool)>,
    next_request: u64,
    last_refresh: f64,
    gesture: Option<Gesture>,
    focused: Option<Action>,
}

impl Default for MarketPanel {
    fn default() -> Self {
        Self {
            open: false,
            input_blocked: false,
            just_closed: false,
            ledger: None,
            market: None,
            notice: String::new(),
            quantity: 1,
            pending: None,
            next_request: 1,
            last_refresh: f64::NEG_INFINITY,
            gesture: None,
            focused: None,
        }
    }
}

impl MarketPanel {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn reply(
        &mut self,
        request_id: u64,
        ledger: PlayerEconomy,
        market: Option<MarketView>,
        notice: String,
        accepted: bool,
    ) {
        let current = self
            .ledger
            .as_ref()
            .is_none_or(|old| ledger.revision >= old.revision);
        if current {
            self.ledger = Some(ledger);
        }
        // Initial private state is unsolicited. Later replies may update the wallet
        // after closing, but cannot reopen a panel or replace a newer request.
        if self.pending.is_some_and(|(id, _, _)| id == request_id) {
            let (_, target, mutation) = self.pending.take().unwrap();
            self.market = market.filter(|view| current && Some(view.village_id) == target);
            if !accepted && notice.is_empty() {
                self.notice =
                    "That action was not accepted. Review the latest market details.".into();
            } else if mutation || !notice.is_empty() {
                self.notice = notice;
            }
        }
    }

    fn request(
        &mut self,
        village_id: Option<u32>,
        action: MarketAction,
        now: f64,
    ) -> Option<ClientMessage> {
        if self.pending.is_some() {
            return None;
        }
        let request_id = self.next_request;
        self.next_request = self.next_request.checked_add(1)?;
        self.pending = Some((request_id, village_id, action != MarketAction::View));
        self.last_refresh = now;
        Some(ClientMessage::Market {
            request_id,
            village_id,
            revision: self.ledger.as_ref().map_or(0, |ledger| ledger.revision),
            action,
        })
    }

    fn close(&mut self) {
        self.open = false;
        self.input_blocked = true;
        self.just_closed = true;
        self.gesture = None;
        self.focused = None;
    }

    fn available_market(&self, nearby: Option<u32>) -> Option<&MarketView> {
        self.market
            .as_ref()
            .filter(|view| Some(view.village_id) == nearby)
    }

    fn action(&self, action: Action, nearby: Option<u32>) -> Option<MarketAction> {
        if self.pending.is_some() {
            return None;
        }
        let ledger = self.ledger.as_ref()?;
        let market = self.available_market(nearby)?;
        match action {
            Action::Buy(kind) | Action::Sell(kind) => {
                let good = market.goods.iter().find(|good| good.kind == kind)?;
                let quantity = self.quantity;
                if matches!(action, Action::Buy(_)) {
                    let total = good.buy_price.checked_mul(u64::from(quantity))?;
                    (quantity <= good.exportable
                        && ledger.coins >= total
                        && ledger.cargo_total().saturating_add(quantity) <= CARGO_CAPACITY)
                        .then_some(MarketAction::Buy {
                            kind,
                            quantity,
                            unit_price: good.buy_price,
                        })
                } else {
                    (ledger.cargo[resource_index(kind)] >= quantity).then_some(MarketAction::Sell {
                        kind,
                        quantity,
                        unit_price: good.sell_price,
                    })
                }
            }
            Action::Accept => market
                .delivery_offer
                .as_ref()
                .filter(|offer| {
                    ledger.delivery.is_none()
                        && ledger.cargo_total().saturating_add(offer.amount) <= CARGO_CAPACITY
                })
                .map(|offer| MarketAction::AcceptDelivery {
                    offer: offer.clone(),
                }),
            Action::Deliver => ledger
                .delivery
                .as_ref()
                .filter(|job| job.destination == market.village_id)
                .map(|_| MarketAction::Deliver),
            Action::Return => ledger
                .delivery
                .as_ref()
                .filter(|job| job.origin == market.village_id)
                .map(|_| MarketAction::ReturnDelivery),
            _ => None,
        }
    }
}

#[derive(Component)]
pub(crate) struct MarketRoot;
#[derive(Component)]
struct Panel;
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    Close,
    Quantity(u32),
    Refresh,
    Buy(ResourceKind),
    Sell(ResourceKind),
    Accept,
    Deliver,
    Return,
}
#[derive(Component, Clone, Copy)]
pub(crate) enum Label {
    Title,
    Wallet,
    Place,
    Goods(ResourceKind),
    Job,
    Notice,
    Button(Action),
}

struct Gesture {
    id: u64,
    start: Vec2,
    previous: Vec2,
    action: Option<Action>,
}

/// Raw touch owns its complete gesture, including the final release frame.
fn touch_gesture(
    gesture: &mut Option<Gesture>,
    event: &TouchInput,
    hit: Option<Action>,
) -> (Option<Action>, f32) {
    if event.phase == TouchPhase::Canceled {
        if gesture.as_ref().is_some_and(|active| active.id == event.id) {
            *gesture = None;
        }
        return (None, 0.);
    }
    if !event.position.is_finite() {
        return (None, 0.);
    }
    if event.phase == TouchPhase::Started {
        if gesture.is_none() {
            *gesture = Some(Gesture {
                id: event.id,
                start: event.position,
                previous: event.position,
                action: hit,
            });
        }
        return (None, 0.);
    }
    let Some(active) = gesture.as_mut().filter(|active| active.id == event.id) else {
        return (None, 0.);
    };
    if active.start.distance(event.position) > 8. {
        active.action = None;
    }
    let delta = if active.action.is_none() {
        active.previous.y - event.position.y
    } else {
        0.
    };
    active.previous = event.position;
    let chosen = if event.phase == TouchPhase::Ended {
        let chosen = active.action.filter(|action| Some(*action) == hit);
        *gesture = None;
        chosen
    } else {
        None
    };
    (chosen, delta)
}

pub(crate) fn nearby_market(session: &Session, world: &VoxelWorld) -> Option<u32> {
    if session.observer.is_some() {
        return None;
    }
    world
        .0
        .settlements()?
        .villages
        .iter()
        .filter(|village| can_reach_market(session.body.position, village.market))
        .min_by(|a, b| {
            Vec3::from_array(a.market)
                .distance_squared(Vec3::from_array(session.body.position))
                .total_cmp(
                    &Vec3::from_array(b.market)
                        .distance_squared(Vec3::from_array(session.body.position)),
                )
        })
        .map(|village| village.id)
}

fn village_name(world: &VoxelWorld, id: u32) -> String {
    world
        .0
        .settlements()
        .and_then(|plan| plan.villages.iter().find(|village| village.id == id))
        .map_or_else(|| format!("Village {id}"), |village| village.name.clone())
}

pub(crate) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands.spawn((GameEntity, MarketRoot, GlobalZIndex(92), ScrollPosition::default(),
        Node { display: Display::None, width: percent(100), height: percent(100), padding: UiRect::all(px(12)),
            align_items: AlignItems::Center, flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(), ..default() },
        BackgroundColor(Color::srgba(0.02, 0.05, 0.05, 0.60))))
        .with_children(|root| {
            root.spawn((Panel, Node { width: px(720), max_width: percent(100), padding: UiRect::all(px(18)),
                flex_direction: FlexDirection::Column, row_gap: px(10), flex_shrink: 0.,
                border_radius: BorderRadius::all(px(9)), ..default() },
                BackgroundColor(Color::srgb(0.08, 0.15, 0.14))))
                .with_children(|panel| {
                    panel.spawn((Label::Title, Text::new("Cargo & work"), TextFont::from_font_size(24.).with_font(font.clone()),
                        TextColor(Color::srgb(0.95, 0.83, 0.56))));
                    panel.spawn((Label::Wallet, Text::new("Loading your cargo…"), TextFont::from_font_size(18.).with_font(font.clone()), TextColor(Color::srgb(0.9, 0.94, 0.86))));
                    panel.spawn((Label::Place, Text::new(""), TextFont::from_font_size(16.).with_font(font.clone()), TextColor(Color::srgb(0.78, 0.85, 0.79))));
                    panel.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(8), row_gap: px(8), ..default() }).with_children(|row| {
                        for action in [Action::Close, Action::Quantity(1), Action::Quantity(5), Action::Refresh] {
                            row.spawn(button(action)).with_child(label(action, &font));
                        }
                    });
                    for kind in RESOURCES {
                        panel.spawn(Node { flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center,
                            column_gap: px(8), row_gap: px(6), padding: UiRect::vertical(px(4)), ..default() })
                            .with_children(|row| {
                                row.spawn((Label::Goods(kind), Text::new(kind.name()), TextFont::from_font_size(16.).with_font(font.clone()),
                                    TextColor(Color::srgb(0.90, 0.94, 0.86)), Node { min_width: px(220), flex_grow: 1., ..default() }));
                                for action in [Action::Buy(kind), Action::Sell(kind)] {
                                    row.spawn(button(action)).with_child(label(action, &font));
                                }
                            });
                    }
                    panel.spawn((Label::Job, Text::new(""), TextFont::from_font_size(17.).with_font(font.clone()), TextColor(Color::srgb(0.95, 0.85, 0.62))));
                    panel.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(8), row_gap: px(8), ..default() }).with_children(|row| {
                        for action in [Action::Accept, Action::Deliver, Action::Return] {
                            row.spawn(button(action)).with_child(label(action, &font));
                        }
                    });
                    panel.spawn((Label::Notice, Text::new(""), TextFont::from_font_size(16.).with_font(font.clone()), TextColor(Color::srgb(0.95, 0.86, 0.68))));
                    panel.spawn(button(Action::Close)).with_child(label(Action::Close, &font));
                    panel.spawn((Text::new("B / Esc: close · Tab / arrows: select · Enter: use · Scroll or swipe for more"),
                        TextFont::from_font_size(14.).with_font(font), TextColor(Color::srgb(0.66, 0.77, 0.71))));
                });
        });
}

fn button(action: Action) -> impl Bundle {
    (
        Button,
        action,
        Node {
            min_height: px(44),
            min_width: px(104),
            padding: UiRect::axes(px(12), px(9)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.35, 0.30)),
        BorderColor::all(Color::NONE),
    )
}
fn label(action: Action, font: &Handle<Font>) -> impl Bundle {
    (
        Label::Button(action),
        Text::new(""),
        TextFont::from_font_size(16.).with_font(font.clone()),
        TextColor(Color::srgb(0.94, 0.95, 0.88)),
    )
}

fn all_actions() -> Vec<Action> {
    [
        Action::Close,
        Action::Quantity(1),
        Action::Quantity(5),
        Action::Refresh,
    ]
    .into_iter()
    .chain(
        RESOURCES
            .into_iter()
            .flat_map(|kind| [Action::Buy(kind), Action::Sell(kind)]),
    )
    .chain([Action::Accept, Action::Deliver, Action::Return])
    .collect()
}

fn enabled(panel: &MarketPanel, action: Action, nearby: Option<u32>) -> bool {
    match action {
        Action::Close | Action::Quantity(_) => true,
        Action::Refresh => panel.pending.is_none(),
        _ => panel.action(action, nearby).is_some(),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn read(
    mut panel: ResMut<MarketPanel>,
    mut session: ResMut<Session>,
    world: Res<VoxelWorld>,
    mut connection: ResMut<Connection>,
    mut touch: ResMut<TouchControls>,
    modals: (
        Option<Res<crate::pause::PauseMenu>>,
        Option<Res<crate::admin_console::AdminConsole>>,
        Option<Res<crate::world_map::WorldMap>>,
        Option<Res<crate::airships::PilotConversation>>,
    ),
    input: (
        Res<ButtonInput<KeyCode>>,
        Res<AccumulatedMouseScroll>,
        Res<Time>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    mut native: MessageReader<MenuKey>,
    touch_input: (MessageReader<TouchInput>, Option<Res<Touches>>),
    actions: Query<(&Action, &Interaction), Changed<Interaction>>,
    targets: Query<(
        Entity,
        &Action,
        &ComputedNode,
        &UiGlobalTransform,
        &Node,
        Option<&InheritedVisibility>,
    )>,
    clipping: Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
    parents: Query<&ChildOf, Without<bevy::ui::OverrideClip>>,
    mut roots: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<MarketRoot>>,
) {
    let (keys, wheel, time) = input;
    let (pause, console, map, pilot) = modals;
    let (mut fingers, touches) = touch_input;
    let was_open = panel.open;
    panel.just_closed = false;
    panel.input_blocked = was_open;
    let mut back = false;
    for key in native.read() {
        back |= key.input.state.is_pressed()
            && !key.input.repeat
            && key.input.logical_key == Key::BrowserBack;
    }
    let events: Vec<_> = fingers.read().copied().collect();
    if session.observer.is_some() || connection.error.is_some() {
        if was_open {
            panel.close();
        }
        return;
    }
    if !windows.iter().any(|window| window.focused) || touch.suspended {
        panel.gesture = None;
        return;
    }
    let other_modal = pause.is_some_and(|menu| menu.open || menu.input_blocked)
        || console.is_some_and(|menu| menu.open || menu.input_blocked)
        || map.is_some_and(|menu| menu.open || menu.input_blocked)
        || pilot.is_some_and(|menu| menu.open() || menu.input_blocked);
    if other_modal {
        panel.gesture = None;
        return;
    }
    let nearby = nearby_market(&session, &world);
    if !was_open {
        if keys.just_pressed(KeyCode::KeyB) || touch.market {
            panel.open = true;
            panel.input_blocked = true;
            panel.focused = Some(Action::Close);
            panel.market = None;
            panel.notice.clear();
            panel.last_refresh = f64::NEG_INFINITY;
            session.help = false;
            session.inspector = false;
            touch.reset();
            for (_, _, mut scroll) in &mut roots {
                scroll.0 = Vec2::ZERO;
            }
        } else {
            return;
        }
    }
    let mut chosen = (was_open
        && (keys.just_pressed(KeyCode::KeyB) || keys.just_pressed(KeyCode::Escape) || back))
        .then_some(Action::Close);
    let native_touch = panel.gesture.is_some()
        || !events.is_empty()
        || touches.is_some_and(|touches| touches.iter().next().is_some());
    let mut scroll_delta = -wheel.delta.y * 28.;
    if was_open {
        for event in &events {
            let Ok(window) = windows.get(event.window) else {
                continue;
            };
            let point = event.position * window.scale_factor();
            let hit =
                targets
                    .iter()
                    .find_map(|(entity, action, node, transform, style, visibility)| {
                        (style.display != Display::None
                            && enabled(&panel, *action, nearby)
                            && visibility.is_none_or(|v| v.get())
                            && node.contains_point(*transform, point)
                            && bevy::ui::clip_check_recursive(point, entity, &clipping, &parents))
                        .then_some(*action)
                    });
            let (action, delta) = touch_gesture(&mut panel.gesture, event, hit);
            chosen = chosen.or(action);
            scroll_delta += delta;
        }
        if !native_touch && !cfg!(target_os = "android") && chosen.is_none() {
            chosen = actions.iter().find_map(|(action, interaction)| {
                (*interaction == Interaction::Pressed).then_some(*action)
            });
        }
        let step = if keys.just_pressed(KeyCode::ArrowUp)
            || (keys.just_pressed(KeyCode::Tab)
                && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)))
        {
            -1
        } else if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::Tab) {
            1
        } else {
            0
        };
        if step != 0 {
            let available: Vec<_> = all_actions()
                .into_iter()
                .filter(|action| enabled(&panel, *action, nearby))
                .collect();
            let index = available
                .iter()
                .position(|action| Some(*action) == panel.focused)
                .unwrap_or(0) as i32;
            panel.focused =
                Some(available[(index + step).rem_euclid(available.len() as i32) as usize]);
            // Keep keyboard focus visible in short landscape windows.
            if let Some((_, _, node, transform, _, _)) = targets
                .iter()
                .find(|(_, action, ..)| Some(**action) == panel.focused)
            {
                for (root, root_transform, _) in &roots {
                    let top = transform.translation.y - node.size.y * 0.5;
                    let bottom = top + node.size.y;
                    let root_top = root_transform.translation.y - root.size.y * 0.5 + 12.;
                    let root_bottom = root_top + root.size.y - 24.;
                    scroll_delta += if top < root_top {
                        (top - root_top) * root.inverse_scale_factor
                    } else if bottom > root_bottom {
                        (bottom - root_bottom) * root.inverse_scale_factor
                    } else {
                        0.
                    };
                }
            }
        }
        if chosen.is_none()
            && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space))
        {
            chosen = panel.focused;
        }
    }
    for (node, _, mut scroll) in &mut roots {
        let max = ((node.content_size.y - node.size.y) * node.inverse_scale_factor).max(0.);
        scroll.0.y = (scroll.0.y + scroll_delta).clamp(0., max);
    }
    if let Some(action) = chosen {
        match action {
            Action::Close => {
                panel.close();
                touch.reset();
                return;
            }
            Action::Quantity(quantity) => panel.quantity = quantity,
            Action::Refresh => panel.last_refresh = f64::NEG_INFINITY,
            action => {
                if let Some(action) = panel.action(action, nearby) {
                    panel.notice.clear();
                    if let Some(message) = panel.request(nearby, action, time.elapsed_secs_f64()) {
                        connection.send(message);
                    }
                }
            }
        }
    }
    if panel.pending.is_none()
        && (time.elapsed_secs_f64() - panel.last_refresh >= 1.
            || panel
                .market
                .as_ref()
                .is_some_and(|view| Some(view.village_id) != nearby))
        && let Some(message) = panel.request(nearby, MarketAction::View, time.elapsed_secs_f64())
    {
        connection.send(message);
    }
}

fn wallet(panel: &MarketPanel) -> String {
    panel.ledger.as_ref().map_or_else(
        || "Loading your cargo…".into(),
        |ledger| {
            format!(
                "{} coins · Cargo {} / {}",
                ledger.coins,
                ledger.cargo_total(),
                CARGO_CAPACITY
            )
        },
    )
}

fn job_text(panel: &MarketPanel, world: &VoxelWorld) -> String {
    if let Some(job) = panel
        .ledger
        .as_ref()
        .and_then(|ledger| ledger.delivery.as_ref())
    {
        format!(
            "DELIVERY · {} {} (sealed)\nBring it to {} market for {} coins.\nReturn it to {} market to cancel. No time limit.",
            job.amount,
            job.kind.name(),
            village_name(world, job.destination),
            job.reward,
            village_name(world, job.origin)
        )
    } else if let Some(offer) = panel
        .market
        .as_ref()
        .and_then(|market| market.delivery_offer.as_ref())
    {
        format!(
            "WORK AVAILABLE · Deliver {} {} to {} market\nEarn {} coins · Cargo supplied · No time limit",
            offer.amount,
            offer.kind.name(),
            village_name(world, offer.destination),
            offer.reward
        )
    } else if panel.market.is_some() {
        "No delivery available here right now. Check another village market for work.".into()
    } else {
        "No active delivery. Visit a village market to find work; deliveries supply their own cargo.".into()
    }
}

pub(crate) fn hud_text(
    panel: &MarketPanel,
    session: &Session,
    world: &VoxelWorld,
    touch: bool,
) -> String {
    if session.observer.is_some() {
        return String::new();
    }
    let Some(ledger) = &panel.ledger else {
        return String::new();
    };
    let mut text = format!(
        "{} coins · Cargo {}/{}",
        ledger.coins,
        ledger.cargo_total(),
        CARGO_CAPACITY
    );
    if let Some(job) = &ledger.delivery {
        text.push_str(&format!(
            " · Deliver {} {} to {}",
            job.amount,
            job.kind.name(),
            village_name(world, job.destination)
        ));
    } else if let Some(id) = nearby_market(session, world) {
        text.push_str(&format!(" · {} market", village_name(world, id)));
    }
    text.push_str(if touch {
        " · Cargo"
    } else {
        " · B: cargo & work"
    });
    text
}

#[allow(clippy::type_complexity)]
pub(crate) fn refresh(
    panel: Res<MarketPanel>,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    mut roots: Query<&mut Node, With<MarketRoot>>,
    mut buttons: Query<
        (&Action, &mut Node, &mut BackgroundColor, &mut BorderColor),
        Without<MarketRoot>,
    >,
    mut labels: Query<(&Label, &mut Text)>,
) {
    let nearby = nearby_market(&session, &world);
    let market = panel.available_market(nearby);
    for mut root in &mut roots {
        root.display = if panel.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (action, mut node, mut background, mut border) in &mut buttons {
        let available = enabled(&panel, *action, nearby);
        let visible = match action {
            Action::Buy(_) | Action::Sell(_) => market.is_some(),
            Action::Accept => {
                market.is_some_and(|market| market.delivery_offer.is_some())
                    && panel
                        .ledger
                        .as_ref()
                        .is_none_or(|ledger| ledger.delivery.is_none())
            }
            Action::Deliver => panel
                .ledger
                .as_ref()
                .and_then(|ledger| ledger.delivery.as_ref())
                .is_some_and(|job| Some(job.destination) == nearby),
            Action::Return => panel
                .ledger
                .as_ref()
                .and_then(|ledger| ledger.delivery.as_ref())
                .is_some_and(|job| Some(job.origin) == nearby),
            _ => true,
        };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        background.0 = if available {
            Color::srgb(0.20, 0.35, 0.30)
        } else {
            Color::srgb(0.13, 0.21, 0.19)
        };
        *border = BorderColor::all(
            if panel.focused == Some(*action) || *action == Action::Quantity(panel.quantity) {
                Color::srgb(0.91, 0.77, 0.43)
            } else {
                Color::NONE
            },
        );
    }
    for (label, mut text) in &mut labels {
        let value = match label {
            Label::Title => market.map_or_else(
                || "Cargo & work".into(),
                |view| format!("{} market", village_name(&world, view.village_id)),
            ),
            Label::Wallet => wallet(&panel),
            Label::Place => {
                if market.is_some() {
                    "Trade from village stores. Food reserves stay with the village. Prices below are totals for your selected quantity.".into()
                } else if let Some(plan) = world.0.settlements() {
                    plan.villages.iter().min_by(|a,b| Vec3::from_array(a.market).distance_squared(Vec3::from_array(session.body.position))
                    .total_cmp(&Vec3::from_array(b.market).distance_squared(Vec3::from_array(session.body.position))))
                    .map_or_else(|| "This world has no village markets.".into(), |village| format!("Visit a market entrance to trade or find work. Nearest: {} · {:.0} m. M opens the world map after closing this panel.", village.name,
                        Vec2::new(village.market[0]-session.body.position[0], village.market[2]-session.body.position[2]).length()))
                } else {
                    "This world has no village markets.".into()
                }
            }
            Label::Goods(kind) => {
                let carrying = panel
                    .ledger
                    .as_ref()
                    .map_or(0, |ledger| ledger.cargo[resource_index(*kind)]);
                market
                    .and_then(|market| market.goods.iter().find(|good| good.kind == *kind))
                    .map_or_else(
                        || format!("{} · For trade {}", kind.name(), carrying),
                        |good| {
                            format!(
                                "{} · For trade {}\nVillage {:.0} · For sale {}",
                                kind.name(),
                                carrying,
                                good.stock.floor(),
                                good.exportable
                            )
                        },
                    )
            }
            Label::Job => job_text(&panel, &world),
            Label::Notice => {
                if panel
                    .pending
                    .is_some_and(|(_, _, mutation)| mutation || panel.market.is_none())
                    && panel.notice.is_empty()
                {
                    "Waiting for the market…".into()
                } else {
                    panel.notice.clone()
                }
            }
            Label::Button(action) => match action {
                Action::Close => "Close".into(),
                Action::Quantity(q) => format!("Quantity {q}"),
                Action::Refresh => "Refresh".into(),
                Action::Accept => "Accept delivery".into(),
                Action::Deliver => "Deliver & collect pay".into(),
                Action::Return => "Return cargo · cancel job".into(),
                Action::Buy(kind) | Action::Sell(kind) => market
                    .and_then(|market| market.goods.iter().find(|good| good.kind == *kind))
                    .map_or_else(String::new, |good| {
                        let buy = matches!(action, Action::Buy(_));
                        format!(
                            "{} {} · {} c",
                            if buy { "Buy" } else { "Sell" },
                            panel.quantity,
                            (if buy { good.buy_price } else { good.sell_price })
                                .saturating_mul(u64::from(panel.quantity))
                        )
                    }),
            },
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        economy::{DeliveryContract, MarketGood},
        protocol::SessionMode,
    };

    fn offer() -> DeliveryContract {
        DeliveryContract {
            origin: 1,
            destination: 2,
            kind: ResourceKind::Stone,
            amount: 6,
            reward: 12,
        }
    }
    fn market(id: u32) -> MarketView {
        MarketView {
            village_id: id,
            goods: vec![MarketGood {
                kind: ResourceKind::Stone,
                stock: 20.,
                exportable: 12,
                buy_price: 3,
                sell_price: 2,
            }],
            delivery_offer: Some(offer()),
        }
    }
    fn panel() -> MarketPanel {
        MarketPanel {
            ledger: Some(PlayerEconomy {
                coins: 30,
                ..default()
            }),
            market: Some(market(1)),
            ..default()
        }
    }
    fn touch(id: u64, phase: TouchPhase, position: Vec2) -> TouchInput {
        TouchInput {
            id,
            phase,
            position,
            window: Entity::PLACEHOLDER,
            force: None,
        }
    }

    #[test]
    fn trades_wait_for_canonical_reply_and_repeated_requests_do_not_spend_twice() {
        let mut panel = panel();
        panel.quantity = 5;
        let action = panel
            .action(Action::Buy(ResourceKind::Stone), Some(1))
            .unwrap();
        assert_eq!(
            action,
            MarketAction::Buy {
                kind: ResourceKind::Stone,
                quantity: 5,
                unit_price: 3
            }
        );
        let message = panel.request(Some(1), action.clone(), 1.).unwrap();
        let ClientMessage::Market {
            request_id,
            revision,
            ..
        } = message
        else {
            panic!()
        };
        assert_eq!(revision, 0);
        assert!(panel.request(Some(1), action, 1.).is_none());
        assert!(
            panel
                .action(Action::Buy(ResourceKind::Stone), Some(1))
                .is_none()
        );
        assert_eq!(panel.ledger.as_ref().unwrap().coins, 30);
        assert_eq!(panel.ledger.as_ref().unwrap().cargo_total(), 0);
        let ledger = PlayerEconomy {
            revision: 1,
            coins: 15,
            cargo: [0, 0, 5, 0, 0],
            delivery: None,
        };
        panel.reply(
            request_id,
            ledger.clone(),
            Some(market(1)),
            "Bought 5 Stone".into(),
            true,
        );
        assert_eq!(panel.ledger.as_ref(), Some(&ledger));
        assert!(panel.pending.is_none());
        assert!(
            panel
                .action(Action::Sell(ResourceKind::Stone), Some(1))
                .is_some()
        );
        panel.reply(
            request_id,
            PlayerEconomy::default(),
            Some(market(2)),
            "Old response".into(),
            false,
        );
        assert_eq!(panel.ledger.as_ref(), Some(&ledger));
        assert_eq!(panel.market.as_ref().unwrap().village_id, 1);
        assert_eq!(panel.notice, "Bought 5 Stone");
    }

    #[test]
    fn rejection_updates_quotes_without_predicting_money_and_late_reply_never_opens_panel() {
        let mut panel = panel();
        panel.open = true;
        panel.request(
            Some(1),
            MarketAction::Buy {
                kind: ResourceKind::Stone,
                quantity: 1,
                unit_price: 3,
            },
            0.,
        );
        panel.close();
        let mut changed = market(1);
        changed.goods[0].exportable = 0;
        let unchanged = panel.ledger.clone().unwrap();
        panel.reply(
            1,
            unchanged.clone(),
            Some(changed),
            "Stock changed".into(),
            false,
        );
        assert!(!panel.open && panel.just_closed && panel.input_blocked);
        assert_eq!(panel.ledger.as_ref(), Some(&unchanged));
        assert_eq!(panel.notice, "Stock changed");
        assert!(
            panel
                .action(Action::Buy(ResourceKind::Stone), Some(1))
                .is_none()
        );
        panel.clear();
        assert!(panel.ledger.is_none() && panel.pending.is_none() && !panel.open);
        panel.reply(0, unchanged.clone(), None, String::new(), true);
        assert_eq!(panel.ledger.as_ref(), Some(&unchanged));
        assert!(!panel.open);
    }

    #[test]
    fn sealed_delivery_counts_against_capacity_cannot_sell_and_requires_the_correct_market() {
        let mut panel = panel();
        panel.ledger.as_mut().unwrap().coins = 0;
        assert!(matches!(
            panel.action(Action::Accept, Some(1)),
            Some(MarketAction::AcceptDelivery { .. })
        ));
        panel.ledger.as_mut().unwrap().delivery = Some(offer());
        assert_eq!(panel.ledger.as_ref().unwrap().cargo_total(), 6);
        assert!(
            panel
                .action(Action::Sell(ResourceKind::Stone), Some(1))
                .is_none()
        );
        assert!(panel.action(Action::Accept, Some(1)).is_none());
        assert_eq!(
            panel.action(Action::Return, Some(1)),
            Some(MarketAction::ReturnDelivery)
        );
        assert!(panel.action(Action::Deliver, Some(1)).is_none());
        assert!(panel.action(Action::Return, None).is_none());
        panel.market = Some(market(2));
        assert_eq!(
            panel.action(Action::Deliver, Some(2)),
            Some(MarketAction::Deliver)
        );
        assert!(panel.action(Action::Return, Some(2)).is_none());
        panel.ledger.as_mut().unwrap().coins = 100;
        panel.ledger.as_mut().unwrap().cargo = [0, 0, 18, 0, 0];
        assert!(
            panel
                .action(Action::Buy(ResourceKind::Stone), Some(2))
                .is_none()
        );
    }

    #[test]
    fn touch_short_tap_scroll_final_delta_and_second_contact_preserve_one_owner() {
        let mut gesture = None;
        let start = Vec2::new(100., 100.);
        let action = Some(Action::Accept);
        assert_eq!(
            touch_gesture(&mut gesture, &touch(1, TouchPhase::Started, start), action),
            (None, 0.)
        );
        // A second finger cannot purchase another action or steal the swipe.
        touch_gesture(
            &mut gesture,
            &touch(2, TouchPhase::Started, start),
            Some(Action::Close),
        );
        assert_eq!(
            touch_gesture(
                &mut gesture,
                &touch(2, TouchPhase::Ended, start),
                Some(Action::Close)
            ),
            (None, 0.)
        );
        assert_eq!(
            touch_gesture(&mut gesture, &touch(1, TouchPhase::Ended, start), action),
            (action, 0.)
        );
        assert!(gesture.is_none());
        touch_gesture(&mut gesture, &touch(1, TouchPhase::Started, start), action);
        assert_eq!(
            touch_gesture(
                &mut gesture,
                &touch(1, TouchPhase::Moved, start - Vec2::Y * 20.),
                action
            ),
            (None, 20.)
        );
        assert_eq!(
            touch_gesture(
                &mut gesture,
                &touch(1, TouchPhase::Ended, start - Vec2::Y * 35.),
                action
            ),
            (None, 15.)
        );
        touch_gesture(&mut gesture, &touch(1, TouchPhase::Started, start), action);
        touch_gesture(
            &mut gesture,
            &touch(1, TouchPhase::Canceled, Vec2::NAN),
            action,
        );
        assert!(gesture.is_none());
    }

    fn app() -> (App, Entity, Entity, std::net::TcpStream) {
        use std::{io::Write, net::TcpListener};
        let welcome = crate::join::tests::welcome(SessionMode::Player);
        let (world, session) = crate::join::session_from_welcome(
            welcome.clone(),
            "test".into(),
            crate::graphics::GraphicsQuality::default(),
            0.,
            SessionMode::Player,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let connecting = std::thread::spawn(move || {
            Connection::connect(&address, "Market test".into(), SessionMode::Player)
                .unwrap()
                .0
        });
        let (mut peer, _) = listener.accept().unwrap();
        let mut bytes = serde_json::to_vec(&welcome).unwrap();
        bytes.push(b'\n');
        peer.write_all(&bytes).unwrap();
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(connecting.join().unwrap())
            .insert_resource(VoxelWorld(world))
            .insert_resource(session)
            .insert_resource(MarketPanel {
                open: true,
                ledger: Some(PlayerEconomy::default()),
                last_refresh: 0.,
                ..default()
            })
            .init_resource::<TouchControls>()
            .init_resource::<Time>()
            .init_resource::<Assets<Font>>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::airships::PilotConversation>()
            .add_message::<MenuKey>()
            .add_systems(Startup, setup)
            .add_systems(Update, (read, refresh).chain());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let close = app
            .world_mut()
            .spawn((
                Action::Close,
                Interaction::None,
                Node::default(),
                ComputedNode {
                    size: Vec2::splat(80.),
                    ..default()
                },
                UiGlobalTransform::from_xy(100., 100.),
                InheritedVisibility::VISIBLE,
                BackgroundColor::default(),
                BorderColor::default(),
            ))
            .id();
        app.update();
        (app, window, close, peer)
    }

    #[test]
    fn native_touch_ignores_stale_mouse_then_real_mouse_closes_with_input_blocked() {
        let (mut app, window, close, _peer) = app();
        let send = |app: &mut App, phase| {
            let mut event = touch(3, phase, Vec2::new(300., 100.));
            event.window = window;
            app.world_mut().write_message(event);
        };
        app.world_mut()
            .entity_mut(close)
            .insert(Interaction::Pressed);
        send(&mut app, TouchPhase::Started);
        app.update();
        assert!(app.world().resource::<MarketPanel>().open);
        app.world_mut()
            .entity_mut(close)
            .insert(Interaction::Pressed);
        app.update();
        assert!(app.world().resource::<MarketPanel>().open);
        app.world_mut()
            .entity_mut(close)
            .insert(Interaction::Pressed);
        send(&mut app, TouchPhase::Ended);
        app.update();
        assert!(app.world().resource::<MarketPanel>().open);
        app.world_mut()
            .entity_mut(close)
            .insert(Interaction::Pressed);
        app.update();
        let panel = app.world().resource::<MarketPanel>();
        assert!(!panel.open && panel.input_blocked && panel.just_closed);
        app.update();
        let panel = app.world().resource::<MarketPanel>();
        assert!(!panel.input_blocked && !panel.just_closed);
    }

    #[test]
    fn native_short_taps_use_scaled_bounds_and_cannot_hit_a_clipped_button() {
        let (mut app, window, close, _peer) = app();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(2.));
        let clip = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::clip(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::splat(40.),
                    ..default()
                },
                UiGlobalTransform::from_xy(100., 100.),
            ))
            .id();
        app.world_mut().entity_mut(close).insert(ChildOf(clip));
        for (point, remains_open) in [(Vec2::new(66., 50.), true), (Vec2::splat(50.), false)] {
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                let mut event = touch(4, phase, point);
                event.window = window;
                app.world_mut().write_message(event);
            }
            app.update();
            assert_eq!(app.world().resource::<MarketPanel>().open, remains_open);
        }
        assert!(app.world().resource::<MarketPanel>().input_blocked);
    }

    #[test]
    fn all_existing_modals_and_their_closing_frames_block_market_open() {
        let (mut app, _, _, _peer) = app();
        for modal in 0..8 {
            app.world_mut().resource_mut::<MarketPanel>().clear();
            *app.world_mut().resource_mut::<crate::pause::PauseMenu>() = default();
            *app.world_mut()
                .resource_mut::<crate::admin_console::AdminConsole>() = default();
            *app.world_mut().resource_mut::<crate::world_map::WorldMap>() = default();
            *app.world_mut()
                .resource_mut::<crate::airships::PilotConversation>() = default();
            match modal {
                0 => {
                    app.world_mut()
                        .resource_mut::<crate::pause::PauseMenu>()
                        .open = true
                }
                1 => {
                    app.world_mut()
                        .resource_mut::<crate::pause::PauseMenu>()
                        .input_blocked = true
                }
                2 => {
                    app.world_mut()
                        .resource_mut::<crate::admin_console::AdminConsole>()
                        .open = true
                }
                3 => {
                    app.world_mut()
                        .resource_mut::<crate::admin_console::AdminConsole>()
                        .input_blocked = true
                }
                4 => {
                    app.world_mut()
                        .resource_mut::<crate::world_map::WorldMap>()
                        .open = true
                }
                5 => {
                    app.world_mut()
                        .resource_mut::<crate::world_map::WorldMap>()
                        .input_blocked = true
                }
                6 => {
                    app.world_mut()
                        .resource_mut::<crate::airships::PilotConversation>()
                        .ship_id = Some(1)
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<crate::airships::PilotConversation>()
                        .input_blocked = true
                }
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyB);
            app.world_mut().resource_mut::<TouchControls>().market = true;
            app.world_mut().run_schedule(Update);
            assert!(!app.world().resource::<MarketPanel>().open, "modal {modal}");
        }
    }
}
