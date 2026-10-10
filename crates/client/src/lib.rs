mod activities;
mod admin_console;
mod airship_mesh;
#[cfg(test)]
mod airship_motion_tests;
#[allow(dead_code)]
mod airships;
mod block_textures;
mod capture;
mod cart_repairs;
mod crash_reporting;
mod crops;
mod flow_gardens;
mod follow_camera;
mod forage;
mod gliders;
mod graphics;
mod ground_details;
mod inspection;
mod inspection_details;
mod inventory;
mod join;
mod join_preferences;
mod map_activities;
mod market;
mod network;
mod observer;
mod palette;
mod parcels;
mod pause;
mod platform;
mod prediction;
mod profile;
mod sky;
mod terrain;
mod terrain_albedo;
mod terrain_material;
mod touch;
mod trade_pictures;
mod tutorials;
mod ui;
mod vehicles;
mod village_details;
mod wildlife;
mod work_animation;
mod work_cues;
mod work_tools;
mod world_map;
mod world_map_image;

#[cfg(test)]
mod admin_integration_tests;
#[cfg(test)]
mod avatar_tests;
#[cfg(test)]
mod inspection_tests;
#[cfg(test)]
mod movement_sync_tests;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    light::{DirectionalLightShadowMap, NotShadowCaster},
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use follow_camera::CameraFollow;
use graphics::{GraphicsQuality, GraphicsSettings};
use network::Connection;
use observer::ObserverCamera;
use prediction::Prediction;
use rubblekin_core::{
    airships::{AirshipNetwork, AirshipRide, deck_position, initial_deck_position},
    physics::{
        Body, EYE_HEIGHT, MoveInput, character_position_is_clear, resolve_character_overlaps,
    },
    protocol::*,
    world::{Block, BlockPos, CELL_SIZE, World as GameWorld},
};
use rubblekin_server::ServerConfig;
use std::{collections::HashMap, path::PathBuf};

pub use palette::MATERIALS as PALETTE;

#[derive(Resource)]
pub struct VoxelWorld(pub GameWorld);

#[derive(Resource)]
pub struct Session {
    pub(crate) parcel_market: Option<[f32; 3]>,
    pub activities: Vec<rubblekin_core::activities::ActivitySnapshot>,
    pub id: u64,
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub camera_distance: f32,
    pub selected: usize,
    pub hotbar: [Block; palette::QUICK_SLOTS],
    pub inventory: inventory::Inventory,
    pub flying: bool,
    pub captured: bool,
    pub can_admin: bool,
    pub inspector: bool,
    pub(crate) inspected: Option<inspection::InspectTarget>,
    inspect_requested: bool,
    pub help: bool,
    pub graphics: GraphicsQuality,
    pub npc: NpcSnapshot,
    pub residents: Vec<ResidentSnapshot>,
    pub villages: Vec<VillageSnapshot>,
    pub wildlife: Vec<rubblekin_core::wildlife::WildlifeSnapshot>,
    pub habitats: Vec<rubblekin_core::wildlife::HabitatSnapshot>,
    pub players: Vec<PlayerSnapshot>,
    pub world_time: f64,
    pub(crate) airships: AirshipNetwork,
    pub(crate) whip_stations: Vec<rubblekin_core::gliders::WhipStation>,
    pub(crate) gliders: Vec<rubblekin_core::gliders::GliderFlight>,
    pub(crate) glider_ride: Option<rubblekin_core::gliders::GliderRide>,
    pub(crate) gliding: bool,
    pub(crate) vehicle: Option<rubblekin_core::vehicles::Vehicle>,
    pub(crate) ride: Option<AirshipRide>,
    pub(crate) deck_position: Option<[f32; 3]>,
    pub(crate) airship_clock: airships::AirshipClock,
    pub status: String,
    pub status_until: f64,
    pub target: Option<(BlockPos, BlockPos)>,
    pub fps: f64,
    pub edits: usize,
    pub connected_to: String,
    pub observer: Option<ObserverCamera>,
    prediction: Prediction,
    edit_clock: f32,
    next_request: u64,
}

fn character_obstacles(
    own_id: u64,
    players: &[PlayerSnapshot],
    npc: &NpcSnapshot,
    residents: &[ResidentSnapshot],
    airships: &AirshipNetwork,
    airship_time: f64,
) -> Vec<[f32; 3]> {
    players
        .iter()
        .filter(|player| player.id != own_id)
        .flat_map(|player| {
            let position = player
                .ride
                .and_then(|ride| {
                    airships.ship(ride.ship_id, airship_time).map(|ship| {
                        deck_position(
                            &ship,
                            player
                                .deck_position
                                .unwrap_or(initial_deck_position(ride.seat)),
                        )
                    })
                })
                .unwrap_or(player.body.position);
            rubblekin_core::vehicles::obstacle_positions(position, player.vehicle)
        })
        .chain(std::iter::once(npc.position))
        .chain(residents.iter().map(|resident| {
            resident
                .ride
                .and_then(|ride| {
                    airships.ship(ride.ship_id, airship_time).map(|ship| {
                        deck_position(
                            &ship,
                            resident
                                .deck_position
                                .unwrap_or(initial_deck_position(ride.seat)),
                        )
                    })
                })
                .unwrap_or(resident.position)
        }))
        .collect()
}

#[derive(Resource, Default)]
struct Avatars {
    players: HashMap<u64, Entity>,
    player_epochs: HashMap<u64, u64>,
    player_deck_motion: HashMap<u64, DeckMotion>,
    resident_deck_motion: HashMap<u64, DeckMotion>,
    npc: Option<Entity>,
    residents: HashMap<u64, Entity>,
}

#[derive(Default)]
struct DeckMotion {
    ship_id: Option<u64>,
    position: [f32; 3],
    moving_until: f64,
}
impl DeckMotion {
    fn walking(&mut self, ride: AirshipRide, local: [f32; 3], now: f64) -> bool {
        if self.ship_id != Some(ride.ship_id) {
            self.ship_id = Some(ride.ship_id);
            self.moving_until = 0.0;
        } else if Vec2::new(local[0], local[2])
            .distance_squared(Vec2::new(self.position[0], self.position[2]))
            > 0.000001
        {
            // Remote snapshots arrive less often than render frames. Preserve
            // the walking pose briefly between local deck position updates.
            self.moving_until = now + 0.15;
        }
        self.position = local;
        now < self.moving_until
    }
}
#[derive(Component)]
struct GameEntity;
#[derive(Component)]
struct GameCamera;
#[derive(Component)]
struct Avatar;
#[derive(Component)]
struct GroundShadow;
#[derive(Component)]
struct CarriedGoods;
#[derive(Component)]
struct Limb {
    phase: f32,
    arm: bool,
}
#[derive(Default)]
struct Options {
    connect: Option<String>,
    local: bool,
    bind: Option<String>,
    save: Option<PathBuf>,
    name: Option<String>,
    screenshot: Option<String>,
    exit_after: Option<f32>,
    graphics: Option<GraphicsQuality>,
    seed: Option<u32>,
    generation: Option<rubblekin_core::world::WorldGeneration>,
    observe: bool,
    touch: bool,
}

fn options() -> Result<Options, String> {
    let mut result = Options::default();
    #[cfg(target_os = "android")]
    let mut args = std::iter::empty::<String>();
    #[cfg(not(target_os = "android"))]
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--local" => result.local = true,
            "--observe" => result.observe = true,
            "--touch" => result.touch = true,
            "--connect" => result.connect = Some(args.next().ok_or("--connect needs host:port")?),
            "--bind" => result.bind = Some(args.next().ok_or("--bind needs host:port")?),
            "--save" => result.save = Some(args.next().ok_or("--save needs a file path")?.into()),
            "--name" => result.name = Some(args.next().ok_or("--name needs a name")?),
            "--seed" => {
                result.seed = Some(
                    args.next()
                        .ok_or("--seed needs a number")?
                        .parse()
                        .map_err(|_| "Invalid seed")?,
                )
            }
            "--generation" => {
                result.generation = Some(match args.next().as_deref() {
                    Some("v3") => rubblekin_core::world::WorldGeneration::GeographyV3,
                    Some("v4") => rubblekin_core::world::WorldGeneration::GeographyV4,
                    Some("v5") => rubblekin_core::world::WorldGeneration::GeographyV5,
                    Some("v6") => rubblekin_core::world::WorldGeneration::GeographyV6,
                    _ => {
                        return Err(
                            "--generation needs v3, v4, v5 or v6 (new local worlds only)".into(),
                        );
                    }
                });
            }
            "--screenshot" => {
                result.screenshot = Some(args.next().ok_or("--screenshot needs a path")?)
            }
            "--exit-after" => {
                let seconds: f32 = args
                    .next()
                    .ok_or("--exit-after needs seconds")?
                    .parse()
                    .map_err(|_| "Invalid duration")?;
                if !seconds.is_finite() || seconds < 1.0 {
                    return Err("Duration must be at least one second".into());
                }
                result.exit_after = Some(seconds);
            }
            "--low" => result.graphics = Some(GraphicsQuality::Low),
            "--balanced" => result.graphics = Some(GraphicsQuality::Balanced),
            "--high" => result.graphics = Some(GraphicsQuality::High),
            "--help" | "-h" => {
                println!(
                    "Rubblekin — a living voxel world\n\nRun without arguments to choose a server or local world.\n  --local              Start and join your local world immediately\n  --connect HOST:PORT   Join an existing server\n  --observe            Read-only admin camera; no player avatar\n  --touch              Preview on-screen touch controls\n  --bind HOST:PORT      Local host address (default 127.0.0.1:7878)\n  --save PATH           World save (default saves/villages.json)\n  --name NAME           Your saved character name\n  --seed NUMBER         Seed for a new world (default 42)\n  --generation v3|v4|v5|v6 Generator for a new local world (default v6)\n  --low                 Baked shading and character ground shadows\n  --balanced            Nearby sun shadows, no MSAA (default)\n  --high                Longer shadows and 4x MSAA\n  --screenshot PATH     Capture after 8 seconds in a joined scene\n  --exit-after SECONDS  Exit after this many seconds of app time\n\nWASD move | mouse look after click | Space jump | Shift sprint\nLeft click dig | Right click build | 1–6 hotbar slot | I inventory | F creative flight\nQ/E lower/raise in flight | scroll zoom | Tab inspect aimed character/block/plot | M world map\nB cargo / work / village market | G whip station / travel | V vehicles | F2 graphics | F6/F7/F8 forager override | F9 reset needs | F12 screenshot\nObserver: WASD fly | Q/E vertical | Shift boost | scroll speed | R / Home return | V next village\nBackquote / tilde admin commands | Escape pause menu | F10 leave world | H controls | close window to quit"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("Unknown option: {arg}. Use --help.")),
        }
    }
    if result.local && result.connect.is_some() {
        return Err("Choose either --local or --connect, not both".into());
    }
    Ok(result)
}

// Bevy's AndroidApp is a OnceLock. Each native game lifecycle must therefore
// finish its process rather than let a second GameActivity reuse a freed app.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android_app: bevy::android::android_activity::AndroidApp) {
    if bevy::android::ANDROID_APP.set(android_app).is_err() {
        finish_android_process(1);
    }
    let result = std::panic::catch_unwind(main);
    // The panic hook reports and flushes before unwinding reaches this point.
    finish_android_process(if result.is_ok() { 0 } else { 1 });
}

#[cfg(target_os = "android")]
fn finish_android_process(status: i32) -> ! {
    unsafe extern "C" {
        fn _exit(status: i32) -> !;
    }
    // Rust's app/resources have already dropped. C exit would additionally
    // destroy GameActivity's C++ globals on this native thread, although their
    // retained JNIEnv belongs to Java's UI thread (fatal DeleteGlobalRef).
    unsafe { _exit(status) }
}

#[cfg_attr(not(target_os = "android"), bevy_main)]
pub fn main() {
    let _crash_reporting = crash_reporting::init();
    if let Err(error) = run() {
        eprintln!("Rubblekin: {error}");
        crash_reporting::startup_error(error.as_ref());
        #[cfg(not(target_os = "android"))]
        drop(_crash_reporting);
        #[cfg(target_os = "android")]
        finish_android_process(1);
        #[cfg(not(target_os = "android"))]
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    platform::prepare_data_directory()?;
    let options = options().map_err(std::io::Error::other)?;
    let mut graphics = GraphicsSettings::load(platform::default_graphics());
    if let Some(quality) = options.graphics {
        graphics.set_quality(quality);
    }
    crash_reporting::context("join", graphics.quality.label());
    let mut menu = join::JoinScreen::new(
        options
            .connect
            .clone()
            .unwrap_or_else(|| "rubblekin.oci.koski.co:7878".into()),
        options.name.unwrap_or_else(|| {
            join_preferences::load(std::path::Path::new(join_preferences::FILE))
        }),
        ServerConfig {
            bind_addr: options.bind.unwrap_or_else(|| "127.0.0.1:7878".into()),
            save_path: options
                .save
                .unwrap_or_else(|| PathBuf::from("saves/villages.json")),
            seed: options.seed.unwrap_or(42),
            generation: options
                .generation
                .unwrap_or(rubblekin_core::world::WorldGeneration::GeographyV6),
            allow_admin: true,
        },
        graphics.quality,
        if options.observe {
            SessionMode::Observer
        } else {
            SessionMode::Player
        },
    );
    if options.local || options.connect.is_some() {
        menu.start(options.local);
    }
    let screenshot = options.screenshot;
    if let Some(path) = &screenshot
        && let Some(parent) = std::path::Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.61, 0.76, 0.81)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.78, 0.86, 1.0),
            brightness: 600.0,
            ..default()
        })
        .insert_resource(DirectionalLightShadowMap {
            size: graphics.quality.shadow_map_size(),
        })
        .insert_resource(menu)
        .insert_resource(graphics)
        .init_resource::<pause::PauseMenu>()
        .init_resource::<world_map::WorldMap>()
        .init_resource::<admin_console::AdminConsole>()
        .init_resource::<airships::PilotConversation>()
        .init_resource::<market::MarketPanel>()
        .insert_resource(capture::Capture::new(screenshot, options.exit_after))
        .init_resource::<Avatars>()
        .init_resource::<tutorials::Tutorials>()
        .insert_resource(touch::TouchControls::new(
            cfg!(target_os = "android") || options.touch,
        ))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(platform::window(options.touch)),
            ..default()
        }))
        .init_resource::<crash_reporting::RendererFailureReported>()
        .insert_resource(bevy::render::error_handler::RenderErrorHandler(
            crash_reporting::renderer_error,
        ))
        .add_plugins(ui::ButtonActivationPlugin)
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(terrain_material::TerrainMaterialPlugin)
        .add_plugins(sky::SkyPlugin)
        .init_resource::<block_textures::BlockIcons>()
        .init_resource::<trade_pictures::TradePictures>()
        .init_resource::<vehicles::Pictures>()
        .add_systems(First, platform::frame_time.before(bevy::time::TimeSystems))
        .add_systems(First, platform::activity_exit)
        .add_message::<join::MenuKey>()
        .add_systems(Startup, join::setup)
        .add_systems(Last, update_crash_context)
        .add_systems(
            PostUpdate,
            tutorials::capture_region
                .after(bevy::ui::UiSystems::Layout)
                .run_if(resource_exists::<Session>),
        )
        .add_systems(
            Update,
            (
                join::native_input,
                join::interact,
                join::android_text_input,
                join::poll_connection,
                (
                    setup,
                    sky::setup,
                    ui::setup_ui,
                    touch::setup,
                    pause::setup,
                    gliders::setup,
                    vehicles::setup,
                    parcels::setup,
                    admin_console::setup,
                    world_map::setup,
                    market::setup,
                    work_cues::setup,
                    inventory::setup,
                    activities::setup,
                    tutorials::setup,
                )
                    .chain()
                    .run_if(resource_added::<Session>),
                tutorials::read.run_if(resource_exists::<Session>),
                touch::read,
                (
                    (
                        receive_network,
                        inventory::read,
                        admin_console::read,
                        market::read,
                        gliders::read,
                        pause::read,
                        world_map::read,
                        airships::advance_clock,
                        gliders::carry,
                        activities::read,
                        controls,
                        graphics::apply_settings,
                        camera,
                        (sky::update, vehicles::weather).chain(),
                    )
                        .chain(),
                    (
                        terrain::stream_terrain,
                        edit_blocks,
                        update_avatars,
                        work_animation::animate,
                        (work_tools::update, work_cues::update).chain(),
                        wildlife::update,
                        (
                            forage::update,
                            activities::update,
                            cart_repairs::update,
                            flow_gardens::update,
                            activities::update_demo,
                        )
                            .chain(),
                        (
                            gliders::update_scene,
                            gliders::animate_whips,
                            vehicles::update,
                        )
                            .chain(),
                        crops::update_crops,
                        inspection::update,
                        ui::update_ui,
                        ui::scroll_panels,
                        touch::refresh,
                        pause::refresh,
                        gliders::refresh,
                        admin_console::refresh,
                        (world_map::refresh, map_activities::refresh).chain(),
                        (market::refresh, parcels::update).chain(),
                        (inventory::refresh, tutorials::update).chain(),
                        join::leave_world,
                    )
                        .chain(),
                )
                    .chain()
                    .run_if(resource_exists::<Session>),
                graphics::save_changed,
                join::layout,
                join::refresh,
                capture::capture_frame,
            )
                .chain(),
        )
        .run();
    Ok(())
}

fn update_crash_context(
    graphics: Res<GraphicsSettings>,
    session: Option<Res<Session>>,
    mut previous: Local<Option<(bool, GraphicsQuality)>>,
) {
    let current = (session.is_some(), graphics.quality);
    if *previous != Some(current) {
        crash_reporting::context(if current.0 { "world" } else { "join" }, current.1.label());
        *previous = Some(current);
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    graphics: Res<GraphicsSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<terrain_material::TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut prepared: ResMut<terrain::PreparedTerrain>,
) {
    let start = std::time::Instant::now();
    let terrain = terrain::install_terrain(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut terrain_materials,
        &mut images,
        std::mem::take(&mut *prepared),
    );
    commands.insert_resource(terrain);
    commands.remove_resource::<terrain::PreparedTerrain>();
    info!(
        "Terrain assets installed in {:.3}s",
        start.elapsed().as_secs_f32()
    );
    commands.spawn((
        GameEntity,
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: if world.0.geography().is_some() {
                terrain::GEOGRAPHIC_VIEW_DISTANCE
            } else {
                1000.0
            },
            fov: 60.0_f32.to_radians(),
            ..default()
        }),
        DistanceFog {
            color: Color::srgb(0.61, 0.76, 0.81),
            falloff: FogFalloff::Linear {
                start: if world.0.geography().is_some() {
                    1800.0
                } else {
                    65.0
                },
                end: if world.0.geography().is_some() {
                    28000.0
                } else {
                    580.0
                },
            },
            ..default()
        },
        graphics.quality.msaa(),
        graphics.quality.shadow_filter(),
        Transform::from_translation(
            Vec3::from_array(session.body.position) + Vec3::new(0.0, 4.0, 6.0),
        ),
        GameCamera,
        CameraFollow::default(),
        IsDefaultUiCamera,
    ));
}

#[allow(clippy::too_many_arguments)]
fn receive_network(
    mut commands: Commands,
    mut connection: ResMut<Connection>,
    mut world: ResMut<VoxelWorld>,
    mut session: ResMut<Session>,
    mut scene: ResMut<terrain::TerrainScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    mut conversation: ResMut<airships::PilotConversation>,
    mut console: Option<ResMut<admin_console::AdminConsole>>,
    mut market: Option<ResMut<market::MarketPanel>>,
    mut activities_scene: Option<ResMut<activities::Scene>>,
    mut tutorials: Option<ResMut<tutorials::Tutorials>>,
    mut follows: Query<&mut CameraFollow, With<GameCamera>>,
) {
    let mut latest_authoritative = None;
    for message in connection.poll() {
        match message {
            ServerMessage::State {
                players,
                npc,
                residents,
                villages,
                world_time,
                gliders,
            } => {
                if session.observer.is_none()
                    && let Some(authoritative) =
                        players.iter().find(|player| player.id == session.id)
                {
                    latest_authoritative = Some(authoritative.clone());
                }
                session.gliders = gliders;
                session.players = players;
                session.npc = npc;
                session.residents = residents;
                session.villages = villages;
                session.world_time = world_time;
                session
                    .airship_clock
                    .observe(world_time, time.elapsed_secs_f64());
            }
            ServerMessage::ActivityState {
                request_id,
                activities,
                notice,
                accepted,
            } => {
                session.activities = activities;
                if let Some(scene) = &mut activities_scene {
                    if accepted
                        && let Some((activity_id, action)) = scene.pending_action(request_id)
                        && let Some(t) = tutorials.as_deref_mut()
                    {
                        let activity = session.activities.iter().find(|a| a.plan.id == activity_id);
                        if activity.is_some_and(|a| {
                            a.plan.kind == rubblekin_core::activities::ActivityKind::FlowGarden
                        }) {
                            t.signal(tutorials::Signal::Garden(
                                activity.is_some_and(|a| a.complete),
                            ));
                        } else {
                            t.signal(tutorials::Signal::Activity(
                                action,
                                activity.is_some_and(|a| a.complete),
                            ));
                        }
                    }
                    scene.reply(request_id, accepted, &session, time.elapsed_secs_f64());
                }
                if !notice.is_empty() {
                    session.status = notice;
                    session.status_until = time.elapsed_secs_f64() + 4.;
                }
            }
            ServerMessage::WildlifeState { animals, habitats } => {
                session.wildlife = animals;
                session.habitats = habitats;
            }
            ServerMessage::BlockChanged {
                request_id,
                player_id,
                edit,
            } => {
                if player_id == session.id
                    && request_id != 0
                    && let Some(t) = tutorials.as_deref_mut()
                {
                    t.signal(tutorials::Signal::Edit(edit.block));
                }
                if world.0.set_block(edit.position, edit.block).is_ok() {
                    terrain::rebuild_chunks(
                        &mut scene,
                        edit.position,
                        &world.0,
                        &mut commands,
                        &mut meshes,
                    );
                    session.edits = world.0.edits().len();
                }
            }
            ServerMessage::Rejected { reason, .. } => {
                session.status = reason;
                session.status_until = time.elapsed_secs_f64() + 4.0;
            }
            ServerMessage::Notice { text } => {
                session.status = text;
                session.status_until = time.elapsed_secs_f64() + 5.0;
            }
            ServerMessage::PilotDialog { ship_id, text } => conversation.reply(ship_id, text),
            ServerMessage::MarketState {
                request_id,
                ledger,
                market: view,
                notice,
                accepted,
            } => {
                if let Some(panel) = &mut market {
                    panel.reply(request_id, ledger, view, notice.clone(), accepted);
                    if panel.completed_delivery.is_some()
                        && let Some(t) = tutorials.as_deref_mut()
                    {
                        t.signal(tutorials::Signal::Finished(tutorials::Lesson::Parcel));
                    }
                }
                if !notice.is_empty() {
                    session.status = notice;
                    session.status_until = time.elapsed_secs_f64() + 5.0;
                }
            }
            ServerMessage::WorkState {
                request_id,
                work,
                ledger,
                notice,
                accepted,
            } => {
                if let Some(panel) = &mut market
                    && panel.work_reply(request_id, work, ledger, notice.clone(), accepted)
                    && let Some(t) = tutorials.as_deref_mut()
                {
                    t.signal(tutorials::Signal::Finished(tutorials::Lesson::Work));
                }
                if !notice.is_empty() {
                    session.status = notice;
                    session.status_until = time.elapsed_secs_f64() + 5.0;
                }
            }
            ServerMessage::AdminCommandResult { text } => {
                session.status = if text.contains('\n') {
                    "Command help is displayed in the admin panel.".into()
                } else {
                    text.clone()
                };
                session.status_until = time.elapsed_secs_f64() + 5.0;
                if let Some(console) = &mut console {
                    console.reply(text);
                }
            }
            _ => {}
        }
    }
    // A stalled render frame can receive several snapshots. Replay once from
    // the newest acknowledgment, with all received terrain edits available.
    if let Some(authoritative) = latest_authoritative {
        let session = &mut *session;
        let previous_epoch = session.prediction.movement_epoch();
        if authoritative.glider_ride.is_some() || authoritative.vehicle.is_some() {
            session.flying = false;
        }
        session.vehicle = authoritative.vehicle;
        let result = if session.vehicle.is_some() {
            session.prediction.reconcile_vehicle(
                &world.0,
                &mut session.body,
                &authoritative,
                |time| {
                    character_obstacles(
                        session.id,
                        &session.players,
                        &session.npc,
                        &session.residents,
                        &session.airships,
                        time,
                    )
                },
                session.world_time,
                &mut session.vehicle,
            )
        } else if session.airships.routes().is_empty() {
            session.prediction.reconcile_gliders(
                &world.0,
                &mut session.body,
                &authoritative,
                |time| {
                    character_obstacles(
                        session.id,
                        &session.players,
                        &session.npc,
                        &session.residents,
                        &session.airships,
                        time,
                    )
                },
                &session.airships,
                session.world_time,
                &session.gliders,
                &mut session.glider_ride,
                &mut session.gliding,
            )
        } else {
            session.prediction.reconcile_airships(
                &world.0,
                &mut session.body,
                &authoritative,
                |time| {
                    character_obstacles(
                        session.id,
                        &session.players,
                        &session.npc,
                        &session.residents,
                        &session.airships,
                        time,
                    )
                },
                &session.airships,
                session.world_time,
                &mut session.ride,
                &mut session.deck_position,
            )
        };
        if let Err(error) = result {
            connection.fail(error.into());
        } else if session.prediction.movement_epoch() != previous_epoch {
            for mut follow in &mut follows {
                *follow = CameraFollow::default();
            }
            session.target = None;
        }
    }
    if let Some(error) = &connection.error {
        session.status = format!("Disconnected: {error}");
        session.status_until = f64::INFINITY;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut cursor: Single<&mut CursorOptions>,
    window: Single<&Window>,
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut session: ResMut<Session>,
    mut connection: ResMut<Connection>,
    mut graphics: ResMut<GraphicsSettings>,
    modals: (
        Option<Res<pause::PauseMenu>>,
        Option<Res<world_map::WorldMap>>,
        Option<Res<admin_console::AdminConsole>>,
        Option<Res<airships::PilotConversation>>,
        Option<Res<market::MarketPanel>>,
    ),
    diagnostics: Res<DiagnosticsStore>,
    touch: Res<touch::TouchControls>,
) {
    let (pause, map, console, conversation, market) = modals;
    let dt = time.delta_secs().min(MAX_INPUT_DT);
    let observing = session.observer.is_some();
    let blocked = pause
        .as_ref()
        .is_some_and(|menu| menu.open || menu.input_blocked)
        || conversation
            .as_ref()
            .is_some_and(|dialog| dialog.open() || dialog.input_blocked);
    let blocked = blocked
        || session.inventory.input_blocked
        || console
            .as_ref()
            .is_some_and(|console| console.input_blocked)
        || map
            .as_ref()
            .is_some_and(|map| map.open || map.input_blocked)
        || market
            .as_ref()
            .is_some_and(|panel| panel.open || panel.input_blocked);
    let resumed = pause.as_ref().is_some_and(|menu| menu.just_closed)
        || conversation
            .as_ref()
            .is_some_and(|dialog| dialog.just_closed);
    let resumed = resumed
        || session.inventory.just_closed
        || console.as_ref().is_some_and(|console| console.just_closed)
        || map.as_ref().is_some_and(|map| map.just_closed)
        || market.as_ref().is_some_and(|panel| panel.just_closed);
    if touch.enabled {
        session.captured =
            window.focused && !blocked && !touch.menu_open && connection.error.is_none();
        if !cfg!(target_os = "android") {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    } else if resumed
        && !map.as_ref().is_some_and(|map| map.open)
        && window.focused
        && connection.error.is_none()
    {
        session.captured = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        session.edit_clock = 0.3;
    } else if blocked || !window.focused {
        session.captured = false;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if mouse.just_pressed(MouseButton::Left) && !session.captured {
        session.captured = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        session.edit_clock = 0.3;
    }
    if session.captured && !blocked {
        let motion = if touch.enabled {
            touch.look
        } else {
            motion.delta
        };
        let scroll = if touch.enabled {
            touch.zoom * dt * 4.0
        } else {
            scroll.delta.y
        };
        let (min_pitch, max_pitch) = if observing { (-1.5, 1.5) } else { (-0.6, 1.2) };
        session.yaw += motion.x * 0.0025;
        // Discrete steps also support brief keyboard taps and accessibility input.
        session.yaw += (f32::from(keys.just_pressed(KeyCode::ArrowRight))
            - f32::from(keys.just_pressed(KeyCode::ArrowLeft)))
            * 0.06;
        session.pitch = (session.pitch
            + (f32::from(keys.just_pressed(KeyCode::ArrowDown))
                - f32::from(keys.just_pressed(KeyCode::ArrowUp)))
                * 0.06)
            .clamp(min_pitch, max_pitch);
        session.yaw += (f32::from(keys.pressed(KeyCode::ArrowRight))
            - f32::from(keys.pressed(KeyCode::ArrowLeft)))
            * dt
            * 1.5;
        session.pitch = (session.pitch
            + (f32::from(keys.pressed(KeyCode::ArrowDown))
                - f32::from(keys.pressed(KeyCode::ArrowUp)))
                * dt)
            .clamp(min_pitch, max_pitch);
        session.pitch = (session.pitch + motion.y * 0.0025).clamp(min_pitch, max_pitch);
        if let Some(observer) = &mut session.observer {
            observer.adjust_speed(scroll);
        } else {
            session.camera_distance = (session.camera_distance - scroll * 0.45).clamp(1.0, 9.0);
        }
    }
    if !blocked
        && window.focused
        && !observing
        && (keys.just_pressed(KeyCode::KeyF) || touch.flight)
    {
        session.flying = !session.flying;
    }
    if observing
        && window.focused
        && ((!blocked && (keys.just_pressed(KeyCode::Home) || keys.just_pressed(KeyCode::KeyR)))
            || touch.return_spawn)
    {
        session.observer = Some(ObserverCamera::new(world.0.spawn_position()));
        session.yaw = -0.45;
        session.pitch = 0.12;
    }
    if observing
        && window.focused
        && ((!blocked && keys.just_pressed(KeyCode::KeyV)) || touch.next_village)
        && let Some(plan) = world.0.settlements()
        && !plan.villages.is_empty()
    {
        let camera = session.observer.as_ref().unwrap().position;
        let nearest = plan
            .villages
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                Vec3::from_array(a.center)
                    .distance_squared(camera)
                    .total_cmp(&Vec3::from_array(b.center).distance_squared(camera))
            })
            .map_or(0, |(index, _)| index);
        let village = &plan.villages[(nearest + 1) % plan.villages.len()];
        let center = Vec3::from_array(village.center);
        let mut observer = ObserverCamera::new(village.center);
        // Keep the streaming square centered on the village while showing its
        // buildings and cultivated outskirts together.
        observer.position = center + Vec3::new(0.0, 75.0, 8.0);
        session.observer = Some(observer);
        session.yaw = 0.0;
        session.pitch = 1.43;
        session.status = format!(
            "{} · {:?} · freshwater {:.0}m · V visits next village",
            village.name, village.kind, village.freshwater_distance
        );
        session.status_until = time.elapsed_secs_f64() + 8.0;
    }
    if !blocked && window.focused && keys.just_pressed(KeyCode::Tab) {
        session.inspector = !session.inspector;
        session.inspect_requested |= session.inspector;
    }
    if touch.inspect {
        session.inspector = !(session.inspector && !session.help);
        session.inspect_requested |= session.inspector;
        session.help = false;
    }
    if !blocked && window.focused && keys.just_pressed(KeyCode::KeyH) {
        session.help = !session.help;
    }
    if touch.help {
        session.help = !session.help;
        if session.help {
            session.inspector = false;
        }
    }
    if !console
        .as_ref()
        .is_some_and(|console| console.input_blocked)
        && window.focused
        && keys.just_pressed(KeyCode::F2)
    {
        let quality = graphics.quality.next();
        graphics.set_quality(quality);
    }
    session.graphics = graphics.quality;
    if !blocked && window.focused && !observing {
        for (slot, key) in [
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
                session.selected = slot;
            }
        }
        if let Some(selected) = touch.selected.filter(|index| *index < palette::QUICK_SLOTS) {
            session.selected = selected;
        }
    }
    if !blocked && window.focused && session.can_admin && !observing {
        let goal = if keys.just_pressed(KeyCode::F6) {
            Some(Some(NpcAction::Forage))
        } else if keys.just_pressed(KeyCode::F7) {
            Some(Some(NpcAction::Rest))
        } else if keys.just_pressed(KeyCode::F8) {
            Some(None)
        } else {
            None
        };
        if let Some(goal) = goal {
            connection.send(ClientMessage::Admin {
                action: AdminAction::SetNpcGoal { goal },
            });
        }
        if keys.just_pressed(KeyCode::F9) {
            connection.send(ClientMessage::Admin {
                action: AdminAction::SetNpcNeeds {
                    hunger: 85.0,
                    energy: 35.0,
                },
            });
        }
        if keys.just_pressed(KeyCode::BracketLeft) {
            connection.send(ClientMessage::Admin {
                action: AdminAction::SetNpcWeights {
                    forage: 0.5,
                    rest: 2.0,
                },
            });
        }
        if keys.just_pressed(KeyCode::BracketRight) {
            connection.send(ClientMessage::Admin {
                action: AdminAction::SetNpcWeights {
                    forage: 1.0,
                    rest: 1.0,
                },
            });
        }
    }
    let mut input = MoveInput::default();
    let mut observer_input = Vec3::ZERO;
    if !blocked && session.captured && window.focused && connection.error.is_none() && touch.enabled
    {
        input = touch.movement_input(session.yaw, session.flying);
        observer_input = Vec3::new(touch.movement.x, touch.vertical, touch.movement.y);
    } else if !blocked && session.captured && window.focused && connection.error.is_none() {
        let forward = Vec2::new(session.yaw.sin(), -session.yaw.cos());
        let right = Vec2::new(session.yaw.cos(), session.yaw.sin());
        let x = f32::from(keys.pressed(KeyCode::KeyD)) - f32::from(keys.pressed(KeyCode::KeyA));
        let z = f32::from(keys.pressed(KeyCode::KeyW)) - f32::from(keys.pressed(KeyCode::KeyS));
        let direction = (forward * z + right * x).normalize_or_zero();
        input.direction = direction.to_array();
        input.jump = keys.pressed(KeyCode::Space);
        input.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        input.vertical =
            f32::from(keys.pressed(KeyCode::KeyE)) - f32::from(keys.pressed(KeyCode::KeyQ));
        observer_input = Vec3::new(x, input.vertical, z);
    }
    input.fly = session.flying;
    input.glide_direction = Some([
        session.yaw.sin() * session.pitch.cos(),
        -session.pitch.sin(),
        -session.yaw.cos() * session.pitch.cos(),
    ]);
    if connection.error.is_none() && dt > 0.0 {
        let session = &mut *session;
        if let Some(observer) = &mut session.observer {
            observer.advance(observer_input, session.yaw, session.pitch, input.sprint, dt);
        } else if match session.prediction.ready_to_advance(
            dt,
            connection.can_send(),
            std::time::Instant::now(),
        ) {
            Ok(ready) => ready,
            Err(error) => {
                connection.fail(error.into());
                false
            }
        } {
            let obstacles = character_obstacles(
                session.id,
                &session.players,
                &session.npc,
                &session.residents,
                &session.airships,
                session.airship_clock.time,
            );
            let command = if session.vehicle.is_some() {
                session.prediction.advance_vehicle(
                    &world.0,
                    &mut session.body,
                    input,
                    session.yaw,
                    dt,
                    &obstacles,
                    session.airship_clock.time,
                    &mut session.vehicle,
                )
            } else if session.airships.routes().is_empty() {
                session.prediction.advance_gliders(
                    &world.0,
                    &mut session.body,
                    input,
                    session.yaw,
                    dt,
                    &obstacles,
                    &session.airships,
                    session.airship_clock.time,
                    &session.gliders,
                    &mut session.glider_ride,
                    &mut session.gliding,
                )
            } else {
                session.prediction.advance_airships(
                    &world.0,
                    &mut session.body,
                    input,
                    session.yaw,
                    dt,
                    &obstacles,
                    &session.airships,
                    session.airship_clock.time,
                    &mut session.ride,
                    &mut session.deck_position,
                )
            };
            match command {
                Ok(message) => connection.send(message),
                Err(error) => connection.fail(error.into()),
            }
        }
    }
    session.edit_clock = (session.edit_clock - dt).max(0.0);
    if let Some(value) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|value| value.smoothed())
    {
        session.fps = value;
    }
}

fn camera(
    world: Res<VoxelWorld>,
    session: Res<Session>,
    time: Res<Time>,
    mut camera: Single<(&mut Transform, &mut CameraFollow), With<GameCamera>>,
) {
    let (transform, follow) = &mut *camera;
    if let Some(observer) = &session.observer {
        **transform = observer.transform(session.yaw, session.pitch);
        return;
    }
    let eye = Vec3::from_array(session.body.position) + Vec3::Y * EYE_HEIGHT;
    let ship = session.ride.and_then(|ride| {
        session
            .airships
            .ship(ride.ship_id, session.airship_clock.time)
    });
    let glider = session.glider_ride.and_then(|r| {
        session
            .gliders
            .iter()
            .find(|f| f.id == r.carriage_id)
            .map(|f| (f.id, f.pose(session.airship_clock.time)))
    });
    let follow_eye = if let Some((id, pose)) = glider {
        follow.advance_on_glider(
            eye,
            time.delta_secs(),
            id,
            Vec3::from_array(pose.position),
            pose.yaw,
        )
    } else if let Some(ship) = &ship {
        follow.advance_on_airship(eye, time.delta_secs(), ship)
    } else if session.gliding {
        follow.advance_gliding(eye)
    } else {
        follow.advance(eye, time.delta_secs())
    };
    **transform = follow_camera::transform(
        &world.0,
        eye,
        follow_eye,
        session.yaw,
        session.pitch,
        session.camera_distance,
    );
    if let Some(ship) = ship {
        transform.translation =
            follow.airship_camera_position(&ship, eye, transform.translation, time.delta_secs());
    }
}

#[allow(clippy::too_many_arguments)]
fn edit_blocks(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    camera: Single<&Transform, With<GameCamera>>,
    world: Res<VoxelWorld>,
    mut session: ResMut<Session>,
    mut connection: ResMut<Connection>,
    mut gizmos: Gizmos,
    touch: Res<touch::TouchControls>,
    pause: Option<Res<pause::PauseMenu>>,
    map: Option<Res<world_map::WorldMap>>,
    conversation: Option<Res<airships::PilotConversation>>,
    console: Option<Res<admin_console::AdminConsole>>,
    market: Option<Res<market::MarketPanel>>,
    mut tutorials: Option<ResMut<tutorials::Tutorials>>,
) {
    if session.inventory.input_blocked
        || pause.is_some_and(|menu| menu.open || menu.input_blocked)
        || map.is_some_and(|map| map.open || map.input_blocked)
        || conversation.is_some_and(|dialog| dialog.open() || dialog.input_blocked)
        || console.is_some_and(|console| console.input_blocked)
        || market.is_some_and(|panel| panel.open || panel.input_blocked)
        || session.ride.is_some()
    {
        session.target = None;
        return;
    }
    if session.observer.is_some() {
        session.target = None;
        return;
    }
    let ray = world.0.raycast(
        camera.translation.to_array(),
        camera.forward().to_array(),
        session.camera_distance + 8.0,
    );
    session.target = ray.and_then(|hit| {
        let p = Vec3::new(
            hit.position.x as f32 + 0.5,
            hit.position.y as f32 + 0.5,
            hit.position.z as f32 + 0.5,
        ) * CELL_SIZE;
        if p.distance(Vec3::from_array(session.body.position) + Vec3::Y * EYE_HEIGHT) <= 6.0 {
            Some((hit.position, hit.previous))
        } else {
            None
        }
    });
    let repeated = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let dig = if touch.enabled {
        touch.dig
    } else {
        mouse.just_pressed(MouseButton::Left) || repeated && mouse.pressed(MouseButton::Left)
    };
    let build = if touch.enabled {
        touch.build
    } else {
        mouse.just_pressed(MouseButton::Right) || repeated && mouse.pressed(MouseButton::Right)
    };
    let attempted = session.captured
        && session.edit_clock <= 0.0
        && connection.error.is_none()
        && (dig || build);
    if attempted && let Some(t) = tutorials.as_deref_mut() {
        t.signal(tutorials::Signal::Open(tutorials::Lesson::Building));
    }
    if attempted && session.target.is_none() {
        session.status = "Move closer to reach a block · aim down to build nearby".into();
        session.status_until = time.elapsed_secs_f64() + 3.0;
        session.edit_clock = 0.16;
    }
    if let Some((position, previous)) = session.target {
        let center = Vec3::new(
            position.x as f32 + 0.5,
            position.y as f32 + 0.5,
            position.z as f32 + 0.5,
        ) * CELL_SIZE;
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(CELL_SIZE + 0.014)),
            Color::srgb(1.0, 0.89, 0.57),
        );
        if attempted {
            let block = if dig {
                Block::Air
            } else {
                session.hotbar[session.selected]
            };
            let target = if dig { position } else { previous };
            let request_id = session.next_request;
            session.next_request += 1;
            session.edit_clock = 0.16;
            connection.send(ClientMessage::Edit {
                request_id,
                position: target,
                block,
            });
        }
    }
}

fn character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    npc: bool,
) -> Entity {
    character_with_role(commands, meshes, materials, npc, None)
}

fn character_with_role(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    npc: bool,
    role: Option<ResidentRole>,
) -> Entity {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let skin = materials.add(Color::srgb(0.79, 0.56, 0.36));
    let cloth = materials.add(match role {
        Some(ResidentRole::Farmer) => Color::srgb(0.47, 0.58, 0.25),
        Some(ResidentRole::Woodcutter) => Color::srgb(0.69, 0.36, 0.20),
        Some(ResidentRole::Quarrier) => Color::srgb(0.43, 0.47, 0.54),
        Some(ResidentRole::Miner) => Color::srgb(0.31, 0.37, 0.46),
        Some(ResidentRole::Trader) => Color::srgb(0.56, 0.33, 0.49),
        None if npc => Color::srgb(0.74, 0.42, 0.18),
        None => Color::srgb(0.20, 0.43, 0.47),
    });
    let boots = materials.add(Color::srgb(0.20, 0.19, 0.17));
    let hair = materials.add(Color::srgb(0.22, 0.15, 0.11));
    let scarf = materials.add(if npc {
        Color::srgb(0.42, 0.55, 0.27)
    } else {
        Color::srgb(0.87, 0.64, 0.29)
    });
    let eyes = materials.add(Color::srgb(0.07, 0.10, 0.10));
    let shadow_mesh = meshes.add(Circle::new(0.38));
    let shadow_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.04, 0.07, 0.06, 0.26),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let id = commands
        .spawn((
            Transform::default(),
            Visibility::default(),
            Avatar,
            GameEntity,
        ))
        .id();
    commands.entity(id).with_children(|parent| {
        if let Some(role) = role {
            let cap = materials.add(match role {
                ResidentRole::Farmer => Color::srgb(0.78, 0.66, 0.37),
                ResidentRole::Woodcutter => Color::srgb(0.24, 0.35, 0.24),
                ResidentRole::Trader => Color::srgb(0.53, 0.31, 0.44),
                _ => Color::srgb(0.37, 0.41, 0.46),
            });
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(cap),
                Transform::from_xyz(0.0, 1.79, 0.0).with_scale(Vec3::new(0.52, 0.13, 0.48)),
            ));
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(boots.clone()),
                Transform::from_xyz(0.0, 0.91, -0.40).with_scale(Vec3::splat(0.35)),
                Visibility::Hidden,
                CarriedGoods,
            ));
        }
        parent.spawn((
            Mesh3d(shadow_mesh),
            MeshMaterial3d(shadow_material),
            Transform::from_xyz(0.0, 0.018, 0.0)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            GroundShadow,
            NotShadowCaster,
        ));
        for (scale, at, material) in [
            (
                Vec3::new(0.45, 0.58, 0.28),
                Vec3::new(0.0, 0.99, 0.0),
                cloth.clone(),
            ),
            (
                Vec3::new(0.38, 0.36, 0.36),
                Vec3::new(0.0, 1.49, 0.0),
                skin.clone(),
            ),
            (
                Vec3::new(0.40, 0.13, 0.38),
                Vec3::new(0.0, 1.70, 0.015),
                hair.clone(),
            ),
            (
                Vec3::new(0.47, 0.09, 0.32),
                Vec3::new(0.0, 1.30, 0.0),
                scarf.clone(),
            ),
            (
                Vec3::new(0.07, 0.055, 0.025),
                Vec3::new(-0.09, 1.51, -0.19),
                eyes.clone(),
            ),
            (
                Vec3::new(0.07, 0.055, 0.025),
                Vec3::new(0.09, 1.51, -0.19),
                eyes.clone(),
            ),
            (
                Vec3::new(0.31, 0.39, 0.20),
                Vec3::new(0.0, 1.03, 0.24),
                boots.clone(),
            ),
        ] {
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(at).with_scale(scale),
            ));
        }
        for (x, phase) in [(-0.13, 0.0), (0.13, std::f32::consts::PI)] {
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(boots.clone()),
                Transform::from_xyz(x, 0.38, 0.0).with_scale(Vec3::new(0.18, 0.66, 0.22)),
                Limb { phase, arm: false },
            ));
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(cloth.clone()),
                Transform::from_xyz(x.signum() * 0.31, 1.0, 0.0)
                    .with_scale(Vec3::new(0.16, 0.52, 0.20)),
                Limb {
                    phase: phase + std::f32::consts::PI,
                    arm: true,
                },
            ));
        }
    });
    id
}

// A clear authoritative pose is the fallback when smoothing would cut through
// another person or a terrain step. Keep earlier rendered poses in the obstacle
// list too, so two individually clear interpolations cannot cross each other.
fn smoothed_avatar_position(
    world: &GameWorld,
    current: Vec3,
    positions: &mut [[f32; 3]],
    index: usize,
    blend: f32,
) -> Vec3 {
    let desired = Vec3::from_array(positions[index]);
    let obstacles: Vec<_> = positions
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(_, position)| *position)
        .collect();
    let candidate = if current.distance_squared(desired) > 64.0 {
        desired
    } else {
        current.lerp(desired, blend)
    };
    let mut body = Body::new(
        if character_position_is_clear(world, candidate.to_array(), &obstacles) {
            candidate.to_array()
        } else {
            desired.to_array()
        },
    );
    resolve_character_overlaps(world, &mut body, &obstacles);
    positions[index] = body.position;
    Vec3::from_array(body.position)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_avatars(
    mut commands: Commands,
    session: Res<Session>,
    mut avatars: ResMut<Avatars>,
    mut transforms: Query<&mut Transform, With<Avatar>>,
    mut limbs: Query<(&mut Transform, &Limb, &ChildOf), Without<Avatar>>,
    mut shadows: Query<(&mut Visibility, &ChildOf), (With<GroundShadow>, Without<CarriedGoods>)>,
    mut cargo: Query<(&mut Visibility, &ChildOf), (With<CarriedGoods>, Without<GroundShadow>)>,
    world: Res<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let mut positions: Vec<_> = session
        .players
        .iter()
        .map(|player| {
            if player.id == session.id {
                session.body.position
            } else if let Some(ride) = player.glider_ride
                && let Some(flight) = session.gliders.iter().find(|f| f.id == ride.carriage_id)
            {
                rubblekin_core::gliders::seat_position(
                    flight.pose(session.airship_clock.time),
                    ride.seat,
                )
            } else if let Some(ride) = player.ride
                && let Some(ship) = session
                    .airships
                    .ship(ride.ship_id, session.airship_clock.time)
            {
                deck_position(
                    &ship,
                    player
                        .deck_position
                        .unwrap_or(initial_deck_position(ride.seat)),
                )
            } else {
                player.body.position
            }
        })
        .chain(std::iter::once(session.npc.position))
        .chain(session.residents.iter().map(|resident| {
            resident
                .ride
                .and_then(|ride| {
                    session
                        .airships
                        .ship(ride.ship_id, session.airship_clock.time)
                        .map(|ship| {
                            deck_position(
                                &ship,
                                resident
                                    .deck_position
                                    .unwrap_or(initial_deck_position(ride.seat)),
                            )
                        })
                })
                .unwrap_or(resident.position)
        }))
        .collect();
    let mut moving = Vec::new();
    let live: Vec<u64> = session.players.iter().map(|p| p.id).collect();
    avatars.players.retain(|id, entity| {
        if !live.contains(id) {
            commands.entity(*entity).despawn();
            false
        } else {
            true
        }
    });
    avatars.player_deck_motion.retain(|id, _| live.contains(id));
    avatars.player_epochs.retain(|id, _| live.contains(id));
    for (index, player) in session.players.iter().enumerate() {
        let teleported = avatars
            .player_epochs
            .insert(player.id, player.movement_epoch)
            .is_some_and(|previous| previous != player.movement_epoch);
        if teleported {
            avatars.player_deck_motion.remove(&player.id);
        }
        let entity = *avatars.players.entry(player.id).or_insert_with(|| {
            let entity = character(&mut commands, &mut meshes, &mut materials, false);
            commands
                .entity(entity)
                .insert(Transform::from_translation(Vec3::from_array(
                    positions[index],
                )));
            entity
        });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let vehicle = if player.id == session.id {
                session.vehicle
            } else {
                player.vehicle
            };
            let (position, yaw) = if player.id == session.id {
                (session.body.position, session.yaw)
            } else if player.ride.is_some() {
                (positions[index], player.yaw)
            } else {
                (player.body.position, player.yaw)
            };
            let seat_offset = vehicle.map_or(0., |v| match v.kind {
                rubblekin_core::vehicles::VehicleKind::Bike => 0.12,
                rubblekin_core::vehicles::VehicleKind::Kayak => -0.42,
                rubblekin_core::vehicles::VehicleKind::Sailboat => 0.08,
            });
            let previous = transform.translation - Vec3::Y * seat_offset;
            transform.translation =
                if player.id == session.id || player.ride.is_some() || teleported {
                    Vec3::from_array(position)
                } else {
                    smoothed_avatar_position(
                        &world.0,
                        previous,
                        &mut positions,
                        index,
                        (time.delta_secs() * 15.0).min(1.0),
                    )
                };
            let (ride, local) = if player.id == session.id {
                (session.ride, session.deck_position)
            } else {
                (player.ride, player.deck_position)
            };
            let walking = if let Some(ride) = ride {
                avatars
                    .player_deck_motion
                    .entry(player.id)
                    .or_default()
                    .walking(
                        ride,
                        local.unwrap_or(initial_deck_position(ride.seat)),
                        time.elapsed_secs_f64(),
                    )
            } else {
                avatars.player_deck_motion.remove(&player.id);
                vehicle.is_none()
                    && previous.xz().distance_squared(transform.translation.xz()) > 0.000001
            };
            if walking {
                moving.push(entity);
            }
            transform.rotation = Quat::from_rotation_y(-vehicle.map_or(yaw, |v| v.heading));
            if let Some(v) = vehicle {
                transform.translation.y += match v.kind {
                    rubblekin_core::vehicles::VehicleKind::Bike => 0.12,
                    rubblekin_core::vehicles::VehicleKind::Kayak => -0.42,
                    rubblekin_core::vehicles::VehicleKind::Sailboat => 0.08,
                };
            }
        }
    }
    let npc = *avatars.npc.get_or_insert_with(|| {
        let entity = character(&mut commands, &mut meshes, &mut materials, true);
        commands
            .entity(entity)
            .insert(Transform::from_translation(Vec3::from_array(
                session.npc.position,
            )));
        entity
    });
    if let Ok(mut transform) = transforms.get_mut(npc) {
        let previous = transform.translation;
        transform.translation = smoothed_avatar_position(
            &world.0,
            previous,
            &mut positions,
            session.players.len(),
            (time.delta_secs() * 12.0).min(1.0),
        );
        if previous.xz().distance_squared(transform.translation.xz()) > 0.000001 {
            moving.push(npc);
        }
        if let Some(target) = session.npc.target {
            let delta = Vec3::from_array(target) - transform.translation;
            if delta.x * delta.x + delta.z * delta.z > 0.03 {
                transform.rotation = Quat::from_rotation_y((-delta.x).atan2(-delta.z));
            }
        }
    }
    let resident_ids: Vec<_> = session.residents.iter().map(|r| r.id).collect();
    avatars.residents.retain(|id, entity| {
        if !resident_ids.contains(id) {
            commands.entity(*entity).despawn();
            false
        } else {
            true
        }
    });
    avatars
        .resident_deck_motion
        .retain(|id, _| resident_ids.contains(id));
    for (index, resident) in session.residents.iter().enumerate() {
        let desired = Vec3::from_array(positions[session.players.len() + 1 + index]);
        let entity = *avatars.residents.entry(resident.id).or_insert_with(|| {
            let entity = character_with_role(
                &mut commands,
                &mut meshes,
                &mut materials,
                true,
                Some(resident.role),
            );
            commands
                .entity(entity)
                .insert(Transform::from_translation(desired));
            entity
        });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let previous = transform.translation;
            transform.translation = if resident.ride.is_some() {
                desired
            } else {
                smoothed_avatar_position(
                    &world.0,
                    previous,
                    &mut positions,
                    session.players.len() + 1 + index,
                    (time.delta_secs() * 12.0).min(1.0),
                )
            };
            let walking = if let Some(ride) = resident.ride {
                avatars
                    .resident_deck_motion
                    .entry(resident.id)
                    .or_default()
                    .walking(
                        ride,
                        resident
                            .deck_position
                            .unwrap_or(initial_deck_position(ride.seat)),
                        time.elapsed_secs_f64(),
                    )
            } else {
                avatars.resident_deck_motion.remove(&resident.id);
                previous.xz().distance_squared(transform.translation.xz()) > 0.000001
            };
            if walking {
                moving.push(entity);
            }
            if let Some(target) = resident.target {
                let delta = Vec3::from_array(target) - transform.translation;
                if delta.x * delta.x + delta.z * delta.z > 0.03 {
                    transform.rotation = Quat::from_rotation_y((-delta.x).atan2(-delta.z));
                }
            }
        }
    }
    for (mut visibility, parent) in &mut cargo {
        *visibility =
            if session.residents.iter().any(|r| {
                avatars.residents.get(&r.id) == Some(&parent.parent()) && r.carrying.is_some()
            }) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
    }
    for (mut visibility, parent) in &mut shadows {
        let grounded = if Some(parent.parent()) == avatars.npc {
            let p = session.npc.position;
            world
                .0
                .raycast([p[0], p[1] + 0.05, p[2]], [0.0, -1.0, 0.0], 0.15)
                .is_some()
        } else if let Some(resident) = session
            .residents
            .iter()
            .find(|r| avatars.residents.get(&r.id) == Some(&parent.parent()))
        {
            let p = resident.position;
            world
                .0
                .raycast([p[0], p[1] + 0.05, p[2]], [0.0, -1.0, 0.0], 0.15)
                .is_some()
        } else {
            session
                .players
                .iter()
                .find(|p| avatars.players.get(&p.id) == Some(&parent.parent()))
                .is_some_and(|p| {
                    if p.id == session.id {
                        session.body.on_ground
                    } else {
                        p.body.on_ground
                    }
                })
        };
        *visibility = if grounded && !session.graphics.shadows() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut transform, limb, parent) in &mut limbs {
        transform.translation.y = if limb.arm { 1.0 } else { 0.38 };
        transform.translation.z = 0.;
        let mounted = session
            .players
            .iter()
            .find(|p| avatars.players.get(&p.id) == Some(&parent.parent()))
            .and_then(|p| {
                if p.id == session.id {
                    session.vehicle
                } else {
                    p.vehicle
                }
            });
        if let Some(v) = mounted {
            use rubblekin_core::vehicles::VehicleKind;
            let pedalling = (time.elapsed_secs() * 5. + limb.phase).sin() * 0.25;
            if !limb.arm && v.kind != VehicleKind::Sailboat {
                transform.translation.y = 0.65;
                transform.translation.z = -0.20;
            }
            transform.rotation = Quat::from_rotation_x(match (v.kind, limb.arm) {
                (VehicleKind::Bike, true) => 0.65,
                (VehicleKind::Bike, false) => 0.9 + pedalling,
                (VehicleKind::Kayak, true) => 0.5 + pedalling,
                (VehicleKind::Kayak, false) => 1.35,
                (VehicleKind::Sailboat, _) => 0.,
            });
            continue;
        }
        let resident = session
            .residents
            .iter()
            .find(|r| avatars.residents.get(&r.id) == Some(&parent.parent()));
        let work_angle = if limb.arm {
            resident.and_then(|resident| {
                let phase = time.elapsed_secs() * 4.0 + resident.id as f32 * 0.6;
                match resident.action {
                    ResidentAction::Planting => Some(0.75 + phase.sin() * 0.28),
                    ResidentAction::Tending => Some(0.85 + phase.sin() * 0.38),
                    ResidentAction::Harvesting => Some(1.1 + phase.sin() * 0.4),
                    ResidentAction::Working => Some(0.65 + (phase + limb.phase).sin() * 0.45),
                    ResidentAction::Eating => Some(1.8 + phase.sin() * 0.12),
                    _ => None,
                }
            })
        } else {
            None
        };
        transform.rotation = Quat::from_rotation_x(if let Some(angle) = work_angle {
            angle
        } else if moving.contains(&parent.parent()) {
            (time.elapsed_secs() * 9.0 + limb.phase).sin() * 0.4
        } else {
            0.0
        });
    }
}
