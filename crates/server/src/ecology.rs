//! A bounded population of persistent individuals. Food and animal decisions
//! advance even with no clients; nothing edits player blocks or village crops.
use rubblekin_core::{
    geography::Biome,
    physics::Body,
    wildlife::{
        HabitatSnapshot, Species, WildlifeAction, WildlifeSnapshot, move_animal, position_is_clear,
    },
    world::{Block, BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_ANIMALS: usize = 256;
const MAX_HABITATS: usize = 64;
const RANGE: f32 = 48.;
// Needs are slow relative to travel. Long lives avoid a seeded cohort dying
// together after a few hours; food, predation and breeding still control numbers.
const RABBIT_LIFETIME: f32 = 24. * 3600.;
const WOLF_LIFETIME: f32 = 72. * 3600.;
const RABBIT_BREEDING: f32 = 2. * 3600.;
const WOLF_BREEDING: f32 = 6. * 3600.;
fn lifetime(species: Species) -> f32 {
    if species == Species::Rabbit {
        RABBIT_LIFETIME
    } else {
        WOLF_LIFETIME
    }
}
fn breeding_period(species: Species) -> f32 {
    if species == Species::Rabbit {
        RABBIT_BREEDING
    } else {
        WOLF_BREEDING
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Habitat {
    pub id: u32,
    pub position: [f32; 3],
    pub forage: f32,
    pub fertility: f32,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Animal {
    pub id: u64,
    pub species: Species,
    pub body: Body,
    pub habitat: u32,
    pub hunger: f32,
    pub age: f32,
    pub breeding: f32,
    pub starving: f32,
    pub action: WildlifeAction,
    pub target: [f32; 3],
    pub decision: f32,
    pub hop: f32,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Ecology {
    pub habitats: Vec<Habitat>,
    pub animals: Vec<Animal>,
    next_id: u64,
    rng: u64,
    pub births: u64,
    pub hunted: u64,
    pub deaths: u64,
    pub migrations: u64,
    pub arrivals: u64,
}
impl Default for Ecology {
    fn default() -> Self {
        Self {
            habitats: vec![],
            animals: vec![],
            next_id: 1,
            rng: 1,
            births: 0,
            hunted: 0,
            deaths: 0,
            migrations: 0,
            arrivals: 0,
        }
    }
}
impl Ecology {
    pub fn new(world: &World) -> Self {
        // Legacy valleys keep their original small forager demonstration.
        let Some(geo) = world.geography() else {
            return Self::default();
        };
        let mut out = Self {
            rng: world.seed as u64 + 1,
            ..Self::default()
        };
        let spawn = world.spawn_position();
        let mut near = vec![
            [spawn[0] + 100., spawn[2]],
            [spawn[0] - 100., spawn[2]],
            [spawn[0], spawn[2] + 100.],
        ];
        if let Some(plan) = world.settlements() {
            let mut candidates = Vec::new();
            for trail in &plan.trails {
                for p in trail.points.iter().step_by(12) {
                    let angle = out.random() * std::f32::consts::TAU;
                    candidates.push([p[0] + angle.cos() * 35., p[2] + angle.sin() * 35.]);
                }
            }
            // Stable shuffle spreads the bounded population over all routes.
            for i in (1..candidates.len()).rev() {
                let j = (out.random() * (i + 1) as f32) as usize;
                candidates.swap(i, j.min(i));
            }
            near.extend(candidates);
        }
        // A few off-route habitats keep wildlife from belonging to roads.
        let radius = world.radius_cells() as f32 * CELL_SIZE;
        let mut wild = Vec::new();
        for _ in 0..512 {
            wild.push([
                (out.random() * 2. - 1.) * radius * 0.85,
                (out.random() * 2. - 1.) * radius * 0.85,
            ]);
        }
        for (candidates, limit) in [(&near, 48), (&wild, MAX_HABITATS)] {
            for &[x, z] in candidates {
                if out.habitats.len() >= limit {
                    break;
                }
                let sample = geo.sample(x, z);
                if !matches!(
                    sample.biome,
                    Biome::Grassland
                        | Biome::Forest
                        | Biome::PineForest
                        | Biome::Rainforest
                        | Biome::Shrubland
                ) || out
                    .habitats
                    .iter()
                    .any(|h| distance(h.position, [x, 0., z]) < 200.)
                {
                    continue;
                }
                let Some(position) = wild_ground(world, x, z, Species::Wolf) else {
                    continue;
                };
                let fertility = match sample.biome {
                    Biome::Grassland | Biome::Rainforest => 1.,
                    Biome::Shrubland => 0.45,
                    _ => 0.75,
                };
                let id = out.habitats.len() as u32;
                out.habitats.push(Habitat {
                    id,
                    position,
                    forage: 80.,
                    fertility,
                });
            }
        }
        for index in 0..out.habitats.len() {
            let center = out.habitats[index].position;
            for _ in 0..3 {
                out.spawn_near(world, Species::Rabbit, index as u32, center, false);
            }
            if index % 9 == 4 {
                for _ in 0..2 {
                    out.spawn_near(world, Species::Wolf, index as u32, center, false);
                }
            }
        }
        out
    }
    fn random(&mut self) -> f32 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.rng >> 40) as u32) as f32 / 16777216.
    }
    fn spawn_near(
        &mut self,
        world: &World,
        species: Species,
        habitat: u32,
        center: [f32; 3],
        young: bool,
    ) -> bool {
        if self.animals.len() >= MAX_ANIMALS {
            return false;
        }
        for _ in 0..24 {
            let x = center[0] + (self.random() * 2. - 1.) * 15.;
            let z = center[2] + (self.random() * 2. - 1.) * 15.;
            if let Some(p) = wild_ground(world, x, z, species) {
                if self
                    .animals
                    .iter()
                    .any(|a| distance(a.body.position, p) < species.radius() * 2. + 0.3)
                {
                    continue;
                }
                let hunger = if young {
                    10.
                } else {
                    20. + self.random() * 40.
                };
                let age = if young {
                    0.
                } else {
                    lifetime(species) * (0.15 + self.random() * 0.5)
                };
                let breeding = breeding_period(species)
                    * if young {
                        1.
                    } else {
                        0.25 + self.random() * 0.75
                    };
                let hop = self.random();
                self.animals.push(Animal {
                    id: self.next_id,
                    species,
                    body: Body::new(p),
                    habitat,
                    hunger,
                    age,
                    breeding,
                    starving: 0.,
                    action: WildlifeAction::Resting,
                    target: p,
                    decision: 0.,
                    hop,
                });
                self.next_id += 1;
                return true;
            }
        }
        false
    }
    pub fn snapshots(&self) -> Vec<WildlifeSnapshot> {
        self.animals
            .iter()
            .map(|a| WildlifeSnapshot {
                id: a.id,
                species: a.species,
                position: a.body.position,
                velocity: a.body.velocity,
                action: a.action,
                hunger: a.hunger,
                habitat: a.habitat,
            })
            .collect()
    }
    pub fn habitat_snapshots(&self) -> Vec<HabitatSnapshot> {
        self.habitats
            .iter()
            .map(|h| HabitatSnapshot {
                id: h.id,
                position: h.position,
                forage: h.forage,
                rabbits: self
                    .animals
                    .iter()
                    .filter(|a| a.habitat == h.id && a.species == Species::Rabbit)
                    .count() as u16,
                wolves: self
                    .animals
                    .iter()
                    .filter(|a| a.habitat == h.id && a.species == Species::Wolf)
                    .count() as u16,
            })
            .collect()
    }
    pub fn validate(&self, world: &World) -> bool {
        let valid_position = |p: [f32; 3]| {
            p.iter().all(|v| v.is_finite())
                && p[0].abs() < world.radius_cells() as f32 * CELL_SIZE
                && p[2].abs() < world.radius_cells() as f32 * CELL_SIZE
                && p[1] >= world.min_y() as f32 * CELL_SIZE
                && p[1] < world.max_y() as f32 * CELL_SIZE
        };
        self.habitats.len() <= MAX_HABITATS
            && self.animals.len() <= MAX_ANIMALS
            && self.next_id > 0
            && self.next_id < 1_000_000_000_000
            && self.habitats.iter().enumerate().all(|(i, h)| {
                h.id == i as u32
                    && valid_position(h.position)
                    && finite_range(h.forage, 0., 100.)
                    && finite_range(h.fertility, 0.1, 1.)
            })
            && self.animals.iter().enumerate().all(|(i, a)| {
                a.id > 0
                    && a.id < self.next_id
                    && !self.animals[..i].iter().any(|b| b.id == a.id)
                    && (a.habitat as usize) < self.habitats.len()
                    && valid_position(a.body.position)
                    && valid_position(a.target)
                    && a.body.velocity.iter().all(|v| finite_range(*v, -30., 30.))
                    && finite_range(a.hunger, 0., 100.)
                    && finite_range(a.age, 0., lifetime(a.species))
                    && finite_range(a.breeding, 0., breeding_period(a.species))
                    && finite_range(a.starving, 0., 601.)
                    && finite_range(a.decision, 0., 31.)
                    && finite_range(a.hop, 0., 2.)
            })
    }
    pub fn tick(&mut self, world: &World, dt: f32, people: &[[f32; 3]]) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let dt = dt.min(0.25);
        for h in &mut self.habitats {
            h.forage = (h.forage + dt * 0.012 * h.fertility).min(100.);
        }
        let before = self.animals.clone();
        let mut attempted_hunts = Vec::new();
        let mut hunted = Vec::new();
        let mut births = Vec::new();
        for index in 0..self.animals.len() {
            let random = self.random();
            let a = &mut self.animals[index];
            a.age += dt;
            a.breeding = (a.breeding - dt).max(0.);
            a.decision = (a.decision - dt).max(0.);
            a.hop = (a.hop - dt).max(0.);
            a.hunger = (a.hunger
                + dt * if a.species == Species::Rabbit {
                    0.025
                } else {
                    0.006
                })
            .min(100.);
            if a.hunger >= 100. {
                a.starving += dt;
            } else {
                a.starving = 0.;
            }
            let nearest_person = people
                .iter()
                .filter(|p| (p[1] - a.body.position[1]).abs() < 5.)
                .min_by(|p, q| {
                    distance(**p, a.body.position).total_cmp(&distance(**q, a.body.position))
                });
            let threat = nearest_person
                .copied()
                .filter(|p| {
                    distance(*p, a.body.position)
                        < if a.species == Species::Rabbit {
                            10.
                        } else {
                            16.
                        }
                })
                .or_else(|| {
                    if a.species == Species::Rabbit {
                        before
                            .iter()
                            .filter(|b| {
                                b.species == Species::Wolf
                                    && (b.body.position[1] - a.body.position[1]).abs() < 6.
                            })
                            .min_by(|p, q| {
                                distance(p.body.position, a.body.position)
                                    .total_cmp(&distance(q.body.position, a.body.position))
                            })
                            .filter(|b| distance(b.body.position, a.body.position) < 24.)
                            .map(|b| b.body.position)
                    } else {
                        None
                    }
                });
            let habitat = &self.habitats[a.habitat as usize];
            let mut speed;
            let mut direction;
            if let Some(threat) = threat {
                a.action = WildlifeAction::Fleeing;
                direction = [
                    a.body.position[0] - threat[0],
                    a.body.position[2] - threat[2],
                ];
                if direction[0].hypot(direction[1]) < 0.01 {
                    direction = [1., 0.];
                }
                let length = direction[0].hypot(direction[1]).max(0.01);
                let limit = world.radius_cells() as f32 * CELL_SIZE - 2.;
                a.target = [
                    (a.body.position[0] + direction[0] / length * 12.).clamp(-limit, limit),
                    a.body.position[1],
                    (a.body.position[2] + direction[1] / length * 12.).clamp(-limit, limit),
                ];
                speed = if a.species == Species::Rabbit {
                    4.6
                } else {
                    5.2
                };
                a.decision = 0.;
            } else {
                if a.action == WildlifeAction::Fleeing {
                    a.decision = 0.;
                }
                // Hungry predators pursue actual living rabbits and eat only
                // after physical arrival, never through a wall or at a distance.
                let prey = if a.species == Species::Wolf && a.hunger > 35. {
                    before
                        .iter()
                        .filter(|b| {
                            b.species == Species::Rabbit
                                && (b.body.position[1] - a.body.position[1]).abs() < 20.
                                && !attempted_hunts.iter().any(|(_, id)| *id == b.id)
                        })
                        .min_by(|p, q| {
                            distance(p.body.position, a.body.position)
                                .total_cmp(&distance(q.body.position, a.body.position))
                        })
                        .filter(|b| distance(b.body.position, a.body.position) < 120.)
                } else {
                    None
                };
                if let Some(prey) = prey {
                    a.action = WildlifeAction::Hunting;
                    a.target = prey.body.position;
                    speed = 5.;
                    if distance(a.body.position, prey.body.position) < 1.0
                        && (a.body.position[1] - prey.body.position[1]).abs() < 0.7
                    {
                        attempted_hunts.push((a.id, prey.id));
                        speed = 0.;
                    }
                } else {
                    if a.decision <= 0. {
                        let residents = before
                            .iter()
                            .filter(|b| b.habitat == a.habitat && b.species == a.species)
                            .count();
                        let depleted = if a.species == Species::Rabbit {
                            habitat.forage < 20. || residents > 7
                        } else {
                            nearby_prey(&before, a.body.position, 120.) < 2
                        };
                        // Finish the physical journey before choosing another
                        // habitat. Counting local prey during travel must not
                        // make the animal alternate destinations every decision.
                        if depleted && a.action != WildlifeAction::Migrating {
                            let destination = self
                                .habitats
                                .iter()
                                .filter(|h| {
                                    h.id != a.habitat
                                        && distance(h.position, a.body.position) < 1800.
                                        && dry_corridor(world, a.body.position, h.position)
                                })
                                .filter(|h| {
                                    if a.species == Species::Rabbit {
                                        h.forage > habitat.forage + 15.
                                    } else {
                                        nearby_prey(&before, h.position, RANGE) >= 3
                                    }
                                })
                                .min_by(|p, q| {
                                    distance(p.position, a.body.position)
                                        .total_cmp(&distance(q.position, a.body.position))
                                });
                            if let Some(h) = destination {
                                a.habitat = h.id;
                                a.target = h.position;
                                a.action = WildlifeAction::Migrating;
                                self.migrations += 1;
                            }
                        }
                        if a.action != WildlifeAction::Migrating {
                            let angle = random * std::f32::consts::TAU;
                            let peer = if a.species == Species::Wolf {
                                before
                                    .iter()
                                    .filter(|b| {
                                        b.id != a.id
                                            && b.species == Species::Wolf
                                            && (b.body.position[1] - a.body.position[1]).abs() < 5.
                                            && distance(b.body.position, a.body.position)
                                                < RANGE * 2.
                                    })
                                    .min_by(|b, c| {
                                        distance(b.body.position, a.body.position)
                                            .total_cmp(&distance(c.body.position, a.body.position))
                                    })
                            } else {
                                None
                            };
                            // Loose pairs keep a chance to meet and breed, while
                            // people avoidance and physical hunting take priority.
                            let radius = if peer.is_some() {
                                4. + random * 12.
                            } else {
                                8. + (random * 19.).fract() * RANGE
                            };
                            let center = peer
                                .map_or(self.habitats[a.habitat as usize].position, |b| {
                                    b.body.position
                                });
                            if let Some(target) = wild_ground(
                                world,
                                center[0] + angle.cos() * radius,
                                center[2] + angle.sin() * radius,
                                a.species,
                            )
                            .or_else(|| {
                                peer.and_then(|b| {
                                    wild_ground(
                                        world,
                                        b.body.position[0],
                                        b.body.position[2],
                                        a.species,
                                    )
                                })
                            }) {
                                a.target = target;
                            }
                            a.action = if a.species == Species::Rabbit
                                && a.hunger > 25.
                                && distance(a.body.position, center) < RANGE
                            {
                                WildlifeAction::Grazing
                            } else if random < 0.25 {
                                WildlifeAction::Resting
                            } else {
                                WildlifeAction::Roaming
                            };
                        }
                        a.decision = 3. + random * 7.;
                    }
                    if a.action == WildlifeAction::Grazing {
                        let h = &mut self.habitats[a.habitat as usize];
                        let cell = BlockPos::new(
                            (a.body.position[0] / CELL_SIZE).floor() as i32,
                            (a.body.position[1] / CELL_SIZE).floor() as i32 - 1,
                            (a.body.position[2] / CELL_SIZE).floor() as i32,
                        );
                        if a.body.on_ground
                            && world.block(cell) == Block::Grass
                            && h.forage > dt * 0.45
                        {
                            h.forage -= dt * 0.45;
                            a.hunger = (a.hunger - dt * 3.).max(0.);
                        } else {
                            a.action = WildlifeAction::Roaming;
                        }
                    }
                    speed = match a.action {
                        WildlifeAction::Roaming => {
                            if a.species == Species::Rabbit {
                                1.4
                            } else {
                                1.8
                            }
                        }
                        WildlifeAction::Migrating => 2.,
                        _ => 0.,
                    };
                }
                direction = [
                    a.target[0] - a.body.position[0],
                    a.target[2] - a.body.position[2],
                ];
                if distance(a.target, a.body.position) < 1.
                    && (a.action != WildlifeAction::Hunting
                        || (a.target[1] - a.body.position[1]).abs() < 0.7)
                {
                    speed = 0.;
                    if a.action == WildlifeAction::Migrating {
                        self.arrivals += 1;
                        a.action = WildlifeAction::Resting;
                        a.decision = 0.;
                    }
                }
            }
            let length = direction[0].hypot(direction[1]);
            if length > 0.01 {
                direction[0] /= length;
                direction[1] /= length;
            }
            // Short fan probes steer around trees, small walls and water.
            // Reject steep drops instead of deliberately jumping off cliffs.
            if speed > 0. {
                let mut found = false;
                for angle in [0_f32, 0.5, -0.5, 1., -1., 1.5, -1.5] {
                    let d = [
                        direction[0] * angle.cos() - direction[1] * angle.sin(),
                        direction[0] * angle.sin() + direction[1] * angle.cos(),
                    ];
                    let p = [
                        a.body.position[0] + d[0] * 0.9,
                        a.body.position[1],
                        a.body.position[2] + d[1] * 0.9,
                    ];
                    let ground = (world.height_at(
                        (p[0] / CELL_SIZE).floor() as i32,
                        (p[2] / CELL_SIZE).floor() as i32,
                    ) + 1) as f32
                        * CELL_SIZE;
                    if habitable(world, p[0], p[2])
                        && ground >= p[1] - 1.5
                        && ground <= p[1] + 0.5
                        && (position_is_clear(world, p, a.species)
                            || position_is_clear(world, [p[0], p[1] + 0.5, p[2]], a.species))
                    {
                        direction = d;
                        found = true;
                        break;
                    }
                }
                if !found {
                    speed = 0.;
                    a.decision = 0.;
                }
            }
            let hop = a.species == Species::Rabbit && speed > 0. && a.hop <= 0. && a.body.on_ground;
            if hop {
                a.hop = 0.7 + random * 0.35;
            }
            let old = a.body.position;
            if position_is_clear(world, a.body.position, a.species) {
                move_animal(world, &mut a.body, a.species, direction, speed, hop, dt);
            } else {
                // Building over an animal never tunnels it through property.
                // Wait for a nearby clear ground position, with no world edits.
                if let Some(p) = wild_ground(
                    world,
                    old[0] + (random - 0.5) * 4.,
                    old[2] + (random * 7.).fract() * 4. - 2.,
                    a.species,
                ) {
                    a.body = Body::new(p);
                }
                a.decision = 0.;
            }
            if speed > 0. && distance(old, a.body.position) < dt * speed * 0.1 {
                a.decision = 0.;
            }
            let count = before
                .iter()
                .filter(|b| b.habitat == a.habitat && b.species == a.species)
                .count();
            let mature = a.age
                > if a.species == Species::Rabbit {
                    600.
                } else {
                    1800.
                };
            let mate = before.iter().any(|b| {
                b.id != a.id
                    && b.species == a.species
                    && b.hunger < 40.
                    && distance(b.body.position, a.body.position) < 24.
            });
            if a.breeding <= 0.
                && mature
                && a.hunger < 30.
                && mate
                && count < if a.species == Species::Rabbit { 8 } else { 3 }
                && if a.species == Species::Rabbit {
                    self.habitats[a.habitat as usize].forage > 40.
                } else {
                    nearby_prey(&before, a.body.position, RANGE) >= 1
                }
            {
                births.push((a.id, a.species, a.habitat, a.body.position));
                a.breeding = breeding_period(a.species);
            }
        }
        // Resolve eating after every body has moved, using the current poses.
        for (wolf_id, prey_id) in attempted_hunts {
            let Some(wolf) = self.animals.iter().find(|a| a.id == wolf_id) else {
                continue;
            };
            let Some(prey) = self.animals.iter().find(|a| a.id == prey_id) else {
                continue;
            };
            let (w, p) = (wolf.body.position, prey.body.position);
            if !hunted.contains(&prey_id) && distance(w, p) < 1.0 && (w[1] - p[1]).abs() < 0.7 {
                let from = [w[0], w[1] + 0.2, w[2]];
                let delta = [p[0] - w[0], p[1] - w[1], p[2] - w[2]];
                let length = (delta.iter().map(|v| v * v).sum::<f32>()).sqrt();
                if length < 0.001 || world.raycast(from, delta, length).is_none() {
                    hunted.push(prey_id);
                    let wolf = self.animals.iter_mut().find(|a| a.id == wolf_id).unwrap();
                    wolf.hunger = (wolf.hunger - 65.).max(0.);
                    wolf.action = WildlifeAction::Resting;
                    wolf.decision = 3.;
                }
            }
        }
        self.hunted += hunted.len() as u64;
        self.animals.retain(|a| !hunted.contains(&a.id));
        let count = self.animals.len();
        self.animals
            .retain(|a| a.starving < 600. && a.age < lifetime(a.species));
        self.deaths += (count - self.animals.len()) as u64;
        for (parent, species, habitat, center) in births {
            if self.animals.iter().any(|a| a.id == parent)
                && self.spawn_near(world, species, habitat, center, true)
            {
                self.births += 1;
            }
        }
    }
}
// Assigned homes include animals still travelling. Predators need actual prey
// near the current place or proposed destination, within their hunting heights.
fn nearby_prey(animals: &[Animal], position: [f32; 3], radius: f32) -> usize {
    animals
        .iter()
        .filter(|a| {
            a.species == Species::Rabbit
                && distance(a.body.position, position) < radius
                && (a.body.position[1] - position[1]).abs() < 20.
        })
        .count()
}
fn finite_range(v: f32, min: f32, max: f32) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
fn habitable(world: &World, x: f32, z: f32) -> bool {
    if x.abs() > world.radius_cells() as f32 * CELL_SIZE - 2.
        || z.abs() > world.radius_cells() as f32 * CELL_SIZE - 2.
    {
        return false;
    }
    if world
        .geography()
        .is_some_and(|g| g.sample(x, z).water.is_some())
    {
        return false;
    }
    !world.settlements().is_some_and(|p| {
        p.villages
            .iter()
            .any(|v| distance(v.center, [x, 0., z]) < 65.)
    })
}
fn wild_ground(world: &World, x: f32, z: f32, species: Species) -> Option<[f32; 3]> {
    if !habitable(world, x, z) {
        return None;
    }
    let cx = (x / CELL_SIZE).floor() as i32;
    let cz = (z / CELL_SIZE).floor() as i32;
    let y = world.height_at(cx, cz);
    let p = [x, (y + 1) as f32 * CELL_SIZE, z];
    (world.block(BlockPos::new(cx, y, cz)) == Block::Grass && position_is_clear(world, p, species))
        .then_some(p)
}

fn dry_corridor(world: &World, from: [f32; 3], to: [f32; 3]) -> bool {
    let steps = (distance(from, to) / 12.).ceil() as usize;
    (1..=steps).all(|i| {
        let t = i as f32 / steps as f32;
        habitable(
            world,
            from[0] + (to[0] - from[0]) * t,
            from[2] + (to[2] - from[2]) * t,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (World, Ecology) {
        let world = World::new(42);
        let mut positions = Vec::new();
        for x in -40..40 {
            for z in -40..40 {
                if let Some(p) = wild_ground(&world, x as f32, z as f32, Species::Wolf) {
                    positions.push(p);
                }
            }
        }
        let p = positions[0];
        let q = *positions
            .iter()
            .find(|q| distance(p, **q) > 20. && distance(p, **q) < 35.)
            .unwrap();
        let mut e = Ecology {
            habitats: vec![
                Habitat {
                    id: 0,
                    position: p,
                    forage: 80.,
                    fertility: 1.,
                },
                Habitat {
                    id: 1,
                    position: q,
                    forage: 90.,
                    fertility: 1.,
                },
            ],
            ..Ecology::default()
        };
        assert!(e.spawn_near(&world, Species::Rabbit, 0, p, false));
        e.animals[0].body = Body::new(p);
        e.animals[0].body.on_ground = true;
        (world, e)
    }
    #[test]
    fn grazing_consumes_only_wild_grass_and_ungrazed_forage_recovers() {
        let (mut world, mut e) = fixture();
        let a = &mut e.animals[0];
        a.hunger = 50.;
        a.action = WildlifeAction::Grazing;
        a.decision = 10.;
        let before = e.clone();
        e.tick(&world, 0.25, &[]);
        assert!(e.animals[0].hunger < before.animals[0].hunger);
        assert!(e.habitats[0].forage < before.habitats[0].forage);
        assert!(e.habitats[1].forage > before.habitats[1].forage);
        let p = e.animals[0].body.position;
        world
            .set_block(
                BlockPos::new(
                    (p[0] / CELL_SIZE).floor() as i32,
                    (p[1] / CELL_SIZE).floor() as i32 - 1,
                    (p[2] / CELL_SIZE).floor() as i32,
                ),
                Block::Brick,
            )
            .unwrap();
        let food = e.habitats[0].forage;
        e.tick(&world, 0.25, &[]);
        assert!(
            e.habitats[0].forage > food,
            "building materials must not become forage"
        );
        assert_eq!(
            world.edits().len(),
            1,
            "wildlife must not edit terrain or property"
        );
    }
    #[test]
    fn both_species_flee_people_and_predation_consumes_exactly_one_actual_prey() {
        let (world, mut e) = fixture();
        let p = e.animals[0].body.position;
        assert!(e.spawn_near(&world, Species::Wolf, 0, p, false));
        for a in &mut e.animals {
            a.body = Body::new(p);
            a.body.on_ground = true;
            a.hunger = 60.;
        }
        let count = e.animals.len();
        e.tick(&world, 0.05, &[[p[0] - 4., p[1], p[2]]]);
        assert_eq!(e.animals.len(), count);
        assert!(
            e.animals
                .iter()
                .all(|a| a.action == WildlifeAction::Fleeing)
        );
        assert!(e.animals.iter().all(|a| a.body.velocity[0] > 0.));
        for a in &mut e.animals {
            a.body = Body::new(p);
            a.body.on_ground = true;
        }
        e.tick(&world, 0.05, &[]);
        assert_eq!(e.hunted, 1);
        assert_eq!(e.animals.len(), 1);
        assert_eq!(e.animals[0].species, Species::Wolf);
        assert!(e.animals[0].hunger < 10.);
        e.tick(&world, 0.05, &[]);
        assert_eq!(e.hunted, 1, "dead prey cannot be eaten again");
    }
    #[test]
    fn food_and_mates_gate_births_and_depletion_sends_animals_to_richer_ground() {
        let (world, mut e) = fixture();
        let p = e.animals[0].body.position;
        assert!(e.spawn_near(&world, Species::Rabbit, 0, p, false));
        for a in &mut e.animals {
            a.hunger = 10.;
            a.age = 1200.;
            a.breeding = 0.;
            a.body = Body::new(p);
        }
        e.habitats[0].forage = 30.;
        e.tick(&world, 0.05, &[]);
        assert_eq!(e.births, 0);
        e.habitats[0].forage = 80.;
        e.tick(&world, 0.05, &[]);
        assert!(e.births > 0);
        assert!(e.animals.iter().any(|a| a.age == 0.));
        e.habitats[0].forage = 1.;
        for a in &mut e.animals {
            a.decision = 0.;
        }
        let before = e.animals[0].body.position;
        e.tick(&world, 0.05, &[]);
        assert!(e.migrations > 0);
        assert_eq!(e.animals[0].habitat, 1);
        assert_eq!(e.animals[0].action, WildlifeAction::Migrating);
        assert!(
            distance(e.animals[0].body.position, before) < 0.2,
            "migration must walk, not teleport to the destination"
        );
        let destination = e.animals[0].target;
        let assigned = e.animals[0].habitat;
        let migrations = e.migrations;
        e.habitats[assigned as usize].forage = 0.;
        for a in &mut e.animals {
            a.decision = 0.;
        }
        e.tick(&world, 0.05, &[]);
        assert_eq!(
            e.animals[0].target, destination,
            "migration must not bounce between food patches before arrival"
        );
        assert_eq!(e.animals[0].habitat, assigned);
        assert_eq!(e.migrations, migrations);
        assert!(e.validate(&world));
    }
    #[test]
    fn starvation_and_age_reduce_population_without_respawning_the_dead() {
        let (world, mut e) = fixture();
        e.animals[0].age = 4. * 3600.;
        e.animals[0].hunger = 10.;
        e.animals[0].action = WildlifeAction::Resting;
        e.animals[0].decision = 3.;
        e.tick(&world, 0.25, &[]);
        assert_eq!(
            e.animals.len(),
            1,
            "four-hour cohort collapse must not return"
        );
        let mut old = e.clone();
        old.animals[0].age = RABBIT_LIFETIME - 0.1;
        old.tick(&world, 0.25, &[]);
        assert!(old.animals.is_empty());
        assert_eq!(old.deaths, 1);
        e.animals[0].hunger = 100.;
        e.animals[0].starving = 599.9;
        e.animals[0].action = WildlifeAction::Resting;
        e.animals[0].decision = 3.;
        e.tick(&world, 0.25, &[]);
        assert!(e.animals.is_empty());
        assert_eq!(e.deaths, 1);
        for _ in 0..100 {
            e.tick(&world, 0.25, &[]);
        }
        assert!(e.animals.is_empty());
    }

    #[test]
    fn wolves_breed_with_mates_and_nearby_prey_even_when_plants_are_depleted() {
        let (world, mut e) = fixture();
        let p = e.animals[0].body.position;
        for _ in 0..2 {
            assert!(e.spawn_near(&world, Species::Rabbit, 0, p, false));
        }
        for _ in 0..2 {
            assert!(e.spawn_near(&world, Species::Wolf, 0, p, false));
        }
        for a in &mut e.animals {
            a.body = Body::new(p);
            a.hunger = 10.;
            a.age = 3600.;
            a.breeding = if a.species == Species::Wolf {
                0.
            } else {
                1000.
            };
            a.action = WildlifeAction::Resting;
            a.decision = 3.;
        }
        e.habitats[0].forage = 0.;
        let mut scarce = e.clone();
        scarce.animals.retain(|a| a.species == Species::Wolf);
        scarce.tick(&world, 0.05, &[]);
        assert_eq!(scarce.births, 0, "no prey must prevent predator births");
        let mut distant = e.clone();
        for a in &mut distant.animals {
            if a.species == Species::Rabbit {
                a.body.position[0] += 70.;
            }
        }
        distant.tick(&world, 0.05, &[]);
        assert_eq!(
            distant.births, 0,
            "distant assigned prey are not nearby food"
        );
        e.tick(&world, 0.05, &[]);
        assert!(
            e.animals
                .iter()
                .any(|a| a.species == Species::Wolf && a.age == 0.)
        );
        assert!(e.validate(&world));
    }
    #[test]
    fn hunting_keeps_moving_when_close_prey_is_below_a_step() {
        let (mut world, mut e) = fixture();
        let p = e.animals[0].body.position;
        let cell = p.map(|v| (v / CELL_SIZE).floor() as i32);
        // A one-meter grassy ledge, with both body bounds clear. The wolf's
        // footprint still rests on the upper lip while its prey is below.
        for x in -6..=6 {
            for z in -6..=6 {
                for y in -1..=8 {
                    world
                        .set_block(
                            BlockPos::new(cell[0] + x, cell[1] + y, cell[2] + z),
                            if y == -1 || (x < 0 && y < 2) {
                                Block::Grass
                            } else {
                                Block::Air
                            },
                        )
                        .unwrap();
                }
            }
        }
        let wolf = [p[0] - 0.3, p[1] + 1., p[2] + 0.25];
        let rabbit = [p[0] + 0.6, p[1], p[2] + 0.25];
        assert!(position_is_clear(&world, wolf, Species::Wolf));
        assert!(position_is_clear(&world, rabbit, Species::Rabbit));
        e.habitats[0].position = wolf;
        assert!(e.spawn_near(&world, Species::Wolf, 0, wolf, false));
        for a in &mut e.animals {
            a.body = Body::new(if a.species == Species::Wolf {
                wolf
            } else {
                rabbit
            });
            a.body.on_ground = true;
            a.hunger = 60.;
            a.hop = 1.;
        }
        e.tick(&world, 0.05, &[]);
        let wolf = e
            .animals
            .iter()
            .find(|a| a.species == Species::Wolf)
            .unwrap();
        assert_eq!(wolf.action, WildlifeAction::Hunting);
        assert!(
            wolf.body.velocity[0].hypot(wolf.body.velocity[2]) > 0.,
            "horizontal proximity must not stop descent toward lower prey"
        );
        assert_eq!(e.hunted, 0, "prey below the ledge is not yet within reach");
    }
    #[test]
    fn roaming_wolves_keep_loose_company_while_people_take_priority() {
        let (world, mut e) = fixture();
        let p = e.animals[0].body.position;
        for _ in 0..2 {
            assert!(e.spawn_near(&world, Species::Rabbit, 0, p, false));
        }
        for _ in 0..2 {
            assert!(e.spawn_near(&world, Species::Wolf, 0, p, false));
        }
        for a in e.animals.iter_mut().filter(|a| a.species == Species::Wolf) {
            a.hunger = 10.;
            a.decision = 0.;
            a.action = WildlifeAction::Roaming;
        }
        let before = e.clone();
        e.tick(&world, 0.05, &[]);
        for a in e.animals.iter().filter(|a| a.species == Species::Wolf) {
            let peer = before
                .animals
                .iter()
                .find(|b| b.species == Species::Wolf && b.id != a.id)
                .unwrap();
            assert!(
                distance(a.target, peer.body.position) <= 16.01,
                "wolf {} target {:?}, peer {:?}, body {:?}",
                a.id,
                a.target,
                peer.body.position,
                a.body.position
            );
        }
        for a in &mut e.animals {
            a.decision = 0.;
            a.hunger = 60.;
        }
        let wolf = e
            .animals
            .iter()
            .find(|a| a.species == Species::Wolf)
            .unwrap()
            .body
            .position;
        e.tick(&world, 0.05, &[[wolf[0] - 2., wolf[1], wolf[2]]]);
        assert!(
            e.animals
                .iter()
                .filter(|a| a.species == Species::Wolf)
                .any(|a| a.action == WildlifeAction::Fleeing)
        );
    }
    #[test]
    fn seeded_population_moves_hops_and_serializes_below_the_wire_limit() {
        use rubblekin_core::{
            protocol::{MAX_MESSAGE_BYTES, ServerMessage},
            world::WorldGeneration,
        };
        let world = World::generate(42, WorldGeneration::GeographyV6);
        let mut e = Ecology::new(&world);
        assert!(e.habitats.len() >= 40);
        assert!(e.animals.len() >= 120);
        assert!(e.validate(&world));
        let initial = e.clone();
        let start = std::time::Instant::now();
        let mut hopped = false;
        for _ in 0..400 {
            e.tick(&world, 0.05, &[]);
            hopped |= e
                .animals
                .iter()
                .any(|a| a.species == Species::Rabbit && a.body.velocity[1] > 1.);
        }
        assert!(hopped);
        assert!(e.animals.iter().any(|a| {
            initial
                .animals
                .iter()
                .any(|b| a.id == b.id && distance(a.body.position, b.body.position) > 2.)
        }));
        assert!(e.validate(&world));
        // Worst bounded population, not only the smaller initial seed.
        let mut snapshots = e.snapshots();
        while snapshots.len() < MAX_ANIMALS {
            let mut a = snapshots[0].clone();
            a.id = u64::MAX;
            a.position = [-12345.678, 4096.123, -12345.678];
            a.velocity = [-29.999, 29.999, 29.999];
            snapshots.push(a);
        }
        let bytes = serde_json::to_vec(&ServerMessage::WildlifeState {
            animals: snapshots,
            habitats: e.habitat_snapshots(),
        })
        .unwrap();
        assert!(
            bytes.len() < MAX_MESSAGE_BYTES,
            "wildlife message has {} bytes",
            bytes.len()
        );
        eprintln!(
            "seed42: {} habitats, {} animals, {} hunted; 20 simulated seconds in {:?}; worst bounded packet {} bytes",
            e.habitats.len(),
            e.animals.len(),
            e.hunted,
            start.elapsed(),
            bytes.len()
        );
        let copy: Ecology = serde_json::from_slice(&serde_json::to_vec(&e).unwrap()).unwrap();
        assert!(copy.validate(&world));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::to_value(copy).unwrap()
        );
    }
}

#[cfg(test)]
mod sustained_tests {
    use super::*;
    #[test]
    fn an_hour_without_people_changes_populations_and_forage_with_bounded_work() {
        let world = World::generate(42, rubblekin_core::world::WorldGeneration::GeographyV6);
        let mut e = Ecology::new(&world);
        let start = std::time::Instant::now();
        let initial = e.animals.len();
        for step in 0..14400 {
            e.tick(&world, 0.25, &[]);
            if step % 2400 == 2399 {
                assert!(e.validate(&world));
                assert!(e.animals.len() <= MAX_ANIMALS);
                eprintln!(
                    "{} minutes: {} animals, {} births, {} hunted, {} other deaths, {} migrations",
                    (step + 1) / 240,
                    e.animals.len(),
                    e.births,
                    e.hunted,
                    e.deaths,
                    e.migrations
                );
            }
        }
        assert!(e.births > 0);
        assert!(e.hunted > 0);
        assert!(e.migrations > 0);
        assert!(
            e.arrivals > 0,
            "migration decisions must result in physical arrivals"
        );
        eprintln!("{} completed habitat arrivals", e.arrivals);
        assert!(e.habitats.iter().any(|h| h.forage < 70.));
        assert!(e.animals.len() > 50);
        assert_eq!(
            e.animals.len() as u64,
            initial as u64 + e.births - e.hunted - e.deaths
        );
        eprintln!("one simulated hour in {:?}", start.elapsed());
    }
}
