//! Export the actual generated geography for review:
//! cargo run --release -p rubblekin_core --example geography -- 42 /tmp/geography.ppm
//! PPM is deliberately dependency-free; open or convert it with an image viewer.
use rubblekin_core::geography::{GRID_SIDE, GRID_SPACING, Geography, WORLD_SIZE};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().collect();
    let seed: u32 = arguments.get(1).map_or(Ok(42), |s| s.parse())?;
    let output = arguments.get(2).map_or("geography.ppm", String::as_str);
    let started = Instant::now();
    let geography = Geography::generate(seed);
    let generated = started.elapsed();
    let side = 1537;
    let mut file = BufWriter::new(File::create(output)?);
    write!(file, "P6\n{side} {side}\n255\n")?;
    let spawn = geography.spawn();
    let mut pixels = Vec::with_capacity(side * side * 3);
    for row in 0..side {
        for col in 0..side {
            let x = (col as f32 / (side - 1) as f32 - 0.5) * WORLD_SIZE;
            let z = (row as f32 / (side - 1) as f32 - 0.5) * WORLD_SIZE;
            let sample = geography.sample(x, z);
            let [mut red, mut green, mut blue, _] = sample.biome.color();
            let left = geography.sample(x - 24.0, z).height;
            let right = geography.sample(x + 24.0, z).height;
            let up = geography.sample(x, z - 24.0).height;
            let down = geography.sample(x, z + 24.0).height;
            let dx = (right - left) / 48.0;
            let dz = (down - up) / 48.0;
            let light = ((0.75 + dx * 0.45 + dz * 0.30) / (1.0 + dx * dx + dz * dz).sqrt())
                .clamp(0.30, 1.13);
            red *= light;
            green *= light;
            blue *= light;
            if let Some(level) = sample.water {
                let depth = ((level - sample.height) / 100.0).clamp(0.0, 1.0);
                red = 0.20 - depth * 0.10;
                green = 0.49 - depth * 0.16;
                blue = 0.62 - depth * 0.16;
            }
            // A small cream cross marks the ordinary player spawn.
            let sx = (x - spawn[0]).abs();
            let sz = (z - spawn[2]).abs();
            if (sx < 28.0 && sz < 135.0) || (sz < 28.0 && sx < 135.0) {
                red = 1.0;
                green = 0.90;
                blue = 0.61;
            }
            pixels.extend([red, green, blue].map(|value| (value * 255.0).clamp(0.0, 255.0) as u8));
        }
    }
    file.write_all(&pixels)?;
    file.flush()?;
    let land = geography.heights().iter().filter(|h| **h > 0.0).count();
    let minimum = geography
        .heights()
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let maximum = geography
        .heights()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    println!(
        "Seed {seed}; generation {:.2?}; map written to {output}",
        generated
    );
    println!(
        "World {:.3} km square; {GRID_SIDE}² grid at {GRID_SPACING} m; land {:.1}%",
        WORLD_SIZE / 1000.0,
        land as f32 * 100.0 / geography.heights().len() as f32
    );
    println!(
        "Elevation {minimum:.1}..{maximum:.1} m; eroded/transported material {:.3} km³",
        geography.eroded_volume() / 1e9
    );
    println!(
        "Spawn [{:.1}, {:.1}, {:.1}] m; map cross marks spawn",
        spawn[0], spawn[1], spawn[2]
    );
    Ok(())
}
