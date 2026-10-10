//! Shared build discoveries and merged distant faces. Detail replaces proxies
//! chunk by chunk only after its mesh is installed, including edits in flight.
use crate::{
    GameEntity, VoxelWorld,
    terrain::{FACES, Geometry, TerrainScene},
};
use bevy::{light::NotShadowCaster, prelude::*};
use rubblekin_core::{
    building::{Build, substantial_builds},
    world::{Block, BlockEdit, BlockPos, CELL_SIZE, CHUNK_SIZE},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
pub(crate) struct Scene {
    edits: Option<Vec<BlockEdit>>,
    pub builds: Vec<Build>,
    meshes: Vec<(Entity, Handle<Mesh>)>,
    markers: Vec<Entity>,
}
#[derive(Component)]
pub(crate) struct Proxy((i32, i32));
#[derive(Component)]
pub(crate) struct Marker(usize);
#[derive(Component)]
pub(crate) struct MarkerLabel(usize);
pub(crate) fn setup(mut commands: Commands) {
    commands.insert_resource(Scene::default());
}

/// Merge coplanar adjacent faces of equal material, preserving silhouette and
/// glass while avoiding one cube/draw call for every saved block.
#[allow(clippy::type_complexity)]
fn geometry(
    world: &rubblekin_core::world::World,
    builds: &[Build],
) -> BTreeMap<(i32, i32), (Geometry, Geometry)> {
    let mut faces: BTreeMap<((i32, i32), usize, i32, usize), BTreeSet<(i32, i32)>> =
        BTreeMap::new();
    for e in builds
        .iter()
        .flat_map(|b| &b.cells)
        .filter(|e| e.block.is_solid())
    {
        let p = e.position;
        let coords = [p.x, p.y, p.z];
        let chunk = (p.x.div_euclid(CHUNK_SIZE), p.z.div_euclid(CHUNK_SIZE));
        for (face, (normal, _)) in FACES.iter().enumerate() {
            let n = BlockPos::new(p.x + normal[0], p.y + normal[1], p.z + normal[2]);
            let adjacent = world.block(n);
            if (adjacent.is_solid() && adjacent != Block::Glass)
                || (e.block == Block::Glass && adjacent == Block::Glass)
            {
                continue;
            }
            let axis = normal.iter().position(|n| *n != 0).unwrap();
            let others: Vec<_> = (0..3).filter(|a| *a != axis).collect();
            let plane = coords[axis] + i32::from(normal[axis] > 0);
            faces
                .entry((chunk, face, plane, e.block.catalog_index().unwrap()))
                .or_default()
                .insert((coords[others[0]], coords[others[1]]));
        }
    }
    let mut chunks: BTreeMap<_, (Geometry, Geometry)> = BTreeMap::new();
    for ((chunk, face, plane, material), mut cells) in faces {
        let block = Block::ALL[material];
        let (normal, corners) = FACES[face];
        let axis = normal.iter().position(|n| *n != 0).unwrap();
        let others: Vec<_> = (0..3).filter(|a| *a != axis).collect();
        while let Some((a, b)) = cells.pop_first() {
            let mut width = 1;
            while cells.contains(&(a + width, b)) {
                width += 1;
            }
            let mut height = 1;
            while (0..width).all(|x| cells.contains(&(a + x, b + height))) {
                height += 1;
            }
            for x in 0..width {
                for y in 0..height {
                    cells.remove(&(a + x, b + y));
                }
            }
            let points = corners.map(|c| {
                let mut p = [0.; 3];
                p[axis] = plane as f32;
                p[others[0]] = a as f32 + c[others[0]] * width as f32;
                p[others[1]] = b as f32 + c[others[1]] * height as f32;
                p.map(|v| v * CELL_SIZE)
            });
            let color = Color::srgba(block.color()[0], block.color()[1], block.color()[2], 1.)
                .to_linear()
                .to_f32_array();
            let entry = chunks.entry(chunk).or_default();
            let mesh = if block == Block::Glass {
                &mut entry.1
            } else {
                &mut entry.0
            };
            mesh.quad(points, normal.map(|n| n as f32), [color; 4]);
        }
    }
    chunks
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    terrain: Res<TerrainScene>,
    mut scene: ResMut<Scene>,
    mut meshes: ResMut<Assets<Mesh>>,
    canvas: Single<Entity, With<crate::world_map::MapCanvas>>,
    mut fonts: ResMut<Assets<Font>>,
    mut proxies: Query<(&Proxy, &mut Visibility)>,
) {
    if scene.edits.is_none() || world.is_changed() {
        let edits = world.0.edits();
        if scene.edits.as_ref() != Some(&edits) {
            for (e, m) in scene.meshes.drain(..) {
                commands.entity(e).despawn();
                meshes.remove(m.id());
            }
            for e in scene.markers.drain(..) {
                commands.entity(e).despawn();
            }
            scene.builds = substantial_builds(&edits);
            let (opaque, glass) = terrain.build_materials();
            for (key, (a, b)) in geometry(&world.0, &scene.builds) {
                for (g, material) in [(a, opaque.clone()), (b, glass.clone())] {
                    let mesh = g.into_mesh();
                    if mesh.count_vertices() == 0 {
                        continue;
                    }
                    let handle = meshes.add(mesh);
                    let entity = commands
                        .spawn((
                            GameEntity,
                            Proxy(key),
                            Mesh3d(handle.clone()),
                            MeshMaterial3d(material),
                            NotShadowCaster,
                            if terrain.detailed_at(key) {
                                Visibility::Hidden
                            } else {
                                Visibility::Visible
                            },
                        ))
                        .id();
                    scene.meshes.push((entity, handle));
                }
            }
            let font = fonts.add(Font::from_bytes(
                include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
            ));
            let builds: Vec<_> = scene.builds.iter().map(|b| b.cells.len()).collect();
            for (index, count) in builds.into_iter().enumerate() {
                let entity = commands
                    .spawn((
                        GameEntity,
                        Marker(index),
                        ZIndex(2),
                        Node {
                            position_type: PositionType::Absolute,
                            display: Display::None,
                            padding: UiRect::all(px(3)),
                            border: UiRect::all(px(1)),
                            width: px(24),
                            height: px(22),
                            ..default()
                        },
                        BorderColor::all(Color::srgb(0.97, 0.75, 0.32)),
                        BackgroundColor(Color::srgba(0.04, 0.13, 0.1, 0.9)),
                    ))
                    .with_children(|marker| {
                        marker
                            .spawn((Node {
                                width: px(18),
                                height: px(14),
                                ..default()
                            },))
                            .with_children(|house| {
                                house.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(4),
                                        top: px(1),
                                        width: px(8),
                                        height: px(8),
                                        ..default()
                                    },
                                    UiTransform::from_rotation(Rot2::radians(
                                        std::f32::consts::FRAC_PI_4,
                                    )),
                                    BackgroundColor(Color::srgb(0.97, 0.75, 0.32)),
                                ));
                                house.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(2),
                                        top: px(5),
                                        width: px(12),
                                        height: px(9),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.97, 0.75, 0.32)),
                                ));
                                house.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(6),
                                        top: px(9),
                                        width: px(4),
                                        height: px(5),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.04, 0.13, 0.1)),
                                ));
                            });
                        marker.spawn((
                            MarkerLabel(index),
                            Text::new(format!("Build {} · {count} blocks", index + 1)),
                            TextFont::from_font_size(12.).with_font(font.clone()),
                            TextColor(Color::srgb(0.97, 0.85, 0.53)),
                            Node {
                                position_type: PositionType::Absolute,
                                display: Display::None,
                                width: px(150),
                                padding: UiRect::all(px(3)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.04, 0.13, 0.1, 0.95)),
                        ));
                    })
                    .id();
                commands.entity(*canvas).add_child(entity);
                scene.markers.push(entity);
            }
            scene.edits = Some(edits);
        }
    }
    for (proxy, mut visible) in &mut proxies {
        *visible = if terrain.detailed_at(proxy.0) {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn map_markers(
    scene: Res<Scene>,
    world: Res<VoxelWorld>,
    map: Res<crate::world_map::WorldMap>,
    session: Res<crate::Session>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    tutorials: Res<crate::tutorials::Tutorials>,
    mut nodes: Query<(&Marker, &mut Node), Without<MarkerLabel>>,
    mut labels: Query<(&MarkerLabel, &mut Node), Without<Marker>>,
) {
    let (mut side, _) = crate::world_map::layout(&window);
    if tutorials.incomplete(crate::tutorials::Lesson::Map) {
        side = (side - 24.).max(64.);
    }
    for (marker, mut node) in &mut nodes {
        let point = scene
            .builds
            .get(marker.0)
            .and_then(|b| map.point(crate::world_map_image::map_uv(&world.0, b.position()), side));
        node.display = if map.open && !map.activities && point.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(p) = point {
            node.left = px((p.x - 12.).clamp(0., (side - 24.).max(0.)));
            node.top = px((p.y - 11.).clamp(0., (side - 22.).max(0.)));
        }
    }
    let mut occupied: Vec<Rect> = std::iter::once(crate::world_map::position(&session))
        .chain(
            session
                .players
                .iter()
                .filter(|p| p.id != session.id)
                .map(|p| p.body.position),
        )
        .filter_map(|p| map.point(crate::world_map_image::map_uv(&world.0, p), side))
        .map(|p| Rect::from_corners(p + Vec2::new(-10., -14.), p + Vec2::new(100., 32.)))
        .collect();
    for (label, mut node) in &mut labels {
        let point = scene
            .builds
            .get(label.0)
            .and_then(|b| map.point(crate::world_map_image::map_uv(&world.0, b.position()), side));
        node.display = if map.zoom() >= 8. && side >= 180. {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(p) = point {
            let parent_left = (p.x - 12.).clamp(0., (side - 24.).max(0.));
            let left = (p.x + 12.).clamp(0., (side - 156.).max(0.)) - parent_left;
            let mut top = 40.;
            let rect = |top| {
                Rect::from_corners(
                    Vec2::new(parent_left + left, p.y - 11. + top),
                    Vec2::new(parent_left + left + 156., p.y - 11. + top + 24.),
                )
            };
            while occupied
                .iter()
                .any(|r| r.intersect(rect(top)).size().min_element() > 0.)
                && p.y + top < side - 24.
            {
                top += 24.;
            }
            if p.y + top >= side - 24. {
                node.display = Display::None;
            }
            occupied.push(rect(top));
            node.left = px(left);
            node.top = px(top);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merged_proxy_keeps_geometry_outside_near_detail_and_glass() {
        let mut w = rubblekin_core::world::World::new(42);
        let cells: Vec<_> = rubblekin_core::building::Selection::new(
            BlockPos::new(40, 80, 40),
            BlockPos::new(47, 80, 47),
        )
        .unwrap()
        .cells()
        .map(|position| BlockEdit {
            position,
            block: Block::Brick,
        })
        .collect();
        for e in &cells {
            w.set_block(e.position, e.block).unwrap();
        }
        let builds = substantial_builds(&w.edits());
        let g = geometry(&w, &builds);
        assert_eq!(g.len(), 1);
        assert_eq!(
            g.into_values()
                .next()
                .unwrap()
                .0
                .into_mesh()
                .count_vertices(),
            24
        );
        w.set_block(cells[0].position, Block::Glass).unwrap();
        assert!(
            geometry(&w, &substantial_builds(&w.edits()))
                .into_values()
                .any(|(_, glass)| glass.into_mesh().count_vertices() > 0)
        );
    }
}
