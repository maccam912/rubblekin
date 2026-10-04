mod follow_camera;
mod graphics;
mod join;
mod network;
mod observer;
mod prediction;
mod terrain;
mod ui;

use bevy::{
    app::AppExit,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    light::{
        CascadeShadowConfig, DirectionalLightShadowMap, NotShadowCaster, ShadowFilteringMethod,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{CursorGrabMode, CursorOptions, PresentMode, WindowResolution},
};
use follow_camera::CameraFollow;
use graphics::GraphicsQuality;
use network::Connection;
use observer::ObserverCamera;
use prediction::Prediction;
use rubblekin_core::{
    physics::{Body, EYE_HEIGHT, MoveInput},
    protocol::*,
    world::{Block, BlockPos, CELL_SIZE, World as GameWorld},
};
use rubblekin_server::ServerConfig;
use std::{collections::HashMap, path::PathBuf};

pub const PALETTE: [Block; 6] = [
    Block::Grass,
    Block::Dirt,
    Block::Stone,
    Block::Wood,
    Block::Brick,
    Block::Glass,
];

#[derive(Resource)]
pub struct VoxelWorld(pub GameWorld);

#[derive(Resource)]
pub struct Session {
    pub id: u64,
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub camera_distance: f32,
    pub selected: usize,
    pub flying: bool,
    pub captured: bool,
    pub can_admin: bool,
    pub inspector: bool,
    pub help: bool,
    pub graphics: GraphicsQuality,
    pub npc: NpcSnapshot,
    pub players: Vec<PlayerSnapshot>,
    pub world_time: f64,
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

#[derive(Resource, Default)]
struct Avatars {
    players: HashMap<u64, Entity>,
    npc: Option<Entity>,
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
struct Limb {
    phase: f32,
}
#[derive(Resource)]
struct Capture {
    path: Option<String>,
    taken: bool,
    exit_after: Option<f32>,
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
    graphics: GraphicsQuality,
    seed: Option<u32>,
    observe: bool,
}

fn options() -> Result<Options, String> {
    let mut result = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--local" => result.local = true,
            "--observe" => result.observe = true,
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
            "--low" => result.graphics = GraphicsQuality::Low,
            "--balanced" => result.graphics = GraphicsQuality::Balanced,
            "--high" => result.graphics = GraphicsQuality::High,
            "--help" | "-h" => {
                println!(
                    "Rubblekin — a living voxel valley\n\nRun without arguments to choose a server or local world.\n  --local              Start and join your local world immediately\n  --connect HOST:PORT   Join an existing server\n  --observe            Read-only admin camera; no player avatar\n  --bind HOST:PORT      Local host address (default 127.0.0.1:7878)\n  --save PATH           World save (default saves/valley.json)\n  --name NAME           Your display name\n  --seed NUMBER         Seed for a new world (default 42)\n  --low                 Baked shading and character ground shadows\n  --balanced            Nearby sun shadows, no MSAA (default)\n  --high                Longer shadows and 4x MSAA\n  --screenshot PATH     Capture the scene after 8 seconds\n  --exit-after SECONDS  Exit automatically for visual testing\n\nWASD move | mouse look after click | Space jump | Shift sprint\nLeft click dig | Right click build | 1–6 material | F creative flight\nQ/E lower/raise in flight | scroll zoom | Tab inspect forager\nF2 graphics | F6/F7/F8 forager override | F9 reset needs | F12 screenshot\nObserver: WASD fly | Q/E vertical | Shift boost | scroll speed | R / Home return\nEscape release cursor | F10 leave world | H controls | close window to quit"
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

fn main() {
    if let Err(error) = run() {
        eprintln!("Rubblekin: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = options().map_err(std::io::Error::other)?;
    let mut menu = join::JoinScreen::new(
        options
            .connect
            .clone()
            .unwrap_or_else(|| "rubblekin.oci.koski.co:7878".into()),
        options.name.unwrap_or_else(|| "Wayfarer".into()),
        ServerConfig {
            bind_addr: options.bind.unwrap_or_else(|| "127.0.0.1:7878".into()),
            save_path: options
                .save
                .unwrap_or_else(|| PathBuf::from("saves/valley.json")),
            seed: options.seed.unwrap_or(42),
            allow_admin: true,
        },
        options.graphics,
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
            size: options.graphics.shadow_map_size(),
        })
        .insert_resource(menu)
        .insert_resource(Capture {
            path: screenshot,
            taken: false,
            exit_after: options.exit_after,
        })
        .init_resource::<Avatars>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Rubblekin · The first valley".into(),
                resolution: WindowResolution::new(1440, 900).with_scale_factor_override(1.0),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_message::<join::MenuKey>()
        .add_systems(Startup, join::setup)
        .add_systems(
            Update,
            (
                join::native_input,
                join::interact,
                join::poll_connection,
                (setup, ui::setup_ui).run_if(resource_added::<Session>),
                (
                    receive_network,
                    controls,
                    camera,
                    edit_blocks,
                    update_avatars,
                    ui::update_ui,
                    join::leave_world,
                )
                    .chain()
                    .run_if(resource_exists::<Session>),
                join::refresh,
                capture_frame,
            )
                .chain(),
        )
        .run();
    Ok(())
}

fn setup(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let start = std::time::Instant::now();
    let terrain = terrain::setup_terrain(&mut commands, &mut meshes, &mut materials, &world.0);
    commands.insert_resource(terrain);
    info!("Terrain generated in {:.2}s", start.elapsed().as_secs_f32());
    commands.spawn((
        GameEntity,
        DirectionalLight {
            illuminance: 10500.0,
            color: Color::srgb(1.0, 0.94, 0.85),
            shadow_maps_enabled: session.graphics.shadows(),
            ..default()
        },
        session.graphics.cascades(),
        Transform::from_xyz(-40.0, 65.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        GameEntity,
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: 1000.0,
            fov: 60.0_f32.to_radians(),
            ..default()
        }),
        DistanceFog {
            color: Color::srgb(0.61, 0.76, 0.81),
            falloff: FogFalloff::Linear {
                start: 65.0,
                end: 580.0,
            },
            ..default()
        },
        session.graphics.msaa(),
        session.graphics.shadow_filter(),
        Transform::from_translation(
            Vec3::from_array(session.body.position) + Vec3::new(0.0, 4.0, 6.0),
        ),
        GameCamera,
        CameraFollow::default(),
        IsDefaultUiCamera,
    ));
}

fn receive_network(
    mut commands: Commands,
    mut connection: ResMut<Connection>,
    mut world: ResMut<VoxelWorld>,
    mut session: ResMut<Session>,
    mut scene: ResMut<terrain::TerrainScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
) {
    let mut latest_authoritative = None;
    for message in connection.poll() {
        match message {
            ServerMessage::State {
                players,
                npc,
                world_time,
            } => {
                if session.observer.is_none()
                    && let Some(authoritative) =
                        players.iter().find(|player| player.id == session.id)
                {
                    latest_authoritative = Some(authoritative.clone());
                }
                session.players = players;
                session.npc = npc;
                session.world_time = world_time;
            }
            ServerMessage::BlockChanged { edit, .. } => {
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
            _ => {}
        }
    }
    // A stalled render frame can receive several snapshots. Replay once from
    // the newest acknowledgment, with all received terrain edits available.
    if let Some(authoritative) = latest_authoritative {
        let session = &mut *session;
        if let Err(error) =
            session
                .prediction
                .reconcile(&world.0, &mut session.body, &authoritative)
        {
            connection.fail(error.into());
        }
    }
    if let Some(error) = &connection.error {
        session.status = format!("Disconnected: {error}");
        session.status_until = f64::INFINITY;
    }
}

#[allow(clippy::too_many_arguments)]
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
    mut lights: Query<(&mut DirectionalLight, &mut CascadeShadowConfig)>,
    mut cameras: Query<(&mut Msaa, &mut ShadowFilteringMethod), With<GameCamera>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let dt = time.delta_secs().min(MAX_INPUT_DT);
    let observing = session.observer.is_some();
    if keys.just_pressed(KeyCode::Escape) || !window.focused {
        session.captured = false;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if mouse.just_pressed(MouseButton::Left) && !session.captured {
        session.captured = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        session.edit_clock = 0.3;
    }
    if session.captured {
        let (min_pitch, max_pitch) = if observing { (-1.5, 1.5) } else { (-0.6, 1.2) };
        session.yaw += motion.delta.x * 0.0025;
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
        session.pitch = (session.pitch + motion.delta.y * 0.0025).clamp(min_pitch, max_pitch);
        if let Some(observer) = &mut session.observer {
            observer.adjust_speed(scroll.delta.y);
        } else {
            session.camera_distance =
                (session.camera_distance - scroll.delta.y * 0.45).clamp(1.0, 9.0);
        }
    }
    if !observing && keys.just_pressed(KeyCode::KeyF) {
        session.flying = !session.flying;
    }
    if observing
        && window.focused
        && (keys.just_pressed(KeyCode::Home) || keys.just_pressed(KeyCode::KeyR))
    {
        session.observer = Some(ObserverCamera::new(world.0.spawn_position()));
        session.yaw = -0.45;
        session.pitch = 0.12;
    }
    if keys.just_pressed(KeyCode::Tab) {
        session.inspector = !session.inspector;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        session.help = !session.help;
    }
    if keys.just_pressed(KeyCode::F2) {
        session.graphics = session.graphics.next();
        shadow_map.size = session.graphics.shadow_map_size();
        for (mut light, mut cascades) in &mut lights {
            light.shadow_maps_enabled = session.graphics.shadows();
            *cascades = session.graphics.cascades();
        }
        for (mut msaa, mut filter) in &mut cameras {
            *msaa = session.graphics.msaa();
            *filter = session.graphics.shadow_filter();
        }
    }
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
            session.selected = i;
        }
    }
    if session.can_admin && !observing {
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
    if session.captured && window.focused && connection.error.is_none() {
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
    if connection.error.is_none() && dt > 0.0 {
        let session = &mut *session;
        if let Some(observer) = &mut session.observer {
            observer.advance(observer_input, session.yaw, session.pitch, input.sprint, dt);
        } else {
            match session
                .prediction
                .advance(&world.0, &mut session.body, input, session.yaw, dt)
            {
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
    let follow_eye = follow.advance(eye, time.delta_secs());
    **transform = follow_camera::transform(
        &world.0,
        eye,
        follow_eye,
        session.yaw,
        session.pitch,
        session.camera_distance,
    );
}

fn edit_blocks(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<&Transform, With<GameCamera>>,
    world: Res<VoxelWorld>,
    mut session: ResMut<Session>,
    mut connection: ResMut<Connection>,
    mut gizmos: Gizmos,
) {
    if session.inspector
        && let Some(target) = session.npc.target
    {
        gizmos.line(
            Vec3::from_array(session.npc.position) + Vec3::Y,
            Vec3::from_array(target) + Vec3::Y * 0.15,
            Color::srgba(0.97, 0.71, 0.28, 0.8),
        );
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
        let repeated = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let dig =
            mouse.just_pressed(MouseButton::Left) || repeated && mouse.pressed(MouseButton::Left);
        let build =
            mouse.just_pressed(MouseButton::Right) || repeated && mouse.pressed(MouseButton::Right);
        if session.captured
            && session.edit_clock <= 0.0
            && connection.error.is_none()
            && (dig || build)
        {
            let block = if dig {
                Block::Air
            } else {
                PALETTE[session.selected]
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
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let skin = materials.add(Color::srgb(0.79, 0.56, 0.36));
    let cloth = materials.add(if npc {
        Color::srgb(0.74, 0.42, 0.18)
    } else {
        Color::srgb(0.20, 0.43, 0.47)
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
                Limb { phase },
            ));
            parent.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(cloth.clone()),
                Transform::from_xyz(x.signum() * 0.31, 1.0, 0.0)
                    .with_scale(Vec3::new(0.16, 0.52, 0.20)),
                Limb {
                    phase: phase + std::f32::consts::PI,
                },
            ));
        }
    });
    id
}

#[allow(clippy::too_many_arguments)]
fn update_avatars(
    mut commands: Commands,
    session: Res<Session>,
    mut avatars: ResMut<Avatars>,
    mut transforms: Query<&mut Transform, With<Avatar>>,
    mut limbs: Query<(&mut Transform, &Limb, &ChildOf), Without<Avatar>>,
    mut shadows: Query<(&mut Visibility, &ChildOf), With<GroundShadow>>,
    world: Res<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let live: Vec<u64> = session.players.iter().map(|p| p.id).collect();
    avatars.players.retain(|id, entity| {
        if !live.contains(id) {
            commands.entity(*entity).despawn();
            false
        } else {
            true
        }
    });
    for player in &session.players {
        let entity = *avatars
            .players
            .entry(player.id)
            .or_insert_with(|| character(&mut commands, &mut meshes, &mut materials, false));
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let (position, yaw) = if player.id == session.id {
                (session.body.position, session.yaw)
            } else {
                (player.body.position, player.yaw)
            };
            transform.translation = if player.id == session.id {
                Vec3::from_array(position)
            } else {
                transform.translation.lerp(
                    Vec3::from_array(position),
                    (time.delta_secs() * 15.0).min(1.0),
                )
            };
            transform.rotation = Quat::from_rotation_y(-yaw);
        }
    }
    let npc = *avatars
        .npc
        .get_or_insert_with(|| character(&mut commands, &mut meshes, &mut materials, true));
    if let Ok(mut transform) = transforms.get_mut(npc) {
        transform.translation = transform.translation.lerp(
            Vec3::from_array(session.npc.position),
            (time.delta_secs() * 12.0).min(1.0),
        );
        if let Some(target) = session.npc.target {
            let delta = Vec3::from_array(target) - transform.translation;
            if delta.x * delta.x + delta.z * delta.z > 0.03 {
                transform.rotation = Quat::from_rotation_y((-delta.x).atan2(-delta.z));
            }
        }
    }
    for (mut visibility, parent) in &mut shadows {
        let grounded = if Some(parent.parent()) == avatars.npc {
            let p = session.npc.position;
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
        let moving = if Some(parent.parent()) == avatars.npc {
            session.npc.action != NpcAction::Rest && session.npc.target.is_some()
        } else {
            session
                .players
                .iter()
                .find(|p| avatars.players.get(&p.id) == Some(&parent.parent()))
                .map(|p| {
                    let v = if p.id == session.id {
                        session.body.velocity
                    } else {
                        p.body.velocity
                    };
                    v[0].abs() + v[2].abs() > 0.1
                })
                .unwrap_or(false)
        };
        transform.rotation = Quat::from_rotation_x(if moving {
            (time.elapsed_secs() * 9.0 + limb.phase).sin() * 0.4
        } else {
            0.0
        });
    }
}

fn capture_frame(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut capture: ResMut<Capture>,
    session: Option<Res<Session>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::F12) {
        let _ = std::fs::create_dir_all("artifacts");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!(
                "artifacts/screenshot-{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            )));
    }
    if !capture.taken && time.elapsed_secs() > 8.0 {
        if let Some(path) = &capture.path {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            if let Some(session) = session {
                info!(
                    "Visual capture: {:.1} FPS; {} explorers; {} terrain edits; NPC {}",
                    session.fps,
                    session.players.len(),
                    session.edits,
                    session.npc.action.label()
                );
            }
        }
        capture.taken = true;
    }
    if capture
        .exit_after
        .is_some_and(|after| time.elapsed_secs() > after)
    {
        exit.write(AppExit::Success);
    }
}
