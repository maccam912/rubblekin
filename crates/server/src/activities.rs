//! Finite shared activities: prop ownership, one-time contributions and saves.
use crate::{
    Connection, ServerConfig, checkpoint_players,
    persistence::Simulation,
    player_economy::{MAX_COINS, Profiles},
};
use rubblekin_core::{
    activities::*,
    economy::resource_index,
    protocol::ServerMessage,
    settlement::ResourceKind,
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
    #[serde(skip)]
    working: Option<RepairWork>,
}
#[derive(Debug, Clone)]
struct RepairWork {
    profile: String,
    start_position: [f32; 3],
    started_at: Option<f64>,
    elapsed_seconds: f32,
    last_update: f64,
}
impl Record {
    fn new(plan: ActivityPlan) -> Self {
        let faces = if plan.kind == ActivityKind::FlowGarden {
            flow_garden::START_FACES
        } else {
            [
                plan.answer[0],
                (plan.answer[1] + 1) % 3,
                (plan.answer[2] + 2) % 3,
            ]
        };
        Self {
            plan,
            revision: 0,
            slots: std::array::from_fn(|_| Slot::Home),
            faces,
            complete: false,
            working: None,
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
    /// Retire the geographic town review area and upgrade POI layouts while
    /// preserving contributions and wallets. A damaged site keeps its saved
    /// layout; it never receives a new payable copy.
    pub fn add_poi_plans(&mut self, world: &World) {
        let Some(settlements) = world.settlements() else {
            return;
        };
        self.records.retain(|r| r.plan.site_id.is_some());
        for record in &mut self.records {
            if record.plan.recipe_version < RECIPE_VERSION
                && record.revision < u64::MAX - 1
                && record.plan.points().all(|p| supported(world, *p))
                && let Some(site) = settlements
                    .composed_sites
                    .iter()
                    .find(|s| Some(s.id) == record.plan.site_id)
                && let Some(plan) = at_site(world, site)
            {
                record.plan = plan;
                record.revision += 1;
            }
        }
        for plan in poi_plans(world) {
            if !self.records.iter().any(|r| r.plan.id == plan.id) && self.records.len() < MAX_PLANS
            {
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
                            matches!(
                                p.kind,
                                ActivityKind::SpilledSupplies | ActivityKind::ShapeStones
                            ) && p.id
                                == if p.kind == ActivityKind::SpilledSupplies {
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
                                            && p.points().all(|v| {
                                                v[0] >= site.bounds[0] as f32 * CELL_SIZE
                                                    && v[0] < site.bounds[2] as f32 * CELL_SIZE
                                                    && v[2] >= site.bounds[1] as f32 * CELL_SIZE
                                                    && v[2] < site.bounds[3] as f32 * CELL_SIZE
                                            })
                                    })
                                })
                        }
                    }
                    && (1..=RECIPE_VERSION).contains(&p.recipe_version)
                    && (p.clues.is_none()
                        || (matches!(p.kind, ActivityKind::ShapeStones | ActivityKind::FlowGarden)
                            && p.site_id.is_some()
                            && p.recipe_version >= 2))
                    && (p.recipe_version < 2
                        || p.site_id.is_none()
                        || p.kind != ActivityKind::ShapeStones
                        || p.clues.is_some())
                    && r.revision < u64::MAX
                    && p.points().all(|v| {
                        v.iter().all(|n| n.is_finite())
                            && v[0].abs() <= radius
                            && v[2].abs() <= radius
                            && v[1] >= world.min_y() as f32 * CELL_SIZE
                            && v[1] <= world.max_y() as f32 * CELL_SIZE
                    })
                    && p.answer.iter().chain(r.faces.iter()).all(|f| {
                        *f < if p.kind == ActivityKind::FlowGarden {
                            4
                        } else {
                            3
                        }
                    })
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
                        ActivityKind::CartRepair => {
                            p.recipe_version >= 3
                                && r.slots.iter().all(|s| !matches!(s, Slot::Held(_)))
                                && (!matches!(r.slots[2], Slot::Placed(_))
                                    || r.slots[..2].iter().all(|s| matches!(s, Slot::Placed(_))))
                                && r.complete
                                    == r.slots.iter().all(|s| matches!(s, Slot::Placed(_)))
                        }
                        ActivityKind::FlowGarden => {
                            p.recipe_version >= 4
                                && p.objects == p.sockets
                                && p.answer == flow_garden::SOLVED_FACES
                                && p.clues.is_some_and(|anchors| {
                                    flow_garden::valid_layout(p.sockets, anchors)
                                })
                                && r.slots.iter().all(|s| *s == Slot::Home)
                                && r.complete
                                    == flow_garden::flow(r.faces)
                                        .is_some_and(|flow| flow.garden_watered)
                        }
                    }
            })
    }
    pub fn recover(&mut self) {
        for r in &mut self.records {
            r.working = None;
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
            if r.working.as_ref().is_some_and(|w| w.profile == profile) {
                r.working = None;
                local = true;
            }
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
                available: r.plan.available(world),
                repair: r.working.as_ref().and_then(|w| {
                    connections
                        .iter()
                        .find(|(_, c)| !c.dead && c.profile_id.as_ref() == Some(&w.profile))
                        .map(|(id, _)| RepairProgress {
                            player_id: *id,
                            elapsed_seconds: w.elapsed_seconds,
                        })
                }),
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
        if self.is_working(profile) {
            return Err("Finish your repair or step away first.".into());
        }
        let r = self
            .records
            .iter_mut()
            .find(|r| r.plan.id == id)
            .ok_or("That activity is unavailable.")?;
        if revision != r.revision {
            return Err("Someone changed this activity. Try again.".into());
        }
        if r.complete && r.plan.kind != ActivityKind::FlowGarden {
            return Err("This activity is already finished.".into());
        }
        if r.revision >= u64::MAX - 2 {
            return Err("This activity cannot accept another change.".into());
        }
        if r.working.is_some() {
            return Err("Someone is finishing this repair.".into());
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
        if !r.plan.available(world) {
            return Err(if r.plan.kind == ActivityKind::FlowGarden {
                "The garden's ground or channels are blocked. Clear the obstruction before turning."
                    .into()
            } else {
                "The activity's ground is blocked or missing. Return the supply to its starting place.".into()
            });
        }
        let (i, target) = match action {
            ActivityAction::Take(i) if r.plan.kind == ActivityKind::SpilledSupplies => {
                (i as usize, r.plan.objects.get(i as usize))
            }
            ActivityAction::Place(i) if r.plan.kind == ActivityKind::SpilledSupplies => {
                (i as usize, r.plan.sockets.get(i as usize))
            }
            ActivityAction::Turn(i)
                if matches!(
                    r.plan.kind,
                    ActivityKind::ShapeStones | ActivityKind::FlowGarden
                ) =>
            {
                (i as usize, r.plan.sockets.get(i as usize))
            }
            ActivityAction::Contribute(i) if r.plan.kind == ActivityKind::CartRepair && i < 2 => {
                (i as usize, r.plan.sockets.get(i as usize))
            }
            ActivityAction::Hammer if r.plan.kind == ActivityKind::CartRepair => {
                (2, r.plan.sockets.get(2))
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
                if r.plan.kind == ActivityKind::FlowGarden {
                    r.faces[i] = (r.faces[i] + 1) % 4;
                    r.complete = flow_garden::flow(r.faces).is_some_and(|f| f.garden_watered);
                } else {
                    r.faces[i] = (r.faces[i] + 1) % 3;
                    r.complete = r.faces == r.plan.answer;
                }
            }
            ActivityAction::Contribute(_) => {
                if holding {
                    return Err("Place or return your carried supply first.".into());
                }
                if r.slots[i] != Slot::Home {
                    return Err("That part is already fitted. Your Timber stays in cargo.".into());
                }
                r.slots[i] = Slot::Placed(profile.into());
                reward = 2;
            }
            ActivityAction::Hammer => {
                if holding || !r.slots[..2].iter().all(|s| matches!(s, Slot::Placed(_))) {
                    return Err(
                        "Fit the wheel and plank first, then finish with the hammer.".into(),
                    );
                }
                r.working = Some(RepairWork {
                    profile: profile.into(),
                    start_position: position,
                    started_at: None,
                    elapsed_seconds: 0.,
                    last_update: 0.,
                });
            }
            ActivityAction::Return => unreachable!(),
        }
        r.revision += 1;
        Ok(reward)
    }
    pub(crate) fn is_working(&self, profile: &str) -> bool {
        self.records
            .iter()
            .any(|r| r.working.as_ref().is_some_and(|w| w.profile == profile))
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
/// Progress is transient; installed parts and their receipts are durable.
/// Persist every completion before broadcasting it or its private wage.
pub(crate) fn advance_repairs(
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
) -> io::Result<()> {
    let mut changed = false;
    let mut completed = false;
    let mut replies = Vec::new();
    for r in &mut sim.activities.records {
        let Some(mut work) = r.working.take() else {
            continue;
        };
        let participant = connections
            .iter()
            .find(|(_, c)| !c.dead && c.profile_id.as_ref() == Some(&work.profile));
        let valid = participant.is_some_and(|(_, c)| {
            c.player.as_ref().is_some_and(|p| {
                c.active_work.is_none()
                    && p.ride.is_none()
                    && !p.gliding
                    && p.body.on_ground
                    && p.body
                        .velocity
                        .iter()
                        .all(|v| v.is_finite() && v.abs() < 0.05)
                    && distance(p.body.position, work.start_position) < 0.8
                    && r.plan.points().all(|point| supported(&sim.world, *point))
                    && can_interact(&sim.world, p.body.position, r.plan.sockets[2])
            })
        });
        if !valid {
            r.revision += 1;
            changed = true;
            if let Some((id, _)) = participant {
                replies.push((
                    *id,
                    "Repair stopped. Fitted parts stay in place.".into(),
                    false,
                ));
            }
            continue;
        }
        let started_at = *work.started_at.get_or_insert(sim.world_time);
        work.elapsed_seconds = (sim.world_time - started_at)
            .max(0.)
            .min(REPAIR_SECONDS as f64) as f32;
        if work.elapsed_seconds >= REPAIR_SECONDS {
            let ledger = &mut sim.profiles.get_mut(&work.profile).unwrap().ledger;
            let id = *participant.unwrap().0;
            if ledger.coins > MAX_COINS - 2 || ledger.revision == u64::MAX {
                replies.push((
                    id,
                    "Your wallet cannot accept the repair wage. Fitted parts stay in place.".into(),
                    false,
                ));
            } else {
                ledger.coins += 2;
                ledger.revision += 1;
                r.slots[2] = Slot::Placed(work.profile);
                r.complete = true;
                completed = true;
                replies.push((id, "Cart repaired: +2 coins".into(), true));
            }
            r.revision += 1;
            changed = true;
        } else {
            if sim.world_time - work.last_update >= 0.25 {
                work.last_update = sim.world_time;
                changed = true;
            }
            r.working = Some(work);
        }
    }
    if completed {
        checkpoint_players(connections, sim);
        sim.save(&config.save_path)?;
    }
    for (id, notice, accepted) in replies {
        crate::send_market_state(connections, sim, id, 0, None, notice, accepted);
    }
    if changed {
        broadcast(connections, sim);
    }
    Ok(())
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
        if matches!(
            action,
            ActivityAction::Place(_) | ActivityAction::Contribute(_) | ActivityAction::Hammer
        ) && (ledger.coins > MAX_COINS - 2 || ledger.revision == u64::MAX)
        {
            return Err("Your wallet cannot accept another reward.".into());
        }
        if matches!(action, ActivityAction::Contribute(_))
            && ledger.cargo[resource_index(ResourceKind::Timber)] == 0
        {
            return Err(
                "Needs 1 Timber from cargo. Buy it at a village market or gather timber salvage."
                    .into(),
            );
        }
        if c.active_work.is_some() {
            return Err("Finish or cancel your current work first.".into());
        }
        if matches!(action, ActivityAction::Hammer)
            && (!player.body.on_ground
                || player
                    .body
                    .velocity
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 0.05)
                || player.ride.is_some()
                || player.gliding)
        {
            return Err("Stand still on the ground beside the cart.".into());
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
            if matches!(action, ActivityAction::Contribute(_)) {
                ledger.cargo[resource_index(ResourceKind::Timber)] -= 1;
            }
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
                    if matches!(action, ActivityAction::Contribute(_)) {
                        "Part fitted: spent 1 Timber · +2 coins".into()
                    } else {
                        "Supply placed: +2 coins".into()
                    },
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
        activities.add_poi_plans(&world);
        let count = poi_plans(&world).len();
        assert_eq!(activities.records.len(), count);
        assert!(activities.records.iter().all(|r| r.plan.site_id.is_some()));
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
        assert_eq!(activities.records.len(), count);
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
    #[test]
    fn earlier_poi_layouts_move_without_resetting_paid_slots_or_stone_faces() {
        let world = World::generate(42, rubblekin_core::world::WorldGeneration::GeographyV6);
        let (_, mut activities, profiles) = fixture();
        activities.records = poi_plans(&world).into_iter().map(Record::new).collect();
        // Repairs and gardens did not exist in recipe1. Only the earlier two
        // families need migration; later records are introduced separately.
        activities.records.retain(|r| {
            matches!(
                r.plan.kind,
                ActivityKind::SpilledSupplies | ActivityKind::ShapeStones
            )
        });
        let profile = profiles.keys().next().unwrap();
        for r in &mut activities.records {
            r.plan.recipe_version = 1;
            r.plan.clues = None;
            // Earlier scenes clustered their props on the first route leg.
            r.plan.objects[2] = r.plan.objects[0];
            if r.plan.kind == ActivityKind::SpilledSupplies {
                r.slots[0] = Slot::Placed(profile.clone());
            } else {
                r.faces = r.plan.answer;
                r.complete = true;
            }
            r.revision = 4;
        }
        assert!(activities.validate(&world, &profiles));
        let before = activities.clone();
        let bytes = serde_json::to_vec(&activities).unwrap();
        let mut restored: Activities = serde_json::from_slice(&bytes).unwrap();
        restored.recover();
        restored.add_poi_plans(&world);
        assert_eq!(restored.records.len(), poi_plans(&world).len());
        assert!(restored.validate(&world, &profiles));
        for (old, new) in before.records.iter().zip(&restored.records) {
            assert_eq!(old.plan.id, new.plan.id);
            assert_ne!(old.plan.objects, new.plan.objects);
            assert_eq!(new.plan.recipe_version, RECIPE_VERSION);
            assert_eq!(new.slots, old.slots);
            assert_eq!(new.faces, old.faces);
            assert_eq!(new.complete, old.complete);
            assert_eq!(new.revision, 5);
            if new.plan.kind == ActivityKind::SpilledSupplies {
                let p = new.plan.clone();
                assert!(
                    restored
                        .clone()
                        .apply(
                            &world,
                            profile,
                            p.objects[0],
                            p.id,
                            5,
                            ActivityAction::Take(0)
                        )
                        .is_err()
                );
            }
        }
        // Upgrade once; repeated loads neither move the scene nor bump state.
        let again = serde_json::to_value(&restored).unwrap();
        restored.add_poi_plans(&world);
        assert_eq!(serde_json::to_value(&restored).unwrap(), again);
        let shape = restored
            .records
            .iter()
            .position(|r| r.plan.kind == ActivityKind::ShapeStones)
            .unwrap();
        let mut bad = restored;
        bad.records[shape].plan.clues.as_mut().unwrap()[0][0] = f32::NAN;
        assert!(!bad.validate(&world, &profiles));
    }
    #[test]
    fn gardens_are_reversible_without_payments_and_recipe_three_adds_one_without_resetting_receipts()
     {
        let world = World::generate(42, rubblekin_core::world::WorldGeneration::GeographyV6);
        let (_, _, profiles) = fixture();
        let profile = profiles.keys().next().unwrap();
        let mut activities = Activities::new(&world);
        let index = activities
            .records
            .iter()
            .position(|r| r.plan.kind == ActivityKind::FlowGarden)
            .unwrap();
        let plan = activities.records[index].plan.clone();
        assert!(activities.validate(&world, &profiles));
        let mut blocked = world.clone();
        let source = plan.clues.unwrap()[0];
        blocked
            .set_block(
                rubblekin_core::world::BlockPos::new(
                    ((source[0] - 0.75) / CELL_SIZE).floor() as i32,
                    ((source[1] + 0.55) / CELL_SIZE).floor() as i32,
                    ((source[2] + 0.75) / CELL_SIZE).floor() as i32,
                ),
                rubblekin_core::world::Block::Stone,
            )
            .unwrap();
        assert!(
            plan.points().all(|p| supported(&blocked, *p)),
            "The reserved prop edge is beyond the standing capsule"
        );
        assert!(!plan.available(&blocked));
        assert!(
            activities
                .clone()
                .apply(
                    &blocked,
                    profile,
                    plan.sockets[0],
                    plan.id,
                    0,
                    ActivityAction::Turn(0)
                )
                .is_err()
        );
        assert!(
            activities.validate(&blocked, &profiles),
            "Blocked scenes keep their saved connections without replacement"
        );
        for (i, turns) in [(0, 3), (1, 1), (2, 1)] {
            for _ in 0..turns {
                let revision = activities.records[index].revision;
                assert_eq!(
                    activities
                        .apply(
                            &world,
                            profile,
                            plan.sockets[i],
                            plan.id,
                            revision,
                            ActivityAction::Turn(i as u8)
                        )
                        .unwrap(),
                    0
                );
            }
        }
        assert!(activities.records[index].complete);
        for expected in [false, true, false, true] {
            let revision = activities.records[index].revision;
            assert_eq!(
                activities
                    .apply(
                        &world,
                        profile,
                        plan.sockets[2],
                        plan.id,
                        revision,
                        ActivityAction::Turn(2)
                    )
                    .unwrap(),
                0
            );
            assert_eq!(activities.records[index].complete, expected);
            assert!(activities.validate(&world, &profiles));
        }
        for variant in 0..7 {
            let mut bad = activities.clone();
            let r = &mut bad.records[index];
            match variant {
                0 => r.faces[0] = 4,
                1 => r.complete = !r.complete,
                2 => r.slots[0] = Slot::Placed(profile.clone()),
                3 => r.plan.clues.as_mut().unwrap()[0][0] += 0.3,
                4 => r.plan.sockets[2][0] += 0.3,
                5 => r.plan.answer[0] = 0,
                _ => r.plan.recipe_version = 3,
            }
            assert!(!bad.validate(&world, &profiles), "variant {variant}");
        }
        let mut old = activities;
        old.records
            .retain(|r| r.plan.kind != ActivityKind::FlowGarden);
        for r in &mut old.records {
            r.plan.recipe_version = 3;
        }
        let supplied = old
            .records
            .iter_mut()
            .find(|r| r.plan.kind == ActivityKind::SpilledSupplies)
            .unwrap();
        supplied.slots[0] = Slot::Placed(profile.clone());
        let before = old.clone();
        old.add_poi_plans(&world);
        assert_eq!(old.records.len(), before.records.len() + 1);
        for saved in &before.records {
            let upgraded = old
                .records
                .iter()
                .find(|r| r.plan.id == saved.plan.id)
                .unwrap();
            assert_eq!(upgraded.slots, saved.slots);
            assert_eq!(upgraded.faces, saved.faces);
            assert_eq!(upgraded.complete, saved.complete);
        }
        assert!(old.validate(&world, &profiles));
        let snapshot = serde_json::to_value(&old).unwrap();
        old.add_poi_plans(&world);
        assert_eq!(serde_json::to_value(&old).unwrap(), snapshot);
    }
}
