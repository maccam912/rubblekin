use rubblekin_core::{
    gliders::{GliderDestination, GliderFlight, reachable, stations},
    world::{World, WorldGeneration},
};
use serde_json::json;

/// A fixed byte order and float bits catch platform math drift before it can
/// reorder village IDs. DefaultHasher deliberately has no stable wire format.
fn geography_fingerprint(world: &World) -> String {
    let geography = world.geography().unwrap();
    let mut hash = 0xcbf29ce484222325_u64;
    let mut add = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    };
    for field in [
        geography.heights(),
        geography.water_heights(),
        geography.flow_accumulation(),
    ] {
        for value in field {
            add(&value.to_bits().to_le_bytes());
        }
    }
    for downstream in geography.drainage() {
        add(&downstream.map_or(u64::MAX, |i| i as u64).to_le_bytes());
    }
    // Include detailed terrain, biome and climate, not only the coarse heights.
    for z in (-16_000..16_000).step_by(127) {
        for x in (-16_000..16_000).step_by(131) {
            let s = geography.sample(x as f32, z as f32);
            for value in [
                s.height,
                s.water.unwrap_or(f32::NAN),
                s.moisture,
                s.temperature,
            ] {
                add(&value.to_bits().to_le_bytes());
            }
            add(&[s.biome as u8]);
        }
    }
    format!("{hash:016x}")
}

#[test]
fn android_and_server_generate_the_same_station_ids_and_terrain() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/portable_generation.json")).unwrap();
    let mut actual = Vec::new();
    for seed in [42, 7, 99, 2_689_504_302] {
        let world = World::generate(seed, WorldGeneration::GeographyV6);
        let stops = stations(&world);
        let mut snapshot = json!({
            "seed": seed,
            "geography": geography_fingerprint(&world),
            "stations": stops.iter().map(|s| json!([s.village_id, s.name, s.position])).collect::<Vec<_>>()
        });
        if seed == 2_689_504_302 {
            let mossvale = stops
                .iter()
                .find(|s| s.name == "Mossvale")
                .expect("the live seed must contain Android's Mossvale station");
            assert_eq!(mossvale.village_id, 2);
            let mut routes = Vec::new();
            for destination in stops.iter().filter(|s| {
                s.village_id != mossvale.village_id && reachable(mossvale.position, s.position)
            }) {
                let flight = GliderFlight::plan(
                    &world,
                    mossvale,
                    GliderDestination::Village(destination.village_id),
                    destination.name.clone(),
                    destination.position,
                    1,
                    0.,
                )
                .unwrap_or_else(|error| panic!("Mossvale -> {}: {error}", destination.name));
                // Store bits so decimal JSON parsing cannot round a flight
                // coordinate or duration differently from to_value().
                routes.push(json!({
                    "id": destination.village_id,
                    "name": destination.name,
                    "from": flight.from.map(f32::to_bits),
                    "to": flight.to.map(f32::to_bits),
                    "apex": flight.apex.to_bits(),
                    "duration": flight.duration.to_bits(),
                }));
            }
            assert_eq!(routes.len(), 7);
            snapshot["mossvale_routes"] = json!(routes);
        }
        actual.push(snapshot);
    }
    // The fixture is shared with the Android executable test, never recomputed
    // from whichever platform happens to be running this assertion.
    assert_eq!(json!(actual), expected);
}
