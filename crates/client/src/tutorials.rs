//! Short lessons advance through real actions. They never send gameplay requests.
use crate::{GameEntity, Session, touch::TouchControls};
use bevy::{
    input::touch::{TouchInput, TouchPhase},
    picking::hover::Hovered,
    prelude::*,
    ui_widgets::{ActivateOnPress, Button},
    window::PrimaryWindow,
};
use rubblekin_core::{activities::ActivityAction, world::Block};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const FILE: &str = "tutorials.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Lesson {
    Hotbar,
    Building,
    Map,
    Cargo,
    Work,
    Parcel,
    Supplies,
    Stones,
    Travel,
    Glide,
    Flight,
    FlowGarden,
}
impl Lesson {
    fn title(self) -> &'static str {
        match self {
            Self::Hotbar => "YOUR HOTBAR",
            Self::Building => "BUILD & DIG",
            Self::Map => "FIND YOUR WAY",
            Self::Cargo => "CARGO & COINS",
            Self::Work => "A LITTLE WORK",
            Self::Parcel => "PICTURE PARCEL",
            Self::Supplies => "CARRY & MATCH",
            Self::Stones => "TURN & MATCH",
            Self::Travel => "WHIP TRAVEL",
            Self::Glide => "YOUR CANOPY",
            Self::Flight => "CREATIVE FLIGHT",
            Self::FlowGarden => "FOLLOW THE WATER",
        }
    }
    fn instruction(self, stage: u8, touch: bool) -> &'static str {
        match (self, stage, touch) {
            (Self::Hotbar, 0, _) => {
                "Choose a block from the pictures. Building stock is unlimited."
            }
            (Self::Hotbar, 1, true) => {
                "Tap one of the six bottom slots to put that block in your hotbar."
            }
            (Self::Hotbar, 1, false) => {
                "Click a bottom slot or press 1–6 to put that block in your hotbar."
            }
            (Self::Hotbar, _, true) => "Tap Done. Choose your slot during play, then use Build.",
            (Self::Hotbar, _, false) => {
                "Press I to return. Choose your slot with 1–6, then right-click to build."
            }
            (Self::Building, 1, true) => {
                "Block placed! Aim at a block you want to remove, then tap Dig."
            }
            (Self::Building, 1, false) => {
                "Block placed! Aim at a block you want to remove, then left-click."
            }
            (Self::Building, 2, true) => {
                "Block removed! Choose a slot, aim at nearby ground and tap Build."
            }
            (Self::Building, 2, false) => {
                "Block removed! Choose 1–6, aim at nearby ground and right-click."
            }
            (Self::Building, _, true) => {
                "Aim close to your feet. Build adds your selected block; Dig removes one."
            }
            (Self::Building, _, false) => {
                "Aim at nearby ground. Right-click adds your selected block; left-click digs."
            }
            (Self::Map, 0, true) => {
                "Try Zoom + or pinch the map. Matching town pictures mark destinations."
            }
            (Self::Map, 0, false) => {
                "Scroll over the map to zoom, or drag it. Town pictures mark destinations."
            }
            (Self::Map, _, true) => {
                "Center on you finds your marker. Tap Return when you are ready."
            }
            (Self::Map, _, false) => {
                "C centers on you; R shows the whole island. Press M to return."
            }
            (Self::Cargo, _, _) => {
                "Cargo is for trade; building blocks stay unlimited. Review an offer, then Close."
            }
            (Self::Work, _, _) => {
                "Stay beside the work until its bar fills. Moving away cancels without a reward."
            }
            (Self::Parcel, 0, true) => {
                "Open Menu, then Map to match your parcel’s town picture. Deliver at its matching market."
            }
            (Self::Parcel, 0, false) => {
                "Press M to match your parcel’s town picture. At its matching market, T delivers for coins."
            }
            (Self::Parcel, _, true) => {
                "Find the matching market sign, then tap Deliver there. There is no time limit."
            }
            (Self::Parcel, _, false) => {
                "Find the matching market sign, then press T there. There is no time limit."
            }
            (Self::Supplies, 0, true) => {
                "Tap Take beside a loose supply. Show me demonstrates the matching place."
            }
            (Self::Supplies, 0, false) => {
                "T takes a loose supply. J demonstrates the matching place."
            }
            (Self::Supplies, _, true) => {
                "Carry to the matching outline, then Place. Return puts it back."
            }
            (Self::Supplies, _, false) => {
                "Carry it to its matching outline, then T. Backspace puts it safely back."
            }
            (Self::Stones, _, true) => {
                "Find its matching dotted picture along the route. Turn changes the stone; Hint guides."
            }
            (Self::Stones, _, false) => {
                "Find its matching dotted picture along the route. T turns; Y guides; J shows."
            }
            (Self::FlowGarden, _, true) => {
                "Turn channels. Join their open ends. Follow the water; Hint finds the break."
            }
            (Self::FlowGarden, _, false) => {
                "T turns channels. Connect their open ends; Y finds the break and J shows a turn."
            }
            (Self::Travel, 0, _) => {
                "Choose a reachable town or explorer to board. Up to four can ride; one is enough."
            }
            (Self::Travel, _, _) => {
                "When everyone is aboard, choose Launch. The carriage flies and lands for you."
            }
            (Self::Glide, 0, true) => {
                "Swipe to look where you want to glide. Looking down gains speed; up spends it."
            }
            (Self::Glide, 0, false) => {
                "Look where you want to glide. Looking down gains speed; looking up spends it."
            }
            (Self::Glide, _, true) => {
                "Try Brake or Dive. Touching the ground closes your canopy automatically."
            }
            (Self::Glide, _, false) => {
                "Hold Space to brake or Shift to dive. Landing closes your canopy automatically."
            }
            (Self::Flight, 0, true) => {
                "Fly lets you build in the air. Hold Rise or Fall to change height."
            }
            (Self::Flight, 0, false) => {
                "Creative flight helps you build. Hold E to rise or Q to descend."
            }
            (Self::Flight, _, true) => "Tap Fly again to return to ordinary movement and gravity.",
            (Self::Flight, _, false) => "Press F again to return to ordinary movement and gravity.",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Signal {
    Open(Lesson),
    ChooseBlock,
    AssignBlock,
    Close(Lesson),
    Edit(Block),
    ExploreMap,
    Finished(Lesson),
    Activity(ActivityAction, bool),
    Garden(bool),
}

#[derive(Resource, Default)]
pub(crate) struct Tutorials {
    completed: BTreeSet<Lesson>,
    stages: BTreeMap<Lesson, u8>,
    active: Option<Lesson>,
    path: Option<PathBuf>,
    name: String,
    dirty: bool,
    reset_pending: bool,
    previous: Observation,
    pub(crate) world_rect: Option<Rect>,
    finger: Option<(u64, Vec2)>,
}
#[derive(Default)]
struct Observation {
    inventory: bool,
    map: bool,
    cargo: bool,
    travel: bool,
    delivery: bool,
    work: bool,
    map_view: Option<(f32, Vec2)>,
    riding: bool,
    gliding: bool,
    flying: bool,
    yaw: f32,
    pitch: f32,
    height: f32,
}
impl Tutorials {
    pub(crate) fn signal(&mut self, event: Signal) {
        match event {
            Signal::Open(lesson) => {
                self.start(lesson);
                if lesson == Lesson::Map {
                    self.advance(Lesson::Parcel, 1);
                }
            }
            Signal::Finished(lesson) => self.finish(lesson),
            Signal::ChooseBlock => {
                self.start(Lesson::Hotbar);
                self.advance(Lesson::Hotbar, 1);
            }
            Signal::AssignBlock => self.advance(Lesson::Hotbar, 2),
            Signal::Close(lesson) => {
                if lesson == Lesson::Map
                    && self.stages.get(&Lesson::Parcel).is_some_and(|s| *s >= 1)
                {
                    self.finish(Lesson::Parcel);
                }
                let enough = match lesson {
                    Lesson::Hotbar => 2,
                    Lesson::Map => 1,
                    Lesson::Cargo => 0,
                    _ => u8::MAX,
                };
                if self.stages.get(&lesson).is_some_and(|s| *s >= enough) {
                    self.finish(lesson);
                }
            }
            Signal::ExploreMap => {
                self.advance(Lesson::Map, 1);
                self.advance(Lesson::Parcel, 1);
            }
            Signal::Edit(block) => {
                self.start(Lesson::Building);
                let stage = if block == Block::Air { 2 } else { 1 };
                if self
                    .stages
                    .get(&Lesson::Building)
                    .is_some_and(|s| *s != 0 && *s != stage)
                {
                    self.finish(Lesson::Building);
                } else {
                    self.advance(Lesson::Building, stage);
                }
            }
            Signal::Garden(complete) => {
                self.start(Lesson::FlowGarden);
                if complete {
                    self.finish(Lesson::FlowGarden);
                }
            }
            Signal::Activity(action, complete) => match action {
                ActivityAction::Take(_) => {
                    self.start(Lesson::Supplies);
                    self.advance(Lesson::Supplies, 1);
                }
                ActivityAction::Place(_) => self.finish(Lesson::Supplies),
                ActivityAction::Return => {
                    if self.stages.contains_key(&Lesson::Supplies) {
                        self.stages.insert(Lesson::Supplies, 0);
                    }
                }
                ActivityAction::Turn(_) => {
                    self.start(Lesson::Stones);
                    if complete {
                        self.finish(Lesson::Stones);
                    }
                }
                // Cargo repair has its own concrete cost/progress card. It is
                // not a carried-supply lesson or a creative-building action.
                ActivityAction::Contribute(_) | ActivityAction::Hammer => {}
            },
        }
    }
    fn start(&mut self, lesson: Lesson) {
        if !self.completed.contains(&lesson) {
            self.stages.entry(lesson).or_insert(0);
            self.active = Some(lesson);
        }
    }
    fn advance(&mut self, lesson: Lesson, stage: u8) {
        if let Some(s) = self.stages.get_mut(&lesson) {
            *s = (*s).max(stage);
        }
    }
    fn finish(&mut self, lesson: Lesson) {
        if self.stages.remove(&lesson).is_some() {
            self.completed.insert(lesson);
            self.dirty = true;
        }
        if self.active == Some(lesson) {
            self.active = None;
        }
    }
    pub(crate) fn incomplete(&self, lesson: Lesson) -> bool {
        self.stages.contains_key(&lesson)
    }
    pub(crate) fn reset(&mut self) {
        self.completed.clear();
        self.stages.clear();
        self.active = None;
        self.dirty = true;
        self.reset_pending = true;
        self.world_rect = None;
    }
    fn visible(&self, context: Context) -> Option<Lesson> {
        self.active
            .filter(|lesson| self.stages.contains_key(lesson) && context.supports(*lesson))
            .or_else(|| {
                self.stages
                    .keys()
                    .copied()
                    .find(|lesson| context.supports(*lesson))
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
    World,
    Inventory,
    Map,
    Market,
    Travel,
    Activity,
}
impl Context {
    fn supports(self, lesson: Lesson) -> bool {
        match self {
            Self::Inventory => lesson == Lesson::Hotbar,
            Self::Map => lesson == Lesson::Map,
            Self::Market => matches!(lesson, Lesson::Cargo | Lesson::Work | Lesson::Parcel),
            Self::Travel => lesson == Lesson::Travel,
            Self::Activity => matches!(
                lesson,
                Lesson::Supplies | Lesson::Stones | Lesson::FlowGarden
            ),
            Self::World => matches!(
                lesson,
                Lesson::Building
                    | Lesson::Work
                    | Lesson::Parcel
                    | Lesson::Travel
                    | Lesson::Glide
                    | Lesson::Flight
            ),
        }
    }
}
#[derive(Component)]
pub(crate) struct Banner(Context);
#[derive(Component)]
pub(crate) struct Copy(Context);
#[derive(Component)]
pub(crate) struct Skip(Context);

pub(crate) fn panel(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, context: Context) {
    parent
        .spawn((
            Banner(context),
            Node {
                display: Display::None,
                width: if context == Context::Map {
                    Val::Auto
                } else {
                    percent(100)
                },
                flex_grow: if context == Context::Map { 1. } else { 0. },
                flex_basis: if context == Context::Map {
                    px(0)
                } else {
                    Val::Auto
                },
                min_width: px(0),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::all(px(6)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.72, 0.58, 0.29)),
            BackgroundColor(Color::srgb(0.10, 0.21, 0.18)),
        ))
        .with_children(|row| {
            row.spawn((
                Copy(context),
                Text::new(""),
                TextFont::from_font_size(15.).with_font(font.clone()),
                TextColor(Color::srgb(0.96, 0.91, 0.75)),
                Node {
                    flex_grow: 1.,
                    flex_basis: px(0),
                    min_width: px(0),
                    ..default()
                },
            ));
            row.spawn((
                Skip(context),
                Button,
                ActivateOnPress,
                Hovered::default(),
                Node {
                    min_width: px(48),
                    min_height: px(44),
                    padding: UiRect::all(px(6)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.20, 0.34, 0.29)),
            ))
            .with_child((
                Text::new("Skip"),
                TextFont::from_font_size(14.).with_font(font.clone()),
                TextColor(Color::srgb(0.96, 0.91, 0.75)),
            ));
        });
}

pub(crate) fn setup(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut tutorials: ResMut<Tutorials>,
    mut fonts: ResMut<Assets<Font>>,
) {
    if session.observer.is_none() {
        session.help = false;
        session.inspector = false;
    }
    let name = session
        .players
        .iter()
        .find(|p| p.id == session.id)
        .map_or("Explorer", |p| p.name.as_str())
        .trim()
        .to_lowercase();
    *tutorials = Tutorials {
        name,
        path: Some(PathBuf::from(FILE)),
        ..default()
    };
    match load(Path::new(FILE)) {
        Ok(saved) => {
            tutorials.completed = saved
                .profiles
                .get(&tutorials.name)
                .cloned()
                .unwrap_or_default()
        }
        Err(error) => warn!("Could not load tutorials: {error}; original file retained"),
    }
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            WorldCard,
            GlobalZIndex(30),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: px(320),
                top: px(116),
                left: px(28),
                ..default()
            },
        ))
        .with_children(|root| panel(root, &font, Context::World));
}
#[derive(Component)]
pub(crate) struct WorldCard;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    mut tutorials: ResMut<Tutorials>,
    mut session: ResMut<Session>,
    touch: Res<TouchControls>,
    map: Res<crate::world_map::WorldMap>,
    market: Res<crate::market::MarketPanel>,
    travel: Res<crate::airships::PilotConversation>,
    pause: Res<crate::pause::PauseMenu>,
    console: Res<crate::admin_console::AdminConsole>,
    connection: Res<crate::network::Connection>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut nodes: ParamSet<(
        Query<&mut Node, With<WorldCard>>,
        Query<(&Banner, &mut Node)>,
        Query<&mut Node, With<crate::activities::CardTitle>>,
    )>,
    activities: Option<Res<crate::activities::Scene>>,
    mut text: Query<(&Copy, &mut Text)>,
) {
    let s = &*session;
    let t = &mut *tutorials;
    let gameplay = window.focused
        && !touch.suspended
        && !pause.input_blocked
        && !console.input_blocked
        && !s.inventory.input_blocked
        && !map.input_blocked
        && !market.input_blocked
        && !travel.input_blocked
        && !s.help
        && !s.inspector;
    if s.observer.is_none() && connection.error.is_none() {
        for (lesson, open, old) in [
            (Lesson::Hotbar, s.inventory.open, t.previous.inventory),
            (Lesson::Map, map.open, t.previous.map),
            (Lesson::Cargo, market.open, t.previous.cargo),
            (Lesson::Travel, travel.open(), t.previous.travel),
        ] {
            if open && !old {
                t.signal(Signal::Open(lesson));
            }
            if !open && old {
                t.signal(Signal::Close(lesson));
            }
        }
        if map.open
            && t.previous.map
            && t.previous
                .map_view
                .is_some_and(|old| old != map.tutorial_view())
        {
            t.signal(Signal::ExploreMap);
        }
        if market.delivery().is_some() && !t.previous.delivery {
            t.start(Lesson::Parcel);
        }
        if market.active_work().is_some() && !t.previous.work {
            t.start(Lesson::Work);
        }
        if s.glider_ride.is_some() && !t.previous.riding {
            t.start(Lesson::Travel);
            t.advance(Lesson::Travel, 1);
        }
        if s.glider_ride
            .and_then(|ride| s.gliders.iter().find(|f| f.id == ride.carriage_id))
            .is_some_and(|f| f.started_at.is_some())
        {
            t.finish(Lesson::Travel);
        }
        if s.gliding && !t.previous.gliding {
            t.start(Lesson::Glide);
        }
        if gameplay
            && s.gliding
            && t.previous.gliding
            && ((s.yaw - t.previous.yaw).abs() > 0.01 || (s.pitch - t.previous.pitch).abs() > 0.01)
        {
            t.advance(Lesson::Glide, 1);
        }
        if gameplay
            && s.gliding
            && t.previous.gliding
            && t.stages.get(&Lesson::Glide) == Some(&1)
            && (touch.jump
                || touch.sprint
                || keys.pressed(KeyCode::Space)
                || keys.pressed(KeyCode::ShiftLeft)
                || keys.pressed(KeyCode::ShiftRight))
        {
            t.finish(Lesson::Glide);
        }
        if s.flying && !t.previous.flying {
            t.start(Lesson::Flight);
            t.previous.height = s.body.position[1];
        }
        if s.flying && (s.body.position[1] - t.previous.height).abs() > 1. {
            t.advance(Lesson::Flight, 1);
        }
        if !s.flying && t.previous.flying && t.stages.get(&Lesson::Flight) == Some(&1) {
            t.finish(Lesson::Flight);
        }
    }
    t.previous.map_view = map.open.then(|| map.tutorial_view());
    t.previous.inventory = s.inventory.open;
    t.previous.map = map.open;
    t.previous.cargo = market.open;
    t.previous.travel = travel.open();
    t.previous.delivery = market.delivery().is_some();
    t.previous.work = market.active_work().is_some();
    t.previous.riding = s.glider_ride.is_some();
    t.previous.gliding = s.gliding;
    t.previous.flying = s.flying;
    t.previous.yaw = s.yaw;
    t.previous.pitch = s.pitch;
    let blocked = !window.focused
        || touch.suspended
        || s.observer.is_some()
        || connection.error.is_some()
        || pause.open
        || pause.input_blocked
        || console.input_blocked
        || ((s.help || s.inspector)
            && !s.inventory.open
            && !map.open
            && !market.open
            && !travel.open());
    let context = if s.inventory.open {
        Context::Inventory
    } else if map.open {
        Context::Map
    } else if market.open {
        Context::Market
    } else if travel.open() {
        Context::Travel
    } else if t.active.is_some_and(|l| Context::Activity.supports(l)) {
        Context::Activity
    } else {
        Context::World
    };
    let blocked = blocked
        || (context == Context::Activity
            && activities
                .as_ref()
                .is_some_and(|scene| scene.demonstrating()));
    let visible = (!blocked && !(context == Context::World && session.building.enabled))
        .then(|| t.visible(context))
        .flatten()
        .filter(|lesson| match lesson {
            Lesson::Work => market.active_work().is_some(),
            Lesson::Parcel => market.delivery().is_some(),
            Lesson::Travel => travel.open() || s.glider_ride.is_some(),
            Lesson::Glide => s.gliding,
            Lesson::Flight => s.flying,
            _ => true,
        });
    if let Some(lesson) = visible {
        t.active = Some(lesson);
    }
    for mut root in &mut nodes.p0() {
        root.display = if visible.is_some() && context == Context::World {
            Display::Flex
        } else {
            Display::None
        };
        root.left = px(if touch.enabled { 210. } else { 28. });
        root.top = px(if touch.enabled { 100. } else { 116. });
        root.width = px(if touch.enabled { 300. } else { 320. });
    }
    for (banner, mut node) in &mut nodes.p1() {
        node.display = if banner.0 == context && visible.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut title in &mut nodes.p2() {
        title.display = if context == Context::Activity && visible.is_some() {
            Display::None
        } else {
            Display::Flex
        };
    }
    for (copy, mut text) in &mut text {
        if copy.0 == context
            && let Some(lesson) = visible
        {
            let stage = t.stages[&lesson];
            let body = lesson.instruction(stage, touch.enabled);
            let value = format!("{}\n{}", lesson.title(), body);
            if text.0 != value {
                text.0 = value;
            }
        }
    }
    if t.dirty {
        t.dirty = false;
        if let Some(path) = &t.path {
            if let Err(error) = save(path, &t.name, &t.completed, t.reset_pending) {
                warn!("Could not save tutorials: {error}");
                session.status =
                    "Tutorial preferences could not be saved; they may reappear next time.".into();
                session.status_until = time.elapsed_secs_f64() + 5.;
            } else {
                t.reset_pending = false;
            }
        }
    }
}

pub(crate) fn capture_region(
    mut tutorials: ResMut<Tutorials>,
    geometry: Query<(&Banner, &ComputedNode, &UiGlobalTransform)>,
) {
    tutorials.world_rect = geometry
        .iter()
        .find(|(banner, node, _)| {
            matches!(banner.0, Context::World | Context::Activity) && node.size().min_element() > 0.
        })
        .map(|(_, node, transform)| {
            Rect::from_center_size(
                transform.translation * node.inverse_scale_factor,
                node.size() * node.inverse_scale_factor,
            )
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read(
    mut tutorials: ResMut<Tutorials>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<TouchInput>,
    buttons: Query<&Skip, Changed<crate::ui::Activated>>,
    targets: Query<(
        &Skip,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    window: Single<&Window, With<PrimaryWindow>>,
    touch: Res<TouchControls>,
) {
    if !window.focused || touch.suspended {
        tutorials.finger = None;
        events.clear();
        return;
    }
    let active_context = tutorials.active.and_then(|lesson| {
        targets
            .iter()
            .find(|(skip, node, _, visibility)| {
                visibility.get() && node.size().min_element() > 0. && skip.0.supports(lesson)
            })
            .map(|(skip, ..)| skip.0)
    });
    let mut skip = active_context.is_some() && keys.just_pressed(KeyCode::F3);
    let mut raw = tutorials.finger.is_some();
    for event in events.read() {
        raw = true;
        let point = event.position * window.scale_factor();
        let hit = targets.iter().any(|(s, node, transform, visibility)| {
            visibility.get()
                && Some(s.0) == active_context
                && node.contains_point(*transform, point)
        });
        match event.phase {
            TouchPhase::Started if hit => tutorials.finger = Some((event.id, event.position)),
            TouchPhase::Ended if tutorials.finger.is_some_and(|(id, _)| id == event.id) => {
                skip |= hit && tutorials.finger.unwrap().1.distance(event.position) < 12.;
                tutorials.finger = None;
            }
            TouchPhase::Canceled if tutorials.finger.is_some_and(|(id, _)| id == event.id) => {
                tutorials.finger = None
            }
            _ => {}
        }
    }
    if !raw {
        skip |= buttons.iter().any(|s| Some(s.0) == active_context);
    }
    if skip && let Some(lesson) = tutorials.active {
        tutorials.finish(lesson);
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    version: u8,
    profiles: BTreeMap<String, BTreeSet<Lesson>>,
}
fn load(path: &Path) -> io::Result<Saved> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(file) => {
            file.take(65537).read_to_end(&mut bytes)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Saved {
                version: 1,
                ..default()
            });
        }
        Err(e) => return Err(e),
    }
    let saved: Saved = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if bytes.len() > 65536
        || saved.version != 1
        || saved.profiles.len() > 128
        || saved.profiles.keys().any(|name| {
            name.is_empty() || name.chars().count() > 72 || name.chars().any(char::is_control)
        })
    {
        return Err(io::Error::other("invalid tutorial preferences"));
    }
    Ok(saved)
}
fn save(path: &Path, name: &str, completed: &BTreeSet<Lesson>, reset: bool) -> io::Result<()> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("json.lock"))?;
    lock.try_lock()?;
    let mut saved = load(path)?;
    if !saved.profiles.contains_key(name) && saved.profiles.len() == 128 {
        return Err(io::Error::other("tutorial profile slots are full"));
    }
    let entry = saved.profiles.entry(name.into()).or_default();
    if reset {
        entry.clear();
    }
    entry.extend(completed);
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        serde_json::to_writer(&mut file, &saved).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn garden_teaches_water_connections_without_starting_the_stone_lesson() {
        let mut t = Tutorials::default();
        t.signal(Signal::Garden(false));
        assert_eq!(t.visible(Context::Activity), Some(Lesson::FlowGarden));
        assert!(!t.stages.contains_key(&Lesson::Stones));
        assert!(
            Lesson::FlowGarden
                .instruction(0, true)
                .contains("open ends")
        );
        t.signal(Signal::Garden(true));
        assert!(t.completed.contains(&Lesson::FlowGarden));
        t.signal(Signal::Garden(false));
        assert!(
            t.visible(Context::Activity).is_none(),
            "Later experiments do not repeat a completed lesson"
        );
    }

    #[test]
    fn hotbar_requires_choose_assign_and_close_and_skip_is_remembered() {
        let mut t = Tutorials::default();
        t.signal(Signal::Open(Lesson::Hotbar));
        t.signal(Signal::Close(Lesson::Hotbar));
        assert!(!t.completed.contains(&Lesson::Hotbar));
        t.signal(Signal::ChooseBlock);
        t.signal(Signal::Close(Lesson::Hotbar));
        assert_eq!(t.stages[&Lesson::Hotbar], 1);
        t.signal(Signal::AssignBlock);
        t.signal(Signal::Close(Lesson::Hotbar));
        assert!(t.completed.contains(&Lesson::Hotbar));
        t.signal(Signal::Open(Lesson::Hotbar));
        assert!(t.active.is_none());
        t.start(Lesson::Cargo);
        t.finish(Lesson::Cargo);
        t.start(Lesson::Cargo);
        assert!(t.active.is_none());
        t.reset();
        t.start(Lesson::Cargo);
        assert_eq!(t.active, Some(Lesson::Cargo));
    }

    #[test]
    fn lessons_resume_independently_after_another_mechanic_and_return_is_not_success() {
        let mut t = Tutorials::default();
        t.signal(Signal::Activity(ActivityAction::Take(0), false));
        t.start(Lesson::Map);
        assert_eq!(t.visible(Context::Activity), Some(Lesson::Supplies));
        t.signal(Signal::Activity(ActivityAction::Return, false));
        assert_eq!(t.stages[&Lesson::Supplies], 0);
        assert!(!t.completed.contains(&Lesson::Supplies));
        t.signal(Signal::Activity(ActivityAction::Take(1), false));
        t.signal(Signal::Activity(ActivityAction::Place(1), false));
        assert!(t.completed.contains(&Lesson::Supplies));
        t.signal(Signal::Activity(ActivityAction::Turn(0), false));
        assert!(!t.completed.contains(&Lesson::Stones));
        t.signal(Signal::Activity(ActivityAction::Turn(2), true));
        assert!(t.completed.contains(&Lesson::Stones));
        t.signal(Signal::Finished(Lesson::Work));
        assert!(!t.completed.contains(&Lesson::Work));
    }

    #[test]
    fn building_requires_both_confirmed_actions_and_map_requires_exploration() {
        let mut t = Tutorials::default();
        t.start(Lesson::Building);
        assert_eq!(t.stages[&Lesson::Building], 0);
        t.signal(Signal::Edit(Block::Wood));
        t.signal(Signal::Edit(Block::Stone));
        assert!(!t.completed.contains(&Lesson::Building));
        t.signal(Signal::Edit(Block::Air));
        assert!(t.completed.contains(&Lesson::Building));
        t.start(Lesson::Map);
        t.signal(Signal::Close(Lesson::Map));
        assert!(!t.completed.contains(&Lesson::Map));
        t.signal(Signal::ExploreMap);
        t.signal(Signal::Close(Lesson::Map));
        assert!(t.completed.contains(&Lesson::Map));
    }

    #[test]
    fn parcel_lesson_ends_after_finding_the_map_without_requiring_the_whole_trip() {
        let mut t = Tutorials::default();
        t.start(Lesson::Parcel);
        t.signal(Signal::Open(Lesson::Map));
        assert!(!t.completed.contains(&Lesson::Parcel));
        t.signal(Signal::Close(Lesson::Map));
        assert!(t.completed.contains(&Lesson::Parcel));
        assert!(!t.completed.contains(&Lesson::Map));
        assert!(t.stages.contains_key(&Lesson::Map));
    }

    #[test]
    fn preferences_preserve_other_characters_merge_writers_and_replay_only_selected_character() {
        let directory =
            std::env::temp_dir().join(format!("rubblekin-tutorials-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(FILE);
        assert!(load(&path).unwrap().profiles.is_empty());
        save(&path, "ian", &BTreeSet::from([Lesson::Hotbar]), false).unwrap();
        save(&path, "violet", &BTreeSet::from([Lesson::Map]), false).unwrap();
        save(&path, "ian", &BTreeSet::from([Lesson::Building]), false).unwrap();
        let saved = load(&path).unwrap();
        assert_eq!(
            saved.profiles["ian"],
            BTreeSet::from([Lesson::Hotbar, Lesson::Building])
        );
        assert_eq!(saved.profiles["violet"], BTreeSet::from([Lesson::Map]));
        save(&path, "ian", &BTreeSet::new(), true).unwrap();
        assert!(load(&path).unwrap().profiles["ian"].is_empty());
        let original = fs::read(&path).unwrap();
        fs::create_dir(path.with_extension(format!("json.{}.tmp", std::process::id()))).unwrap();
        assert!(save(&path, "ian", &BTreeSet::from([Lesson::Flight]), false).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_dir(path.with_extension(format!("json.{}.tmp", std::process::id()))).unwrap();
        for bad in [
            b"broken".as_slice(),
            b"{\"version\":2,\"profiles\":{}}",
            b"{\"version\":1,\"profiles\":{\"ian\":[\"Unknown\"]}}",
        ] {
            fs::write(&path, bad).unwrap();
            assert!(save(&path, "ian", &BTreeSet::new(), false).is_err());
            assert_eq!(fs::read(&path).unwrap(), bad);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    fn app() -> (App, std::net::TcpStream) {
        use rubblekin_core::protocol::SessionMode;
        use std::{io::Write, net::TcpListener};
        let welcome = crate::join::tests::welcome(SessionMode::Player);
        let (_, mut session) = crate::join::session_from_welcome(
            welcome.clone(),
            "tutorial test".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.help = false;
        session.inspector = false;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let client = std::thread::spawn(move || {
            crate::network::Connection::connect(
                &address,
                "Tutorial test".into(),
                SessionMode::Player,
            )
            .unwrap()
            .0
        });
        let (mut peer, _) = listener.accept().unwrap();
        let mut bytes = serde_json::to_vec(&welcome).unwrap();
        bytes.push(b'\n');
        peer.write_all(&bytes).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(client.join().unwrap())
            .init_resource::<Tutorials>()
            .init_resource::<TouchControls>()
            .init_resource::<Time>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::market::MarketPanel>()
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::admin_console::AdminConsole>()
            .init_resource::<crate::airships::PilotConversation>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<TouchInput>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        for context in [
            Context::World,
            Context::Inventory,
            Context::Map,
            Context::Market,
            Context::Travel,
            Context::Activity,
        ] {
            app.world_mut().spawn((Banner(context), Node::default()));
            app.world_mut().spawn((Copy(context), Text::new("")));
        }
        app.world_mut().spawn((WorldCard, Node::default()));
        (app, peer)
    }

    #[test]
    fn opening_real_panels_starts_one_lesson_and_pause_hides_without_losing_it() {
        let (mut app, _peer) = app();
        app.update();
        assert!(app.world().resource::<Tutorials>().active.is_none());
        app.world_mut().resource_mut::<Session>().inventory.open = true;
        app.update();
        assert_eq!(
            app.world().resource::<Tutorials>().active,
            Some(Lesson::Hotbar)
        );
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = true;
        app.update();
        assert_eq!(
            app.world().resource::<Tutorials>().active,
            Some(Lesson::Hotbar)
        );
        let world = app.world_mut();
        let mut query = world.query::<(&Banner, &Node)>();
        assert!(query.iter(world).all(|(_, n)| n.display == Display::None));
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = false;
        app.world_mut().resource_mut::<Session>().inventory.open = false;
        app.world_mut()
            .resource_mut::<crate::world_map::WorldMap>()
            .open = true;
        app.update();
        assert_eq!(
            app.world().resource::<Tutorials>().active,
            Some(Lesson::Map)
        );
        assert!(
            !app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Hotbar)
        );
    }

    #[test]
    fn confirmed_box_fill_keeps_the_ordinary_dig_lesson_hidden_until_selection_exits() {
        let (mut app, _peer) = app();
        app.world_mut().resource_mut::<Session>().building.enabled = true;
        app.world_mut()
            .resource_mut::<Tutorials>()
            .signal(Signal::Edit(Block::Wood));
        app.update();
        let world = app.world_mut();
        let mut cards = world.query_filtered::<&Node, With<WorldCard>>();
        assert_eq!(cards.single(world).unwrap().display, Display::None);
        app.world_mut().resource_mut::<Session>().building.enabled = false;
        app.update();
        let world = app.world_mut();
        assert_eq!(cards.single(world).unwrap().display, Display::Flex);
        assert!(
            !world
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Building)
        );
    }

    #[test]
    fn glider_keys_in_a_menu_cannot_complete_the_flying_lesson() {
        let (mut app, _peer) = app();
        app.world_mut().resource_mut::<Session>().gliding = true;
        app.update();
        app.world_mut()
            .resource_mut::<Tutorials>()
            .advance(Lesson::Glide, 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.world_mut()
            .resource_mut::<crate::world_map::WorldMap>()
            .input_blocked = true;
        app.update();
        assert!(
            !app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Glide)
        );
        app.world_mut()
            .resource_mut::<crate::world_map::WorldMap>()
            .input_blocked = false;
        app.update();
        assert!(
            app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Glide)
        );
    }

    #[test]
    fn jump_out_and_the_carriage_yaw_change_do_not_skip_the_canopy_lesson() {
        let (mut app, _peer) = app();
        app.world_mut().resource_mut::<Session>().gliding = true;
        app.world_mut().resource_mut::<Session>().yaw = 1.;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        assert_eq!(
            app.world().resource::<Tutorials>().stages[&Lesson::Glide],
            0
        );
        assert!(
            !app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Glide)
        );
        app.update();
        assert_eq!(
            app.world().resource::<Tutorials>().stages[&Lesson::Glide],
            0
        );
    }

    #[test]
    fn canceling_a_touch_on_skip_does_not_dismiss_and_a_tap_does() {
        let (mut app, _peer) = app();
        app.add_systems(Update, read.before(update));
        app.world_mut()
            .resource_mut::<Tutorials>()
            .start(Lesson::Building);
        app.world_mut().spawn((
            Skip(Context::World),
            ComputedNode {
                size: Vec2::splat(48.),
                ..default()
            },
            UiGlobalTransform::from_xy(240., 120.),
            InheritedVisibility::VISIBLE,
        ));
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        let send = |app: &mut App, phase| {
            app.world_mut().write_message(TouchInput {
                id: 1,
                phase,
                position: Vec2::new(240., 120.),
                window,
                force: None,
            });
            app.update();
        };
        send(&mut app, TouchPhase::Started);
        send(&mut app, TouchPhase::Canceled);
        assert!(
            !app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Building)
        );
        send(&mut app, TouchPhase::Started);
        send(&mut app, TouchPhase::Ended);
        assert!(
            app.world()
                .resource::<Tutorials>()
                .completed
                .contains(&Lesson::Building)
        );
    }

    #[test]
    fn swiping_the_tutorial_does_not_steer_or_edit_the_world() {
        let (mut app, _peer) = app();
        app.init_resource::<ButtonInput<MouseButton>>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>()
            .add_systems(Update, crate::touch::read.before(update));
        app.world_mut().resource_mut::<TouchControls>().enabled = true;
        app.world_mut().resource_mut::<Tutorials>().world_rect = Some(Rect::from_corners(
            Vec2::new(210., 100.),
            Vec2::new(510., 200.),
        ));
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        for (phase, position) in [
            (TouchPhase::Started, Vec2::new(450., 130.)),
            (TouchPhase::Moved, Vec2::new(470., 150.)),
            (TouchPhase::Ended, Vec2::new(480., 160.)),
        ] {
            app.world_mut().write_message(TouchInput {
                id: 2,
                phase,
                position,
                window,
                force: None,
            });
            app.update();
            let controls = app.world().resource::<TouchControls>();
            assert_eq!(controls.look, Vec2::ZERO);
            assert!(!controls.build && !controls.dig && !controls.activity);
        }
    }
}
