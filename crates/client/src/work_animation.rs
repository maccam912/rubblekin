//! Local work poses use private, confirmed activity; they never move the body.
use bevy::prelude::*;
use rubblekin_core::economy::{WORK_REACH, WorkKind, WorkProgress};

use crate::{Avatar, Avatars, Limb, Session, market::MarketPanel};

fn can_pose(session: &Session, work: &WorkProgress) -> bool {
    session.observer.is_none()
        && !session.flying
        && session.ride.is_none()
        && session.body.on_ground
        && session
            .body
            .velocity
            .iter()
            .all(|speed| speed.is_finite() && speed.abs() < 0.05)
        && Vec2::new(
            session.body.position[0] - work.offer.position[0],
            session.body.position[2] - work.offer.position[2],
        )
        .length_squared()
            <= WORK_REACH.powi(2)
        && (session.body.position[1] - work.offer.position[1]).abs() <= 1.5
}

fn arm_angle(kind: WorkKind, phase: f32, limb_phase: f32) -> f32 {
    match kind {
        WorkKind::TendField => 0.85 + phase.sin() * 0.38,
        WorkKind::HarvestField => 1.1 + phase.sin() * 0.4,
        WorkKind::WorkshopMaintenance | WorkKind::QuarryStone => {
            0.65 + (phase + limb_phase).sin() * 0.45
        }
    }
}

/// Runs after the ordinary avatar pose, which restores neutral/walking arms on
/// every frame. Removing confirmed work or requesting cancellation needs no
/// separate cleanup, and other players/NPCs retain their own existing poses.
#[allow(clippy::type_complexity)]
pub(crate) fn animate(
    session: Res<Session>,
    market: Res<MarketPanel>,
    avatars: Res<Avatars>,
    time: Res<Time>,
    mut bodies: Query<&mut Transform, With<Avatar>>,
    mut limbs: Query<(&mut Transform, &Limb, &ChildOf), Without<Avatar>>,
) {
    let Some(work) = market.active_work().filter(|work| can_pose(&session, work)) else {
        return;
    };
    let Some(entity) = avatars.players.get(&session.id) else {
        return;
    };
    if let Ok(mut transform) = bodies.get_mut(*entity) {
        let delta = Vec3::from_array(work.offer.position) - Vec3::from_array(session.body.position);
        if delta.x * delta.x + delta.z * delta.z > 0.03 {
            transform.rotation = Quat::from_rotation_y((-delta.x).atan2(-delta.z));
        }
    }
    for (mut transform, limb, parent) in &mut limbs {
        if limb.arm && parent.parent() == *entity {
            // A continuous visual cycle, independent of reward/progress timing.
            transform.rotation = Quat::from_rotation_x(arm_angle(
                work.offer.site.kind,
                time.elapsed_secs() * 4.,
                limb.phase,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VoxelWorld, graphics::GraphicsQuality};
    use rubblekin_core::{
        economy::{PlayerEconomy, WorkOffer, WorkReward, WorkSite, WorkState},
        protocol::SessionMode,
    };

    fn scene() -> (App, WorkProgress) {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "test".into(),
            GraphicsQuality::default(),
            0.,
            SessionMode::Player,
        )
        .unwrap();
        session.body.on_ground = true;
        session.body.velocity = [0.; 3];
        session.flying = false;
        let mut other = session.players[0].clone();
        other.id += 100;
        other.body.position[0] += 8.;
        session.players.push(other);
        let mut position = session.body.position;
        position[0] += 0.5;
        let work = WorkProgress {
            offer: WorkOffer {
                site: WorkSite {
                    village_id: 1,
                    kind: WorkKind::TendField,
                    index: 0,
                },
                position,
                label: "Field work".into(),
                reward: WorkReward::Coins(2),
                duration_seconds: 6.,
                unavailable_reason: None,
            },
            elapsed_seconds: 1.,
        };
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(VoxelWorld(world))
            .init_resource::<Avatars>()
            .init_resource::<MarketPanel>()
            .init_resource::<Time>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, (crate::update_avatars, animate).chain());
        app.update();
        app.update();
        (app, work)
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

    fn arm_rotations(app: &mut App, id: u64) -> Vec<Quat> {
        let entity = app.world().resource::<Avatars>().players[&id];
        app.world_mut()
            .query::<(&Transform, &Limb, &ChildOf)>()
            .iter(app.world())
            .filter(|(_, limb, parent)| limb.arm && parent.parent() == entity)
            .map(|(transform, _, _)| transform.rotation)
            .collect()
    }

    #[test]
    fn confirmed_local_work_animates_only_own_arms_and_leaves_physics_untouched() {
        let (mut app, mut work) = scene();
        let id = app.world().resource::<Session>().id;
        let position = app.world().resource::<Session>().body.position;
        assert!(
            arm_rotations(&mut app, id)
                .iter()
                .all(|rotation| *rotation == Quat::IDENTITY)
        );
        confirm(&mut app, Some(work.clone()));
        app.update();
        let first = arm_rotations(&mut app, id);
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|rotation| *rotation != Quat::IDENTITY));
        assert!(
            arm_rotations(&mut app, id + 100)
                .iter()
                .all(|rotation| *rotation == Quat::IDENTITY)
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert_ne!(arm_rotations(&mut app, id), first);
        assert_eq!(app.world().resource::<Session>().body.position, position);
        let avatar = app.world().resource::<Avatars>().players[&id];
        assert_eq!(
            app.world()
                .get::<Transform>(avatar)
                .unwrap()
                .translation
                .to_array(),
            position
        );
        assert!(
            app.world_mut()
                .query::<(&Transform, &Limb, &ChildOf)>()
                .iter(app.world())
                .filter(|(_, limb, parent)| !limb.arm && parent.parent() == avatar)
                .all(|(transform, _, _)| transform.rotation == Quat::IDENTITY)
        );
        assert_eq!(
            app.world()
                .resource::<MarketPanel>()
                .ledger
                .as_ref()
                .unwrap()
                .coins,
            0
        );
        let tending = arm_rotations(&mut app, id);
        work.offer.site.kind = WorkKind::HarvestField;
        confirm(&mut app, Some(work.clone()));
        app.update();
        assert_ne!(
            arm_rotations(&mut app, id),
            tending,
            "Gathering uses its own confirmed pose"
        );
        work.offer.site.kind = WorkKind::QuarryStone;
        confirm(&mut app, Some(work));
        app.update();
        assert!(
            arm_rotations(&mut app, id)
                .iter()
                .all(|rotation| *rotation != Quat::IDENTITY)
        );
        confirm(&mut app, None);
        app.update();
        assert!(
            arm_rotations(&mut app, id)
                .iter()
                .all(|rotation| *rotation == Quat::IDENTITY)
        );
    }

    #[test]
    fn motion_and_lost_support_stop_the_pose_and_leaving_clears_it() {
        let (mut app, mut work) = scene();
        work.offer.site.kind = WorkKind::WorkshopMaintenance;
        let id = app.world().resource::<Session>().id;
        confirm(&mut app, Some(work.clone()));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(200));
        app.update();
        let arms = arm_rotations(&mut app, id);
        assert_ne!(arms[0], arms[1], "Workshop hands alternate");
        for condition in 0..4 {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.velocity = [0.; 3];
            session.body.on_ground = true;
            session.flying = false;
            match condition {
                0 => session.body.velocity[0] = 1.,
                1 => session.body.on_ground = false,
                2 => session.flying = true,
                _ => session.body.position[0] += 20.,
            }
            assert!(!can_pose(&session, &work));
            app.update();
            app.update();
            assert!(
                arm_rotations(&mut app, id)
                    .iter()
                    .all(|rotation| *rotation == Quat::IDENTITY),
                "condition {condition}"
            );
        }
        app.world_mut().resource_mut::<MarketPanel>().clear();
        assert!(
            app.world()
                .resource::<MarketPanel>()
                .active_work()
                .is_none()
        );
        assert!(app.world().resource::<MarketPanel>().ledger.is_none());
    }
}
