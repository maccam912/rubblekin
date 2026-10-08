//! A locked inspection target, selected along the camera's center ray on opening.
use crate::{Avatar, Avatars, GameCamera, Session, VoxelWorld, crops, pause};
use bevy::prelude::*;
use rubblekin_core::world::{BlockPos, CELL_SIZE, World};

const INSPECT_DISTANCE: f32 = 128.0;
const CHARACTER_SIZE: Vec3 = Vec3::new(0.80, 1.88, 0.80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InspectTarget {
    Npc,
    Resident(u64),
    Player(u64),
    Wildlife(u64),
    Block(BlockPos),
    FarmPlot { village: u32, field: usize },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    camera: Single<&Transform, With<GameCamera>>,
    world: Res<VoxelWorld>,
    mut session: ResMut<Session>,
    avatars: Res<Avatars>,
    transforms: Query<&Transform, With<Avatar>>,
    wildlife: Query<(&crate::wildlife::Wildlife, &Transform)>,
    pause: Option<Res<pause::PauseMenu>>,
    mut gizmos: Gizmos,
) {
    if !session.inspector || pause.is_some_and(|menu| menu.open || menu.input_blocked) {
        return;
    }
    // Prefer rendered poses: remote characters are smoothed between snapshots.
    let rendered = |entity: Option<&Entity>, fallback: [f32; 3]| {
        entity
            .and_then(|entity| transforms.get(*entity).ok())
            .map_or(Vec3::from_array(fallback), |pose| pose.translation)
    };
    let animal_pose = |id, fallback| {
        wildlife
            .iter()
            .find(|(a, _)| a.id == id)
            .map_or(Vec3::from_array(fallback), |(_, p)| p.translation)
    };
    if session.inspect_requested {
        let characters = std::iter::once((
            InspectTarget::Npc,
            rendered(avatars.npc.as_ref(), session.npc.position),
        ))
        .chain(session.residents.iter().map(|resident| {
            (
                InspectTarget::Resident(resident.id),
                rendered(avatars.residents.get(&resident.id), resident.position),
            )
        }))
        .chain(
            session
                .players
                .iter()
                .filter(|player| player.id != session.id)
                .map(|player| {
                    (
                        InspectTarget::Player(player.id),
                        rendered(avatars.players.get(&player.id), player.body.position),
                    )
                }),
        );
        let characters = characters.chain(
            session
                .wildlife
                .iter()
                .map(|a| (InspectTarget::Wildlife(a.id), animal_pose(a.id, a.position))),
        );
        let selected = pick(&world.0, &session, &camera, characters);
        session.inspected = selected;
        session.inspect_requested = false;
    }
    let color = Color::srgb(0.40, 0.90, 0.84);
    let character = match session.inspected {
        Some(InspectTarget::Npc) => Some((
            rendered(avatars.npc.as_ref(), session.npc.position),
            session.npc.target,
        )),
        Some(InspectTarget::Resident(id)) => session
            .residents
            .iter()
            .find(|r| r.id == id)
            .map(|r| (rendered(avatars.residents.get(&id), r.position), r.target)),
        Some(InspectTarget::Player(id)) => session
            .players
            .iter()
            .find(|p| p.id == id)
            .map(|p| (rendered(avatars.players.get(&id), p.body.position), None)),
        Some(InspectTarget::Wildlife(id)) => session
            .wildlife
            .iter()
            .find(|a| a.id == id)
            .map(|a| (animal_pose(id, a.position), None)),
        Some(InspectTarget::Block(position)) => {
            gizmos.cube(
                Transform::from_translation(block_center(position))
                    .with_scale(Vec3::splat(CELL_SIZE + 0.025)),
                color,
            );
            None
        }
        Some(InspectTarget::FarmPlot { village, field }) => {
            if let Some(plot) = world
                .0
                .settlements()
                .and_then(|plan| plan.villages.iter().find(|v| v.id == village))
                .and_then(|v| v.fields.get(field))
            {
                let size = Vec3::new(
                    plot.width as f32 * CELL_SIZE,
                    0.03,
                    plot.depth as f32 * CELL_SIZE,
                );
                let center = Vec3::new(
                    plot.origin.x as f32 * CELL_SIZE + size.x * 0.5,
                    (plot.origin.y + 1) as f32 * CELL_SIZE + 0.02,
                    plot.origin.z as f32 * CELL_SIZE + size.z * 0.5,
                );
                gizmos.cube(Transform::from_translation(center).with_scale(size), color);
            }
            None
        }
        None => None,
    };
    if let Some((feet, target)) = character {
        let size = target_size(&session, session.inspected.unwrap());
        gizmos.cube(
            Transform::from_translation(feet + Vec3::Y * size.y * 0.5).with_scale(size),
            color,
        );
        if let Some(target) = target {
            gizmos.line(
                feet + Vec3::Y,
                Vec3::from_array(target) + Vec3::Y * 0.15,
                color,
            );
        }
    }
}

fn target_size(session: &Session, target: InspectTarget) -> Vec3 {
    if let InspectTarget::Wildlife(id) = target {
        session
            .wildlife
            .iter()
            .find(|a| a.id == id)
            .map_or(CHARACTER_SIZE, |a| {
                crate::wildlife::rendered_size(a.species)
            })
    } else {
        CHARACTER_SIZE
    }
}

fn pick(
    world: &World,
    session: &Session,
    camera: &Transform,
    characters: impl IntoIterator<Item = (InspectTarget, Vec3)>,
) -> Option<InspectTarget> {
    let origin = camera.translation;
    let direction = camera.forward().as_vec3();
    let terrain = world.raycast(origin.to_array(), direction.to_array(), INSPECT_DISTANCE);
    let mut closest = terrain
        .as_ref()
        .map_or(INSPECT_DISTANCE, |hit| hit.distance);
    let mut selected = terrain.map(|hit| block_target(world, hit.position));
    for (target, feet) in characters {
        let size = target_size(session, target);
        let center = feet + Vec3::Y * size.y * 0.5;
        if let Some(distance) = box_hit(origin, direction, center, size, closest) {
            closest = distance;
            selected = Some(target);
        }
    }
    if let Some(plan) = world.settlements() {
        for village in &plan.villages {
            let stage = crops::growth_stage(
                session
                    .villages
                    .iter()
                    .find(|v| v.id == village.id)
                    .map_or(0.0, |v| v.crop_growth),
            );
            for (field, plot) in village.fields.iter().enumerate() {
                // Skip entire remote fields before checking individual decorative plants.
                let size = Vec3::new(
                    plot.width as f32 * CELL_SIZE,
                    0.9,
                    plot.depth as f32 * CELL_SIZE,
                );
                let center = Vec3::new(
                    plot.origin.x as f32 * CELL_SIZE + size.x * 0.5,
                    (plot.origin.y + 1) as f32 * CELL_SIZE + size.y * 0.5,
                    plot.origin.z as f32 * CELL_SIZE + size.z * 0.5,
                );
                if box_hit(origin, direction, center, size, closest).is_none() {
                    continue;
                }
                let kind = crops::crop_kind(world, village, field);
                for soil in plot.plant_positions() {
                    for (center, size, _) in crops::visible_plant_parts(world, soil, stage, kind) {
                        if let Some(distance) = box_hit(origin, direction, center, size, closest) {
                            closest = distance;
                            selected = Some(InspectTarget::FarmPlot {
                                village: village.id,
                                field,
                            });
                        }
                    }
                }
            }
        }
    }
    selected
}

fn block_target(world: &World, position: BlockPos) -> InspectTarget {
    if let Some(plan) = world.settlements() {
        for village in &plan.villages {
            if let Some(field) = village.fields.iter().position(|plot| {
                position.y == plot.origin.y
                    && (plot.origin.x..plot.origin.x + plot.width).contains(&position.x)
                    && (plot.origin.z..plot.origin.z + plot.depth).contains(&position.z)
            }) {
                return InspectTarget::FarmPlot {
                    village: village.id,
                    field,
                };
            }
        }
    }
    InspectTarget::Block(position)
}

fn block_center(position: BlockPos) -> Vec3 {
    Vec3::new(
        position.x as f32 + 0.5,
        position.y as f32 + 0.5,
        position.z as f32 + 0.5,
    ) * CELL_SIZE
}

/// Slab intersection in meters, including origins inside a box and parallel rays.
fn box_hit(origin: Vec3, direction: Vec3, center: Vec3, size: Vec3, limit: f32) -> Option<f32> {
    let min = center - size * 0.5;
    let max = center + size * 0.5;
    let mut entry = 0.0_f32;
    let mut exit = limit;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-8 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
        }
        if entry > exit {
            return None;
        }
    }
    (exit >= 0.0).then_some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics::GraphicsQuality, join::session_from_welcome};
    use rubblekin_core::protocol::{ServerMessage, SessionMode, VillageSnapshot};
    use rubblekin_core::world::{Block, WorldGeneration};

    fn fixture(generation: WorldGeneration) -> (World, Session) {
        let mut welcome = crate::join::tests::welcome(SessionMode::Observer);
        if let ServerMessage::Welcome {
            generation: value, ..
        } = &mut welcome
        {
            *value = generation;
        }
        session_from_welcome(
            welcome,
            "test".into(),
            GraphicsQuality::Low,
            0.0,
            SessionMode::Observer,
        )
        .unwrap()
    }

    #[test]
    fn wildlife_uses_its_small_aimed_bounds_and_cannot_be_inspected_through_a_wall() {
        use rubblekin_core::wildlife::{Species, WildlifeAction, WildlifeSnapshot};
        let (mut world, mut session) = fixture(WorldGeneration::ValleyV1);
        session.wildlife.push(WildlifeSnapshot {
            id: 7,
            species: Species::Rabbit,
            position: [1., 50., 0.],
            velocity: [0.; 3],
            action: WildlifeAction::Grazing,
            hunger: 40.,
            habitat: 0,
        });
        let target = (InspectTarget::Wildlife(7), Vec3::new(1., 50., 0.));
        let camera = Transform::from_xyz(-2., 50.4, 0.).looking_to(Vec3::X, Vec3::Y);
        assert_eq!(
            pick(&world, &session, &camera, [target]),
            Some(InspectTarget::Wildlife(7))
        );
        let high = Transform::from_xyz(-2., 51.2, 0.).looking_to(Vec3::X, Vec3::Y);
        assert_eq!(
            pick(&world, &session, &high, [target]),
            None,
            "the rabbit must not have human-sized aimed bounds"
        );
        let wall = BlockPos::new(0, 100, 0);
        world
            .set_block(wall, rubblekin_core::world::Block::Brick)
            .unwrap();
        assert_eq!(
            pick(&world, &session, &camera, [target]),
            Some(InspectTarget::Block(wall))
        );
    }

    #[test]
    fn center_ray_selects_nearest_character_or_block_and_respects_walls() {
        let (mut world, session) = fixture(WorldGeneration::ValleyV1);
        let wall = BlockPos::new(0, 101, 0);
        world.set_block(wall, Block::Brick).unwrap();
        let camera = Transform::from_xyz(-2.0, 50.8, 0.25).looking_to(Vec3::X, Vec3::Y);
        let behind_wall = (InspectTarget::Npc, Vec3::new(2.0, 50.0, 0.25));
        assert_eq!(
            pick(&world, &session, &camera, [behind_wall]),
            Some(InspectTarget::Block(wall))
        );
        let nearby = (InspectTarget::Resident(7), Vec3::new(-1.0, 50.0, 0.25));
        assert_eq!(
            pick(&world, &session, &camera, [behind_wall, nearby]),
            Some(InspectTarget::Resident(7))
        );
        assert_eq!(
            pick(&world, &session, &camera, [nearby, behind_wall]),
            Some(InspectTarget::Resident(7))
        );
        let empty = camera.looking_to(Vec3::Y, Vec3::Z);
        assert_eq!(pick(&world, &session, &empty, [behind_wall]), None);
    }

    #[test]
    fn crop_shapes_and_soil_select_the_plot_but_removed_soil_and_underground_do_not() {
        let (mut world, mut session) = fixture(WorldGeneration::GeographyV3);
        let village = &world.settlements().unwrap().villages[0];
        let id = village.id;
        let soil = village.fields[0].plant_positions().next().unwrap();
        let target = InspectTarget::FarmPlot {
            village: id,
            field: 0,
        };
        session.villages.push(VillageSnapshot {
            id,
            food: 80.0,
            timber: 0.0,
            stone: 0.0,
            clay: 0.0,
            iron: 0.0,
            crop_growth: 1.0,
            population: 6,
            housing_capacity: 9,
            food_reserve: 20.0,
            capacity_for_growth: false,
        });
        let surface = block_center(soil) + Vec3::Y * CELL_SIZE * 0.5;
        let crop_ray = Transform::from_translation(surface + Vec3::new(-0.4, 0.55, 0.0))
            .looking_to(Vec3::X, Vec3::Y);
        // A horizontal ray hits the decorative plant, even though it hits no voxel soil.
        assert_eq!(pick(&world, &session, &crop_ray, []), Some(target));
        let downward =
            Transform::from_translation(surface + Vec3::Y * 3.0).looking_to(Vec3::NEG_Y, Vec3::Z);
        assert_eq!(pick(&world, &session, &downward, []), Some(target));
        let upward =
            Transform::from_translation(surface + Vec3::Y * 0.55).looking_to(Vec3::Y, Vec3::Z);
        assert_eq!(pick(&world, &session, &upward, []), Some(target));
        let ceiling = BlockPos::new(soil.x, soil.y + 2, soil.z);
        world.set_block(ceiling, Block::Wood).unwrap();
        let beneath_cover =
            Transform::from_translation(surface + Vec3::Y * 0.25).looking_to(Vec3::Y, Vec3::Z);
        assert_eq!(
            pick(&world, &session, &beneath_cover, []),
            Some(InspectTarget::Block(ceiling)),
            "a covered mature plant must disappear from aimed inspection as well as its mesh"
        );
        world.set_block(ceiling, Block::Air).unwrap();
        let below = BlockPos::new(soil.x, soil.y - 1, soil.z);
        assert_eq!(block_target(&world, below), InspectTarget::Block(below));
        world.set_block(soil, Block::Air).unwrap();
        // A ray above the now-empty site and facing up must clear the old selection.
        assert_eq!(pick(&world, &session, &upward, []), None);
        assert_eq!(
            pick(&world, &session, &downward, []),
            Some(InspectTarget::Block(below))
        );
    }
}
