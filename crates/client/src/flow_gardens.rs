//! A bounded local water mechanism. Water and flowers follow confirmed ports;
//! this does not simulate fluids or produce tradable crops.
use crate::{GameEntity, Session, terrain::Geometry};
use bevy::prelude::*;
use rubblekin_core::activities::{ActivityKind, flow_garden::*};
use std::collections::HashMap;

type Part = (Vec3, Vec3, [f32; 4]);
const STONE: [f32; 4] = [0.54, 0.57, 0.50, 1.];
const FLOOR: [f32; 4] = [0.27, 0.30, 0.25, 1.];
const WATER: [f32; 4] = [0.22, 0.73, 0.89, 1.];
const GOLD: [f32; 4] = [0.95, 0.76, 0.30, 1.];
const GREEN: [f32; 4] = [0.20, 0.55, 0.32, 1.];
const SOIL: [f32; 4] = [0.39, 0.25, 0.14, 1.];
fn part(p: [f32; 3], size: [f32; 3], color: [f32; 4]) -> Part {
    (Vec3::from_array(p), Vec3::from_array(size), color)
}
fn water_parts(piece: usize, face: u8) -> Vec<Part> {
    ports(piece, face)
        .unwrap()
        .into_iter()
        .map(|edge| {
            let [x, z] = edge.offset();
            part(
                [x as f32 * TILE / 4., 0.23, z as f32 * TILE / 4.],
                if x == 0 {
                    [0.42, 0.025, TILE / 2.]
                } else {
                    [TILE / 2., 0.025, 0.42]
                },
                WATER,
            )
        })
        .collect()
}
fn channel_parts(piece: usize, face: u8, wet: bool) -> Vec<Part> {
    let mut parts = vec![part(
        [0., 0.06, 0.],
        [TILE * 0.95, 0.12, TILE * 0.95],
        FLOOR,
    )];
    for edge in ports(piece, face).unwrap() {
        let [x, z] = edge.offset();
        let center = [x as f32 * TILE / 4., 0.16, z as f32 * TILE / 4.];
        parts.push(part(
            center,
            if x == 0 {
                [0.56, 0.10, TILE / 2.]
            } else {
                [TILE / 2., 0.10, 0.56]
            },
            STONE,
        ));
        for side in [-1., 1.] {
            parts.push(part(
                [
                    center[0] - z as f32 * side * 0.31,
                    0.34,
                    center[2] + x as f32 * side * 0.31,
                ],
                if x == 0 {
                    [0.07, 0.30, TILE / 2.]
                } else {
                    [TILE / 2., 0.30, 0.07]
                },
                STONE,
            ));
        }
    }
    for n in 0..=piece {
        parts.push(part(
            [TILE * 0.39, 0.56, TILE * 0.30 + n as f32 * 0.08],
            [0.08, 0.08, 0.08],
            GOLD,
        ));
    }
    if wet {
        parts.extend(water_parts(piece, face));
    }
    parts
}
fn source_parts() -> Vec<Part> {
    let mut parts = vec![
        part([-0.38, 0.51, 0.], [0.32, 1.02, 0.68], STONE),
        part([-0.15, 0.63, 0.], [0.45, 0.10, 0.16], GOLD),
        part([0., 0.43, 0.], [0.08, 0.36, 0.08], WATER),
        part([0., 0.1, 0.], [0.9, 0.2, 0.9], STONE),
        part([0., 0.22, 0.], [0.8, 0.025, 0.8], WATER),
        part([TILE / 4., 0.16, 0.], [TILE / 2., 0.1, 0.56], STONE),
        part([TILE / 4., 0.23, 0.], [TILE / 2., 0.025, 0.42], WATER),
    ];
    for z in [-0.46, 0.46] {
        parts.push(part([0., 0.27, z], [1., 0.3, 0.08], STONE));
    }
    for z in [-0.31, 0.31] {
        parts.push(part([TILE / 4., 0.34, z], [TILE / 2., 0.3, 0.07], STONE));
    }
    parts
}
fn bed_parts() -> Vec<Part> {
    let mut parts = vec![part([0., 0.08, 0.], [1.2, 0.16, 1.1], SOIL)];
    for x in [-0.62, 0.62] {
        parts.push(part([x, 0.14, 0.], [0.10, 0.28, 1.2], STONE));
    }
    for z in [-0.57, 0.57] {
        parts.push(part([0., 0.14, z], [1.2, 0.28, 0.10], STONE));
    }
    parts.push(part([-TILE / 4., 0.16, 0.], [TILE / 2., 0.10, 0.56], STONE));
    for z in [-0.31, 0.31] {
        parts.push(part([-TILE / 4., 0.34, z], [TILE / 2., 0.30, 0.07], STONE));
    }
    parts
}
fn flower_parts(wet: bool) -> Vec<Part> {
    let mut parts = Vec::new();
    for (i, x) in [-0.34, 0., 0.34].into_iter().enumerate() {
        for z in [-0.28, 0.28] {
            parts.push(part(
                [x, 0.42, z],
                [0.05, 0.60, 0.05],
                if wet { GREEN } else { SOIL },
            ));
            if wet {
                let color = if i == 1 { [0.87, 0.48, 0.66, 1.] } else { GOLD };
                for [dx, dz] in [[-0.10, 0.], [0.10, 0.], [0., -0.10], [0., 0.10]] {
                    parts.push(part([x + dx, 0.74, z + dz], [0.15, 0.10, 0.15], color));
                }
                parts.push(part([x + 0.08, 0.40, z], [0.2, 0.06, 0.10], GREEN));
            } else {
                parts.push(part([x, 0.72, z], [0.12, 0.14, 0.12], SOIL));
            }
        }
    }
    parts
}
fn mesh(parts: &[Part]) -> Mesh {
    let mut geometry = Geometry::default();
    for (p, size, color) in parts {
        geometry.cuboid(*p, *size, *color);
    }
    geometry.into_mesh()
}
fn top_picture(parts: &[Part]) -> Image {
    let flat: Vec<_> = parts
        .iter()
        .map(|(p, size, color)| {
            (
                Vec3::new(p.x * 0.58, 0.55 - p.z * 0.5, 0.),
                Vec3::new(size.x * 0.58, size.z * 0.5, 0.1),
                *color,
            )
        })
        .collect();
    crate::activities::picture(&flat)
}
#[derive(Resource)]
pub(crate) struct Art {
    channels: [[Handle<Mesh>; 4]; 3],
    water: [[Handle<Mesh>; 4]; 3],
    source: Handle<Mesh>,
    bed: Handle<Mesh>,
    bed_water: Handle<Mesh>,
    flowers: [Handle<Mesh>; 2],
    drip: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    pub(crate) pictures: [[[Handle<Image>; 2]; 4]; 3],
    pub(crate) finished: Handle<Image>,
    objects: HashMap<(u64, u8), Entity>,
    growth: HashMap<u64, f32>,
}
impl Art {
    pub(crate) fn new(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let mut finished = bed_parts();
        finished.extend(
            flower_parts(true)
                .into_iter()
                .map(|(p, s, c)| (p + Vec3::Y * 0.17, s, c)),
        );
        Self {
            channels: std::array::from_fn(|i| {
                std::array::from_fn(|f| meshes.add(mesh(&channel_parts(i, f as u8, false))))
            }),
            water: std::array::from_fn(|i| {
                std::array::from_fn(|f| meshes.add(mesh(&water_parts(i, f as u8))))
            }),
            source: meshes.add(mesh(&source_parts())),
            bed: meshes.add(mesh(&bed_parts())),
            bed_water: meshes.add(mesh(&[part(
                [-TILE / 4., 0.23, 0.],
                [TILE / 2., 0.025, 0.42],
                WATER,
            )])),
            flowers: std::array::from_fn(|i| meshes.add(mesh(&flower_parts(i == 1)))),
            drip: meshes.add(mesh(&[part([0., 0., 0.], [0.05, 0.07, 0.05], WATER)])),
            material: materials.add(StandardMaterial {
                unlit: true,
                perceptual_roughness: 1.,
                ..default()
            }),
            pictures: std::array::from_fn(|i| {
                std::array::from_fn(|f| {
                    std::array::from_fn(|wet| {
                        images.add(top_picture(&channel_parts(i, f as u8, wet == 1)))
                    })
                })
            }),
            finished: images.add(crate::activities::picture(&finished)),
            objects: HashMap::new(),
            growth: HashMap::new(),
        }
    }
}
pub(crate) fn update(
    mut commands: Commands,
    session: Res<Session>,
    mut art: ResMut<Art>,
    time: Res<Time>,
    mut transforms: Query<(&mut Transform, &mut Mesh3d)>,
) {
    art.objects.retain(|_, e| transforms.contains(*e));
    let eye = session
        .observer
        .as_ref()
        .map_or(session.body.position, |camera| camera.position.to_array());
    let mut wanted = Vec::new();
    for a in session.activities.iter().filter(|a| {
        a.plan.kind == ActivityKind::FlowGarden
            && a.plan
                .points()
                .any(|p| rubblekin_core::activities::distance(eye, *p) < 100.)
    }) {
        let (Some(flow), Some(anchors)) = (flow(a.faces), a.plan.clues) else {
            continue;
        };
        let across = Vec3::from_array(a.plan.sockets[2]) - Vec3::from_array(a.plan.sockets[1]);
        let rotation = Quat::from_rotation_y((-across.z).atan2(across.x));
        let goal = if flow.garden_watered { 1. } else { 0.25 };
        let growth = art.growth.entry(a.plan.id).or_insert(goal);
        *growth += (goal - *growth) * (1. - (-time.delta_secs() * 5.).exp());
        let flower_scale = *growth;
        let mut pieces = vec![
            (
                3,
                art.source.clone(),
                Vec3::from_array(anchors[0]),
                Vec3::ONE,
            ),
            (4, art.bed.clone(), Vec3::from_array(anchors[1]), Vec3::ONE),
            (
                5,
                art.flowers[usize::from(flow.garden_watered)].clone(),
                Vec3::from_array(anchors[1]) + Vec3::Y * 0.17,
                Vec3::new(1., flower_scale, 1.),
            ),
        ];
        if flow.garden_watered {
            pieces.push((
                6,
                art.bed_water.clone(),
                Vec3::from_array(anchors[1]),
                Vec3::ONE,
            ));
        }
        for i in 0..3 {
            let position = Vec3::from_array(a.plan.sockets[i]);
            pieces.push((
                i as u8,
                art.channels[i][a.faces[i] as usize].clone(),
                position,
                Vec3::ONE,
            ));
            if flow.wet[i] {
                pieces.push((
                    10 + i as u8,
                    art.water[i][a.faces[i] as usize].clone(),
                    position,
                    Vec3::ONE,
                ));
            }
        }
        if let Some(spill) = flow.spill {
            let (piece, edge) = match spill {
                Spill::Source => (0, Direction::West),
                Spill::Channel { piece, edge } => (piece, edge),
            };
            let [x, z] = edge.offset();
            let stop = Vec3::from_array(a.plan.sockets[piece])
                + rotation * Vec3::new(x as f32 * TILE / 2., 0., z as f32 * TILE / 2.);
            for i in 0..3 {
                let drop = (time.elapsed_secs() * 1.4 + i as f32 / 3.).fract();
                pieces.push((
                    20 + i,
                    art.drip.clone(),
                    stop + Vec3::Y * (0.27 - drop * 0.22),
                    Vec3::ONE,
                ));
            }
        }
        for (index, handle, position, scale) in pieces {
            let key = (a.plan.id, index);
            wanted.push(key);
            let pose = Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(scale);
            if let Some(e) = art.objects.get(&key).copied() {
                if let Ok((mut t, mut mesh)) = transforms.get_mut(e) {
                    *t = pose;
                    mesh.0 = handle;
                }
            } else {
                let entity = commands
                    .spawn((
                        GameEntity,
                        Mesh3d(handle),
                        MeshMaterial3d(art.material.clone()),
                        pose,
                    ))
                    .id();
                art.objects.insert(key, entity);
            }
        }
    }
    art.objects.retain(|key, e| {
        if wanted.contains(key) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    art.growth
        .retain(|id, _| wanted.iter().any(|(active, _)| active == id));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pictures_show_the_actual_open_ends_and_water_and_preserve_equivalent_straights() {
        let dry = |piece, face| {
            top_picture(&channel_parts(piece, face, false))
                .data
                .unwrap()
        };
        for a in 0..4 {
            for b in a + 1..4 {
                assert_ne!(dry(0, a), dry(0, b));
            }
        }
        assert_eq!(dry(2, 0), dry(2, 2));
        assert_eq!(dry(2, 1), dry(2, 3));
        assert_ne!(dry(2, 0), dry(2, 1));
        for piece in 0..3 {
            for face in 0..4 {
                assert_ne!(
                    dry(piece, face),
                    top_picture(&channel_parts(piece, face, true)).data.unwrap()
                );
            }
        }
        assert!(flower_parts(true).len() > flower_parts(false).len());
    }
    #[test]
    fn the_local_garden_scene_has_bounded_objects_and_releases_them_when_leaving() {
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "garden art".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let plan = rubblekin_core::activities::ActivityPlan {
            id: 77,
            recipe_version: 4,
            kind: ActivityKind::FlowGarden,
            site_id: Some(77),
            objects: [[0., 0., 0.], [0., 0., -TILE], [TILE, 0., -TILE]],
            sockets: [[0., 0., 0.], [0., 0., -TILE], [TILE, 0., -TILE]],
            clues: Some([[-TILE, 0., 0.], [2. * TILE, 0., -TILE], [0., 0., TILE]]),
            answer: SOLVED_FACES,
        };
        session.body.position = plan.sockets[0];
        session.activities = vec![rubblekin_core::activities::ActivitySnapshot {
            plan,
            revision: 0,
            props: [rubblekin_core::activities::PropState::Home; 3],
            faces: START_FACES,
            complete: false,
            available: true,
            repair: None,
        }];
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let art = Art::new(&mut meshes, &mut images, &mut materials);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(session)
            .insert_resource(art)
            .add_systems(Update, update);
        app.update();
        let count = app.world().resource::<Art>().objects.len();
        assert_eq!(count, 9);
        app.world_mut().resource_mut::<Session>().activities[0].faces = SOLVED_FACES;
        app.update();
        assert_eq!(app.world().resource::<Art>().objects.len(), 10);
        for _ in 0..3 {
            app.update();
            assert_eq!(app.world().resource::<Art>().objects.len(), 10);
        }
        app.world_mut().resource_mut::<Session>().body.position[0] += 1000.;
        app.update();
        assert!(app.world().resource::<Art>().objects.is_empty());
        assert!(app.world().resource::<Art>().growth.is_empty());
        assert_eq!(
            app.world_mut()
                .query::<&GameEntity>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
