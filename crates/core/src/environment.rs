//! Shared wind and channel flow. Simulation time controls weather on both peers.
use crate::world::{BlockPos, CELL_SIZE, World};

/// Horizontal air velocity, in metres per second (the direction it blows TO).
/// Slow seeded variation avoids abrupt changes while a player is tacking.
pub fn wind(seed: u32, time: f64) -> [f32; 2] {
    let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
    let phase = f64::from(seed % 6283) * 0.001;
    let angle = phase + libm::sin(time / 180.0 + phase) * 0.28;
    let speed = 6.0 + libm::sin(time / 73.0 + phase) * 1.2;
    [
        (libm::cos(angle) * speed) as f32,
        (libm::sin(angle) * speed) as f32,
    ]
}

/// Generated water remains bounded by real edited solid cells. Filling a
/// channel blocks watercraft; digging does not invent a new water simulation.
pub fn water_surface(world: &World, x: f32, z: f32) -> Option<f32> {
    if !x.is_finite() || !z.is_finite() {
        return None;
    }
    let level = if let Some(geo) = world.geography() {
        geo.sample(x, z).water?
    } else {
        let center = world.river_center((z / CELL_SIZE).floor() as i32) * CELL_SIZE;
        if (x - center).abs() > 2.1
            || x.abs().max(z.abs()) >= world.radius_cells() as f32 * CELL_SIZE
        {
            return None;
        }
        crate::world::WATER_LEVEL
    };
    let p = [x, level - 0.05, z].map(|v| (v / CELL_SIZE).floor() as i32);
    (!world.block(BlockPos::new(p[0], p[1], p[2])).is_solid()).then_some(level)
}

pub fn current(world: &World, x: f32, z: f32) -> [f32; 2] {
    if water_surface(world, x, z).is_none() {
        return [0.0; 2];
    }
    if let Some(geo) = world.geography() {
        geo.river_current(x, z)
    } else {
        let target_z = z - 3.;
        let target_x = world.river_center((target_z / CELL_SIZE).floor() as i32) * CELL_SIZE;
        let dx = target_x - x;
        let dz = target_z - z;
        let length = libm::hypotf(dx, dz).max(0.01);
        [dx / length * 0.6, dz / length * 0.6]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wind_is_shared_finite_and_smooth_and_legacy_water_respects_edits() {
        let a = wind(42, 100.);
        let b = wind(42, 100.01);
        assert_eq!(a, wind(42, 100.));
        assert_ne!(a, wind(7, 100.));
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01));
        assert!(wind(42, f64::NAN).iter().all(|v| v.is_finite()));
        let mut world = World::new(42);
        let x = world.river_center(0) * CELL_SIZE;
        assert!(water_surface(&world, x, 0.).is_some());
        assert!(current(&world, x, 0.)[1] < 0.);
        let p = BlockPos::new((x / CELL_SIZE).floor() as i32, 1, 0);
        world.set_block(p, crate::world::Block::Stone).unwrap();
        assert!(water_surface(&world, x, 0.).is_none());
        assert_eq!(current(&world, x, 0.), [0.; 2]);
    }
}
