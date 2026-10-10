//! Nearby emission is real PBR light, with a bounded budget in every preset.
use crate::{
    GameEntity, Session, VoxelWorld,
    graphics::{GraphicsQuality, GraphicsSettings},
};
use bevy::{
    light::{NotShadowCaster, ShadowFilteringMethod},
    prelude::*,
};
use rubblekin_core::world::{Block, BlockPos, CELL_SIZE};
use std::collections::HashMap;

#[derive(Resource)]
pub(crate) struct Scene {
    sources: Vec<(BlockPos, Block)>,
    glass: Vec<BlockPos>,
    cube: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    flame: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
    lights: HashMap<BlockPos, Entity>,
    next_refresh: f64,
}
#[derive(Component)]
pub(crate) struct Source {
    block: Block,
    position: BlockPos,
}
#[derive(Component)]
pub(crate) struct Flame(BlockPos);
fn center(p: BlockPos) -> Vec3 {
    Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5) * CELL_SIZE
}
pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glowing = |color: Color| StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * 6.,
        ..default()
    };
    commands.insert_resource(Scene {
        sources: Vec::new(),
        glass: Vec::new(),
        cube: meshes.add(Cuboid::default()),
        wood: materials.add(Color::srgb(0.31, 0.16, 0.07)),
        flame: materials.add(glowing(Color::srgb(1., 0.35, 0.03))),
        core: materials.add(glowing(Color::srgb(1., 0.88, 0.23))),
        lamp: materials.add(glowing(Color::srgb(1., 0.83, 0.44))),
        lights: HashMap::new(),
        next_refresh: 0.,
    });
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    graphics: Res<GraphicsSettings>,
    time: Res<Time>,
    mut scene: ResMut<Scene>,
    mut lights: Query<(&Source, &mut PointLight)>,
    mut flames: Query<(&Flame, &mut Transform)>,
    mut cameras: Query<&mut ShadowFilteringMethod, With<crate::GameCamera>>,
) {
    let now = time.elapsed_secs_f64();
    if world.is_changed() || scene.next_refresh == 0. {
        let edits = world.0.edits();
        scene.glass = edits
            .iter()
            .filter(|e| e.block == Block::Glass)
            .map(|e| e.position)
            .collect();
        let sources: Vec<_> = edits
            .into_iter()
            .filter(|e| e.block.light().is_some())
            .map(|e| (e.position, e.block))
            .collect();
        if scene.sources != sources {
            for (_, e) in scene.lights.drain() {
                commands.entity(e).despawn();
            }
            scene.sources = sources;
        }
        scene.next_refresh = 0.;
    }
    if now >= scene.next_refresh {
        scene.next_refresh = now + 0.2;
        let here = Vec3::from_array(
            session
                .observer
                .as_ref()
                .map_or(session.body.position, |o| o.position.to_array()),
        );
        // Hardware's four taps expose the glass coverage pattern. Use the
        // wider filter only when transparent casters are near the shadow view.
        let near_glass = graphics.quality.shadows()
            && scene.glass.iter().any(|p| {
                center(*p).distance_squared(here) < (graphics.shadow_distance + 8.).powi(2)
            });
        let filter = if near_glass {
            ShadowFilteringMethod::Gaussian
        } else {
            graphics.quality.shadow_filter()
        };
        for mut camera_filter in &mut cameras {
            if *camera_filter != filter {
                *camera_filter = filter;
            }
        }
        let mut nearest: Vec<_> = scene
            .sources
            .iter()
            .copied()
            .filter(|(p, _)| center(*p).distance_squared(here) < 64f32.powi(2))
            .collect();
        nearest.sort_by(|(a, _), (b, _)| {
            center(*a)
                .distance_squared(here)
                .total_cmp(&center(*b).distance_squared(here))
                .then_with(|| (a.x, a.y, a.z).cmp(&(b.x, b.y, b.z)))
        });
        nearest.truncate(if graphics.quality == GraphicsQuality::Low {
            16
        } else {
            32
        });
        let stale: Vec<_> = scene
            .lights
            .keys()
            .copied()
            .filter(|p| !nearest.iter().any(|(n, _)| n == p))
            .collect();
        for p in stale {
            commands.entity(scene.lights.remove(&p).unwrap()).despawn();
        }
        for (p, block) in &nearest {
            if scene.lights.contains_key(p) {
                continue;
            }
            let p = *p;
            let block = *block;
            let (rgb, intensity) = block.light().unwrap();
            let entity = commands
                .spawn((
                    GameEntity,
                    Source { block, position: p },
                    PointLight {
                        color: Color::srgb(rgb[0], rgb[1], rgb[2]),
                        intensity,
                        range: 10.,
                        radius: 0.10,
                        contact_shadows_enabled: true,
                        ..default()
                    },
                    Transform::from_translation(center(p) + Vec3::Y * 0.12),
                    Visibility::default(),
                ))
                .with_children(|parent| {
                    if block == Block::Torch {
                        parent.spawn((
                            Mesh3d(scene.cube.clone()),
                            MeshMaterial3d(scene.wood.clone()),
                            Transform::from_xyz(0., -0.17, 0.)
                                .with_scale(Vec3::new(0.075, 0.32, 0.075)),
                        ));
                        parent
                            .spawn((
                                Flame(p),
                                Mesh3d(scene.cube.clone()),
                                MeshMaterial3d(scene.flame.clone()),
                                NotShadowCaster,
                                Transform::from_xyz(0., 0.02, 0.)
                                    .with_scale(Vec3::new(0.12, 0.16, 0.12)),
                            ))
                            .with_child((
                                Mesh3d(scene.cube.clone()),
                                MeshMaterial3d(scene.core.clone()),
                                NotShadowCaster,
                                Transform::from_xyz(0., -0.1, 0.)
                                    .with_scale(Vec3::new(0.55, 0.65, 0.55)),
                            ));
                    } else {
                        parent.spawn((
                            Mesh3d(scene.cube.clone()),
                            MeshMaterial3d(scene.lamp.clone()),
                            NotShadowCaster,
                            Transform::from_xyz(0., -0.12, 0.)
                                .with_scale(Vec3::splat(CELL_SIZE + 0.004)),
                        ));
                    }
                })
                .id();
            scene.lights.insert(p, entity);
        }
        // Only the nearest four lights use six-face shadow maps; contact shadows
        // still give Low cheap local occlusion. Re-evaluate when quality changes.
        for (source, mut light) in &mut lights {
            let rank = nearest.iter().position(|(p, _)| *p == source.position);
            light.shadow_maps_enabled = graphics.quality.shadows() && rank.is_some_and(|i| i < 4);
        }
    }
    for (source, mut light) in &mut lights {
        let flicker = if source.block == Block::Torch {
            flicker(source.position, now as f32)
        } else {
            1.
        };
        light.intensity = source.block.light().unwrap().1 * flicker;
    }
    for (flame, mut transform) in &mut flames {
        transform.scale.y = 0.16 * flicker(flame.0, now as f32);
    }
}
fn flicker(p: BlockPos, time: f32) -> f32 {
    let phase = p.x as f32 * 1.7 + p.y as f32 * 0.3 + p.z as f32 * 2.1;
    0.94 + 0.06 * (time * 8.3 + phase).sin() + 0.035 * (time * 17.1 + phase * 0.7).sin()
}
