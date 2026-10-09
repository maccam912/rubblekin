//! Finite shared activities: prop ownership, one-time contributions and saves.
use crate::{
    Connection, ServerConfig, checkpoint_players,
    persistence::Simulation,
    player_economy::{MAX_COINS, Profiles},
};
use rubblekin_core::{
    activities::*,
    protocol::ServerMessage,
    world::{CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Slot {
    Home,
    Held(String),
    Placed(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    plan: ActivityPlan,
    revision: u64,
    slots: [Slot; 3],
    faces: [u8; 3],
    complete: bool,
}
impl Record {
    fn new(plan: ActivityPlan) -> Self {
        let faces = [
            plan.answer[0],
            (plan.answer[1] + 1) % 3,
            (plan.answer[2] + 2) % 3,
        ];
        Self {
            plan,
            revision: 0,
            slots: std::array::from_fn(|_| Slot::Home),
            faces,
            complete: false,
        }
    }
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct Activities {
    records: Vec<Record>,
}
impl Activities {
    pub fn new(world: &World) -> Self {
        Self {
            records: plans(world).into_iter().map(Record::new).collect(),
        }
    }
    /// Add this content to existing Save9 worlds without moving saved scenes,
    /// losing receipts, or replacing a damaged scene with a new payable copy.
    pub fn add_poi_plans(&mut self, world: &World) {
        let Some(settlements) = world.settlements() else {
            return;
        };
        for plan in poi_plans(world) {
            let arrangement = settlements
                .composed_sites
                .iter()
                .find(|s| Some(s.id) == plan.site_id)
                .map(|s| s.arrangement);
            let present = self.records.iter().any(|r| {
                r.plan.id == plan.id
                    || r.plan.site_id.and_then(|id| {
                        settlements
                            .composed_sites
                            .iter()
                            .find(|s| s.id == id)
                            .map(|s| s.arrangement)
                    }) == arrangement
            });
            if !present && self.records.len() < MAX_PLANS {
                self.records.push(Record::new(plan));
            }
        }
    }
    pub fn validate(&self, world: &World, profiles: &Profiles) -> bool {
        let mut ids = BTreeSet::new();
        let mut holders = BTreeSet::new();
        let mut sites = BTreeSet::new();
        self.records.len() <= MAX_PLANS
            && self.records.iter().all(|r| {
                let p = &r.plan;
                let radius = world.radius_cells() as f32 * CELL_SIZE;
                ids.insert(p.id)
                    && match p.site_id {
                        None => {
                            p.id == if p.kind == ActivityKind::SpilledSupplies {
                                1
                            } else {
                                2
                            }
                        }
                        Some(id) => {
                            sites.insert(id)
                                && id > 2
                                && p.id == id
                                && world.settlements().is_some_and(|s| {
                                    s.composed_sites.iter().any(|site| {
                                        site.id == id
                                            && site_kind(site.arrangement) == Some(p.kind)
                                            && p.objects.iter().chain(&p.sockets).all(|v| {
                                                v[0] >= site.bounds[0] as f32 * CELL_SIZE
                                                    && v[0] < site.bounds[2] as f32 * CELL_SIZE
                                                    && v[2] >= site.bounds[1] as f32 * CELL_SIZE
                                                    && v[2] < site.bounds[3] as f32 * CELL_SIZE
                                            })
                                    })
                                })
                        }
                    }
                    && p.recipe_version == RECIPE_VERSION
                    && r.revision < u64::MAX
                    && p.objects.iter().chain(p.sockets.iter()).all(|v| {
                        v.iter().all(|n| n.is_finite())
                            && v[0].abs() <= radius
                            && v[2].abs() <= radius
                            && v[1] >= world.min_y() as f32 * CELL_SIZE
                            && v[1] <= world.max_y() as f32 * CELL_SIZE
                    })
                    && p.answer.iter().chain(r.faces.iter()).all(|f| *f < 3)
                    && r.slots.iter().all(|s| match s {
                        Slot::Home => true,
                        Slot::Held(id) => profiles.contains_key(id) && holders.insert(id.clone()),
                        Slot::Placed(id) => profiles.contains_key(id),
                    })
                    && match p.kind {
                        ActivityKind::SpilledSupplies => {
                            r.complete == r.slots.iter().all(|s| matches!(s, Slot::Placed(_)))
                        }
                        ActivityKind::ShapeStones => {
                            r.slots.iter().all(|s| *s == Slot::Home)
                                && r.complete == (r.faces == p.answer)
                        }
                    }
            })
    }
    pub fn recover(&mut self) {
        for r in &mut self.records {
            let mut changed = false;
            for s in &mut r.slots {
                if matches!(s, Slot::Held(_)) {
                    *s = Slot::Home;
                    changed = true;
                }
            }
            if changed {
                r.revision += 1;
            }
        }
    }
    fn release(&mut self, profile: &str) -> bool {
        let mut changed = false;
        for r in &mut self.records {
            let mut local = false;
            for s in &mut r.slots {
                if matches!(s,Slot::Held(id) if id==profile) {
                    *s = Slot::Home;
                    local = true;
                }
            }
            if local {
                r.revision += 1;
                changed = true;
            }
        }
        changed
    }
    pub fn snapshots(
        &self,
        world: &World,
        connections: &BTreeMap<u64, Connection>,
    ) -> Vec<ActivitySnapshot> {
        self.records
            .iter()
            .map(|r| ActivitySnapshot {
                plan: r.plan.clone(),
                revision: r.revision,
                faces: r.faces,
                complete: r.complete,
                available: r
                    .plan
                    .objects
                    .iter()
                    .chain(r.plan.sockets.iter())
                    .all(|p| supported(world, *p)),
                props: std::array::from_fn(|i| match &r.slots[i] {
                    Slot::Home => PropState::Home,
                    Slot::Placed(_) => PropState::Placed,
                    Slot::Held(profile) => connections
                        .iter()
                        .find(|(_, c)| c.profile_id.as_ref() == Some(profile))
                        .map_or(PropState::Home, |(id, _)| PropState::Held(*id)),
                }),
            })
            .collect()
    }
    fn apply(
        &mut self,
        world: &World,
        profile: &str,
        position: [f32; 3],
        id: u64,
        revision: u64,
        action: ActivityAction,
    ) -> Result<u64, String> {
        let holding = self.records.iter().any(|r| {
            r.slots
                .iter()
                .any(|s| matches!(s,Slot::Held(h) if h==profile))
        });
        let r = self
            .records
            .iter_mut()
            .find(|r| r.plan.id == id)
            .ok_or("That activity is unavailable.")?;
        if revision != r.revision {
            return Err("Someone changed this activity. Try again.".into());
        }
        if r.complete {
            return Err("This activity is already finished.".into());
        }
        if action == ActivityAction::Return {
            if !r
                .slots
                .iter()
                .any(|s| matches!(s,Slot::Held(h) if h==profile))
            {
                return Err("You are not carrying one of these supplies.".into());
            }
            for s in &mut r.slots {
                if matches!(s,Slot::Held(h) if h==profile) {
                    *s = Slot::Home;
                }
            }
            r.revision += 1;
            return Ok(0);
        }
        if !r
            .plan
            .objects
            .iter()
            .chain(r.plan.sockets.iter())
            .all(|p| supported(world, *p))
        {
            return Err("The activity's ground is blocked or missing. Return the supply to its starting place.".into());
        }
        let (i, target) = match action {
            ActivityAction::Take(i) if r.plan.kind == ActivityKind::SpilledSupplies => {
                (i as usize, r.plan.objects.get(i as usize))
            }
            ActivityAction::Place(i) if r.plan.kind == ActivityKind::SpilledSupplies => {
                (i as usize, r.plan.sockets.get(i as usize))
            }
            ActivityAction::Turn(i) if r.plan.kind == ActivityKind::ShapeStones => {
                (i as usize, r.plan.sockets.get(i as usize))
            }
            _ => return Err("That action does not fit this activity.".into()),
        };
        let target = target.ok_or("That piece does not exist.")?;
        if !can_interact(world, position, *target) {
            return Err("Move closer with a clear view of the piece.".into());
        }
        let mut reward = 0;
        match action {
            ActivityAction::Take(_) => {
                if holding {
                    return Err("Place or return the supply you are already carrying.".into());
                }
                if r.slots[i] != Slot::Home {
                    return Err("That supply is already being carried or placed.".into());
                }
                r.slots[i] = Slot::Held(profile.into());
            }
            ActivityAction::Place(_) => {
                let held = r
                    .slots
                    .iter()
                    .position(|s| matches!(s,Slot::Held(h) if h==profile))
                    .ok_or("Pick up a supply first.")?;
                if held != i {
                    return Err(
                        "That tray needs a different shape. Your supply stays in your hands."
                            .into(),
                    );
                }
                r.slots[i] = Slot::Placed(profile.into());
                reward = 2;
                r.complete = r.slots.iter().all(|s| matches!(s, Slot::Placed(_)));
            }
            ActivityAction::Turn(_) => {
                r.faces[i] = (r.faces[i] + 1) % 3;
                r.complete = r.faces == r.plan.answer;
            }
            ActivityAction::Return => unreachable!(),
        }
        r.revision += 1;
        Ok(reward)
    }
}
pub(crate) fn send(
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    id: u64,
    request_id: u64,
    notice: String,
    accepted: bool,
) {
    let activities = sim.activities.snapshots(&sim.world, connections);
    connections
        .get_mut(&id)
        .unwrap()
        .send(&ServerMessage::ActivityState {
            request_id,
            activities,
            notice,
            accepted,
        });
}
pub(crate) fn broadcast(connections: &mut BTreeMap<u64, Connection>, sim: &Simulation) {
    let activities = sim.activities.snapshots(&sim.world, connections);
    for c in connections
        .values_mut()
        .filter(|c| c.mode.is_some() && !c.dead)
    {
        c.send(&ServerMessage::ActivityState {
            request_id: 0,
            activities: activities.clone(),
            notice: String::new(),
            accepted: true,
        });
    }
}
pub(crate) fn release_disconnected(
    connections: &BTreeMap<u64, Connection>,
    sim: &mut Simulation,
) -> bool {
    let mut changed = false;
    for c in connections.values().filter(|c| c.dead) {
        if let Some(p) = &c.profile_id {
            changed |= sim.activities.release(p);
        }
    }
    changed
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(
    id: u64,
    request_id: u64,
    activity_id: u64,
    revision: u64,
    action: ActivityAction,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
) -> io::Result<()> {
    let result = (|| -> Result<u64, String> {
        let c = connections.get_mut(&id).unwrap();
        if request_id == 0 || c.last_activity_id.is_some_and(|last| request_id <= last) {
            return Err("This activity request was already handled.".into());
        }
        c.last_activity_id = Some(request_id);
        let now = Instant::now();
        if c.last_activity_request
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(150))
        {
            return Err("Try again in a moment.".into());
        }
        c.last_activity_request = Some(now);
        let profile = c
            .profile_id
            .as_ref()
            .ok_or("Reconnect with your saved guest profile to do activities.")?;
        let player = c.player.as_ref().ok_or("Only players can do activities.")?;
        let ledger = &sim
            .profiles
            .get(profile)
            .ok_or("Your saved progress is missing.")?
            .ledger;
        if matches!(action, ActivityAction::Place(_))
            && (ledger.coins > MAX_COINS - 2 || ledger.revision == u64::MAX)
        {
            return Err("Your wallet cannot accept another reward.".into());
        }
        let reward = sim.activities.apply(
            &sim.world,
            profile,
            player.body.position,
            activity_id,
            revision,
            action,
        )?;
        if reward > 0 {
            let ledger = &mut sim.profiles.get_mut(profile).unwrap().ledger;
            ledger.coins += reward;
            ledger.revision += 1;
        }
        Ok(reward)
    })();
    match result {
        Ok(reward) => {
            checkpoint_players(connections, sim);
            sim.save(&config.save_path)?;
            send(connections, sim, id, request_id, String::new(), true);
            if reward > 0 {
                crate::send_market_state(
                    connections,
                    sim,
                    id,
                    0,
                    None,
                    "Supply placed: +2 coins".into(),
                    true,
                );
            }
            broadcast(connections, sim);
        }
        Err(reason) => send(connections, sim, id, request_id, reason, false),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_economy::SavedPlayer;
    use rubblekin_core::protocol::PlayerSnapshot;
    fn fixture() -> (World, Activities, Profiles) {
        let world = World::new(42);
        let mut profiles = Profiles::default();
        for id in [
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ] {
            let player = PlayerSnapshot {
                parcel_destination: None,
                id: 1,
                name: "Helper".into(),
                body: rubblekin_core::physics::Body::new(world.spawn_position()),
                yaw: 0.,
                last_input_sequence: 0,
                movement_epoch: 0,
                ride: None,
                glider_ride: None,
                gliding: false,
                deck_position: None,
            };
            profiles.insert(id.into(), SavedPlayer::new(&player));
        }
        let activities = Activities::new(&world);
        (world, activities, profiles)
    }
    #[test]
    fn ownership_matching_contributions_and_recovery_are_finite() {
        let (world, mut a, profiles) = fixture();
        let p = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let q = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let plan = a.records[0].plan.clone();
        assert_eq!(
            a.apply(&world, p, plan.objects[0], 1, 0, ActivityAction::Take(0))
                .unwrap(),
            0
        );
        assert!(
            a.apply(&world, q, plan.objects[0], 1, 1, ActivityAction::Take(0))
                .is_err()
        );
        assert!(
            a.apply(&world, p, plan.objects[1], 1, 1, ActivityAction::Take(1))
                .is_err()
        );
        assert!(
            a.apply(&world, p, plan.sockets[1], 1, 1, ActivityAction::Place(1))
                .is_err()
        );
        assert_eq!(a.records[0].slots[0], Slot::Held(p.into()));
        assert!(
            a.apply(
                &world,
                p,
                world.spawn_position(),
                1,
                1,
                ActivityAction::Place(0)
            )
            .is_err()
        );
        assert_eq!(
            a.apply(&world, p, plan.sockets[0], 1, 1, ActivityAction::Place(0))
                .unwrap(),
            2
        );
        assert!(
            a.apply(&world, p, plan.sockets[0], 1, 1, ActivityAction::Place(0))
                .is_err()
        );
        assert!(
            a.apply(&world, p, plan.sockets[0], 1, 2, ActivityAction::Place(0))
                .is_err()
        );
        assert_eq!(
            a.apply(&world, q, plan.objects[1], 1, 2, ActivityAction::Take(1))
                .unwrap(),
            0
        );
        a.recover();
        assert_eq!(a.records[0].slots[1], Slot::Home);
        assert_eq!(a.records[0].slots[0], Slot::Placed(p.into()));
        assert!(a.validate(&world, &profiles));
        assert!(
            a.apply(
                &world,
                p,
                plan.objects[1],
                1,
                4,
                ActivityAction::Take(u8::MAX)
            )
            .is_err()
        );
        for i in 1..3 {
            let revision = a.records[0].revision;
            a.apply(
                &world,
                q,
                plan.objects[i],
                1,
                revision,
                ActivityAction::Take(i as u8),
            )
            .unwrap();
            let revision = a.records[0].revision;
            assert_eq!(
                a.apply(
                    &world,
                    q,
                    plan.sockets[i],
                    1,
                    revision,
                    ActivityAction::Place(i as u8)
                )
                .unwrap(),
                2
            );
        }
        assert!(a.records[0].complete);
        assert!(a.validate(&world, &profiles));
        assert!(
            a.apply(
                &world,
                p,
                plan.objects[0],
                1,
                a.records[0].revision,
                ActivityAction::Take(0)
            )
            .is_err()
        );
    }
    #[test]
    fn puzzle_has_a_solution_and_completion_cannot_be_undone() {
        let (world, mut a, profiles) = fixture();
        let profile = profiles.keys().next().unwrap();
        let plan = a.records[1].plan.clone();
        assert!(!a.records[1].complete);
        for i in 0..3 {
            for _ in 0..3 {
                if a.records[1].faces[i] == plan.answer[i] {
                    break;
                }
                let revision = a.records[1].revision;
                assert_eq!(
                    a.apply(
                        &world,
                        profile,
                        plan.sockets[i],
                        2,
                        revision,
                        ActivityAction::Turn(i as u8)
                    )
                    .unwrap(),
                    0
                );
            }
        }
        assert!(a.records[1].complete);
        assert!(a.validate(&world, &profiles));
        assert!(
            a.apply(
                &world,
                profile,
                plan.sockets[0],
                2,
                a.records[1].revision,
                ActivityAction::Turn(0)
            )
            .is_err()
        );
        let json = serde_json::to_string(&a).unwrap();
        let again: Activities = serde_json::from_str(&json).unwrap();
        assert!(again.records[1].complete);
    }
    #[test]
    fn malformed_progress_and_duplicate_holders_fail_validation() {
        let (world, a, profiles) = fixture();
        for variant in 0..8 {
            let mut bad = a.clone();
            match variant {
                0 => bad.records[0].plan.objects[0][0] = f32::NAN,
                1 => bad.records[1].plan.id = 1,
                2 => bad.records[0].complete = true,
                3 => bad.records[1].faces[0] = 3,
                4 => bad.records[0].slots[0] = Slot::Held("missing".into()),
                5 => {
                    let p = profiles.keys().next().unwrap().clone();
                    bad.records[0].slots[0] = Slot::Held(p.clone());
                    bad.records[0].slots[1] = Slot::Held(p);
                }
                6 => bad.records[0].plan.recipe_version = 99,
                _ => bad.records[0].revision = u64::MAX,
            }
            assert!(!bad.validate(&world, &profiles), "variant {variant}");
        }
    }
    #[test]
    fn poi_progress_is_additive_durable_and_not_replaced_when_ground_changes() {
        let world = World::generate(42, rubblekin_core::world::WorldGeneration::GeographyV6);
        let (_, _, profiles) = fixture();
        let profile = profiles.keys().next().unwrap();
        let mut activities = Activities {
            records: review_plans(&world).into_iter().map(Record::new).collect(),
        };
        activities.records[1].faces = activities.records[1].plan.answer;
        activities.records[1].complete = true;
        let original = serde_json::to_value(&activities.records).unwrap();
        activities.add_poi_plans(&world);
        assert_eq!(activities.records.len(), 8);
        assert_eq!(
            serde_json::to_value(&activities.records[..2]).unwrap(),
            original
        );
        assert!(activities.validate(&world, &profiles));
        let index = activities
            .records
            .iter()
            .position(|r| r.plan.site_id.is_some() && r.plan.kind == ActivityKind::SpilledSupplies)
            .unwrap();
        let p = activities.records[index].plan.clone();
        activities
            .apply(
                &world,
                profile,
                p.objects[0],
                p.id,
                0,
                ActivityAction::Take(0),
            )
            .unwrap();
        assert_eq!(
            activities
                .apply(
                    &world,
                    profile,
                    p.sockets[0],
                    p.id,
                    1,
                    ActivityAction::Place(0)
                )
                .unwrap(),
            2
        );
        assert!(
            activities
                .apply(
                    &world,
                    profile,
                    p.sockets[0],
                    p.id,
                    2,
                    ActivityAction::Place(0)
                )
                .is_err()
        );
        let mut edited = world.clone();
        let support = rubblekin_core::world::BlockPos::new(
            (p.objects[1][0] / CELL_SIZE).floor() as i32,
            ((p.objects[1][1] - 0.04) / CELL_SIZE).floor() as i32,
            (p.objects[1][2] / CELL_SIZE).floor() as i32,
        );
        edited
            .set_block(support, rubblekin_core::world::Block::Air)
            .unwrap();
        activities.add_poi_plans(&edited);
        assert_eq!(activities.records.len(), 8);
        assert!(activities.validate(&edited, &profiles));
        assert!(
            activities
                .apply(
                    &edited,
                    profile,
                    p.objects[1],
                    p.id,
                    2,
                    ActivityAction::Take(1)
                )
                .is_err()
        );
        let encoded = serde_json::to_vec(&activities).unwrap();
        let mut restored: Activities = serde_json::from_slice(&encoded).unwrap();
        restored.recover();
        restored.add_poi_plans(&edited);
        assert!(restored.validate(&edited, &profiles));
        assert_eq!(
            restored.records[index].slots[0],
            Slot::Placed(profile.clone())
        );
        for variant in 0..4 {
            let mut bad = restored.clone();
            match variant {
                0 => bad.records[index].plan.site_id = Some(999),
                1 => bad.records[index].plan.id = 999,
                2 => bad.records[index].plan.kind = ActivityKind::ShapeStones,
                _ => bad.records[index].plan.objects[0][0] += 1_000.,
            }
            assert!(!bad.validate(&edited, &profiles));
        }
    }
}
