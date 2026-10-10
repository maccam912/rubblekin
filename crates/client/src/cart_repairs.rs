//! A bounded repair scene. These props illustrate shared progress; they are
//! neither cargo items nor simulated residents or vehicles.
use crate::{GameEntity, Session, terrain::Geometry};
use bevy::prelude::*;
use rubblekin_core::activities::{ActivityKind, PropState};
use std::collections::HashMap;

type Part = (Vec3, Vec3, [f32; 4]);
const WOOD: [f32; 4] = [0.47, 0.29, 0.13, 1.];
const GOLD: [f32; 4] = [0.95, 0.76, 0.30, 1.];
const GREEN: [f32; 4] = [0.20, 0.45, 0.30, 1.];
fn part(p: [f32; 3], s: [f32; 3], c: [f32; 4]) -> Part {
    (Vec3::from_array(p), Vec3::from_array(s), c)
}
fn wheel_parts() -> Vec<Part> {
    let mut parts = Vec::new();
    for n in 0..16 {
        let a = n as f32 * std::f32::consts::TAU / 16.;
        parts.push(part(
            [a.cos() * 0.30, 0.34 + a.sin() * 0.30, 0.],
            [0.13, 0.13, 0.12],
            WOOD,
        ));
    }
    parts.push(part([0., 0.34, 0.], [0.08, 0.52, 0.10], GOLD));
    parts.push(part([0., 0.34, 0.], [0.52, 0.08, 0.10], GOLD));
    parts
}
fn plank_parts() -> Vec<Part> {
    vec![part([0., 0.25, 0.], [0.86, 0.15, 0.22], WOOD)]
}
fn body_parts(plank: bool) -> Vec<Part> {
    let mut parts = vec![
        part([0., 0.34, 0.], [1.38, 0.10, 0.10], GOLD),
        part([0., 0.48, -0.40], [1.10, 0.12, 0.18], WOOD),
        part([0., 0.48, 0.40], [1.10, 0.12, 0.18], WOOD),
        part([0., 0.72, -0.50], [1.10, 0.42, 0.09], WOOD),
    ];
    for x in [-0.49, 0.49] {
        parts.push(part([x, 0.72, 0.], [0.12, 0.42, 1.10], WOOD));
        parts.push(part([x, 0.46, 0.92], [0.07, 0.07, 1.], WOOD));
    }
    if plank {
        parts.push(part([0., 0.48, 0.], [0.88, 0.12, 0.62], WOOD));
    }
    parts
}
fn owner_parts() -> Vec<Part> {
    vec![
        part([-0.12, 0.27, 0.], [0.15, 0.54, 0.18], WOOD),
        part([0.12, 0.27, 0.], [0.15, 0.54, 0.18], WOOD),
        part([0., 0.81, 0.], [0.46, 0.54, 0.28], GREEN),
        part([0., 1.26, 0.], [0.32, 0.36, 0.30], GOLD),
        part([0., 1.48, 0.], [0.48, 0.07, 0.40], GREEN),
    ]
}
fn mesh(parts: &[Part]) -> Mesh {
    let mut geometry = Geometry::default();
    for (p, size, color) in parts {
        geometry.cuboid(*p, *size, *color);
    }
    geometry.into_mesh()
}

#[derive(Resource)]
pub(crate) struct Art {
    bodies: [Handle<Mesh>; 2],
    wheel: Handle<Mesh>,
    owner: Handle<Mesh>,
    arm: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    pub(crate) pictures: [Handle<Image>; 3],
    pub(crate) finished: Handle<Image>,
    objects: HashMap<(u64, u8), Entity>,
}
impl Art {
    pub(crate) fn new(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let mut finished = body_parts(true);
        // The same cart parts, scaled to fit the existing picture frame.
        finished.extend(
            wheel_parts()
                .into_iter()
                .map(|(p, s, c)| (p + Vec3::new(-0.3, 0., -0.1), s, c)),
        );
        for (p, s, _) in &mut finished {
            *p *= 0.65;
            *s *= 0.65;
        }
        Self {
            bodies: std::array::from_fn(|i| meshes.add(mesh(&body_parts(i == 1)))),
            wheel: meshes.add(mesh(
                &wheel_parts()
                    .into_iter()
                    .map(|(p, s, c)| (p - Vec3::Y * 0.34, s, c))
                    .collect::<Vec<_>>(),
            )),
            owner: meshes.add(mesh(&owner_parts())),
            arm: meshes.add(mesh(&[part([0., -0.20, 0.], [0.14, 0.44, 0.16], GREEN)])),
            material: materials.add(StandardMaterial {
                perceptual_roughness: 1.,
                unlit: true,
                ..default()
            }),
            pictures: [
                images.add(crate::activities::picture(&wheel_parts())),
                images.add(crate::activities::picture(&plank_parts())),
                images.add(crate::work_tools::picture(1)),
            ],
            finished: images.add(crate::activities::picture(&finished)),
            objects: HashMap::new(),
        }
    }
}

pub(crate) fn pose(session: &Session) -> Option<[f32; 3]> {
    session
        .activities
        .iter()
        .find(|a| {
            a.plan.kind == ActivityKind::CartRepair
                && !a.complete
                && a.available
                && a.repair.is_some_and(|w| w.player_id == session.id)
        })
        .map(|a| a.plan.sockets[2])
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
        a.plan.kind == ActivityKind::CartRepair
            && a.plan
                .points()
                .any(|p| rubblekin_core::activities::distance(eye, *p) < 100.)
    }) {
        let center = Vec3::from_array(a.plan.sockets[2]);
        let across = Vec3::from_array(a.plan.sockets[1]) - Vec3::from_array(a.plan.sockets[0]);
        let rotation =
            Quat::from_rotation_y(across.x.atan2(across.z) - std::f32::consts::FRAC_PI_2);
        let wheel = a.props[0] == PropState::Placed;
        let plank = a.props[1] == PropState::Placed;
        let tilt = if wheel {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_z(-0.20)
        };
        let mut pieces = vec![
            (0, art.bodies[usize::from(plank)].clone(), Vec3::ZERO, tilt),
            (
                1,
                art.wheel.clone(),
                Vec3::new(0.66, 0.34, 0.),
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ),
            (
                3,
                art.owner.clone(),
                Vec3::new(0., 0., -1.25),
                Quat::IDENTITY,
            ),
        ];
        if wheel {
            // A repeated, in-place wheel test shows the repaired connection.
            let spin = if a.complete {
                (time.elapsed_secs() * 1.5).sin() * 0.6
            } else {
                0.
            };
            pieces.push((
                2,
                art.wheel.clone(),
                Vec3::new(-0.66, 0.34, 0.),
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_z(spin),
            ));
        }
        let wave = if a.complete {
            -2.3 + (time.elapsed_secs() * 3.).sin() * 0.3
        } else {
            -0.6
        };
        pieces.push((
            4,
            art.arm.clone(),
            Vec3::new(0.30, 1.05, -1.25),
            Quat::from_rotation_z(wave),
        ));
        for (index, handle, offset, local_rotation) in pieces {
            let key = (a.plan.id, index);
            wanted.push(key);
            let transform = Transform::from_translation(center + rotation * offset)
                .with_rotation(rotation * local_rotation);
            if let Some(e) = art.objects.get(&key).copied() {
                if let Ok((mut t, mut m)) = transforms.get_mut(e) {
                    *t = transform;
                    m.0 = handle;
                }
            } else {
                let e = commands
                    .spawn((
                        GameEntity,
                        Mesh3d(handle),
                        MeshMaterial3d(art.material.clone()),
                        transform,
                    ))
                    .id();
                art.objects.insert(key, e);
            }
        }
    }
    art.objects.retain(|k, e| {
        if wanted.contains(k) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
}
