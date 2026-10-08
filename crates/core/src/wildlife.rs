//! Shared wildlife poses and small-body collision against actual edited voxels.
use crate::{
    physics::Body,
    world::{BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Species {
    Rabbit,
    Wolf,
}
impl Species {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rabbit => "Rabbit",
            Self::Wolf => "Wolf",
        }
    }
    pub fn radius(self) -> f32 {
        match self {
            Self::Rabbit => 0.41,
            Self::Wolf => 0.70,
        }
    }
    pub fn height(self) -> f32 {
        match self {
            Self::Rabbit => 0.74,
            Self::Wolf => 1.06,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WildlifeAction {
    Resting,
    Roaming,
    Grazing,
    Fleeing,
    Hunting,
    Migrating,
}
impl WildlifeAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Resting => "Resting",
            Self::Roaming => "Exploring its home range",
            Self::Grazing => "Grazing wild forage",
            Self::Fleeing => "Keeping a safe distance",
            Self::Hunting => "Hunting a rabbit",
            Self::Migrating => "Moving to another home range",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WildlifeSnapshot {
    pub id: u64,
    pub species: Species,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub action: WildlifeAction,
    pub hunger: f32,
    pub habitat: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitatSnapshot {
    pub id: u32,
    pub position: [f32; 3],
    pub forage: f32,
    pub rabbits: u16,
    pub wolves: u16,
}

pub fn position_is_clear(world: &World, p: [f32; 3], species: Species) -> bool {
    if !p.iter().all(|v| v.is_finite())
        || p[0].abs() + species.radius() >= world.radius_cells() as f32 * CELL_SIZE
        || p[2].abs() + species.radius() >= world.radius_cells() as f32 * CELL_SIZE
        || p[1] < world.min_y() as f32 * CELL_SIZE
        || p[1] + species.height() >= world.max_y() as f32 * CELL_SIZE
    {
        return false;
    }
    let r = species.radius();
    let skin = 0.002;
    let min =
        [p[0] - r + skin, p[1] + skin, p[2] - r + skin].map(|v| (v / CELL_SIZE).floor() as i32);
    let max = [
        p[0] + r - skin,
        p[1] + species.height() - skin,
        p[2] + r - skin,
    ]
    .map(|v| (v / CELL_SIZE).floor() as i32);
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if world.block(BlockPos::new(x, y, z)).is_solid() {
                    return false;
                }
            }
        }
    }
    true
}

/// Bounded, swept movement for wildlife. Half-cell steps and rabbit hops use
/// real gravity; walls, edited ceilings and holes remain physical obstacles.
pub fn move_animal(
    world: &World,
    body: &mut Body,
    species: Species,
    direction: [f32; 2],
    speed: f32,
    hop: bool,
    dt: f32,
) {
    if !dt.is_finite()
        || dt <= 0.0
        || !direction.iter().all(|v| v.is_finite())
        || !speed.is_finite()
    {
        return;
    }
    let dt = dt.min(0.25);
    let length = direction[0].hypot(direction[1]).max(1.0);
    body.velocity[0] = direction[0] / length * speed.clamp(0., 6.);
    body.velocity[2] = direction[1] / length * speed.clamp(0., 6.);
    if hop && body.on_ground {
        body.velocity[1] = 4.4;
        body.on_ground = false;
    }
    body.velocity[1] = (body.velocity[1] - 22. * dt).max(-30.);
    let steps = (body
        .velocity
        .iter()
        .map(|v| v.abs() * dt)
        .fold(0., f32::max)
        / 0.1)
        .ceil()
        .max(1.) as usize;
    let step_dt = dt / steps as f32;
    for _ in 0..steps {
        for axis in [0, 2, 1] {
            let amount = body.velocity[axis] * step_dt;
            if amount.abs() < 0.000001 {
                continue;
            }
            let mut next = body.position;
            next[axis] += amount;
            if position_is_clear(world, next, species) {
                body.position = next;
                if axis == 1 {
                    body.on_ground = false;
                }
            } else if axis != 1 && body.on_ground {
                next[1] += CELL_SIZE;
                let mut above = body.position;
                above[1] += CELL_SIZE;
                if position_is_clear(world, above, species)
                    && position_is_clear(world, next, species)
                {
                    body.position = next;
                    body.on_ground = false;
                } else {
                    body.velocity[axis] = 0.;
                }
            } else {
                // Bisect the final substep so feet rest on the surface rather
                // than hovering one full sweep increment above it.
                let (mut low, mut high) = (0., 1.);
                for _ in 0..10 {
                    let t = (low + high) * 0.5;
                    let mut p = body.position;
                    p[axis] += amount * t;
                    if position_is_clear(world, p, species) {
                        low = t;
                    } else {
                        high = t;
                    }
                }
                body.position[axis] += amount * low;
                if axis == 1 {
                    body.on_ground = amount < 0.;
                }
                body.velocity[axis] = 0.;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Block;
    fn flat() -> World {
        let mut world = World::new(42);
        for x in -20..=20 {
            for z in -6..=6 {
                world
                    .set_block(BlockPos::new(x, 99, z), Block::Grass)
                    .unwrap();
            }
        }
        world
    }
    #[test]
    fn rabbit_hops_land_and_small_bodies_use_the_actual_ceiling_and_walls() {
        let mut world = flat();
        let mut rabbit = Body::new([-3., 50., 0.]);
        for _ in 0..4 {
            move_animal(
                &world,
                &mut rabbit,
                Species::Rabbit,
                [0., 0.],
                0.,
                false,
                0.05,
            );
        }
        assert!(rabbit.on_ground);
        let mut peak = 50_f32;
        for i in 0..40 {
            move_animal(
                &world,
                &mut rabbit,
                Species::Rabbit,
                [0., 0.],
                0.,
                i == 0,
                0.05,
            );
            peak = peak.max(rabbit.position[1]);
        }
        assert!(peak > 50.3);
        assert!(rabbit.on_ground);
        assert!((rabbit.position[1] - 50.).abs() < 0.01);
        for x in 0..=8 {
            for z in -4..=4 {
                world
                    .set_block(BlockPos::new(x, 102, z), Block::Stone)
                    .unwrap();
            }
        }
        let mut wolf = Body::new([-3., 50., 0.]);
        for _ in 0..60 {
            for (body, species) in [(&mut rabbit, Species::Rabbit), (&mut wolf, Species::Wolf)] {
                move_animal(&world, body, species, [1., 0.], 2., false, 0.05);
                assert!(position_is_clear(&world, body.position, species));
            }
        }
        assert!(
            rabbit.position[0] > 2.,
            "rabbit should fit under the one-meter ceiling"
        );
        assert!(
            wolf.position[0] < 0.,
            "wolf must not fit through the rabbit tunnel"
        );
        for z in -4..=4 {
            for y in 100..=104 {
                world
                    .set_block(BlockPos::new(12, y, z), Block::Stone)
                    .unwrap();
            }
        }
        for _ in 0..15 {
            move_animal(
                &world,
                &mut rabbit,
                Species::Rabbit,
                [1., 0.],
                6.,
                true,
                0.25,
            );
        }
        assert!(
            rabbit.position[0] < 6. - Species::Rabbit.radius() + 0.01,
            "fast hops must not tunnel through a wall"
        );
    }
}
