use rubblekin_core::{
    physics::{Body, MoveInput, move_character_with_obstacles, resolve_character_overlaps},
    protocol::{AdminAction, NpcAction, NpcSnapshot},
    world::{CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

use crate::navigation::{Navigation, Walker, Walking};

pub use rubblekin_core::world::berry_patch_positions;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Forager {
    pub snapshot: NpcSnapshot,
    body: Body,
    forced_goal: Option<NpcAction>,
    forage_weight: f32,
    rest_weight: f32,
    decision_elapsed: f32,
    harvest_elapsed: f32,
    wander_step: usize,
    #[serde(default)]
    home: [f32; 2],
    #[serde(default)]
    stuck_elapsed: f32,
    #[serde(skip)]
    navigation: Navigation,
}

impl Forager {
    pub fn new(world: &World) -> Self {
        // The forager's home stays fixed when building changes the player's
        // safe spawn location. Its berry patches use the same stable anchor.
        let origin = world.geography().map_or([0.0; 3], |geography| {
            let spawn = geography.spawn();
            [
                (spawn[0] / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE * 0.5,
                spawn[1],
                (spawn[2] / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE * 0.5,
            ]
        });
        let (x, z) = (origin[0] + 3.0, origin[2]);
        let position = [x, world.surface_height(x, z), z];
        let mut result = Self {
            snapshot: NpcSnapshot {
                name: "Moss".into(),
                position,
                hunger: 62.0,
                energy: 80.0,
                action: NpcAction::Forage,
                reason: "Looking for berries because I am hungry".into(),
                berries: 0,
                forced: false,
                target: None,
            },
            body: Body::new(position),
            forced_goal: None,
            forage_weight: 1.0,
            rest_weight: 1.0,
            decision_elapsed: 0.0,
            harvest_elapsed: 0.0,
            wander_step: 0,
            home: [origin[0], origin[2]],
            stuck_elapsed: 0.0,
            navigation: Navigation::default(),
        };
        result.decide(world);
        result
    }

    pub fn validate(&self) -> bool {
        self.snapshot.position.iter().all(|v| v.is_finite())
            && self.home.iter().all(|v| v.is_finite())
            && self.body.position.iter().all(|v| v.is_finite())
            && self.body.velocity.iter().all(|v| v.is_finite())
            && self
                .snapshot
                .target
                .is_none_or(|p| p.iter().all(|v| v.is_finite()))
            && [self.snapshot.hunger, self.snapshot.energy]
                .iter()
                .all(|v| (0.0..=100.0).contains(v))
            && [self.forage_weight, self.rest_weight]
                .iter()
                .all(|v| (0.0..=10.0).contains(v))
            && self.decision_elapsed.is_finite()
            && self.harvest_elapsed.is_finite()
            && self.stuck_elapsed.is_finite()
            && self.snapshot.position == self.body.position
    }

    pub fn admin(&mut self, world: &World, action: AdminAction) -> Result<(), String> {
        match action {
            AdminAction::SetNpcGoal { goal } => self.forced_goal = goal,
            AdminAction::SetNpcNeeds { hunger, energy } => {
                if ![hunger, energy].iter().all(|n| (0.0..=100.0).contains(n)) {
                    return Err("NPC hunger and energy must be finite numbers from 0 to 100".into());
                }
                self.snapshot.hunger = hunger;
                self.snapshot.energy = energy;
            }
            AdminAction::SetNpcWeights { forage, rest } => {
                if ![forage, rest].iter().all(|n| (0.0..=10.0).contains(n)) {
                    return Err("NPC weights must be finite numbers from 0 to 10".into());
                }
                self.forage_weight = forage;
                self.rest_weight = rest;
            }
        }
        self.decide(world);
        Ok(())
    }

    fn decide(&mut self, world: &World) {
        let forage = self.snapshot.hunger * self.forage_weight;
        let rest = (100.0 - self.snapshot.energy) * self.rest_weight;
        let choice = self.forced_goal.unwrap_or({
            if forage > rest && forage > 25.0 {
                NpcAction::Forage
            } else if rest > 25.0 {
                NpcAction::Rest
            } else {
                NpcAction::Wander
            }
        });
        if choice != self.snapshot.action {
            self.harvest_elapsed = 0.0;
            self.snapshot.target = None;
        }
        self.snapshot.action = choice;
        self.snapshot.forced = self.forced_goal.is_some();
        self.snapshot.reason = if self.snapshot.forced {
            format!(
                "Admin override: {} (clear override to restore autonomy)",
                choice.label()
            )
        } else {
            format!(
                "Forage score {forage:.0}, rest score {rest:.0}, wander threshold 25; {}",
                choice.label()
            )
        };
        self.snapshot.target = match choice {
            NpcAction::Rest => None,
            NpcAction::Forage => berry_patch_positions(world).into_iter().min_by(|a, b| {
                horizontal_distance(self.body.position, *a)
                    .total_cmp(&horizontal_distance(self.body.position, *b))
            }),
            NpcAction::Wander => {
                let points = [(3.0, 0.0), (0.0, 7.0), (-6.0, 0.0), (0.0, -5.0)];
                let (x, z) = points[self.wander_step % points.len()];
                let (x, z) = (x + self.home[0], z + self.home[1]);
                Some([x, world.surface_height(x, z), z])
            }
        };
    }

    pub fn resolve_overlaps(&mut self, world: &World, obstacles: &[[f32; 3]]) {
        // Reuse terrain recovery too: a center-column spawn can straddle a
        // higher neighboring column. This normalization does not advance needs.
        move_character_with_obstacles(
            world,
            &mut self.body,
            MoveInput::default(),
            0.000001,
            obstacles,
        );
        resolve_character_overlaps(world, &mut self.body, obstacles);
        self.snapshot.position = self.body.position;
    }

    #[cfg(test)]
    pub fn tick(&mut self, world: &World, dt: f32) {
        self.tick_with_obstacles(world, dt, &[]);
    }

    pub fn tick_with_obstacles(&mut self, world: &World, dt: f32, obstacles: &[[f32; 3]]) {
        self.snapshot.hunger = (self.snapshot.hunger + dt * 0.18).min(100.0);
        self.snapshot.energy = (self.snapshot.energy - dt * 0.10).max(0.0);
        self.decision_elapsed += dt;
        if self.decision_elapsed >= 0.5 {
            self.decision_elapsed = 0.0;
            self.decide(world);
        }
        let mut input = MoveInput::default();
        match self.snapshot.action {
            NpcAction::Rest => {
                self.snapshot.energy = (self.snapshot.energy + dt * 3.0).min(100.0);
            }
            NpcAction::Forage | NpcAction::Wander => {
                if let Some(target) = self.snapshot.target {
                    let distance = horizontal_distance(self.body.position, target);
                    if distance > 0.7 {
                        input.direction = [
                            (target[0] - self.body.position[0]) / distance * 0.45,
                            (target[2] - self.body.position[2]) / distance * 0.45,
                        ];
                        // Terrain jumps remain separate from the shared crowd steering below.
                        input.jump = self.body.on_ground && self.stuck_elapsed > 0.4;
                        self.harvest_elapsed = 0.0;
                    } else if (self.body.position[1] - target[1]).abs() < 1.0 {
                        if self.snapshot.action == NpcAction::Forage {
                            self.harvest_elapsed += dt;
                            if self.harvest_elapsed >= 3.0 {
                                self.harvest_elapsed = 0.0;
                                self.snapshot.berries = self.snapshot.berries.saturating_add(3);
                                if self.snapshot.hunger >= 20.0 {
                                    self.snapshot.berries -= 1;
                                    self.snapshot.hunger = (self.snapshot.hunger - 22.0).max(0.0);
                                }
                            }
                        } else {
                            self.wander_step = self.wander_step.wrapping_add(1);
                            self.decide(world);
                        }
                    }
                }
            }
        }
        if input.direction != [0.0; 2]
            && let Some(target) = self.snapshot.target
        {
            let mut walker = Walker::on_foot(&self.body);
            // Crowd jams use walking detours, rather than jumping over people.
            let jump = input.jump
                && !obstacles
                    .iter()
                    .any(|p| horizontal_distance(self.body.position, *p) < 2.0);
            Walking {
                world,
                obstacles,
                airships: None,
            }
            .walk(
                &mut walker,
                &mut self.navigation,
                target,
                0.45,
                jump,
                dt,
                |_| true,
            );
            self.body = walker.body;
            self.stuck_elapsed = self.navigation.stalled;
        } else {
            move_character_with_obstacles(world, &mut self.body, input, dt, obstacles);
            self.stuck_elapsed = 0.0;
        }
        self.snapshot.position = self.body.position;
    }
}

fn horizontal_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forager_stops_at_an_embodied_character_without_overlapping() {
        let world = World::new(42);
        let mut npc = Forager::new(&world);
        npc.snapshot.action = NpcAction::Wander;
        npc.forced_goal = Some(NpcAction::Wander);
        npc.snapshot.target = Some([
            npc.body.position[0] + 4.0,
            npc.body.position[1],
            npc.body.position[2],
        ]);
        let blocker = [
            npc.body.position[0] + 1.0,
            npc.body.position[1],
            npc.body.position[2],
        ];
        let start = npc.body.position;
        // Keep this short enough to avoid an unrelated periodic goal change.
        for _ in 0..8 {
            npc.tick_with_obstacles(&world, 0.05, &[blocker]);
            assert!(!rubblekin_core::physics::characters_overlap(
                npc.snapshot.position,
                blocker
            ));
        }
        assert!(npc.snapshot.position[0] > start[0]);
        assert!(
            npc.snapshot.position[0]
                <= blocker[0] - rubblekin_core::physics::PLAYER_RADIUS * 2.0 + 0.001
        );
    }

    #[test]
    fn forager_moves_harvests_and_eats_without_a_player() {
        let world = World::new(42);
        let mut npc = Forager::new(&world);
        let start = npc.snapshot.position;
        let hunger = npc.snapshot.hunger;
        for _ in 0..600 {
            npc.tick(&world, 0.05);
        }
        assert_ne!(start, npc.snapshot.position);
        assert!(npc.snapshot.berries > 0, "{:?}", npc.snapshot);
        assert!(npc.snapshot.hunger < hunger, "{:?}", npc.snapshot);
    }

    #[test]
    fn override_can_be_cleared_and_weights_change_decisions() {
        let world = World::new(42);
        let mut npc = Forager::new(&world);
        npc.admin(
            &world,
            AdminAction::SetNpcGoal {
                goal: Some(NpcAction::Rest),
            },
        )
        .unwrap();
        assert_eq!(npc.snapshot.action, NpcAction::Rest);
        assert!(npc.snapshot.forced);
        npc.admin(&world, AdminAction::SetNpcGoal { goal: None })
            .unwrap();
        assert_eq!(npc.snapshot.action, NpcAction::Forage);
        npc.admin(
            &world,
            AdminAction::SetNpcWeights {
                forage: 0.0,
                rest: 10.0,
            },
        )
        .unwrap();
        assert_eq!(npc.snapshot.action, NpcAction::Rest);
        assert!(!npc.snapshot.forced);
        assert!(
            npc.admin(
                &world,
                AdminAction::SetNpcNeeds {
                    hunger: f32::NAN,
                    energy: 50.0
                }
            )
            .is_err()
        );
    }
}
