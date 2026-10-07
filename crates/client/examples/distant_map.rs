//! Export the exact distant terrain atlas, without its mip levels or UI marks.
//! cargo run --release -p rubblekin_client --example distant_map -- 42 /tmp/distant-map.ppm
#[path = "../src/terrain_albedo.rs"]
mod terrain_albedo;

use rubblekin_core::world::{World, WorldGeneration};
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::Instant,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let seed = args.get(1).map_or(Ok(42), |s| s.parse())?;
    let output = args.get(2).map_or("distant-map.ppm", String::as_str);
    let max_side = args.get(3).map_or(Ok(8192), |s| s.parse())?;
    let started = Instant::now();
    let world = World::generate(seed, WorldGeneration::GeographyV4);
    println!("World generated in {:.2?}", started.elapsed());
    let started = Instant::now();
    let side = terrain_albedo::atlas_side(max_side, false);
    let image = terrain_albedo::prepare_albedo(&world, side, |_| Ok(()))?;
    let side = image.texture_descriptor.size.width;
    let data = image.data.as_ref().ok_or("missing atlas pixels")?;
    println!(
        "{side}² atlas and {} mip levels generated in {:.2?}; {} bytes",
        image.texture_descriptor.mip_level_count,
        started.elapsed(),
        data.len()
    );
    let mut file = BufWriter::new(File::create(output)?);
    write!(file, "P6\n{side} {side}\n255\n")?;
    for pixel in data[..(side * side * 4) as usize].as_chunks::<4>().0 {
        file.write_all(&pixel[..3])?;
    }
    file.flush()?;
    println!("Wrote {output}");
    Ok(())
}
