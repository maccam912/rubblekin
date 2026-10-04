//! Terrain presentation has one direct path: world cells -> exposed faces -> chunk mesh.
//! Chunk meshes contain real editable terrain; the distant mountain ring is scenery.
use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages, light::NotShadowCaster, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};
use rubblekin_core::world::{
    Block, BlockPos, CELL_SIZE, CHUNK_SIZE, MAX_Y, MIN_Y, WATER_LEVEL, WORLD_RADIUS, World,
    berry_patch_positions,
};

/// An edit rebuilds its chunk and touching neighbours, including corner AO.
#[derive(Resource)]
pub struct TerrainScene {
    chunks: HashMap<(i32, i32), ChunkMesh>,
    opaque_material: Handle<StandardMaterial>,
    glass_material: Handle<StandardMaterial>,
    pub triangle_count: usize,
}

struct ChunkMesh {
    opaque: Handle<Mesh>,
    glass: Option<(Entity, Handle<Mesh>)>,
    triangles: usize,
}

/// Explicit tag for decorative geometry that is outside the playable world.
#[derive(Component)]
struct DistantScenery;

pub fn setup_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &World,
) -> TerrainScene {
    let opaque_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        reflectance: 0.12,
        ..default()
    });
    let glass_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.77, 0.94, 0.95, 0.36),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.22,
        cull_mode: None,
        ..default()
    });
    let mut scene = TerrainScene {
        chunks: HashMap::new(),
        opaque_material,
        glass_material,
        triangle_count: 0,
    };
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
        // Atmospheric colors already account for distance. Lighting these
        // giant triangles like nearby blocks creates distracting facets.
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
    let water_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.23, 0.52, 0.59),
        perceptual_roughness: 0.32,
        reflectance: 0.28,
        ..default()
    });
    commands.spawn((
        crate::GameEntity,
        Mesh3d(meshes.add(river_mesh(world))),
        MeshMaterial3d(water_material),
        NotShadowCaster,
    ));
    scene
}

pub fn rebuild_chunks(
    scene: &mut TerrainScene,
    changed: BlockPos,
    world: &World,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    for key in affected_chunks(changed) {
        if scene.chunks.contains_key(&key) {
            rebuild_one(scene, key, world, commands, meshes);
        }
    }
}

fn affected_chunks(changed: BlockPos) -> Vec<(i32, i32)> {
    let cx = changed.x.div_euclid(CHUNK_SIZE);
    let cz = changed.z.div_euclid(CHUNK_SIZE);
    let mut xs = vec![cx];
    let mut zs = vec![cz];
    // Diagonal chunks also share the ambient-occlusion sample at a corner.
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
    key: (i32, i32),
    world: &World,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    let (opaque, glass) = chunk_geometry(world, key.0, key.1);
    let triangles = (opaque.indices.len() + glass.indices.len()) / 3;
    if let Some(chunk) = scene.chunks.get_mut(&key) {
        scene.triangle_count = scene.triangle_count - chunk.triangles + triangles;
        chunk.triangles = triangles;
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
        let opaque_handle = meshes.add(opaque.into_mesh());
        commands.spawn((
            crate::GameEntity,
            Mesh3d(opaque_handle.clone()),
            MeshMaterial3d(scene.opaque_material.clone()),
        ));
        let glass_part = if glass.indices.is_empty() {
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
                opaque: opaque_handle,
                glass: glass_part,
                triangles,
            },
        );
    }
}

/// One-cell halo makes visibility and vertex AO ordinary array reads.
struct CellCache {
    blocks: Vec<Block>,
    tops: Vec<i32>,
    x0: i32,
    z0: i32,
    height: usize,
}

impl CellCache {
    const WIDTH: usize = CHUNK_SIZE as usize + 2;

    fn new(world: &World, cx: i32, cz: i32) -> Self {
        let x0 = cx * CHUNK_SIZE - 1;
        let z0 = cz * CHUNK_SIZE - 1;
        let mut tops = Vec::with_capacity(Self::WIDTH * Self::WIDTH);
        for x in 0..Self::WIDTH {
            for z in 0..Self::WIDTH {
                tops.push(
                    (world.surface_height(
                        (x0 + x as i32) as f32 * CELL_SIZE + CELL_SIZE * 0.5,
                        (z0 + z as i32) as f32 * CELL_SIZE + CELL_SIZE * 0.5,
                    ) / CELL_SIZE) as i32
                        - 1,
                );
            }
        }
        let max_y = tops.iter().copied().max().unwrap_or(MIN_Y).max(MIN_Y);
        let height = (max_y - MIN_Y + 3) as usize;
        let mut result = Self {
            blocks: vec![Block::Air; Self::WIDTH * Self::WIDTH * height],
            tops,
            x0,
            z0,
            height,
        };
        for x in 0..Self::WIDTH {
            for z in 0..Self::WIDTH {
                for y in MIN_Y..=result.tops[x * Self::WIDTH + z] {
                    let index = (x * Self::WIDTH + z) * height + (y - MIN_Y + 1) as usize;
                    result.blocks[index] = world.block(BlockPos {
                        x: result.x0 + x as i32,
                        y,
                        z: result.z0 + z as i32,
                    });
                }
            }
        }
        result
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Block {
        let x = (x - self.x0) as usize;
        let z = (z - self.z0) as usize;
        let y = (y - MIN_Y + 1) as usize;
        self.blocks[(x * Self::WIDTH + z) * self.height + y]
    }

    fn top(&self, x: i32, z: i32) -> i32 {
        self.tops[(x - self.x0) as usize * Self::WIDTH + (z - self.z0) as usize]
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

fn chunk_geometry(world: &World, cx: i32, cz: i32) -> (Geometry, Geometry) {
    let cache = CellCache::new(world, cx, cz);
    let mut opaque = Geometry::default();
    let mut glass = Geometry::default();
    for x in cx * CHUNK_SIZE..(cx + 1) * CHUNK_SIZE {
        for z in cz * CHUNK_SIZE..(cz + 1) * CHUNK_SIZE {
            for y in MIN_Y..=cache.top(x, z) {
                let block = cache.get(x, y, z);
                if block == Block::Air {
                    continue;
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
                        Block::Leaves => [0.24, 0.40, 0.31, 1.0],
                        _ => block.color(),
                    };
                    if block == Block::Grass && normal[1] == 0 {
                        let soil = Block::Dirt.color();
                        for i in 0..3 {
                            color[i] = color[i] * 0.90 + soil[i] * 0.10;
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
                        let shade = variation * [1.0, 0.88, 0.76, 0.64][level as usize];
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
            add_meadow_details(&mut opaque, world, &cache, x, z);
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
fn add_meadow_details(mesh: &mut Geometry, world: &World, cache: &CellCache, x: i32, z: i32) {
    let y = world.height_at(x, z);
    if !(MIN_Y..MAX_Y).contains(&y)
        || cache.top(x, z) < y
        || cache.get(x, y, z) != Block::Grass
        || cache.get(x, y + 1, z) != Block::Air
    {
        return;
    }
    let chance = hash(x, 713, z, world.seed);
    if chance > 0.036 || y > 50 {
        return;
    }
    let base = Vec3::new(
        (x as f32 + 0.5) * CELL_SIZE,
        (y + 1) as f32 * CELL_SIZE,
        (z as f32 + 0.5) * CELL_SIZE,
    );
    let flowering = chance < 0.009;
    let stem = if flowering {
        [0.26, 0.38, 0.21, 1.0]
    } else {
        [0.46, 0.58, 0.29, 1.0]
    };
    for i in 0..3 {
        let offset = Vec3::new((i as f32 - 1.) * 0.08, 0., (i % 2) as f32 * 0.09);
        let height = 0.13 + hash(x, i + 17, z, world.seed) * 0.18;
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
struct Geometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Geometry {
    fn quad(&mut self, vertices: [[f32; 3]; 4], normal: [f32; 3], colors: [[f32; 4]; 4]) {
        let start = self.positions.len() as u32;
        self.positions.extend(vertices);
        self.normals.extend([normal; 4]);
        self.colors.extend(colors);
        let brightness = colors.map(|color| color[..3].iter().sum::<f32>());
        // Run the diagonal through the darker pair so interpolated AO does not
        // leave a bright crease through an otherwise occluded face.
        let indices = if brightness[0] + brightness[2] > brightness[1] + brightness[3] {
            [0, 1, 3, 1, 2, 3]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        self.indices.extend(indices.map(|index| start + index));
    }

    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        let start = self.positions.len() as u32;
        let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
        self.positions
            .extend([a.to_array(), b.to_array(), c.to_array()]);
        self.normals.extend([normal; 3]);
        self.colors.extend([srgb_linear(color); 3]);
        self.indices.extend([start, start + 1, start + 2]);
    }

    fn cuboid(&mut self, center: Vec3, dimensions: Vec3, color: [f32; 4]) {
        for (normal, corners) in FACES {
            let vertices = corners.map(|p| {
                (center + (Vec3::from_array(p) - Vec3::splat(0.5)) * dimensions).to_array()
            });
            self.quad(vertices, normal.map(|n| n as f32), [srgb_linear(color); 4]);
        }
    }

    fn into_mesh(self) -> Mesh {
        let count = self.positions.len();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; count])
        .with_inserted_indices(Indices::U32(self.indices))
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
}
