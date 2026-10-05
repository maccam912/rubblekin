//! Nearby editable voxels and distant terrain share the same geography.
//! Geographic worlds stream a bounded local square; legacy saves retain their valley.
use std::collections::HashMap;

use crate::{
    graphics::{GraphicsSettings, MAX_TREE_DISTANCE, MIN_TREE_DISTANCE},
    terrain_material::{TerrainMaterial, terrain_material},
};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::PrimitiveTopology,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use rubblekin_core::{
    geography::{Biome, Geography},
    world::{
        Block, BlockPos, CELL_SIZE, CHUNK_SIZE, GeneratedTree, TreeKind, WATER_LEVEL, WORLD_RADIUS,
        World, WorldGeneration, berry_patch_positions,
    },
};

#[cfg(test)]
const DETAIL_RADIUS: i32 = 6;
const DETAIL_JOBS: usize = 2;
const CHUNK_METERS: f32 = CHUNK_SIZE as f32 * CELL_SIZE;
// Eight samples across a maximum 1024 m leaf retain 128 m mountain detail.
const MAX_LOD_TILE_CHUNKS: i32 = 128;
// Buildings bridge the local voxel square; tree range is a client preference.
const LOD_BUILDING_DISTANCE: f32 = 128.0;
const TREE_GRID_METERS: f32 = 12.0;

type ChunkKey = (i32, i32);
type ChunkGeometry = (Geometry, Geometry);

/// Mesh and job ownership stays here so leaving a world cancels its work.
#[derive(Resource)]
pub struct TerrainScene {
    chunks: HashMap<ChunkKey, ChunkMesh>,
    opaque_material: Handle<TerrainMaterial>,
    glass_material: Handle<StandardMaterial>,
    water_material: Handle<StandardMaterial>,
    landscape: Option<Landscape>,
    pending_landscape: Option<LandscapeJob>,
    pending_chunks: HashMap<ChunkKey, Task<ChunkGeometry>>,
    pub triangle_count: usize,
}

struct ChunkMesh {
    entity: Entity,
    opaque: Handle<Mesh>,
    glass: Option<(Entity, Handle<Mesh>)>,
    water: Option<(Entity, Handle<Mesh>)>,
    triangles: usize,
    detailed: bool,
}

struct Landscape {
    center: ChunkKey,
    near_radius: i32,
    tree_distance: f32,
    terrain: Handle<Mesh>,
    water: Handle<Mesh>,
    triangles: usize,
}

struct LandscapeJob {
    near_radius: i32,
    tree_distance: f32,
    task: Task<(ChunkKey, Geometry, Geometry)>,
}

/// Legacy decorative geometry is never added to a geographic world.
#[derive(Component)]
struct DistantScenery;

#[allow(clippy::too_many_arguments)]
pub fn setup_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    terrain_materials: &mut Assets<TerrainMaterial>,
    images: &mut Assets<Image>,
    world: &World,
    center: [f32; 3],
    near_radius: i32,
    tree_distance: f32,
) -> TerrainScene {
    let opaque_material = terrain_materials.add(terrain_material(None));
    let glass_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.77, 0.94, 0.95, 0.36),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.22,
        cull_mode: None,
        ..default()
    });
    let water_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.23, 0.52, 0.59),
        perceptual_roughness: 0.32,
        reflectance: 0.28,
        ..default()
    });
    let mut scene = TerrainScene {
        chunks: HashMap::new(),
        opaque_material,
        glass_material,
        water_material,
        landscape: None,
        pending_landscape: None,
        pending_chunks: HashMap::new(),
        triangle_count: 0,
    };
    if world.geography().is_some() {
        let albedo = images.add(crate::terrain_albedo::distant_albedo(world));
        let mut map_water = terrain_material(Some(albedo.clone()));
        map_water.base.base_color = Color::srgb(0.23, 0.52, 0.59);
        let map_water_material = terrain_materials.add(map_water);
        let landscape_material = terrain_materials.add(terrain_material(Some(albedo)));
        let center = chunk_key(center);
        let (land, water) = landscape_geometry(world, center, near_radius, tree_distance);
        let triangles = (land.indices.len() + water.indices.len()) / 3;
        let terrain = meshes.add(land.into_mesh());
        let water = meshes.add(water.into_mesh());
        commands.spawn((
            crate::GameEntity,
            Mesh3d(terrain.clone()),
            MeshMaterial3d(landscape_material),
            NotShadowCaster,
        ));
        commands.spawn((
            crate::GameEntity,
            Mesh3d(water.clone()),
            MeshMaterial3d(map_water_material),
            NotShadowCaster,
        ));
        scene.landscape = Some(Landscape {
            center,
            near_radius,
            tree_distance,
            terrain,
            water,
            triangles,
        });
        scene.triangle_count += triangles;
        move_local_square(&mut scene, center, near_radius, world, commands, meshes);
    } else {
        let first = (-WORLD_RADIUS).div_euclid(CHUNK_SIZE);
        let last = (WORLD_RADIUS - 1).div_euclid(CHUNK_SIZE);
        for cx in first..=last {
            for cz in first..=last {
                rebuild_one(&mut scene, (cx, cz), world, commands, meshes);
            }
        }
        let scenery_material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            unlit: true,
            ..default()
        });
        commands.spawn((
            crate::GameEntity,
            Mesh3d(meshes.add(distant_mountains(world.seed))),
            MeshMaterial3d(scenery_material),
            DistantScenery,
            NotShadowCaster,
        ));
        commands.spawn((
            crate::GameEntity,
            Mesh3d(meshes.add(river_mesh(world))),
            MeshMaterial3d(scene.water_material.clone()),
            NotShadowCaster,
        ));
    }
    scene
}

/// Only bounded uploads and job scheduling run on the frame thread. The coarse
/// surface remains present while a detailed mesh or a new local square is built.
pub fn stream_terrain(
    world: Res<crate::VoxelWorld>,
    session: Res<crate::Session>,
    settings: Res<GraphicsSettings>,
    mut scene: ResMut<TerrainScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if scene.landscape.is_none() {
        return;
    }
    let position = session
        .observer
        .as_ref()
        .map_or(session.body.position, |camera| camera.position.to_array());
    let center = chunk_key(position);
    let near_radius = settings.near_radius_chunks();
    let tree_distance = settings
        .tree_distance
        .clamp(MIN_TREE_DISTANCE, MAX_TREE_DISTANCE);
    if scene
        .pending_landscape
        .as_ref()
        .is_some_and(|job| job.near_radius != near_radius || job.tree_distance != tree_distance)
    {
        // A cancelled distance change must not install an obsolete cutout.
        scene.pending_landscape = None;
    }
    if let Some((ready_center, land, water)) = scene
        .pending_landscape
        .as_mut()
        .and_then(|job| check_ready(&mut job.task))
    {
        scene.pending_landscape = None;
        // Install the cutout and its local replacement together: no empty ring
        // can appear during rapid flight or a camera reset.
        move_local_square(
            &mut scene,
            ready_center,
            near_radius,
            &world.0,
            &mut commands,
            &mut meshes,
        );
        let next_triangles = (land.indices.len() + water.indices.len()) / 3;
        let landscape = scene.landscape.as_mut().unwrap();
        let old_triangles = landscape.triangles;
        landscape.center = ready_center;
        landscape.near_radius = near_radius;
        landscape.tree_distance = tree_distance;
        landscape.triangles = next_triangles;
        if let Some(mut mesh) = meshes.get_mut(&landscape.terrain) {
            *mesh = land.into_mesh();
        }
        if let Some(mut mesh) = meshes.get_mut(&landscape.water) {
            *mesh = water.into_mesh();
        }
        scene.triangle_count = scene.triangle_count - old_triangles + next_triangles;
    }
    let landscape = scene.landscape.as_ref().unwrap();
    if scene.pending_landscape.is_none()
        && (landscape.center != center
            || landscape.near_radius != near_radius
            || landscape.tree_distance != tree_distance)
    {
        let snapshot = world.0.clone();
        scene.pending_landscape = Some(LandscapeJob {
            near_radius,
            tree_distance,
            task: AsyncComputeTaskPool::get().spawn(async move {
                let (land, water) =
                    landscape_geometry(&snapshot, center, near_radius, tree_distance);
                (center, land, water)
            }),
        });
    }
    let ready: Vec<_> = scene
        .pending_chunks
        .iter_mut()
        .filter_map(|(&key, task)| check_ready(task).map(|mesh| (key, mesh)))
        .collect();
    for (key, geometry) in ready {
        scene.pending_chunks.remove(&key);
        if scene.chunks.contains_key(&key) {
            install_chunk(&mut scene, key, geometry, true, &mut commands, &mut meshes);
        }
    }
    let mut needed: Vec<_> = scene
        .chunks
        .iter()
        .filter(|(key, chunk)| !chunk.detailed && !scene.pending_chunks.contains_key(key))
        .map(|(&key, _)| key)
        .collect();
    needed.sort_unstable_by_key(|&(x, z)| {
        (x as i64 - center.0 as i64).pow(2) + (z as i64 - center.1 as i64).pow(2)
    });
    for key in needed
        .into_iter()
        .take(DETAIL_JOBS.saturating_sub(scene.pending_chunks.len()))
    {
        let snapshot = world.0.clone();
        scene.pending_chunks.insert(
            key,
            AsyncComputeTaskPool::get()
                .spawn(async move { chunk_geometry(&snapshot, key.0, key.1) }),
        );
    }
}

fn chunk_key(position: [f32; 3]) -> ChunkKey {
    (
        (position[0] / CHUNK_METERS).floor() as i32,
        (position[2] / CHUNK_METERS).floor() as i32,
    )
}

fn local_keys(center: ChunkKey, near_radius: i32, world: &World) -> Vec<ChunkKey> {
    let first = (-world.radius_cells()).div_euclid(CHUNK_SIZE);
    let last = (world.radius_cells() - 1).div_euclid(CHUNK_SIZE);
    let mut keys = Vec::new();
    for x in center.0.saturating_sub(near_radius)..=center.0.saturating_add(near_radius) {
        for z in center.1.saturating_sub(near_radius)..=center.1.saturating_add(near_radius) {
            if (first..=last).contains(&x) && (first..=last).contains(&z) {
                keys.push((x, z));
            }
        }
    }
    keys
}

fn move_local_square(
    scene: &mut TerrainScene,
    center: ChunkKey,
    near_radius: i32,
    world: &World,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    let wanted = local_keys(center, near_radius, world);
    let first = (-world.radius_cells()).div_euclid(CHUNK_SIZE);
    let last = (world.radius_cells() - 1).div_euclid(CHUNK_SIZE);
    let wanted_x = center.0.saturating_sub(near_radius).max(first)
        ..=center.0.saturating_add(near_radius).min(last);
    let wanted_z = center.1.saturating_sub(near_radius).max(first)
        ..=center.1.saturating_add(near_radius).min(last);
    let expired: Vec<_> = scene
        .chunks
        .keys()
        .filter(|&&(x, z)| !wanted_x.contains(&x) || !wanted_z.contains(&z))
        .copied()
        .collect();
    for key in expired {
        scene.pending_chunks.remove(&key); // Dropping a task cancels its stale result.
        let chunk = scene.chunks.remove(&key).unwrap();
        scene.triangle_count -= chunk.triangles;
        commands.entity(chunk.entity).despawn();
        meshes.remove(chunk.opaque.id());
        for (entity, handle) in chunk.glass.into_iter().chain(chunk.water) {
            commands.entity(entity).despawn();
            meshes.remove(handle.id());
        }
    }
    for key in wanted {
        if scene.chunks.contains_key(&key) {
            continue;
        }
        let mut terrain = Geometry::default();
        let mut water = Geometry::default();
        let surface = surface_tile(
            world.geography().unwrap(),
            [
                key.0 as f32 * CHUNK_METERS,
                key.1 as f32 * CHUNK_METERS,
                CHUNK_METERS,
            ],
            8,
            [None; 4],
            &mut terrain,
            &mut water,
        );
        add_chunk_tree_proxies(world, key, &surface, &mut terrain);
        let x = key.0 as f32 * CHUNK_METERS;
        let z = key.1 as f32 * CHUNK_METERS;
        add_village_proxies(
            world,
            center,
            ProxyClip::Inside([x, z, x + CHUNK_METERS, z + CHUNK_METERS]),
            &mut terrain,
        );
        install_chunk(
            scene,
            key,
            (terrain, Geometry::default()),
            false,
            commands,
            meshes,
        );
        if !water.indices.is_empty() {
            let triangles = water.indices.len() / 3;
            let handle = meshes.add(water.into_mesh());
            let entity = commands
                .spawn((
                    crate::GameEntity,
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(scene.water_material.clone()),
                    NotShadowCaster,
                ))
                .id();
            let chunk = scene.chunks.get_mut(&key).unwrap();
            chunk.water = Some((entity, handle));
            chunk.triangles += triangles;
            scene.triangle_count += triangles;
        }
    }
}

pub fn rebuild_chunks(
    scene: &mut TerrainScene,
    changed: BlockPos,
    world: &World,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    for key in affected_chunks(changed) {
        // Remove any old snapshot before installing an accepted terrain edit.
        // Completed-but-unpolled work cannot overwrite the current world.
        scene.pending_chunks.remove(&key);
        if scene.chunks.contains_key(&key) {
            rebuild_one(scene, key, world, commands, meshes);
        }
    }
}

fn affected_chunks(changed: BlockPos) -> Vec<ChunkKey> {
    let cx = changed.x.div_euclid(CHUNK_SIZE);
    let cz = changed.z.div_euclid(CHUNK_SIZE);
    let mut xs = vec![cx];
    let mut zs = vec![cz];
    if changed.x.rem_euclid(CHUNK_SIZE) == 0 {
        xs.push(cx - 1);
    } else if changed.x.rem_euclid(CHUNK_SIZE) == CHUNK_SIZE - 1 {
        xs.push(cx + 1);
    }
    if changed.z.rem_euclid(CHUNK_SIZE) == 0 {
        zs.push(cz - 1);
    } else if changed.z.rem_euclid(CHUNK_SIZE) == CHUNK_SIZE - 1 {
        zs.push(cz + 1);
    }
    xs.into_iter()
        .flat_map(|x| zs.iter().map(move |z| (x, *z)))
        .collect()
}

fn rebuild_one(
    scene: &mut TerrainScene,
    key: ChunkKey,
    world: &World,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    install_chunk(
        scene,
        key,
        chunk_geometry(world, key.0, key.1),
        true,
        commands,
        meshes,
    );
}

fn install_chunk(
    scene: &mut TerrainScene,
    key: ChunkKey,
    (opaque, glass): ChunkGeometry,
    detailed: bool,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    let mut triangles = (opaque.indices.len() + glass.indices.len()) / 3;
    if let Some(chunk) = scene.chunks.get_mut(&key) {
        if let Some((_, water)) = &chunk.water {
            triangles += meshes
                .get(water)
                .and_then(Mesh::indices)
                .map_or(0, |i| i.len() / 3);
        }
        scene.triangle_count = scene.triangle_count - chunk.triangles + triangles;
        chunk.triangles = triangles;
        chunk.detailed = detailed;
        if let Some(mut mesh) = meshes.get_mut(&chunk.opaque) {
            *mesh = opaque.into_mesh();
        }
        match (&chunk.glass, glass.indices.is_empty()) {
            (Some((_, handle)), false) => {
                if let Some(mut mesh) = meshes.get_mut(handle) {
                    *mesh = glass.into_mesh();
                }
            }
            (Some((entity, handle)), true) => {
                commands.entity(*entity).despawn();
                meshes.remove(handle.id());
                chunk.glass = None;
            }
            (None, false) => {
                let handle = meshes.add(glass.into_mesh());
                let entity = commands
                    .spawn((
                        crate::GameEntity,
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(scene.glass_material.clone()),
                    ))
                    .id();
                chunk.glass = Some((entity, handle));
            }
            (None, true) => {}
        }
    } else {
        let opaque = meshes.add(opaque.into_mesh());
        let entity = commands
            .spawn((
                crate::GameEntity,
                Mesh3d(opaque.clone()),
                MeshMaterial3d(scene.opaque_material.clone()),
            ))
            .id();
        let glass = if glass.indices.is_empty() {
            None
        } else {
            let handle = meshes.add(glass.into_mesh());
            let entity = commands
                .spawn((
                    crate::GameEntity,
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(scene.glass_material.clone()),
                ))
                .id();
            Some((entity, handle))
        };
        scene.triangle_count += triangles;
        scene.chunks.insert(
            key,
            ChunkMesh {
                entity,
                opaque,
                glass,
                water: None,
                triangles,
                detailed,
            },
        );
    }
}

/// Per-column storage avoids allocating the mountain's entire solid interior.
/// The lowest changed air cell and adjacent terrain set each column's floor.
struct CachedColumn {
    bottom: i32,
    top: i32,
    blocks: Vec<Block>,
}
struct CellCache {
    columns: Vec<CachedColumn>,
    x0: i32,
    z0: i32,
    min_y: i32,
}

impl CellCache {
    const WIDTH: usize = CHUNK_SIZE as usize + 2;
    fn new(world: &World, cx: i32, cz: i32) -> Self {
        let x0 = cx * CHUNK_SIZE - 1;
        let z0 = cz * CHUNK_SIZE - 1;
        let mut columns = Vec::with_capacity(Self::WIDTH * Self::WIDTH);
        for x in 0..Self::WIDTH {
            for z in 0..Self::WIDTH {
                let x = x0 + x as i32;
                let z = z0 + z as i32;
                let top = (world
                    .surface_height((x as f32 + 0.5) * CELL_SIZE, (z as f32 + 0.5) * CELL_SIZE)
                    / CELL_SIZE)
                    .round() as i32
                    - 1;
                let mut bottom = world.column_mesh_floor(x, z);
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        bottom = bottom.min(world.column_mesh_floor(x + dx, z + dz));
                    }
                }
                bottom = (bottom - 1).max(world.min_y());
                let blocks = (bottom..=top)
                    .map(|y| world.block(BlockPos::new(x, y, z)))
                    .collect();
                columns.push(CachedColumn {
                    bottom,
                    top,
                    blocks,
                });
            }
        }
        Self {
            columns,
            x0,
            z0,
            min_y: world.min_y(),
        }
    }
    fn column(&self, x: i32, z: i32) -> &CachedColumn {
        &self.columns[(x - self.x0) as usize * Self::WIDTH + (z - self.z0) as usize]
    }
    fn get(&self, x: i32, y: i32, z: i32) -> Block {
        let column = self.column(x, z);
        if y > column.top || y < self.min_y {
            Block::Air
        } else if y < column.bottom {
            Block::Stone
        } else {
            column.blocks[(y - column.bottom) as usize]
        }
    }
    fn top(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).top
    }
    fn bottom(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).bottom
    }
}

/// A quadtree concentrates samples around the camera, with at most eight
/// samples across each leaf. Every leaf uses the authoritative geography.
fn landscape_geometry(
    world: &World,
    center: ChunkKey,
    near_radius: i32,
    tree_distance: f32,
) -> (Geometry, Geometry) {
    let geography = world.geography().expect("geographic landscape");
    let mut land = Geometry::default();
    let mut water = Geometry::default();
    let first = (-world.radius_cells()).div_euclid(CHUNK_SIZE);
    let side = world.radius_cells() * 2 / CHUNK_SIZE;
    let tiles = landscape_tiles(first, first, side, center, near_radius);
    let tile_lookup: HashMap<_, _> = tiles.iter().map(|&(x, z, size)| ((x, z), size)).collect();
    let mut surfaces = HashMap::with_capacity(tiles.len());
    for (x, z, size) in tiles {
        let neighbor_steps = neighbor_steps(&tile_lookup, x, z, size);
        let surface = surface_tile(
            geography,
            [
                x as f32 * CHUNK_METERS,
                z as f32 * CHUNK_METERS,
                size as f32 * CHUNK_METERS,
            ],
            8,
            neighbor_steps,
            &mut land,
            &mut water,
        );
        surfaces.insert((x, z), surface);
    }
    add_landscape_trees(
        world,
        center,
        near_radius,
        tree_distance,
        &tile_lookup,
        &surfaces,
        &mut land,
    );
    add_village_proxies(
        world,
        center,
        ProxyClip::Outside(local_bounds(center, near_radius)),
        &mut land,
    );
    (land, water)
}

/// Nearby silhouettes bridge the painted map to editable assets and remain
/// visible while detailed chunks load. The map represents distant settlements.
fn add_village_proxies(world: &World, center: ChunkKey, clip: ProxyClip, geometry: &mut Geometry) {
    let Some(plan) = world.settlements() else {
        return;
    };
    let camera = Vec2::new(
        center.0 as f32 * CHUNK_METERS,
        center.1 as f32 * CHUNK_METERS,
    );
    for village in &plan.villages {
        for building in &village.buildings {
            let [width, height, depth] = building.dimensions();
            let base = Vec3::new(
                building.origin.x as f32,
                building.origin.y as f32,
                building.origin.z as f32,
            ) * CELL_SIZE;
            let size = Vec3::new(width as f32, height as f32, depth as f32) * CELL_SIZE;
            let building_center = Vec2::new(base.x + size.x * 0.5, base.z + size.z * 0.5);
            if building_center.distance(camera) > LOD_BUILDING_DISTANCE {
                continue;
            }
            let walls = (size.y * 0.58).min(3.5);
            proxy_cuboid(
                geometry,
                base + Vec3::new(size.x * 0.5, walls * 0.5, size.z * 0.5),
                Vec3::new(size.x, walls, size.z),
                Block::Sand.color(),
                clip,
            );
            // Stepped roof silhouettes agree with the fine assets' warm palette.
            let tiers = 4;
            for tier in 0..tiers {
                let y = walls + (size.y - walls) * (tier as f32 + 0.5) / tiers as f32;
                let roof_width = size.x * (1.0 - tier as f32 * 0.18);
                proxy_cuboid(
                    geometry,
                    base + Vec3::new(size.x * 0.5, y, size.z * 0.5),
                    Vec3::new(roof_width, (size.y - walls) / tiers as f32, size.z),
                    Block::Brick.color(),
                    clip,
                );
            }
        }
    }
}

fn lod_tree_cells(center: ChunkKey, distance: f32) -> Vec<(i32, i32)> {
    let distance = distance.clamp(MIN_TREE_DISTANCE, MAX_TREE_DISTANCE);
    let x = (center.0 as f32 + 0.5) * CHUNK_METERS;
    let z = (center.1 as f32 + 0.5) * CHUNK_METERS;
    let first_x = ((x - distance) / TREE_GRID_METERS).floor() as i32;
    let last_x = ((x + distance) / TREE_GRID_METERS).floor() as i32;
    let first_z = ((z - distance) / TREE_GRID_METERS).floor() as i32;
    let last_z = ((z + distance) / TREE_GRID_METERS).floor() as i32;
    let mut cells = Vec::new();
    for gx in first_x..=last_x {
        for gz in first_z..=last_z {
            let dx = (gx as f32 + 0.5) * TREE_GRID_METERS - x;
            let dz = (gz as f32 + 0.5) * TREE_GRID_METERS - z;
            let distance_squared = dx * dx + dz * dz;
            if distance_squared > distance.powi(2) {
                continue;
            }
            cells.push((gx, gz));
        }
    }
    // The distance bounds the work. Keep full density throughout that circle;
    // a fixed nearest-tree budget would silently erase the requested far trees.
    cells
}

fn local_bounds(center: ChunkKey, near_radius: i32) -> [f32; 4] {
    [
        (center.0 - near_radius) as f32 * CHUNK_METERS,
        (center.1 - near_radius) as f32 * CHUNK_METERS,
        (center.0 + near_radius + 1) as f32 * CHUNK_METERS,
        (center.1 + near_radius + 1) as f32 * CHUNK_METERS,
    ]
}

fn add_landscape_trees(
    world: &World,
    center: ChunkKey,
    near_radius: i32,
    tree_distance: f32,
    tiles: &HashMap<ChunkKey, i32>,
    surfaces: &HashMap<ChunkKey, SampledSurface>,
    land: &mut Geometry,
) {
    let cutout = local_bounds(center, near_radius);
    for (gx, gz) in lod_tree_cells(center, tree_distance) {
        let Some(tree) = world.tree_at(gx, gz) else {
            continue;
        };
        let x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
        let z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
        let chunk = chunk_key([x, 0.0, z]);
        let mut size = 1;
        let mut ground = tree.base.y as f32 * CELL_SIZE;
        while size <= MAX_LOD_TILE_CHUNKS {
            let origin = (
                chunk.0.div_euclid(size) * size,
                chunk.1.div_euclid(size) * size,
            );
            if tiles.get(&origin) == Some(&size) {
                ground = surfaces[&origin].height_at(x, z);
                break;
            }
            size *= 2;
        }
        add_tree_proxy(
            land,
            tree,
            ground,
            ProxyClip::Outside(cutout),
            tree_leaf_color(world, tree.kind),
        );
    }
}

#[derive(Clone, Copy)]
enum ProxyClip {
    Outside([f32; 4]),
    Inside([f32; 4]),
}

fn add_chunk_tree_proxies(
    world: &World,
    key: ChunkKey,
    surface: &SampledSurface,
    land: &mut Geometry,
) {
    let x0 = key.0 as f32 * CHUNK_METERS;
    let z0 = key.1 as f32 * CHUNK_METERS;
    let bounds = [x0, z0, x0 + CHUNK_METERS, z0 + CHUNK_METERS];
    let margin = 4.5 * CELL_SIZE;
    let first_x = ((x0 - margin) / TREE_GRID_METERS).floor() as i32;
    let last_x = ((bounds[2] + margin) / TREE_GRID_METERS).floor() as i32;
    let first_z = ((z0 - margin) / TREE_GRID_METERS).floor() as i32;
    let last_z = ((bounds[3] + margin) / TREE_GRID_METERS).floor() as i32;
    for gx in first_x..=last_x {
        for gz in first_z..=last_z {
            let Some(tree) = world.tree_at(gx, gz) else {
                continue;
            };
            let x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
            let z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
            let ground = if chunk_key([x, 0.0, z]) == key {
                surface.height_at(x, z)
            } else {
                // Crossing crowns use their owning placeholder's surface,
                // so adjacent chunks do not shift the same tree vertically.
                placeholder_ground_height(world.geography().unwrap(), x, z)
            };
            add_tree_proxy(
                land,
                tree,
                ground,
                ProxyClip::Inside(bounds),
                tree_leaf_color(world, tree.kind),
            );
        }
    }
}

fn placeholder_ground_height(geography: &Geography, x: f32, z: f32) -> f32 {
    let key = chunk_key([x, 0.0, z]);
    let x0 = key.0 as f32 * CHUNK_METERS;
    let z0 = key.1 as f32 * CHUNK_METERS;
    let u = x - x0;
    let v = z - z0;
    let ix = u.floor() as usize;
    let iz = v.floor() as usize;
    let cell_x = x0 + ix as f32;
    let cell_z = z0 + iz as f32;
    let corners = [
        surface_vertex(geography, cell_x, cell_z + 1.0),
        surface_vertex(geography, cell_x + 1.0, cell_z + 1.0),
        surface_vertex(geography, cell_x + 1.0, cell_z),
        surface_vertex(geography, cell_x, cell_z),
    ];
    triangle_height(corners, u - ix as f32, v - iz as f32)
}

fn tree_leaf_color(world: &World, kind: TreeKind) -> [f32; 4] {
    if world.generation() == WorldGeneration::GeographyV2
        || world.generation() == WorldGeneration::GeographyV3
    {
        kind.leaf_color()
    } else {
        [0.24, 0.40, 0.31, 1.0]
    }
}

fn add_tree_proxy(
    land: &mut Geometry,
    tree: GeneratedTree,
    ground: f32,
    clip: ProxyClip,
    foliage: [f32; 4],
) {
    let x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
    let z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
    let trunk = tree.trunk_height as f32 * CELL_SIZE;
    proxy_cuboid(
        land,
        Vec3::new(x, ground + trunk * 0.5, z),
        Vec3::new(CELL_SIZE, trunk, CELL_SIZE),
        Block::Wood.color(),
        clip,
    );
    let Some((bottom, top)) = tree.leaf_bounds(0, 0) else {
        return;
    };
    let mut tier_bottom = bottom;
    let mut previous_radius = -1;
    for y in bottom..=top + 1 {
        let radius = if y > top {
            -1
        } else {
            (0..=tree.crown_radius)
                .rev()
                .find(|&r| {
                    tree.leaf_bounds(r, 0)
                        .is_some_and(|(lo, hi)| (lo..=hi).contains(&y))
                })
                .unwrap_or(0)
        };
        if radius != previous_radius {
            if previous_radius >= 0 {
                let width = (previous_radius * 2 + 1) as f32 * CELL_SIZE;
                let low_y = ground + (tier_bottom - tree.base.y) as f32 * CELL_SIZE;
                let high_y = ground + (y - tree.base.y) as f32 * CELL_SIZE;
                proxy_cuboid(
                    land,
                    Vec3::new(x, (low_y + high_y) * 0.5, z),
                    Vec3::new(width, high_y - low_y, width),
                    foliage,
                    clip,
                );
            }
            tier_bottom = y;
            previous_radius = radius;
        }
    }
}

fn proxy_cuboid(
    land: &mut Geometry,
    center: Vec3,
    dimensions: Vec3,
    color: [f32; 4],
    clip: ProxyClip,
) {
    match clip {
        ProxyClip::Outside(bounds) => cuboid_outside_local(land, center, dimensions, color, bounds),
        ProxyClip::Inside([x0, z0, x1, z1]) => {
            let mut low = center - dimensions * 0.5;
            let mut high = center + dimensions * 0.5;
            low.x = low.x.max(x0);
            low.z = low.z.max(z0);
            high.x = high.x.min(x1);
            high.z = high.z.min(z1);
            if high.x > low.x && high.z > low.z {
                land.cuboid((low + high) * 0.5, high - low, color);
            }
        }
    }
}

/// Clip each proxy cuboid against the detail cutout. A crown crossing the
/// boundary remains visible outside it without drawing duplicate near leaves.
fn cuboid_outside_local(
    land: &mut Geometry,
    center: Vec3,
    dimensions: Vec3,
    color: [f32; 4],
    [cut_x0, cut_z0, cut_x1, cut_z1]: [f32; 4],
) {
    let low = center - dimensions * 0.5;
    let high = center + dimensions * 0.5;
    if high.x <= cut_x0 || low.x >= cut_x1 || high.z <= cut_z0 || low.z >= cut_z1 {
        land.cuboid(center, dimensions, color);
        return;
    }
    let middle_x0 = low.x.max(cut_x0);
    let middle_x1 = high.x.min(cut_x1);
    for [x0, z0, x1, z1] in [
        [low.x, low.z, high.x.min(cut_x0), high.z],
        [low.x.max(cut_x1), low.z, high.x, high.z],
        [middle_x0, low.z, middle_x1, high.z.min(cut_z0)],
        [middle_x0, low.z.max(cut_z1), middle_x1, high.z],
    ] {
        if x1 > x0 && z1 > z0 {
            land.cuboid(
                Vec3::new((x0 + x1) * 0.5, center.y, (z0 + z1) * 0.5),
                Vec3::new(x1 - x0, dimensions.y, z1 - z0),
                color,
            );
        }
    }
}

fn landscape_tiles(
    x: i32,
    z: i32,
    size: i32,
    center: ChunkKey,
    near_radius: i32,
) -> Vec<(i32, i32, i32)> {
    fn visit(
        x: i32,
        z: i32,
        size: i32,
        center: ChunkKey,
        near_radius: i32,
        output: &mut Vec<(i32, i32, i32)>,
    ) {
        let low_x = center.0.saturating_sub(near_radius);
        let low_z = center.1.saturating_sub(near_radius);
        let high_x = center.0.saturating_add(near_radius + 1);
        let high_z = center.1.saturating_add(near_radius + 1);
        if x >= low_x && z >= low_z && x + size <= high_x && z + size <= high_z {
            return;
        }
        let intersects = x < high_x && x + size > low_x && z < high_z && z + size > low_z;
        let dx = (center.0 as f64 + 0.5 - (x as f64 + size as f64 * 0.5)).abs() - size as f64 * 0.5;
        let dz = (center.1 as f64 + 0.5 - (z as f64 + size as f64 * 0.5)).abs() - size as f64 * 0.5;
        let distance = dx.max(0.0).max(dz.max(0.0));
        if size > 1 && (size > MAX_LOD_TILE_CHUNKS || intersects || distance < size as f64 * 2.0) {
            let half = size / 2;
            for (dx, dz) in [(0, 0), (half, 0), (0, half), (half, half)] {
                visit(x + dx, z + dz, half, center, near_radius, output);
            }
        } else {
            output.push((x, z, size));
        }
    }
    let mut tiles = Vec::new();
    visit(x, z, size, center, near_radius, &mut tiles);
    tiles
}

/// North, south, west, east neighbors; aligned quadtree coordinates make
/// lookup logarithmic. A smaller neighbor stitches itself to this tile.
fn neighbor_steps(tiles: &HashMap<ChunkKey, i32>, x: i32, z: i32, size: i32) -> [Option<f32>; 4] {
    [
        (x + size / 2, z - 1),
        (x + size / 2, z + size),
        (x - 1, z + size / 2),
        (x + size, z + size / 2),
    ]
    .map(|(px, pz)| {
        let mut candidate = 1;
        while candidate <= MAX_LOD_TILE_CHUNKS {
            let origin = (
                px.div_euclid(candidate) * candidate,
                pz.div_euclid(candidate) * candidate,
            );
            if tiles.get(&origin) == Some(&candidate) {
                return Some(candidate as f32 * CHUNK_METERS / 8.0);
            }
            candidate *= 2;
        }
        None
    })
}

type SurfaceVertex = (Vec3, [f32; 4], Option<f32>, [f32; 3]);

/// Retain the actual rendered surface so nearby proxy trunks meet stitched
/// triangles, rather than floating above an independently sampled height.
struct SampledSurface {
    origin: [f32; 2],
    step: f32,
    steps: usize,
    vertices: Vec<SurfaceVertex>,
}

impl SampledSurface {
    fn corners(&self, ix: usize, iz: usize) -> [SurfaceVertex; 4] {
        let at = |x, z| self.vertices[z * (self.steps + 1) + x];
        [
            at(ix, iz + 1),
            at(ix + 1, iz + 1),
            at(ix + 1, iz),
            at(ix, iz),
        ]
    }

    fn height_at(&self, x: f32, z: f32) -> f32 {
        let u = ((x - self.origin[0]) / self.step).clamp(0.0, self.steps as f32);
        let v = ((z - self.origin[1]) / self.step).clamp(0.0, self.steps as f32);
        let ix = (u.floor() as usize).min(self.steps - 1);
        let iz = (v.floor() as usize).min(self.steps - 1);
        let u = u - ix as f32;
        let v = v - iz as f32;
        triangle_height(self.corners(ix, iz), u, v)
    }
}

fn triangle_height(corners: [SurfaceVertex; 4], u: f32, v: f32) -> f32 {
    let h = corners.map(|corner| corner.0.y);
    if quad_diagonal(corners.map(|corner| corner.1)) == [0, 1, 2, 0, 2, 3] {
        if u + v >= 1.0 {
            h[0] * (1.0 - u) + h[1] * (u + v - 1.0) + h[2] * (1.0 - v)
        } else {
            h[0] * v + h[2] * u + h[3] * (1.0 - u - v)
        }
    } else if v >= u {
        h[0] * (v - u) + h[1] * u + h[3] * (1.0 - v)
    } else {
        h[1] * v + h[2] * (u - v) + h[3] * (1.0 - u)
    }
}

/// Quantization and sampling match World::height_at. Continuous slope normals
/// keep distant geography smooth while the nearby voxel faces remain crisp.
fn surface_vertex(geography: &Geography, x: f32, z: f32) -> SurfaceVertex {
    let sample = geography.sample(
        (x / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE * 0.5,
        (z / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE * 0.5,
    );
    let y = (sample.height / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE;
    const NORMAL_STEP: f32 = 8.0;
    let dx =
        geography.sample(x + NORMAL_STEP, z).height - geography.sample(x - NORMAL_STEP, z).height;
    let dz =
        geography.sample(x, z + NORMAL_STEP).height - geography.sample(x, z - NORMAL_STEP).height;
    let normal = Vec3::new(-dx, NORMAL_STEP * 2.0, -dz)
        .normalize_or_zero()
        .to_array();
    (
        Vec3::new(x, y, z),
        srgb_linear(sample.biome.color()),
        sample.water,
        normal,
    )
}

fn stitch_vertex(geo: &Geography, vertex: &mut SurfaceVertex, step: f32, along_x: bool) {
    let coordinate = if along_x { vertex.0.x } else { vertex.0.z };
    let a = (coordinate / step).floor() * step;
    let t = (coordinate - a) / step;
    if t < 0.0001 {
        return;
    }
    let (low, high) = if along_x {
        (
            surface_vertex(geo, a, vertex.0.z),
            surface_vertex(geo, a + step, vertex.0.z),
        )
    } else {
        (
            surface_vertex(geo, vertex.0.x, a),
            surface_vertex(geo, vertex.0.x, a + step),
        )
    };
    vertex.0.y = low.0.y * (1.0 - t) + high.0.y * t;
    vertex.1 = std::array::from_fn(|i| low.1[i] * (1.0 - t) + high.1[i] * t);
    vertex.3 = Vec3::from_array(low.3)
        .lerp(Vec3::from_array(high.3), t)
        .normalize_or_zero()
        .to_array();
}

fn water_vertices(corners: [SurfaceVertex; 4], step: f32) -> Option<[[f32; 3]; 4]> {
    let mut wet_count = 0;
    let mut wet_sum = 0.0;
    for corner in &corners {
        if let Some(y) = corner.2 {
            wet_sum += y;
            wet_count += 1;
        }
    }
    if wet_count == 0 {
        return None;
    }
    let level = wet_sum / wet_count as f32;
    Some(corners.map(|v| {
        // A dry valley may lie below a wet mountain corner. Submerge dry
        // vertices under their OWN terrain, never under the average water.
        // The inset also limits coarse river triangles spreading over land.
        let y = v.2.map_or(level.min(v.0.y - step.max(1.0)), |y| y + 0.025);
        [v.0.x, y, v.0.z]
    }))
}

fn surface_tile(
    geography: &Geography,
    [x, z, size]: [f32; 3],
    steps: usize,
    neighbors: [Option<f32>; 4],
    land: &mut Geometry,
    water: &mut Geometry,
) -> SampledSurface {
    let land_start = land.positions.len();
    let water_start = water.positions.len();
    let step = size / steps as f32;
    let mut vertices = Vec::with_capacity((steps + 1) * (steps + 1));
    for iz in 0..=steps {
        for ix in 0..=steps {
            let mut vertex = surface_vertex(geography, x + ix as f32 * step, z + iz as f32 * step);
            for (edge, on_edge) in [iz == 0, iz == steps, ix == 0, ix == steps]
                .into_iter()
                .enumerate()
            {
                if on_edge
                    && let Some(coarse) = neighbors[edge]
                    && coarse > step
                {
                    stitch_vertex(geography, &mut vertex, coarse, edge < 2);
                }
            }
            vertices.push(vertex);
        }
    }
    let at = |ix: usize, iz: usize| vertices[iz * (steps + 1) + ix];
    for iz in 0..steps {
        for ix in 0..steps {
            let corners = [
                at(ix, iz + 1),
                at(ix + 1, iz + 1),
                at(ix + 1, iz),
                at(ix, iz),
            ];
            let colors = corners.map(|v| v.1);
            let diagonal = quad_diagonal(colors);
            land.quad_normals(
                corners.map(|v| v.0.to_array()),
                corners.map(|v| v.3),
                colors,
                diagonal,
            );
            if let Some(points) = water_vertices(corners, step) {
                // Shared triangulation is essential on steep nonplanar quads:
                // each dry water edge must remain below its matching land edge.
                water.quad_normals(points, [[0., 1., 0.]; 4], [[1.; 4]; 4], diagonal);
            }
        }
    }
    // Ordinary LOD edges are stitched, so only the actual cutout and world
    // border need a shallow skirt for half-meter voxel stair steps.
    for i in 0..steps {
        for (edge, (a, b)) in [
            (at(i, 0), at(i + 1, 0)),
            (at(i + 1, steps), at(i, steps)),
            (at(0, i + 1), at(0, i)),
            (at(steps, i), at(steps, i + 1)),
        ]
        .into_iter()
        .enumerate()
        {
            if neighbors[edge].is_some() {
                continue;
            }
            let c = b.0 - Vec3::Y * 2.0;
            let d = a.0 - Vec3::Y * 2.0;
            let normal = (b.0 - a.0).cross(d - a.0).normalize_or_zero().to_array();
            land.quad(
                [a.0.to_array(), b.0.to_array(), c.to_array(), d.to_array()],
                normal,
                [a.1, b.1, b.1, a.1],
            );
        }
    }
    land.uvs[land_start..].fill([1.0, 0.0]);
    water.uvs[water_start..].fill([1.0, 0.0]);
    SampledSurface {
        origin: [x, z],
        step,
        steps,
        vertices,
    }
}

// Vertices are counter-clockwise when viewed from outside the solid cell.
const FACES: [([i32; 3], [[f32; 3]; 4]); 6] = [
    (
        [1, 0, 0],
        [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
    ),
    (
        [-1, 0, 0],
        [[0., 0., 1.], [0., 1., 1.], [0., 1., 0.], [0., 0., 0.]],
    ),
    (
        [0, 1, 0],
        [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
    ),
    (
        [0, -1, 0],
        [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
    ),
    (
        [0, 0, 1],
        [[1., 0., 1.], [1., 1., 1.], [0., 1., 1.], [0., 0., 1.]],
    ),
    (
        [0, 0, -1],
        [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
    ),
];

fn occludes(block: Block) -> bool {
    block != Block::Air && block != Block::Glass
}

/// Keep drops readable when their vertical face points away from the camera.
/// Existing top vertices provide a soft lip only at actual exposed edges;
/// continuous ground does not acquire a grid. Foliage and transparent blocks
/// retain their existing colors, and side contrast is independent of sun angle.
fn terrain_shape_shade(
    cache: &CellCache,
    position: BlockPos,
    block: Block,
    normal: [i32; 3],
    corner: [f32; 3],
) -> f32 {
    if !matches!(
        block,
        Block::Grass | Block::Dirt | Block::Stone | Block::Sand | Block::Snow
    ) {
        return 1.0;
    }
    if normal[1] == 0 {
        return 0.76;
    }
    if normal != [0, 1, 0] {
        return 1.0;
    }
    let dx = if corner[0] < 0.5 { -1 } else { 1 };
    let dz = if corner[2] < 0.5 { -1 } else { 1 };
    if !occludes(cache.get(position.x + dx, position.y, position.z))
        || !occludes(cache.get(position.x, position.y, position.z + dz))
    {
        0.80
    } else {
        1.0
    }
}

fn chunk_geometry(world: &World, cx: i32, cz: i32) -> (Geometry, Geometry) {
    let cache = CellCache::new(world, cx, cz);
    let mut opaque = Geometry::default();
    let mut glass = Geometry::default();
    for x in cx * CHUNK_SIZE..(cx + 1) * CHUNK_SIZE {
        for z in cz * CHUNK_SIZE..(cz + 1) * CHUNK_SIZE {
            let geographic_biome = world.geography().map(|geography| {
                geography
                    .sample((x as f32 + 0.5) * CELL_SIZE, (z as f32 + 0.5) * CELL_SIZE)
                    .biome
            });
            let terrain_height = world.height_at(x, z);
            let mut leaf_color = None;
            for y in cache.bottom(x, z)..=cache.top(x, z) {
                let block = cache.get(x, y, z);
                if block == Block::Air {
                    continue;
                }
                if block == Block::Leaves && leaf_color.is_none() {
                    leaf_color = Some(
                        world
                            .tree_at(x.div_euclid(24), z.div_euclid(24))
                            .map_or([0.24, 0.40, 0.31, 1.0], |tree| {
                                tree_leaf_color(world, tree.kind)
                            }),
                    );
                }
                for (normal, corners) in FACES {
                    let adjacent = cache.get(x + normal[0], y + normal[1], z + normal[2]);
                    if occludes(adjacent) || (block == Block::Glass && adjacent == Block::Glass) {
                        continue;
                    }
                    let geometry = if block == Block::Glass {
                        &mut glass
                    } else {
                        &mut opaque
                    };
                    let mut color = match block {
                        Block::Grass => [0.36, 0.50, 0.36, 1.0],
                        Block::Leaves => leaf_color.unwrap(),
                        _ => block.color(),
                    };
                    if y == terrain_height
                        && block == world.surface_block(x, z)
                        && let Some(biome) = geographic_biome
                        && (matches!(block, Block::Grass | Block::Sand | Block::Snow)
                            || (block == Block::Stone && biome == Biome::Alpine))
                    {
                        color = biome.color();
                    }
                    if block == Block::Grass && normal[1] == 0 {
                        let soil = Block::Dirt.color();
                        for i in 0..3 {
                            color[i] = color[i] * 0.65 + soil[i] * 0.35;
                        }
                    }
                    let variation = 0.98 + hash(x, y, z, world.seed) * 0.04;
                    let base = srgb_linear(color);
                    let mut colors = [base; 4];
                    for (index, corner) in corners.iter().enumerate() {
                        let mut tangent_axes = [0; 2];
                        let mut count = 0;
                        for (axis, component) in normal.iter().enumerate() {
                            if *component == 0 {
                                tangent_axes[count] = axis;
                                count += 1;
                            }
                        }
                        let mut side_a = normal;
                        let mut side_b = normal;
                        side_a[tangent_axes[0]] +=
                            if corner[tangent_axes[0]] < 0.5 { -1 } else { 1 };
                        side_b[tangent_axes[1]] +=
                            if corner[tangent_axes[1]] < 0.5 { -1 } else { 1 };
                        let diagonal = [
                            side_a[0] + side_b[0] - normal[0],
                            side_a[1] + side_b[1] - normal[1],
                            side_a[2] + side_b[2] - normal[2],
                        ];
                        let solid = |offset: [i32; 3]| {
                            occludes(cache.get(x + offset[0], y + offset[1], z + offset[2]))
                        };
                        let a = solid(side_a);
                        let b = solid(side_b);
                        let level = if a && b {
                            3
                        } else {
                            a as u8 + b as u8 + solid(diagonal) as u8
                        };
                        // Baked corner shading adds depth without another render pass.
                        let shade = variation
                            * [1.0, 0.88, 0.76, 0.64][level as usize]
                            * terrain_shape_shade(
                                &cache,
                                BlockPos::new(x, y, z),
                                block,
                                normal,
                                *corner,
                            );
                        for channel in &mut colors[index][..3] {
                            *channel *= shade;
                        }
                    }
                    let points = corners.map(|p| {
                        [
                            (x as f32 + p[0]) * CELL_SIZE,
                            (y as f32 + p[1]) * CELL_SIZE,
                            (z as f32 + p[2]) * CELL_SIZE,
                        ]
                    });
                    geometry.quad(points, normal.map(|n| n as f32), colors);
                }
            }
            add_meadow_details(&mut opaque, world, &cache, x, z, geographic_biome);
        }
    }
    for position in berry_patch_positions(world) {
        let center = Vec3::from_array(position);
        let patch_x = (center.x / CELL_SIZE).floor() as i32;
        let patch_z = (center.z / CELL_SIZE).floor() as i32;
        if patch_x.div_euclid(CHUNK_SIZE) != cx || patch_z.div_euclid(CHUNK_SIZE) != cz {
            continue;
        }
        // These three renewable food sources are logical NPC resources for the
        // first prototype. Their small visual shrubs do not block movement.
        opaque.cuboid(
            center + Vec3::Y * 0.22,
            Vec3::new(0.80, 0.43, 0.65),
            [0.24, 0.42, 0.31, 1.],
        );
        opaque.cuboid(
            center + Vec3::new(0.10, 0.48, -0.05),
            Vec3::new(0.48, 0.23, 0.46),
            [0.34, 0.51, 0.34, 1.],
        );
        for offset in [
            Vec3::new(-0.31, 0.42, 0.28),
            Vec3::new(0.30, 0.38, 0.25),
            Vec3::new(0.08, 0.61, -0.08),
            Vec3::new(-0.14, 0.39, -0.31),
            Vec3::new(0.40, 0.25, -0.12),
        ] {
            opaque.cuboid(center + offset, Vec3::splat(0.11), [0.44, 0.24, 0.57, 1.]);
        }
    }
    (opaque, glass)
}

/// Small plants are ornamental and intentionally have no collision or simulation.
/// They follow the supporting grass cell and disappear when that cell is edited.
fn add_meadow_details(
    mesh: &mut Geometry,
    world: &World,
    cache: &CellCache,
    x: i32,
    z: i32,
    biome: Option<Biome>,
) {
    let y = world.height_at(x, z);
    if !(world.min_y()..world.max_y()).contains(&y)
        || cache.top(x, z) < y
        || cache.get(x, y, z) != Block::Grass
        || cache.get(x, y + 1, z) != Block::Air
    {
        return;
    }
    let chance = hash(x, 713, z, world.seed);
    let (density, flower_density, foliage, height_scale) = if world.generation()
        == WorldGeneration::GeographyV2
        || world.generation() == WorldGeneration::GeographyV3
    {
        match biome {
            Some(Biome::Shrubland) => (0.01, 0.0, [0.60, 0.58, 0.33, 1.0], 0.85),
            Some(Biome::Tundra) => (0.012, 0.0, [0.52, 0.55, 0.40, 1.0], 0.65),
            Some(Biome::PineForest) => (0.014, 0.0, [0.31, 0.46, 0.30, 1.0], 0.65),
            _ => (0.036, 0.009, [0.46, 0.58, 0.29, 1.0], 1.0),
        }
    } else {
        (0.036, 0.009, [0.46, 0.58, 0.29, 1.0], 1.0)
    };
    if chance > density || (world.geography().is_none() && y > 50) {
        return;
    }
    let base = Vec3::new(
        (x as f32 + 0.5) * CELL_SIZE,
        (y + 1) as f32 * CELL_SIZE,
        (z as f32 + 0.5) * CELL_SIZE,
    );
    let flowering = chance < flower_density;
    let stem = if flowering {
        [0.26, 0.38, 0.21, 1.0]
    } else {
        foliage
    };
    for i in 0..3 {
        let offset = Vec3::new((i as f32 - 1.) * 0.08, 0., (i % 2) as f32 * 0.09);
        let height = (0.13 + hash(x, i + 17, z, world.seed) * 0.18) * height_scale;
        mesh.cuboid(
            base + offset + Vec3::Y * height * 0.5,
            Vec3::new(0.032, height, 0.032),
            stem,
        );
        if flowering {
            let petal = if chance < 0.004 {
                [0.87, 0.76, 0.39, 1.0]
            } else {
                [0.73, 0.71, 0.86, 1.0]
            };
            mesh.cuboid(
                base + offset + Vec3::Y * height,
                Vec3::new(0.10, 0.055, 0.10),
                petal,
            );
        }
    }
}

fn river_mesh(world: &World) -> Mesh {
    let mut geometry = Geometry::default();
    for z in -WORLD_RADIUS..WORLD_RADIUS {
        // The sculpted stream is broad enough for this opaque ribbon to meet its banks.
        let x0 = world.river_center(z) * CELL_SIZE;
        let x1 = world.river_center(z + 1) * CELL_SIZE;
        let z0 = z as f32 * CELL_SIZE;
        let z1 = (z + 1) as f32 * CELL_SIZE;
        let width = 2.1;
        let tint = if z.rem_euclid(19) < 2 {
            [0.88, 0.98, 1., 1.]
        } else {
            [1.; 4]
        };
        geometry.quad(
            [
                [x0 - width, WATER_LEVEL, z0],
                [x1 - width, WATER_LEVEL, z1],
                [x1 + width, WATER_LEVEL, z1],
                [x0 + width, WATER_LEVEL, z0],
            ],
            [0., 1., 0.],
            [tint; 4],
        );
    }
    geometry.into_mesh()
}

fn distant_mountains(seed: u32) -> Mesh {
    let mut mesh = Geometry::default();
    const SEGMENTS: usize = 112;
    // Low-poly bands make a layered horizon without a heightmap texture or extra draw calls.
    let radii = [114., 180., 310., 460., 650.];
    let point = |ring: usize, index: usize| {
        let angle = index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let jagged = hash(index as i32, ring as i32, 99, seed);
        let broad = ((angle * 3. + 0.8).sin() * 0.62 + (angle * 7. - 0.2).cos() * 0.38)
            .abs()
            .powf(1.5);
        let height = match ring {
            0 => -9.,
            1 => 10. + broad * 20. + jagged * 4.,
            2 => 32. + broad * 58. + jagged * 10.,
            3 => 38. + broad * 45. + jagged * 8.,
            _ => -10.,
        };
        Vec3::new(angle.cos() * radii[ring], height, angle.sin() * radii[ring])
    };
    for ring in 0..radii.len() - 1 {
        for segment in 0..SEGMENTS {
            let next = (segment + 1) % SEGMENTS;
            let a = point(ring, segment);
            let b = point(ring, next);
            let c = point(ring + 1, next);
            let d = point(ring + 1, segment);
            let mut color = match ring {
                0 => [0.43, 0.56, 0.51, 1.0],
                1 => [0.46, 0.61, 0.67, 1.0],
                _ => [0.59, 0.70, 0.76, 1.0],
            };
            let shade = 0.985 + hash(segment as i32, ring as i32, 239, seed) * 0.03;
            for channel in &mut color[..3] {
                *channel *= shade;
            }
            mesh.triangle(a, b, c, color);
            mesh.triangle(a, c, d, color);
            if ring == 1 && c.y.max(d.y) > 82. {
                // Small broken snow caps sit on the far ridge; warm meadows stay dominant.
                let snow_a = c.lerp(b, 0.16);
                let snow_b = d.lerp(a, 0.16);
                mesh.triangle(
                    c + Vec3::Y * 0.05,
                    snow_a + Vec3::Y * 0.05,
                    snow_b + Vec3::Y * 0.05,
                    [0.80, 0.84, 0.84, 1.],
                );
                mesh.triangle(
                    c + Vec3::Y * 0.05,
                    snow_b + Vec3::Y * 0.05,
                    d + Vec3::Y * 0.05,
                    [0.80, 0.84, 0.84, 1.],
                );
            }
        }
    }
    mesh.into_mesh()
}

#[derive(Default)]
pub(crate) struct Geometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    // Landscape mask for the distant albedo; actual sampling is world-aligned.
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Geometry {
    fn quad(&mut self, vertices: [[f32; 3]; 4], normal: [f32; 3], colors: [[f32; 4]; 4]) {
        self.quad_normals(vertices, [normal; 4], colors, quad_diagonal(colors));
    }
    fn quad_normals(
        &mut self,
        vertices: [[f32; 3]; 4],
        normals: [[f32; 3]; 4],
        colors: [[f32; 4]; 4],
        diagonal: [u32; 6],
    ) {
        let start = self.positions.len() as u32;
        self.positions.extend(vertices);
        self.normals.extend(normals);
        self.colors.extend(colors);
        self.uvs.extend([[0.0; 2]; 4]);
        self.indices.extend(diagonal.map(|index| start + index));
    }

    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        self.triangle_linear(a, b, c, srgb_linear(color));
    }

    fn triangle_linear(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        let start = self.positions.len() as u32;
        let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
        self.positions
            .extend([a.to_array(), b.to_array(), c.to_array()]);
        self.normals.extend([normal; 3]);
        self.colors.extend([color; 3]);
        self.uvs.extend([[0.0; 2]; 3]);
        self.indices.extend([start, start + 1, start + 2]);
    }

    pub(crate) fn cuboid(&mut self, center: Vec3, dimensions: Vec3, color: [f32; 4]) {
        for (normal, corners) in FACES {
            let vertices = corners.map(|p| {
                (center + (Vec3::from_array(p) - Vec3::splat(0.5)) * dimensions).to_array()
            });
            self.quad(vertices, normal.map(|n| n as f32), [srgb_linear(color); 4]);
        }
    }

    pub(crate) fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn quad_diagonal(colors: [[f32; 4]; 4]) -> [u32; 6] {
    let brightness = colors.map(|color| color[..3].iter().sum::<f32>());
    // Keep the ambient occlusion diagonal consistent across geometry layers.
    if brightness[0] + brightness[2] > brightness[1] + brightness[3] {
        [0, 1, 3, 1, 2, 3]
    } else {
        [0, 1, 2, 0, 2, 3]
    }
}

fn srgb_linear(color: [f32; 4]) -> [f32; 4] {
    let linear = LinearRgba::from(Srgba::new(color[0], color[1], color[2], color[3]));
    [linear.red, linear.green, linear.blue, linear.alpha]
}

fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut value = (x as u32).wrapping_mul(0x9e37_79b9)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ (z as u32).wrapping_mul(0xc2b2_ae35)
        ^ seed;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    (value & 0xffff) as f32 / 65535.
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::MAX_Y;

    #[test]
    fn distant_albedo_marks_ground_without_recoloring_batched_foliage() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let mut land = Geometry::default();
        surface_tile(
            world.geography().unwrap(),
            [2400.0, -1800.0, 64.0],
            8,
            [None; 4],
            &mut land,
            &mut Geometry::default(),
        );
        let ground_vertices = land.positions.len();
        assert!(ground_vertices > 0);
        assert!(land.uvs.iter().all(|&uv| uv == [1.0, 0.0]));
        let mut water = Geometry::default();
        surface_tile(
            world.geography().unwrap(),
            [-16_000.0, -16_000.0, 64.0],
            8,
            [None; 4],
            &mut Geometry::default(),
            &mut water,
        );
        assert!(!water.positions.is_empty());
        assert!(water.uvs.iter().all(|&uv| uv == [1.0, 0.0]));
        add_tree_proxy(
            &mut land,
            GeneratedTree {
                base: BlockPos::new(4820, 400, -3580),
                trunk_height: 20,
                crown_radius: 4,
                kind: TreeKind::Broadleaf,
            },
            200.0,
            ProxyClip::Outside([0.0, 0.0, 1.0, 1.0]),
            TreeKind::Broadleaf.leaf_color(),
        );
        assert!(land.uvs.len() > ground_vertices);
        assert!(land.uvs[ground_vertices..].iter().all(|&uv| uv == [0.0; 2]));
        let count = land.positions.len();
        let mesh = land.into_mesh();
        assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap().len(), count);
    }

    #[test]
    fn both_ao_diagonals_keep_every_face_winding_outward() {
        for (normal, vertices) in FACES {
            for (shades, expected) in [
                ([0.64, 1.0, 0.64, 1.0], [0, 1, 2, 0, 2, 3]),
                ([1.0, 0.64, 1.0, 0.64], [0, 1, 3, 1, 2, 3]),
            ] {
                let mut mesh = Geometry::default();
                mesh.quad(
                    vertices,
                    normal.map(|n| n as f32),
                    shades.map(|shade| [shade, shade, shade, 1.0]),
                );
                assert_eq!(mesh.indices, expected);
                for triangle in mesh.indices.as_chunks::<3>().0 {
                    let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
                    let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
                    let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
                    assert!(
                        (b - a)
                            .cross(c - a)
                            .dot(Vec3::from_array(normal.map(|n| n as f32)))
                            > 0.0
                    );
                }
            }
        }
    }

    #[test]
    fn chunk_edges_rebuild_the_neighbour_including_negative_coordinates() {
        assert_eq!(affected_chunks(BlockPos { x: 2, y: 4, z: 3 }), vec![(0, 0)]);
        let edge = affected_chunks(BlockPos { x: -1, y: 4, z: 3 });
        assert_eq!(edge, vec![(-1, 0), (0, 0)]);
        let corner = affected_chunks(BlockPos { x: 0, y: 4, z: 0 });
        assert!(corner.contains(&(-1, -1)));
        assert_eq!(corner.len(), 4);
    }

    fn rendered_face_colors(
        mesh: &Geometry,
        position: BlockPos,
        normal: [i32; 3],
    ) -> [[f32; 4]; 4] {
        let normal = Vec3::from_array(normal.map(|n| n as f32));
        let center = Vec3::new(
            position.x as f32 + 0.5,
            position.y as f32 + 0.5,
            position.z as f32 + 0.5,
        ) * CELL_SIZE
            + normal * CELL_SIZE * 0.5;
        let index = mesh
            .positions
            .as_chunks::<4>()
            .0
            .iter()
            .zip(mesh.normals.as_chunks::<4>().0)
            .position(|(points, normals)| {
                let face_center = points.iter().copied().map(Vec3::from_array).sum::<Vec3>() * 0.25;
                face_center.distance(center) < 0.001 && normals[0] == normal.to_array()
            })
            .expect("the requested block face is visible");
        mesh.colors[index * 4..index * 4 + 4].try_into().unwrap()
    }

    fn level_platform(position: BlockPos, block: Block) -> World {
        let mut world = World::new(7);
        for dx in -1..=1 {
            for dz in -1..=1 {
                for y in position.y - 1..=position.y {
                    world
                        .set_block(BlockPos::new(position.x + dx, y, position.z + dz), block)
                        .unwrap();
                }
            }
        }
        world
    }

    #[test]
    fn level_ground_keeps_uniform_top_colors_without_false_drop_edges() {
        let position = BlockPos::new(2, MAX_Y - 4, 2);
        for block in [
            Block::Grass,
            Block::Dirt,
            Block::Stone,
            Block::Sand,
            Block::Snow,
        ] {
            let world = level_platform(position, block);
            let mesh = chunk_geometry(&world, 0, 0).0;
            let colors = rendered_face_colors(&mesh, position, [0, 1, 0]);
            assert!(colors.iter().all(|color| *color == colors[0]));
            let base = srgb_linear(if block == Block::Grass {
                [0.36, 0.50, 0.36, 1.0]
            } else {
                block.color()
            });
            let variation = 0.98 + hash(position.x, position.y, position.z, world.seed) * 0.04;
            assert!((colors[0][0] / base[0] - variation).abs() < 0.001);
        }
    }

    #[test]
    fn descending_steps_have_top_edge_cues_in_all_four_directions() {
        let position = BlockPos::new(2, MAX_Y - 4, 2);
        let platform = level_platform(position, Block::Stone);
        let flat_colors =
            rendered_face_colors(&chunk_geometry(&platform, 0, 0).0, position, [0, 1, 0]);
        let top_corners = FACES
            .iter()
            .find(|(normal, _)| *normal == [0, 1, 0])
            .unwrap()
            .1;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut world = platform.clone();
            let lowered = BlockPos::new(position.x + dx, position.y, position.z + dz);
            world.set_block(lowered, Block::Air).unwrap();
            assert_eq!(
                world.block(BlockPos::new(lowered.x, lowered.y - 1, lowered.z)),
                Block::Stone,
                "the neighbor is a real one-cell descent"
            );
            let colors = rendered_face_colors(&chunk_geometry(&world, 0, 0).0, position, [0, 1, 0]);
            for (index, corner) in top_corners.iter().enumerate() {
                let borders_drop = (dx == 1 && corner[0] == 1.0)
                    || (dx == -1 && corner[0] == 0.0)
                    || (dz == 1 && corner[2] == 1.0)
                    || (dz == -1 && corner[2] == 0.0);
                let brightness = colors[index][0] / flat_colors[index][0];
                if borders_drop {
                    assert!(brightness < 0.85, "the downhill top edge is visibly darker");
                } else {
                    assert!(
                        (brightness - 1.0).abs() < 0.001,
                        "the opposite edge stays unchanged"
                    );
                }
            }
        }
    }

    #[test]
    fn top_drop_cues_follow_edits_across_negative_chunk_boundaries() {
        let position = BlockPos::new(-1, MAX_Y - 4, -1);
        let mut world = level_platform(position, Block::Stone);
        let before = rendered_face_colors(&chunk_geometry(&world, -1, -1).0, position, [0, 1, 0]);
        let neighbor = BlockPos::new(0, position.y, -1);
        assert!(affected_chunks(neighbor).contains(&(-1, -1)));
        world.set_block(neighbor, Block::Air).unwrap();
        let after = rendered_face_colors(&chunk_geometry(&world, -1, -1).0, position, [0, 1, 0]);
        assert!(after[1][0] < before[1][0] && after[2][0] < before[2][0]);
        assert_eq!(after[0], before[0]);
        assert_eq!(after[3], before[3]);
        world.set_block(neighbor, Block::Stone).unwrap();
        let restored = rendered_face_colors(&chunk_geometry(&world, -1, -1).0, position, [0, 1, 0]);
        assert_eq!(restored, before);
    }

    #[test]
    fn ground_side_contrast_is_axis_independent_and_excludes_foliage_and_glass() {
        let position = BlockPos::new(2, MAX_Y - 4, 2);
        for block in [
            Block::Grass,
            Block::Dirt,
            Block::Stone,
            Block::Sand,
            Block::Snow,
            Block::Wood,
            Block::Leaves,
            Block::Brick,
            Block::Glass,
        ] {
            let mut world = World::new(7);
            world.set_block(position, block).unwrap();
            let geometry = chunk_geometry(&world, 0, 0);
            let mesh = if block == Block::Glass {
                &geometry.1
            } else {
                &geometry.0
            };
            let top = rendered_face_colors(mesh, position, [0, 1, 0]);
            let side = rendered_face_colors(mesh, position, [1, 0, 0]);
            for normal in [[-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
                assert_eq!(rendered_face_colors(mesh, position, normal), side);
            }
            if matches!(
                block,
                Block::Wood | Block::Leaves | Block::Brick | Block::Glass
            ) {
                assert_eq!(
                    side, top,
                    "decorative and transparent blocks keep their colors"
                );
            } else {
                let brightness = |color: [f32; 4]| color[..3].iter().sum::<f32>();
                assert!(brightness(side[0]) < brightness(top[0]));
            }
        }
    }

    #[test]
    fn corner_ao_follows_edits_across_negative_chunk_boundaries() {
        let mut world = World::new(7);
        let y = MAX_Y - 4;
        world
            .set_block(BlockPos::new(-1, y, -1), Block::Stone)
            .unwrap();
        // The top corner of this block samples a diagonal cell in chunk (0, 0).
        let diagonal = BlockPos::new(0, y + 1, 0);
        assert!(affected_chunks(diagonal).contains(&(-1, -1)));
        let corner_brightness = |world: &World| {
            let mesh = chunk_geometry(world, -1, -1).0;
            let index = mesh
                .positions
                .iter()
                .zip(&mesh.normals)
                .position(|(position, normal)| {
                    *position == [0.0, (y + 1) as f32 * CELL_SIZE, 0.0]
                        && *normal == [0.0, 1.0, 0.0]
                })
                .expect("isolated block has a visible top corner");
            mesh.colors[index][0]
        };
        let exposed = corner_brightness(&world);
        world.set_block(diagonal, Block::Stone).unwrap();
        let diagonal_shade = corner_brightness(&world);
        assert!(diagonal_shade < exposed);
        world.set_block(diagonal, Block::Glass).unwrap();
        assert_eq!(corner_brightness(&world), exposed);
        world.set_block(diagonal, Block::Air).unwrap();
        assert_eq!(corner_brightness(&world), exposed);

        // Two solid sides close the corner completely, regardless of its diagonal.
        world
            .set_block(BlockPos::new(0, y + 1, -1), Block::Stone)
            .unwrap();
        world
            .set_block(BlockPos::new(-1, y + 1, 0), Block::Stone)
            .unwrap();
        let closed_corner = corner_brightness(&world);
        assert!(closed_corner < diagonal_shade);
        world.set_block(diagonal, Block::Stone).unwrap();
        assert_eq!(corner_brightness(&world), closed_corner);
    }

    #[test]
    fn placing_adjacent_blocks_hides_the_shared_face() {
        let mut world = World::new(7);
        let count = |world: &World| chunk_geometry(world, 0, 0).0.indices.len() / 3;
        let before = count(&world);
        world
            .set_block(BlockPos::new(2, MAX_Y - 2, 3), Block::Stone)
            .unwrap();
        assert_eq!(count(&world), before + 12);
        world
            .set_block(BlockPos::new(3, MAX_Y - 2, 3), Block::Stone)
            .unwrap();
        assert_eq!(count(&world), before + 20);
        world
            .set_block(BlockPos::new(2, MAX_Y - 2, 3), Block::Air)
            .unwrap();
        assert_eq!(count(&world), before + 12);
    }
    #[test]
    fn landscape_tiles_cover_the_world_except_exactly_the_local_square() {
        const FIRST: i32 = -2048;
        const SIDE: i32 = 4096;
        for center in [
            (0, 0),
            (-137, 91),
            (2040, 2040),
            (-2400, 0),
            (100_000, 100_000),
        ] {
            for near_radius in [3, DETAIL_RADIUS, 12] {
                let tiles = landscape_tiles(FIRST, FIRST, SIDE, center, near_radius);
                assert!(tiles.len() < 1800, "far terrain work stays bounded");
                let clipped_side = |c: i32| {
                    ((c + near_radius + 1).min(FIRST + SIDE) - (c - near_radius).max(FIRST)).max(0)
                        as i64
                };
                let expected =
                    SIDE as i64 * SIDE as i64 - clipped_side(center.0) * clipped_side(center.1);
                assert_eq!(
                    tiles.iter().map(|t| t.2 as i64 * t.2 as i64).sum::<i64>(),
                    expected
                );
                for &(x, z, size) in &tiles {
                    assert!(
                        x >= FIRST
                            && z >= FIRST
                            && x + size <= FIRST + SIDE
                            && z + size <= FIRST + SIDE
                    );
                    assert!(
                        x + size <= center.0 - near_radius
                            || x > center.0 + near_radius
                            || z + size <= center.1 - near_radius
                            || z > center.1 + near_radius
                    );
                }
            }
        }
        assert_eq!(chunk_key([-0.01, 1000., -8.01]), (-1, -2));
    }

    #[test]
    fn nearby_tree_selection_is_bounded_stable_and_preserves_full_density() {
        for center in [(0, 0), (-130, 42), (2047, -2048), (100_000, 100_000)] {
            let cells = lod_tree_cells(center, MIN_TREE_DISTANCE);
            assert_eq!(cells, lod_tree_cells(center, MIN_TREE_DISTANCE));
            assert!(cells.len() <= 512, "near candidates stay bounded");
            let camera = Vec2::new(
                (center.0 as f32 + 0.5) * CHUNK_METERS,
                (center.1 as f32 + 0.5) * CHUNK_METERS,
            );
            for (x, z) in cells {
                let position = Vec2::new(
                    (x as f32 + 0.5) * TREE_GRID_METERS,
                    (z as f32 + 0.5) * TREE_GRID_METERS,
                );
                assert!(position.distance(camera) <= MIN_TREE_DISTANCE + 0.1);
            }
            let nearest = (
                (camera.x / TREE_GRID_METERS).floor() as i32,
                (camera.y / TREE_GRID_METERS).floor() as i32,
            );
            assert!(lod_tree_cells(center, MIN_TREE_DISTANCE).contains(&nearest));
        }
        let extended = lod_tree_cells((-130, 42), MAX_TREE_DISTANCE);
        assert!(extended.len() > 130_000);
        assert!(
            extended.len() < 140_000,
            "maximum range bounds candidate work"
        );
        let extended: std::collections::HashSet<_> = extended.into_iter().collect();
        assert!(
            lod_tree_cells((-130, 42), MIN_TREE_DISTANCE)
                .into_iter()
                .all(|cell| extended.contains(&cell))
        );
    }

    #[test]
    fn default_landscape_keeps_painted_heightmap_and_short_tree_range() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let geo = world.geography().unwrap();
        let peak = geo
            .heights()
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
        let [peak_x, peak_z] = geo.grid_position(peak);
        for (name, center) in [
            ("spawn", chunk_key(world.spawn_position())),
            ("forest", chunk_key([-10_606., 0., -3_150.])),
            ("peak", chunk_key([peak_x, 0., peak_z])),
        ] {
            let camera = Vec2::new(
                center.0 as f32 * CHUNK_METERS,
                center.1 as f32 * CHUNK_METERS,
            );
            let (land, water) =
                landscape_geometry(&world, center, DETAIL_RADIUS, MIN_TREE_DISTANCE);
            let triangles = (land.indices.len() + water.indices.len()) / 3;
            eprintln!("{name}: {triangles} landscape triangles");
            assert!(triangles < 350_000, "stitched heightmap stays bounded");
            for (position, uv) in land.positions.iter().zip(&land.uvs) {
                if uv[0] == 0.0 {
                    let distance = Vec2::new(position[0], position[2]).distance(camera);
                    // Include the final crown/roof footprint at the band edge.
                    assert!(distance <= MIN_TREE_DISTANCE + 16.0);
                }
            }
            assert!(land.uvs.iter().any(|uv| uv[0] == 1.0));
            if name == "forest" {
                assert!(
                    land.uvs.iter().any(|uv| uv[0] == 0.0),
                    "near silhouettes remain"
                );
            }
        }
    }

    #[test]
    fn maximum_tree_range_draws_full_forests_beyond_the_old_tree_budget() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let center = chunk_key([-10_606., 0., -3_150.]);
        let camera = Vec2::new(
            (center.0 as f32 + 0.5) * CHUNK_METERS,
            (center.1 as f32 + 0.5) * CHUNK_METERS,
        );
        let start = std::time::Instant::now();
        let (land, water) = landscape_geometry(&world, center, DETAIL_RADIUS, MAX_TREE_DISTANCE);
        eprintln!(
            "5000-block forest: {} triangles generated in {:.2}s",
            (land.indices.len() + water.indices.len()) / 3,
            start.elapsed().as_secs_f32()
        );
        let cutout = local_bounds(center, DETAIL_RADIUS);
        let mut farthest = 0.0_f32;
        let mut trunk_bottom_vertices = 0;
        for (((position, uv), normal), color) in land
            .positions
            .iter()
            .zip(&land.uvs)
            .zip(&land.normals)
            .zip(&land.colors)
        {
            if uv[0] != 0.0 {
                continue;
            }
            let distance = Vec2::new(position[0], position[2]).distance(camera);
            farthest = farthest.max(distance);
            assert!(distance <= MAX_TREE_DISTANCE + TREE_GRID_METERS);
            assert!(
                position[0] <= cutout[0]
                    || position[0] >= cutout[2]
                    || position[2] <= cutout[1]
                    || position[2] >= cutout[3],
                "proxies never overlap the detailed voxel square"
            );
            if *normal == [0.0, -1.0, 0.0] && *color == srgb_linear(Block::Wood.color()) {
                trunk_bottom_vertices += 1;
            }
        }
        assert!(trunk_bottom_vertices / 4 > 512, "no fixed nearest-tree cap");
        assert!(
            farthest > MAX_TREE_DISTANCE - 50.0,
            "trees reach the selected outer range"
        );
    }

    #[test]
    fn changing_tree_range_replaces_an_obsolete_landscape_job_without_moving() {
        use crate::graphics::GraphicsQuality;
        use rubblekin_core::protocol::SessionMode;
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "terrain test".into(),
            GraphicsQuality::Balanced,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.body.position = [0.; 3];
        let mut settings = GraphicsSettings::new(GraphicsQuality::Balanced);
        let near_radius = settings.near_radius_chunks();
        settings.tree_distance = MAX_TREE_DISTANCE;
        let scene = TerrainScene {
            chunks: HashMap::new(),
            opaque_material: default(),
            glass_material: default(),
            water_material: default(),
            landscape: Some(Landscape {
                center: (0, 0),
                near_radius,
                tree_distance: MIN_TREE_DISTANCE,
                terrain: default(),
                water: default(),
                triangles: 0,
            }),
            pending_landscape: Some(LandscapeJob {
                near_radius,
                tree_distance: MIN_TREE_DISTANCE,
                task: AsyncComputeTaskPool::get().spawn(std::future::pending()),
            }),
            pending_chunks: HashMap::new(),
            triangle_count: 0,
        };
        let mut app = App::new();
        app.insert_resource(crate::VoxelWorld(World::generate(
            42,
            WorldGeneration::GeographyV1,
        )))
        .insert_resource(session)
        .insert_resource(settings)
        .insert_resource(scene)
        .init_resource::<Assets<Mesh>>()
        .add_systems(Update, stream_terrain);
        app.update();
        let scene = app.world().resource::<TerrainScene>();
        assert_eq!(
            scene.landscape.as_ref().unwrap().tree_distance,
            MIN_TREE_DISTANCE
        );
        assert_eq!(
            scene.pending_landscape.as_ref().unwrap().tree_distance,
            MAX_TREE_DISTANCE
        );
        // Returning to the installed range cancels the larger pending mesh too.
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .tree_distance = MIN_TREE_DISTANCE;
        app.update();
        assert!(
            app.world()
                .resource::<TerrainScene>()
                .pending_landscape
                .is_none()
        );
    }

    #[test]
    fn tree_proxies_preserve_shared_dimensions_and_clip_against_near_voxels() {
        for kind in [TreeKind::Broadleaf, TreeKind::Conifer, TreeKind::Scrub] {
            let tree = GeneratedTree {
                base: BlockPos::new(10, 1000, 12),
                trunk_height: 16,
                kind,
                crown_radius: if kind == TreeKind::Scrub { 2 } else { 4 },
            };
            let ground = 71.5;
            let mut complete = Geometry::default();
            add_tree_proxy(
                &mut complete,
                tree,
                ground,
                ProxyClip::Outside([-100., -100., -90., -90.]),
                kind.leaf_color(),
            );
            let min_y = complete
                .positions
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min);
            let max_y = complete
                .positions
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(min_y, ground);
            let (_, top) = tree.leaf_bounds(0, 0).unwrap();
            assert_eq!(max_y, ground + (top + 1 - tree.base.y) as f32 * CELL_SIZE);
            assert!(
                complete.indices.len() / 3 <= 72,
                "small stepped canopy budget"
            );
            let mut clipped = Geometry::default();
            add_tree_proxy(
                &mut clipped,
                tree,
                ground,
                ProxyClip::Outside([0., 0., 6., 20.]),
                kind.leaf_color(),
            );
            assert!(!clipped.indices.is_empty());
            for triangle in clipped.indices.as_chunks::<3>().0 {
                let center = triangle
                    .iter()
                    .map(|&i| Vec3::from_array(clipped.positions[i as usize]))
                    .sum::<Vec3>()
                    / 3.0;
                assert!(center.x >= 6.0, "proxy faces never enter the detail cutout");
            }
        }
    }

    #[test]
    fn every_lod_ground_cell_retains_its_heightmap_triangles_without_caps() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let geo = world.geography().unwrap();
        for step in [1.0, 4.0, 8.0, 16.0, 128.0] {
            let mut land = Geometry::default();
            let mut water = Geometry::default();
            let surface = surface_tile(
                geo,
                [-10_640.0, -3_168.0, step * 8.0],
                8,
                [Some(step); 4],
                &mut land,
                &mut water,
            );
            assert_eq!(
                land.indices.len() / 3,
                8 * 8 * 2,
                "two triangles per cell at {step} m"
            );
            assert_eq!(land.positions.len(), 8 * 8 * 4);
            assert!(land.normals.iter().all(|normal| normal[1] > 0.0));
            for iz in 0..8 {
                for ix in 0..8 {
                    let corners = surface.corners(ix, iz);
                    let first = (iz * 8 + ix) * 4;
                    assert_eq!(
                        land.positions[first..first + 4],
                        corners.map(|corner| corner.0.to_array())
                    );
                    for (u, v) in [(0.25, 0.75), (0.75, 0.75), (0.5, 0.25)] {
                        let x = surface.origin[0] + (ix as f32 + u) * step;
                        let z = surface.origin[1] + (iz as f32 + v) * step;
                        assert!(
                            (surface.height_at(x, z) - triangle_height(corners, u, v)).abs()
                                < 0.001
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn proxy_ground_height_matches_both_landscape_triangle_diagonals() {
        let make_surface = |colors: [[f32; 4]; 4]| SampledSurface {
            origin: [0., 0.],
            step: 8.0,
            steps: 1,
            vertices: vec![
                (Vec3::new(0., 0., 0.), colors[3], None, [0., 1., 0.]),
                (Vec3::new(8., 0., 0.), colors[2], None, [0., 1., 0.]),
                (Vec3::new(0., 0., 8.), colors[0], None, [0., 1., 0.]),
                (Vec3::new(8., 8., 8.), colors[1], None, [0., 1., 0.]),
            ],
        };
        let first = make_surface([[1.; 4]; 4]);
        assert_eq!(first.height_at(2., 6.), 0.0);
        assert_eq!(first.height_at(6., 6.), 4.0);
        let second = make_surface([1., 0.5, 1., 0.5].map(|s| [s, s, s, 1.]));
        assert_eq!(second.height_at(2., 6.), 2.0);
        assert_eq!(second.height_at(6., 6.), 6.0);
    }

    #[test]
    fn entering_chunks_show_tree_proxies_until_atomic_detailed_replacement() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let original_center = chunk_key(world.spawn_position());
        let entering_center = (original_center.0 + 8, original_center.1);
        let original_bounds = local_bounds(original_center, DETAIL_RADIUS);
        let entering_keys = local_keys(entering_center, DETAIL_RADIUS, &world);
        let tree = lod_tree_cells(entering_center, MIN_TREE_DISTANCE)
            .into_iter()
            .filter_map(|(x, z)| world.tree_at(x, z))
            .find(|tree| {
                let x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
                let z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
                x > original_bounds[2] + 3.0 && entering_keys.contains(&chunk_key([x, 0., z]))
            })
            .expect("a generated tree in the entering strip");
        let tree_x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
        let tree_z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
        let key = chunk_key([tree_x, 0., tree_z]);
        let mut ecs = bevy::prelude::World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut terrain_materials = Assets::<TerrainMaterial>::default();
        let mut images = Assets::<Image>::default();
        let mut commands = Commands::new(&mut queue, &ecs);
        let mut scene = setup_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut terrain_materials,
            &mut images,
            &world,
            world.spawn_position(),
            DETAIL_RADIUS,
            MIN_TREE_DISTANCE,
        );
        queue.apply(&mut ecs);
        assert!(
            scene.triangle_count < 700_000,
            "near silhouettes and painted heightmap remain bounded"
        );
        assert!(!scene.chunks.contains_key(&key));
        let mut commands = Commands::new(&mut queue, &ecs);
        move_local_square(
            &mut scene,
            entering_center,
            DETAIL_RADIUS,
            &world,
            &mut commands,
            &mut meshes,
        );
        queue.apply(&mut ecs);
        let chunk = scene.chunks.get(&key).unwrap();
        assert!(!chunk.detailed);
        let handle = chunk.opaque.id();
        let ground = placeholder_ground_height(world.geography().unwrap(), tree_x, tree_z);
        let positions = meshes
            .get(&chunk.opaque)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = positions else {
            panic!("mesh positions")
        };
        let trunk_corner = [
            tree.base.x as f32 * CELL_SIZE,
            ground + tree.trunk_height as f32 * CELL_SIZE,
            tree.base.z as f32 * CELL_SIZE,
        ];
        assert!(
            positions.contains(&trunk_corner),
            "entering placeholder includes its tree immediately"
        );
        let before = meshes.get(&chunk.opaque).unwrap().indices().unwrap().len();
        let mut commands = Commands::new(&mut queue, &ecs);
        rebuild_one(&mut scene, key, &world, &mut commands, &mut meshes);
        queue.apply(&mut ecs);
        let chunk = scene.chunks.get(&key).unwrap();
        assert!(chunk.detailed);
        assert_eq!(
            chunk.opaque.id(),
            handle,
            "replacement retains the visible mesh asset"
        );
        assert_ne!(
            meshes.get(&chunk.opaque).unwrap().indices().unwrap().len(),
            before
        );
    }

    #[test]
    fn a_conifer_crossing_a_biome_boundary_keeps_its_shared_foliage_color() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let leaf = BlockPos::new(-21_211, 788, -6_298);
        let tree = world
            .tree_at(leaf.x.div_euclid(24), leaf.z.div_euclid(24))
            .unwrap();
        assert_eq!(tree.kind, TreeKind::Conifer);
        assert_eq!(world.block(leaf), Block::Leaves);
        assert_eq!(
            world
                .geography()
                .unwrap()
                .sample(
                    (leaf.x as f32 + 0.5) * CELL_SIZE,
                    (leaf.z as f32 + 0.5) * CELL_SIZE,
                )
                .biome,
            Biome::Forest,
            "a crown crosses the biome patch boundary"
        );
        let mesh = chunk_geometry(
            &world,
            leaf.x.div_euclid(CHUNK_SIZE),
            leaf.z.div_euclid(CHUNK_SIZE),
        )
        .0;
        let face_center = Vec3::new(
            leaf.x as f32 * CELL_SIZE,
            (leaf.y as f32 + 0.5) * CELL_SIZE,
            (leaf.z as f32 + 0.5) * CELL_SIZE,
        );
        let index = mesh
            .positions
            .as_chunks::<4>()
            .0
            .iter()
            .position(|points| {
                let center = points.iter().copied().map(Vec3::from_array).sum::<Vec3>() / 4.0;
                center.distance(face_center) < 0.001
            })
            .expect("the outer crown face is visible");
        let expected = srgb_linear(tree.kind.leaf_color());
        for color in &mesh.colors[index * 4..index * 4 + 4] {
            for channel in 1..3 {
                assert!(
                    (color[channel] / color[0] - expected[channel] / expected[0]).abs() < 0.001
                );
            }
        }
        assert_eq!(
            tree_leaf_color(&World::new(42), TreeKind::Broadleaf),
            [0.24, 0.40, 0.31, 1.0]
        );
    }

    #[test]
    fn revised_water_matches_voxel_banks_at_the_real_elevated_slab() {
        let original = Geography::generate(42);
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let revised = world.geography().unwrap();
        for (x, z) in [(5480.0, 9088.0), (5480.5, 9088.5), (5504.0, 9088.0)] {
            let vertex = surface_vertex(revised, x, z);
            let cx = (x / CELL_SIZE).floor() as i32;
            let cz = (z / CELL_SIZE).floor() as i32;
            assert_eq!(vertex.0.y, (world.height_at(cx, cz) + 1) as f32 * CELL_SIZE);
            if x < 5504.0 {
                assert!(
                    vertex.2.is_none(),
                    "the formerly raised slab is dry after voxel sampling"
                );
            } else {
                let level = vertex.2.expect("the actual channel retains water");
                assert!(level > vertex.0.y && level - vertex.0.y <= 2.7);
            }
        }
        let water_patch = |geography: &Geography| {
            let mut land = Geometry::default();
            let mut water = Geometry::default();
            surface_tile(
                geography,
                [5480.0, 9088.0, 1.0],
                2,
                [None; 4],
                &mut land,
                &mut water,
            );
            water
        };
        assert!(
            !water_patch(&original).indices.is_empty(),
            "the original rendering reproduces the slab"
        );
        assert!(
            water_patch(revised).indices.is_empty(),
            "no water mesh is left over those dry voxel banks"
        );
    }

    #[test]
    fn mixed_water_quads_stay_below_dry_valleys_and_share_land_triangulation() {
        for shades in [[1., 0.5, 1., 0.5], [0.5, 1., 0.5, 1.]] {
            let colors = shades.map(|shade| [shade, shade, shade, 1.]);
            let corners: [SurfaceVertex; 4] = [
                (Vec3::new(0., 985., 0.), colors[0], Some(987.), [0., 1., 0.]),
                (Vec3::new(512., 356., 0.), colors[1], None, [0., 1., 0.]),
                (Vec3::new(512., 480., 512.), colors[2], None, [0., 1., 0.]),
                (Vec3::new(0., 700., 512.), colors[3], None, [0., 1., 0.]),
            ];
            let points = water_vertices(corners, 512.).unwrap();
            for i in 1..4 {
                assert!(points[i][1] < corners[i].0.y);
            }
            let mut land = Geometry::default();
            let mut water = Geometry::default();
            let diagonal = quad_diagonal(colors);
            land.quad_normals(
                corners.map(|c| c.0.to_array()),
                [[0., 1., 0.]; 4],
                colors,
                diagonal,
            );
            water.quad_normals(points, [[0., 1., 0.]; 4], [[1.; 4]; 4], diagonal);
            assert_eq!(land.indices, water.indices);
            // Every interpolated point along the completely dry opposite
            // edges stays submerged, even when the wet corner is630m higher.
            for (a, b) in [(1, 2), (2, 3)] {
                for weight in [0., 0.25, 0.5, 0.75, 1.] {
                    let water_y = points[a][1] * (1. - weight) + points[b][1] * weight;
                    let land_y = corners[a].0.y * (1. - weight) + corners[b].0.y * weight;
                    assert!(water_y < land_y);
                }
            }
        }
    }

    #[test]
    fn deep_excavation_exposes_walls_without_storing_the_entire_world_height() {
        let mut world = World::new(7);
        let baseline = CellCache::new(&world, 0, 0);
        let stored = baseline
            .columns
            .iter()
            .map(|c| c.blocks.len())
            .sum::<usize>();
        assert!(stored < CellCache::WIDTH * CellCache::WIDTH * 16);
        let position = BlockPos::new(6, world.min_y() + 3, 6);
        world.set_block(position, Block::Air).unwrap();
        let mesh = chunk_geometry(&world, 0, 0).0;
        // All six walls of this isolated carved cell point into the opening.
        for (normal, _) in FACES {
            let center = Vec3::new(
                (position.x as f32 + 0.5) * CELL_SIZE,
                (position.y as f32 + 0.5) * CELL_SIZE,
                (position.z as f32 + 0.5) * CELL_SIZE,
            );
            let wall = center - Vec3::from_array(normal.map(|n| n as f32)) * CELL_SIZE * 0.5;
            assert!(
                mesh.positions
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(mesh.normals.as_chunks::<4>().0.iter())
                    .any(|(points, normals)| {
                        let face_center =
                            points.iter().copied().map(Vec3::from_array).sum::<Vec3>() / 4.;
                        face_center.distance(wall) < 0.001 && normals[0] == normal.map(|n| n as f32)
                    }),
                "missing excavated wall {normal:?}"
            );
        }
    }

    #[test]
    fn near_distance_changes_keep_retained_chunks_and_release_expired_assets() {
        let world = World::generate(42, WorldGeneration::GeographyV1);
        let center = chunk_key(world.spawn_position());
        let mut ecs = bevy::prelude::World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut terrain_materials = Assets::<TerrainMaterial>::default();
        let mut images = Assets::<Image>::default();
        let mut commands = Commands::new(&mut queue, &ecs);
        let mut scene = setup_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut terrain_materials,
            &mut images,
            &world,
            world.spawn_position(),
            DETAIL_RADIUS,
            MIN_TREE_DISTANCE,
        );
        rebuild_one(&mut scene, center, &world, &mut commands, &mut meshes);
        queue.apply(&mut ecs);
        let retained = scene.chunks[&center].opaque.id();
        let retained_entity = scene.chunks[&center].entity;
        let expired_key = (center.0 + DETAIL_RADIUS, center.1);
        let expired_asset = scene.chunks[&expired_key].opaque.id();
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        scene.pending_chunks.insert(
            expired_key,
            AsyncComputeTaskPool::get().spawn(async { (Geometry::default(), Geometry::default()) }),
        );
        for near_radius in [3, 12, 3] {
            let mut commands = Commands::new(&mut queue, &ecs);
            move_local_square(
                &mut scene,
                center,
                near_radius,
                &world,
                &mut commands,
                &mut meshes,
            );
            queue.apply(&mut ecs);
            let count = (near_radius * 2 + 1).pow(2) as usize;
            assert_eq!(scene.chunks.len(), count);
            assert_eq!(scene.chunks[&center].opaque.id(), retained);
            assert_eq!(scene.chunks[&center].entity, retained_entity);
            assert!(scene.chunks[&center].detailed);
            assert!(meshes.get(expired_asset).is_none());
            assert!(!scene.pending_chunks.contains_key(&expired_key));
            assert!(meshes.len() <= 2 + count * 3);
            assert_eq!(
                scene.triangle_count,
                scene.landscape.as_ref().unwrap().triangles
                    + scene
                        .chunks
                        .values()
                        .map(|chunk| chunk.triangles)
                        .sum::<usize>()
            );
        }
    }

    #[test]
    fn local_streaming_discards_old_mesh_assets_and_matches_geographic_elevation() {
        use rubblekin_core::world::WorldGeneration;
        let mut world = World::generate(42, WorldGeneration::GeographyV1);
        let geo = world.geography().unwrap();
        for (x, z) in [(0., 0.), (-2400., 3300.), (11999., -5022.)] {
            let (vertex, _, _, _) = surface_vertex(geo, x, z);
            let expected = (world.height_at(
                (x / CELL_SIZE).floor() as i32,
                (z / CELL_SIZE).floor() as i32,
            ) + 1) as f32
                * CELL_SIZE;
            assert_eq!(vertex.y, expected);
        }
        let tiles = landscape_tiles(-2048, -2048, 4096, (0, 0), DETAIL_RADIUS);
        let lookup: HashMap<_, _> = tiles.iter().map(|&(x, z, size)| ((x, z), size)).collect();
        let &(tx, tz, size) = tiles
            .iter()
            .find(|&&(x, z, size)| {
                neighbor_steps(&lookup, x, z, size)
                    .iter()
                    .flatten()
                    .any(|step| *step > size as f32)
            })
            .expect("a fine tile borders a coarser tile");
        let neighbors = neighbor_steps(&lookup, tx, tz, size);
        let edge = neighbors
            .iter()
            .position(|neighbor| neighbor.is_some_and(|step| step > size as f32))
            .unwrap();
        let coarse_step = neighbors[edge].unwrap();
        let mut land = Geometry::default();
        let mut water = Geometry::default();
        surface_tile(
            geo,
            [
                tx as f32 * CHUNK_METERS,
                tz as f32 * CHUNK_METERS,
                size as f32 * CHUNK_METERS,
            ],
            8,
            neighbors,
            &mut land,
            &mut water,
        );
        let boundary = match edge {
            0 => tz as f32 * CHUNK_METERS,
            1 => (tz + size) as f32 * CHUNK_METERS,
            2 => tx as f32 * CHUNK_METERS,
            _ => (tx + size) as f32 * CHUNK_METERS,
        };
        for vertex in land
            .positions
            .iter()
            .zip(&land.normals)
            .filter(|(_, normal)| normal[1] > 0.0)
            .map(|(position, _)| position)
            .filter(|v| v[if edge < 2 { 2 } else { 0 }] == boundary)
        {
            let along = vertex[if edge < 2 { 0 } else { 2 }];
            let low = (along / coarse_step).floor() * coarse_step;
            let fraction = (along - low) / coarse_step;
            let (a, b) = if edge < 2 {
                (
                    surface_vertex(geo, low, boundary),
                    surface_vertex(geo, low + coarse_step, boundary),
                )
            } else {
                (
                    surface_vertex(geo, boundary, low),
                    surface_vertex(geo, boundary, low + coarse_step),
                )
            };
            assert!(
                (vertex[1] - (a.0.y * (1. - fraction) + b.0.y * fraction)).abs() < 0.001,
                "fine boundary follows the actual coarse triangle edge"
            );
        }
        let peak = geo
            .heights()
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
        let [peak_x, peak_z] = geo.grid_position(peak);
        let peak_key = chunk_key([peak_x, 0., peak_z]);
        let cache = CellCache::new(&world, peak_key.0, peak_key.1);
        let stored = cache.columns.iter().map(|c| c.blocks.len()).sum::<usize>();
        let peak_top = cache.columns.iter().map(|c| c.top).max().unwrap();
        assert!(peak_top > 1000, "test a genuinely elevated mountain");
        assert!(
            stored < CellCache::WIDTH * CellCache::WIDTH * 128,
            "solid mountain interior must not be materialized"
        );
        let mut ecs = bevy::prelude::World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut terrain_materials = Assets::<TerrainMaterial>::default();
        let mut images = Assets::<Image>::default();
        let mut commands = Commands::new(&mut queue, &ecs);
        let mut scene = setup_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut terrain_materials,
            &mut images,
            &world,
            world.spawn_position(),
            DETAIL_RADIUS,
            MIN_TREE_DISTANCE,
        );
        assert!(scene.chunks.len() <= 169);
        assert!(scene.triangle_count < 600_000);
        queue.apply(&mut ecs);
        for center in [(100, 100), (-130, 42), (1200, -1700), (0, 0)] {
            let expired: Vec<_> = scene
                .chunks
                .values()
                .map(|chunk| chunk.opaque.id())
                .collect();
            let mut commands = Commands::new(&mut queue, &ecs);
            move_local_square(
                &mut scene,
                center,
                DETAIL_RADIUS,
                &world,
                &mut commands,
                &mut meshes,
            );
            queue.apply(&mut ecs);
            assert!(scene.chunks.len() <= 169);
            assert!(
                meshes.len() <= 2 + 169 * 2,
                "terrain and water assets are bounded"
            );
            for handle in expired {
                assert!(meshes.get(handle).is_none());
            }
        }
        // A task captured before a network edit must never replace that edit.
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let snapshot = world.clone();
        scene.pending_chunks.insert(
            (0, 0),
            AsyncComputeTaskPool::get().spawn(async move { chunk_geometry(&snapshot, 0, 0) }),
        );
        let edit = BlockPos::new(2, world.max_y() - 2, 3);
        world.set_block(edit, Block::Brick).unwrap();
        let mut commands = Commands::new(&mut queue, &ecs);
        rebuild_chunks(&mut scene, edit, &world, &mut commands, &mut meshes);
        queue.apply(&mut ecs);
        assert!(!scene.pending_chunks.contains_key(&(0, 0)));
        let chunk = scene.chunks.get(&(0, 0)).unwrap();
        assert!(chunk.detailed);
        let positions = meshes
            .get(&chunk.opaque)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = positions else {
            panic!("mesh positions");
        };
        assert!(
            positions
                .iter()
                .any(|p| p[1] == (edit.y + 1) as f32 * CELL_SIZE)
        );
    }
}
