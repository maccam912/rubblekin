//! Pictures and emblems refer to the existing sealed market contract.
use crate::{GameEntity, Session, VoxelWorld, market::MarketPanel, terrain::Geometry};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{
    settlement::Village,
    world::{BlockPos, CELL_SIZE, World},
};
use std::collections::HashMap;

// Ten recognizable shapes for the generator's ten stable village IDs. Shape,
// rather than color or a town name, connects parcel, sign and destination.
const EMBLEMS: [[&str; 7]; 10] = [
    [
        "...#...", "..###..", ".#####.", "#######", "...#...", "...#...", "..###..",
    ], // tree
    [
        ".....#.", "..##.##", ".######", "#######", ".######", "..##.##", ".....#.",
    ], // fish
    [
        "...#...", "..###..", ".#####.", "##...##", "#######", "#######", "#######",
    ], // mountain
    [
        "#..#..#", ".#####.", ".#####.", "#######", ".#####.", ".#####.", "#..#..#",
    ], // sun
    [
        "..####.", ".###...", "###....", "###....", "###....", ".###...", "..####.",
    ], // moon
    [
        "..###..", ".#.#.#.", "##.#.##", ".#####.", "...#...", ".#.#.#.", "..###..",
    ], // flower
    [
        "...#...", "..###..", ".#####.", "#######", ".#...#.", ".#.###.", ".#.###.",
    ], // house
    [
        "...#...", "...##..", "...###.", "...#...", "#######", ".#####.", "..###..",
    ], // boat
    [
        "....###", "...####", "..##.##", ".##.##.", "##.##..", "####...", "###....",
    ], // leaf
    [
        "...#...", "..###..", ".#####.", "..###..", "...#...", "..#....", "...##..",
    ], // kite
];
const CREAM: [f32; 4] = [0.97, 0.89, 0.65, 1.];
const GREEN: [f32; 4] = [0.07, 0.18, 0.15, 1.];
const PAPER: [f32; 4] = [0.67, 0.49, 0.29, 1.];

fn image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
fn rect(pixels: &mut [u8], width: usize, height: usize, bounds: [i32; 4], color: [f32; 4]) {
    for y in bounds[1].max(0)..bounds[3].min(height as i32) {
        for x in bounds[0].max(0)..bounds[2].min(width as i32) {
            let at = (y as usize * width + x as usize) * 4;
            for n in 0..4 {
                pixels[at + n] = (color[n] * 255.) as u8;
            }
        }
    }
}
fn emblem_cells(id: u32) -> impl Iterator<Item = (usize, usize)> {
    EMBLEMS.get(id as usize).into_iter().flat_map(|rows| {
        rows.iter().enumerate().flat_map(|(y, row)| {
            row.bytes()
                .enumerate()
                .filter_map(move |(x, cell)| (cell == b'#').then_some((x, y)))
        })
    })
}
fn emblem_picture(id: u32, parcel: bool) -> Image {
    let mut pixels = vec![0; 64 * 64 * 4];
    rect(
        &mut pixels,
        64,
        64,
        [3, 3, 61, 61],
        if parcel { PAPER } else { GREEN },
    );
    if parcel {
        rect(&mut pixels, 64, 64, [3, 28, 61, 36], CREAM);
        rect(&mut pixels, 64, 64, [28, 3, 36, 61], CREAM);
        rect(&mut pixels, 64, 64, [10, 10, 54, 54], GREEN);
    }
    for (x, y) in emblem_cells(id) {
        let cell = if parcel { 5 } else { 7 };
        let margin = if parcel { 14 } else { 7 };
        let left = margin + x as i32 * cell;
        let top = margin + y as i32 * cell;
        rect(
            &mut pixels,
            64,
            64,
            [left, top, left + cell, top + cell],
            CREAM,
        );
    }
    image(64, 64, pixels)
}
/// An isometric miniature of actual nearby destination buildings, not a generic
/// stock picture. Includes their saved edits and quarter-turn orientation.
fn destination_picture(world: &World, village: &Village) -> Image {
    let mut buildings: Vec<_> = village.buildings.iter().collect();
    buildings.sort_by(|a, b| {
        let d = |b: &rubblekin_core::settlement::BuildingPlot| {
            (b.origin.x as f32 * CELL_SIZE - village.market[0])
                .hypot(b.origin.z as f32 * CELL_SIZE - village.market[2])
        };
        d(a).total_cmp(&d(b))
    });
    buildings.truncate(3);
    let mut points = Vec::new();
    for building in buildings {
        let [w, h, d] = building.dimensions();
        for x in 0..w {
            for z in 0..d {
                for y in 0..h {
                    let p = BlockPos::new(
                        building.origin.x + x,
                        building.origin.y + y,
                        building.origin.z + z,
                    );
                    let block = world.block(p);
                    if !block.is_solid() {
                        continue;
                    }
                    let dx = p.x as f32 * CELL_SIZE - village.market[0];
                    let dz = p.z as f32 * CELL_SIZE - village.market[2];
                    let dy = p.y as f32 * CELL_SIZE - village.market[1];
                    points.push((
                        [dx - dz, (dx + dz) * 0.38 - dy * 1.1, dx + dz + dy],
                        block.color(),
                    ));
                }
            }
        }
    }
    let mut pixels = vec![0; 128 * 96 * 4];
    rect(
        &mut pixels,
        128,
        96,
        [0, 0, 128, 96],
        [0.12, 0.24, 0.20, 1.],
    );
    if !points.is_empty() {
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for (p, _) in &points {
            for i in 0..2 {
                low[i] = low[i].min(p[i]);
                high[i] = high[i].max(p[i]);
            }
        }
        let scale = (112. / (high[0] - low[0] + 1.)).min(72. / (high[1] - low[1] + 1.));
        points.sort_by(|a, b| a.0[2].total_cmp(&b.0[2]));
        for (p, color) in points {
            let x = (8. + (p[0] - low[0]) * scale) as i32;
            let y = (10. + (p[1] - low[1]) * scale) as i32;
            let size = (scale * 0.65).ceil().max(1.) as i32;
            rect(&mut pixels, 128, 96, [x, y, x + size, y + size], color);
        }
    }
    image(128, 96, pixels)
}
fn parcel_mesh(id: u32) -> Mesh {
    let mut g = Geometry::default();
    g.cuboid(Vec3::ZERO, Vec3::new(0.54, 0.5, 0.36), PAPER);
    g.cuboid(
        Vec3::new(0., 0., -0.185),
        Vec3::new(0.5, 0.06, 0.012),
        CREAM,
    );
    g.cuboid(
        Vec3::new(0., 0., -0.19),
        Vec3::new(0.33, 0.33, 0.012),
        GREEN,
    );
    for (x, y) in emblem_cells(id) {
        g.cuboid(
            Vec3::new((x as f32 - 3.) * 0.04, (3. - y as f32) * 0.04, -0.2),
            Vec3::new(0.04, 0.04, 0.015),
            CREAM,
        );
    }
    g.into_mesh()
}
fn sign_mesh(id: u32) -> Mesh {
    let mut g = Geometry::default();
    g.cuboid(Vec3::new(0., 0.75, 0.), Vec3::new(0.09, 1.5, 0.09), PAPER);
    g.cuboid(Vec3::new(0., 1.65, 0.), Vec3::new(0.9, 0.9, 0.10), GREEN);
    for (x, y) in emblem_cells(id) {
        for z in [-0.06, 0.06] {
            g.cuboid(
                Vec3::new((x as f32 - 3.) * 0.11, 1.65 + (3. - y as f32) * 0.11, z),
                Vec3::new(0.11, 0.11, 0.025),
                CREAM,
            );
        }
    }
    g.into_mesh()
}
struct PlaceArt {
    emblem: Handle<Image>,
    parcel: Handle<Image>,
    thumbnail: Handle<Image>,
    mesh: Handle<Mesh>,
}
#[derive(Resource)]
pub(crate) struct Pictures {
    places: HashMap<u32, PlaceArt>,
    material: Handle<StandardMaterial>,
    carriers: HashMap<u64, Entity>,
    receipt: Option<(u32, f64, Entity, Vec3)>,
}
impl Pictures {
    pub(crate) fn emblem(&self, id: u32) -> Option<Handle<Image>> {
        self.places.get(&id).map(|p| p.emblem.clone())
    }
}
#[derive(Component)]
pub(crate) struct MarketPictures;
#[derive(Component)]
pub(crate) struct PictureSlot(pub usize);
#[derive(Component)]
pub(crate) struct ParcelButtonIcon(pub bool);
#[derive(Component)]
pub(crate) struct Pinned;
#[derive(Component)]
pub(crate) struct PinnedName;
#[derive(Component)]
pub(crate) struct Parcel;

#[allow(clippy::too_many_arguments)]
pub(crate) fn setup(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    touch: Res<crate::touch::TouchControls>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let material = materials.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    let mut places = HashMap::new();
    if let Some(plan) = world.0.settlements() {
        for village in &plan.villages {
            places.insert(
                village.id,
                PlaceArt {
                    emblem: images.add(emblem_picture(village.id, false)),
                    parcel: images.add(emblem_picture(village.id, true)),
                    thumbnail: images.add(destination_picture(&world.0, village)),
                    mesh: meshes.add(parcel_mesh(village.id)),
                },
            );
            let sign = meshes.add(sign_mesh(village.id));
            for position in std::iter::once(village.market).chain(
                session
                    .whip_stations
                    .iter()
                    .filter(|s| s.village_id == village.id)
                    .map(|s| s.position),
            ) {
                // Decorative sign beside the interaction point, never a collision body.
                let p = Vec3::from_array(position) + Vec3::X * 1.2;
                commands.spawn((
                    GameEntity,
                    Mesh3d(sign.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(p.x, world.0.surface_height(p.x, p.z), p.z),
                ));
            }
        }
    }
    commands.insert_resource(Pictures {
        places,
        material,
        carriers: HashMap::new(),
        receipt: None,
    });
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            Pinned,
            Node {
                position_type: PositionType::Absolute,
                left: px(12.),
                top: px(if touch.enabled { 104. } else { 94. }),
                width: px(if touch.enabled { 154. } else { 218. }),
                padding: UiRect::all(px(8.)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.),
                display: Display::None,
                border_radius: BorderRadius::all(px(8.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.10, 0.09, 0.93)),
        ))
        .with_children(|p| {
            p.spawn((
                PinnedName,
                Text::new(""),
                TextFont::from_font_size(16.).with_font(font),
                TextColor(Color::srgb(0.95, 0.85, 0.65)),
            ));
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.),
                ..default()
            })
            .with_children(|row| {
                picture_row(row);
            });
        });
}
pub(crate) fn picture_row(row: &mut ChildSpawnerCommands) {
    for (i, w, h) in [(0, 32., 32.), (1, 48., 48.), (2, 96., 72.)] {
        row.spawn((
            PictureSlot(i),
            ImageNode::default(),
            Node {
                width: px(w),
                height: px(h),
                flex_shrink: 0.,
                ..default()
            },
        ));
    }
}
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    mut session: ResMut<Session>,
    world: Res<VoxelWorld>,
    mut panel: ResMut<MarketPanel>,
    touch: Res<crate::touch::TouchControls>,
    mut pictures: ResMut<Pictures>,
    mut roots: Query<(&mut Node, Option<&Pinned>), Or<(With<Pinned>, With<MarketPictures>)>>,
    mut slots: Query<
        (&PictureSlot, &ChildOf, &mut ImageNode, &mut Node),
        (
            Without<ParcelButtonIcon>,
            Without<Pinned>,
            Without<MarketPictures>,
        ),
    >,
    mut icons: Query<(&ParcelButtonIcon, &mut ImageNode), Without<PictureSlot>>,
    parents: Query<Option<&ChildOf>, Without<PictureSlot>>,
    mut names: Query<&mut Text, With<PinnedName>>,
    mut props: Query<(&mut Transform, &mut Mesh3d), With<Parcel>>,
    avatars: Option<Res<crate::Avatars>>,
    poses: Query<&Transform, (With<crate::Avatar>, Without<Parcel>)>,
    time: Res<Time>,
) {
    if let Some(receipt) = panel.completed_delivery.take()
        && let Some(mesh) = pictures
            .places
            .get(&receipt.destination)
            .map(|art| art.mesh.clone())
        && let Some(village) = world
            .0
            .settlements()
            .and_then(|p| p.villages.iter().find(|v| v.id == receipt.destination))
    {
        if let Some((_, _, e, _)) = pictures.receipt.take() {
            commands.entity(e).despawn();
        }
        let e = commands
            .spawn((
                GameEntity,
                Parcel,
                Mesh3d(mesh),
                MeshMaterial3d(pictures.material.clone()),
                Transform::from_translation(Vec3::from_array(village.market) + Vec3::Y),
            ))
            .id();
        pictures.receipt = Some((
            receipt.destination,
            time.elapsed_secs_f64(),
            e,
            Vec3::from_array(session.body.position),
        ));
    }
    if let Some((id, began, e, start)) = pictures.receipt {
        let t = (time.elapsed_secs_f64() - began) as f32;
        if t > 4. {
            commands.entity(e).despawn();
            pictures.receipt = None;
        } else if let Some(village) = world
            .0
            .settlements()
            .and_then(|p| p.villages.iter().find(|v| v.id == id))
            && let Ok((mut transform, _)) = props.get_mut(e)
        {
            let sign = Vec3::from_array(village.market) + Vec3::X * 1.2;
            transform.translation = start.lerp(sign, (t / 1.2).clamp(0., 1.)) + Vec3::Y * 0.9;
        }
    }
    let job = panel.delivery();
    session.parcel_market = job.and_then(|j| {
        world
            .0
            .settlements()?
            .villages
            .iter()
            .find(|v| v.id == j.destination)
            .map(|v| v.market)
    });
    let offer = panel.picture_offer(&session, &world);
    for (icon, mut image) in &mut icons {
        if let Some(j) = offer {
            let id = if icon.0 { j.origin } else { j.destination };
            if let Some(art) = pictures.places.get(&id) {
                image.image = art.parcel.clone();
            }
        }
    }
    let hidden = session.observer.is_some()
        || session.help
        || session.inspector
        || session.inventory.open
        || panel.open
        || touch.menu_open;
    for (mut node, pinned) in &mut roots {
        node.display = if if pinned.is_some() {
            job.is_some() && !hidden
        } else {
            offer.is_some()
        } {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut text in &mut names {
        let name = job
            .and_then(|j| {
                world
                    .0
                    .settlements()?
                    .villages
                    .iter()
                    .find(|v| v.id == j.destination)
            })
            .map_or("", |v| v.name.as_str());
        if text.0 != name {
            text.0 = name.into();
        }
    }
    for (slot, parent, mut img, mut node) in &mut slots {
        let mut ancestor = parent.parent();
        let mut pinned = false;
        for _ in 0..3 {
            if roots.get(ancestor).is_ok_and(|(_, p)| p.is_some()) {
                pinned = true;
                break;
            }
            let Ok(Some(p)) = parents.get(ancestor) else {
                break;
            };
            ancestor = p.parent();
        }
        if pinned && touch.enabled {
            let (w, h) = match slot.0 {
                0 => (24., 24.),
                1 => (36., 36.),
                _ => (64., 48.),
            };
            if node.width != px(w) || node.height != px(h) {
                node.width = px(w);
                node.height = px(h);
            }
        }
        if let Some(j) = if pinned { job } else { offer } {
            let art = if slot.0 == 0 {
                pictures.places.get(&j.origin)
            } else {
                pictures.places.get(&j.destination)
            };
            if let Some(art) = art {
                img.image = match slot.0 {
                    0 => art.emblem.clone(),
                    1 => art.parcel.clone(),
                    _ => art.thumbnail.clone(),
                };
            }
        }
    }
    pictures.carriers.retain(|_, e| props.contains(*e));
    let mut wanted = Vec::new();
    for player in &session.players {
        let destination = if player.id == session.id {
            job.map(|j| j.destination)
        } else {
            player.parcel_destination
        };
        let Some(art) = destination.and_then(|id| pictures.places.get(&id)) else {
            continue;
        };
        let mesh = art.mesh.clone();
        let (position, yaw) = if player.id == session.id {
            (session.body.position, session.yaw)
        } else {
            (player.body.position, player.yaw)
        };
        // Keep hands free for local activity props and work tools; the parcel is worn.
        let fallback = Transform::from_translation(Vec3::from_array(position))
            .with_rotation(Quat::from_rotation_y(-yaw));
        let pose = avatars
            .as_ref()
            .and_then(|a| a.players.get(&player.id))
            .and_then(|e| poses.get(*e).ok())
            .unwrap_or(&fallback);
        let transform = Transform::from_translation(pose.transform_point(Vec3::new(0., 1.05, 0.5)))
            .with_rotation(pose.rotation * Quat::from_rotation_y(std::f32::consts::PI));
        wanted.push(player.id);
        if let Some(e) = pictures.carriers.get(&player.id) {
            if let Ok((mut t, mut m)) = props.get_mut(*e) {
                *t = transform;
                m.0 = mesh;
            }
        } else {
            let e = commands
                .spawn((
                    GameEntity,
                    Parcel,
                    Mesh3d(mesh),
                    MeshMaterial3d(pictures.material.clone()),
                    transform,
                ))
                .id();
            pictures.carriers.insert(player.id, e);
        }
    }
    pictures.carriers.retain(|id, e| {
        if wanted.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        economy::{DeliveryContract, PlayerEconomy},
        protocol::{ServerMessage, SessionMode},
        settlement::ResourceKind,
        world::WorldGeneration,
    };
    #[test]
    fn village_emblems_are_distinct_without_color_and_pictures_follow_actual_edits() {
        let mut all = Vec::new();
        for id in 0..10 {
            let picture = emblem_picture(id, false).data.unwrap();
            assert!(!all.contains(&picture));
            assert!(emblem_cells(id).count() > 10);
            all.push(picture);
        }
        let mut world = World::generate(42, WorldGeneration::GeographyV4);
        let village = world.settlements().unwrap().villages[0].clone();
        let before = destination_picture(&world, &village).data.unwrap();
        assert_eq!(before, destination_picture(&world, &village).data.unwrap());
        for building in &village.buildings {
            let [w, h, d] = building.dimensions();
            for x in 0..w {
                for z in 0..d {
                    for y in 0..h {
                        world
                            .set_block(
                                BlockPos::new(
                                    building.origin.x + x,
                                    building.origin.y + y,
                                    building.origin.z + z,
                                ),
                                rubblekin_core::world::Block::Air,
                            )
                            .unwrap();
                    }
                }
            }
        }
        assert_ne!(before, destination_picture(&world, &village).data.unwrap());
    }
    #[test]
    fn confirmed_local_and_public_remote_parcels_follow_players_and_clear_without_duplicates() {
        let mut welcome = crate::join::tests::welcome(SessionMode::Player);
        if let ServerMessage::Welcome { generation, .. } = &mut welcome {
            *generation = WorldGeneration::GeographyV4;
        }
        let (world, mut session) = crate::join::session_from_welcome(
            welcome,
            "parcel".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.help = false;
        session.inspector = false;
        let id = session.id;
        let mut remote = session.players[0].clone();
        remote.id = id + 1;
        remote.parcel_destination = Some(2);
        session.players.push(remote);
        let mut app = App::new();
        app.insert_resource(VoxelWorld(world))
            .insert_resource(session)
            .init_resource::<Time>()
            .init_resource::<crate::touch::TouchControls>()
            .init_resource::<MarketPanel>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Font>>()
            .add_systems(Startup, setup)
            .add_systems(Update, update);
        let job = DeliveryContract {
            origin: 0,
            destination: 1,
            kind: ResourceKind::Food,
            amount: 6,
            reward: 12,
        };
        app.world_mut().resource_mut::<MarketPanel>().ledger = Some(PlayerEconomy {
            delivery: Some(job.clone()),
            ..default()
        });
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Pictures>().carriers.len(), 2);
        let market = app
            .world()
            .resource::<VoxelWorld>()
            .0
            .settlements()
            .unwrap()
            .villages[1]
            .market;
        assert_eq!(
            app.world().resource::<Session>().parcel_market,
            Some(market)
        );
        let mut pinned = app.world_mut().query_filtered::<&Node, With<Pinned>>();
        assert_eq!(pinned.single(app.world()).unwrap().display, Display::Flex);
        app.world_mut().resource_mut::<Session>().help = true;
        app.update();
        assert_eq!(pinned.single(app.world()).unwrap().display, Display::None);
        app.world_mut()
            .resource_mut::<MarketPanel>()
            .ledger
            .as_mut()
            .unwrap()
            .delivery = None;
        app.world_mut()
            .resource_mut::<MarketPanel>()
            .completed_delivery = Some(job);
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Pictures>().carriers.len(), 1);
        assert!(app.world().resource::<Pictures>().receipt.is_some());
        assert!(app.world().resource::<Session>().parcel_market.is_none());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(5));
        app.world_mut()
            .resource_mut::<Session>()
            .players
            .retain(|p| p.id == id);
        app.update();
        assert!(app.world().resource::<Pictures>().receipt.is_none());
        assert!(app.world().resource::<Pictures>().carriers.is_empty());
    }
}
