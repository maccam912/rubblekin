//! Temporary local work props follow confirmed activity, not inventory or rewards.
use bevy::{light::NotShadowCaster, prelude::*};
use rubblekin_core::economy::WorkKind;

use crate::{Avatars, Limb, Session, market::MarketPanel, terrain::Geometry};

#[derive(Component)]
pub(crate) struct WorkTool;

#[derive(Default)]
pub(crate) struct CachedTools {
    assets: Option<([Handle<Mesh>; 3], Handle<StandardMaterial>)>,
}

fn tool_index(kind: WorkKind) -> usize {
    match kind {
        WorkKind::TendField => 0,
        WorkKind::WorkshopMaintenance | WorkKind::QuarryStone => 1,
        WorkKind::HarvestField => 2,
    }
}

fn mesh(index: usize) -> Mesh {
    let mut mesh = Geometry::default();
    let wood = [0.57, 0.37, 0.18, 1.];
    let metal = [0.52, 0.56, 0.55, 1.];
    let straw = [0.70, 0.53, 0.28, 1.];
    let mut part =
        |at, size, color| mesh.cuboid(Vec3::from_array(at), Vec3::from_array(size), color);
    match index {
        0 => {
            part([0., -0.34, 0.], [0.045, 0.82, 0.045], wood);
            part([0., -0.73, -0.035], [0.31, 0.07, 0.14], metal);
        }
        1 => {
            part([0., -0.12, 0.], [0.052, 0.38, 0.052], wood);
            part([0., -0.30, 0.], [0.25, 0.13, 0.12], metal);
        }
        _ => {
            // Open, empty basket: prospective harvesting never invents goods.
            part([0., -0.28, 0.], [0.32, 0.035, 0.24], straw);
            for x in [-0.145, 0.145] {
                part([x, -0.19, 0.], [0.03, 0.18, 0.24], straw);
                part([x, -0.04, 0.], [0.022, 0.14, 0.024], wood);
            }
            for z in [-0.105, 0.105] {
                part([0., -0.19, z], [0.26, 0.18, 0.03], straw);
            }
            part([0., 0.035, 0.], [0.31, 0.024, 0.024], wood);
        }
    }
    mesh.into_mesh()
}

/// The arm's existing mesh uses a non-unit scale. Cancel it for meter-sized
/// tools, while placing the grip at the arm's lower end in its local coordinates.
fn grip(arm: &Transform) -> Transform {
    Transform::from_xyz(0., -0.5, 0.).with_scale(arm.scale.recip())
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    mut commands: Commands,
    session: Option<Res<Session>>,
    panel: Res<MarketPanel>,
    avatars: Res<Avatars>,
    limbs: Query<(Entity, &Limb, &ChildOf, &Transform), Without<WorkTool>>,
    mut tools: Query<(Entity, &mut Mesh3d, &mut Visibility, &ChildOf), With<WorkTool>>,
    mut cached: Local<CachedTools>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (_, _, mut visibility, _) in &mut tools {
        *visibility = Visibility::Hidden;
    }
    let Some(session) = session else { return };
    let Some(work) = panel
        .active_work()
        .filter(|work| crate::work_animation::can_pose(&session, work))
    else {
        return;
    };
    let Some(avatar) = avatars.players.get(&session.id) else {
        return;
    };
    let Some((arm, _, _, transform)) = limbs.iter().find(|(_, limb, parent, transform)| {
        limb.arm && parent.parent() == *avatar && transform.translation.x > 0.
    }) else {
        return;
    };
    let (handles, material) = cached.assets.get_or_insert_with(|| {
        (
            std::array::from_fn(|index| meshes.add(mesh(index))),
            materials.add(StandardMaterial {
                perceptual_roughness: 0.9,
                ..default()
            }),
        )
    });
    let handle = handles[tool_index(work.offer.site.kind)].clone();
    if let Some((entity, mut mesh, mut visibility, parent)) = tools.iter_mut().next() {
        if mesh.0 != handle {
            mesh.0 = handle;
        }
        *visibility = Visibility::Inherited;
        if parent.parent() != arm {
            commands
                .entity(entity)
                .insert((ChildOf(arm), grip(transform)));
        }
    } else {
        commands.spawn((
            WorkTool,
            ChildOf(arm),
            Mesh3d(handle),
            MeshMaterial3d(material.clone()),
            grip(transform),
            Visibility::Inherited,
            NotShadowCaster,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::economy::{
        PlayerEconomy, WorkOffer, WorkProgress, WorkReward, WorkSite, WorkState,
    };
    use rubblekin_core::protocol::SessionMode;

    fn scene() -> (App, WorkProgress, Entity, Entity) {
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "tools".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.flying = false;
        session.body.on_ground = true;
        session.body.velocity = [0.; 3];
        let work = WorkProgress {
            offer: WorkOffer {
                site: WorkSite {
                    village_id: 0,
                    kind: WorkKind::TendField,
                    index: 0,
                },
                position: session.body.position,
                label: "Field work".into(),
                reward: WorkReward::Coins(2),
                duration_seconds: 6.,
                unavailable_reason: None,
            },
            elapsed_seconds: 1.,
        };
        let id = session.id;
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<MarketPanel>()
            .init_resource::<Avatars>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_plugins(bevy::transform::TransformPlugin)
            .add_systems(Update, update);
        let local = app
            .world_mut()
            .spawn((Transform::from_xyz(4., 2., 1.), Visibility::Inherited))
            .id();
        let remote = app
            .world_mut()
            .spawn((Transform::default(), Visibility::Inherited))
            .id();
        // Actual avatar arm dimensions; remote and left arms must never gain tools.
        let mut right = Entity::PLACEHOLDER;
        for avatar in [local, remote] {
            for x in [-0.31, 0.31] {
                let arm = app
                    .world_mut()
                    .spawn((
                        Limb {
                            arm: true,
                            phase: 0.,
                        },
                        ChildOf(avatar),
                        Visibility::Inherited,
                        Transform::from_xyz(x, 1., 0.).with_scale(Vec3::new(0.16, 0.52, 0.20)),
                    ))
                    .id();
                if avatar == local && x > 0. {
                    right = arm;
                }
            }
        }
        app.world_mut()
            .resource_mut::<Avatars>()
            .players
            .extend([(id, local), (id + 1, remote)]);
        (app, work, right, local)
    }

    fn confirm(app: &mut App, work: Option<WorkProgress>) {
        app.world_mut().resource_mut::<MarketPanel>().work_reply(
            0,
            WorkState {
                offer: work.as_ref().map(|work| work.offer.clone()),
                active: work,
            },
            PlayerEconomy::default(),
            String::new(),
            true,
        );
    }

    #[test]
    fn confirmed_local_tools_follow_the_hand_reuse_assets_and_hide_after_work_or_leave() {
        let (mut app, mut work, right, avatar) = scene();
        app.world_mut().resource_mut::<MarketPanel>().work_reply(
            0,
            WorkState {
                offer: Some(work.offer.clone()),
                active: None,
            },
            PlayerEconomy::default(),
            String::new(),
            true,
        );
        app.update();
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(
            app.world_mut()
                .query::<&WorkTool>()
                .iter(app.world())
                .next()
                .is_none()
        );
        confirm(&mut app, Some(work.clone()));
        app.update();
        let tool = app
            .world_mut()
            .query_filtered::<Entity, With<WorkTool>>()
            .single(app.world())
            .unwrap();
        assert_eq!(app.world().get::<ChildOf>(tool).unwrap().parent(), right);
        let initial = *app.world().get::<GlobalTransform>(tool).unwrap();
        let (scale, _, position) = initial.to_scale_rotation_translation();
        assert!(scale.abs_diff_eq(Vec3::ONE, 0.0001));
        assert!(position.abs_diff_eq(Vec3::new(4.31, 2.74, 1.), 0.0001));
        app.world_mut()
            .get_mut::<Transform>(right)
            .unwrap()
            .rotation = Quat::from_rotation_x(0.8);
        app.update();
        assert_ne!(
            app.world()
                .get::<GlobalTransform>(tool)
                .unwrap()
                .translation(),
            position
        );
        let mut distinct = std::collections::HashSet::new();
        for kind in [
            WorkKind::TendField,
            WorkKind::WorkshopMaintenance,
            WorkKind::QuarryStone,
            WorkKind::HarvestField,
        ] {
            work.offer.site.kind = kind;
            confirm(&mut app, Some(work.clone()));
            for _ in 0..20 {
                app.update();
            }
            assert_eq!(
                app.world().get::<Visibility>(tool),
                Some(&Visibility::Inherited)
            );
            distinct.insert(app.world().get::<Mesh3d>(tool).unwrap().0.id());
            assert_eq!(
                app.world_mut()
                    .query::<&WorkTool>()
                    .iter(app.world())
                    .count(),
                1
            );
        }
        assert_eq!(distinct.len(), 3);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 3);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 1);
        // The same gate as the arm pose rejects movement before a cancellation reply.
        app.world_mut().resource_mut::<Session>().body.velocity[0] = 0.2;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(tool),
            Some(&Visibility::Hidden)
        );
        app.world_mut().resource_mut::<Session>().body.velocity = [0.; 3];
        confirm(&mut app, None);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(tool),
            Some(&Visibility::Hidden)
        );
        confirm(&mut app, Some(work));
        app.world_mut().remove_resource::<Session>();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(tool),
            Some(&Visibility::Hidden)
        );
        app.world_mut().entity_mut(avatar).despawn();
        assert!(app.world().get_entity(tool).is_err());
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 3);
    }
}
