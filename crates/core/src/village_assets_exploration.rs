//! GeographyV6 discoveries. Ordinary editable cells, with open walking space.
use crate::world::Block;

pub(super) fn arch(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if (x < 8 && z > 17) || (x > 28 && z < 7) {
            Block::Stone
        } else {
            Block::Grass
        };
    }
    // Unequal rounded shoulders and an eroded oval opening, with a variable
    // depth. Integer ellipses keep the same cells on every target platform.
    let dx = x * 2 - 35;
    let thickness = 3 + ((x / 5 + y / 4) % 3);
    if (z - 12).abs() <= thickness {
        let outer = dx * dx * 24 * 24 + y * y * 36 * 36;
        let inner = dx * dx * 17 * 17 + y * y * 23 * 23;
        if outer <= 36 * 36 * 24 * 24 && inner >= 23 * 23 * 17 * 17 {
            return Block::Stone;
        }
    }
    if y <= 2
        && ((3..=7).contains(&x) && (18..=21).contains(&z)
            || (28..=32).contains(&x) && (3..=6).contains(&z))
    {
        return Block::Stone;
    }
    Block::Air
}

pub(super) fn stones(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // Unequal paired uprights with two lintels leave the front and central
    // circle open. Low broken stones suggest the missing parts of the ring.
    for (sx, sz, height) in [
        (4, 8, 12),
        (4, 18, 16),
        (11, 24, 14),
        (19, 24, 10),
        (24, 18, 14),
        (24, 8, 11),
    ] {
        if (sx..sx + 3).contains(&x) && (sz..sz + 3).contains(&z) && y <= height - (x + z) % 2 {
            return Block::Stone;
        }
    }
    if ((4..=6).contains(&x) && (9..=19).contains(&z) && y == 12)
        || ((11..=21).contains(&x) && (24..=26).contains(&z) && y == 11)
    {
        return Block::Stone;
    }
    if y <= 2
        && ((8..=10).contains(&x) && (3..=5).contains(&z)
            || (21..=24).contains(&x) && (2..=4).contains(&z))
    {
        return Block::Stone;
    }
    Block::Air
}

pub(super) fn fallen_giant(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // Hollow fallen trunk along the rear, with pale end grain and a root fan.
    let dy = y - 5;
    let dz = z - 18;
    let r2 = dy * dy + dz * dz;
    if (5..=31).contains(&x) && (8..=25).contains(&r2) {
        return if x == 5 || x == 31 {
            Block::Sand
        } else {
            Block::Wood
        };
    }
    if (2..=5).contains(&x)
        && (y <= 12 && (z == 13 || z == 23) || (2..=8).contains(&y) && (12..=24).contains(&z))
    {
        return Block::Wood;
    }
    if (12..=14).contains(&x) && (8..=15).contains(&z) && (5..=7).contains(&y) {
        return Block::Wood;
    }
    if (27..=32).contains(&x) && (20..=25).contains(&z) && (8..=10).contains(&y) {
        return Block::Leaves;
    }
    Block::Air
}

pub(super) fn camp(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if (x - 14).pow(2) + (z - 21).pow(2) < 49 || (13..=15).contains(&x) {
            Block::Dirt
        } else {
            Block::Grass
        };
    }
    // Two differently sized canvas tents flank a clear central gathering space.
    for (left, right, front, back) in [(2, 10, 9, 23), (20, 26, 13, 24)] {
        if (left..=right).contains(&x) && (front..=back).contains(&z) {
            let roof = 2 + (x - left).min(right - x);
            if y == roof || x == (left + right) / 2 && y == roof + 1 {
                return if x == (left + right) / 2 {
                    Block::Wood
                } else {
                    Block::Snow
                };
            }
            if z == back && y < roof {
                return Block::Sand;
            }
            if y == 1 && x == left + 2 && z > front + 3 {
                return Block::Wood;
            }
        }
    }
    if y == 1 && (12..=17).contains(&x) && (20..=25).contains(&z) {
        let edge = x == 12 || x == 17 || z == 20 || z == 25;
        return if edge { Block::Stone } else { Block::Dirt };
    }
    if y == 1 && ((x == 9 || x == 20) && (3..=6).contains(&z)) {
        return Block::Wood;
    }
    Block::Air
}

pub(super) fn tower(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if (x * x + z * 7 + x * z) % 23 < 3 {
            Block::Dirt
        } else {
            Block::Stone
        };
    }
    // A broken octagonal shell with a five-cell doorway and open roof.
    let dx = (x - 13).abs();
    let dz = (z - 17).abs();
    let ring = dx.max(dz) <= 10 && dx + dz <= 16 && (dx >= 8 || dz >= 8 || dx + dz >= 14);
    if ring && !(z < 12 && (11..=15).contains(&x) && y <= 6) {
        let top = 16 + (x * 5 + z * 3) % 11;
        if y <= top {
            if (9..=12).contains(&y)
                && (dx >= 8 && (15..=18).contains(&z) || dz >= 8 && (11..=14).contains(&x))
            {
                return Block::Air;
            }
            return Block::Stone;
        }
    }
    // A broad straight flight leads to the surviving rear observation ledge.
    if (11..=15).contains(&x) && (13..=22).contains(&z) && y <= 1 + (z - 13) / 2 {
        return Block::Stone;
    }
    if (7..=19).contains(&x) && (23..=24).contains(&z) && y <= 5 {
        return Block::Stone;
    }
    Block::Air
}

pub(super) fn kiln(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if z > 11 && x < 13 {
            Block::Clay
        } else {
            Block::Dirt
        };
    }
    // A cold, open-front firing chamber, leaning roof and stacks of old pots.
    if (3..=12).contains(&x) && (12..=23).contains(&z) {
        let wall = x == 3 || x == 12 || z == 23;
        if wall && y <= 8 || y == 9 && (4..=11).contains(&x) && z >= 15 {
            return Block::Brick;
        }
        if (7..=9).contains(&x) && (19..=21).contains(&z) && y <= 14 {
            return Block::Brick;
        }
    }
    if ((x == 18 || x == 25) && (z == 13 || z == 24) && y <= 9)
        || y == 10 && (17..=26).contains(&x) && (12..=25).contains(&z) && z % 4 != 0
    {
        return Block::Wood;
    }
    if (19..=23).contains(&x) && (15..=22).contains(&z) && y <= 2 && (x + z) % 3 != 0 {
        return Block::Clay;
    }
    Block::Air
}

pub(super) fn cairn(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // Several hand-stacked piles, with a clear middle aisle.
    for (cx, cz, height) in [(1, 8, 7), (8, 9, 4), (1, 3, 2)] {
        let radius = if y < 3 { 2 } else { 1 };
        if y <= height && (x - cx).abs() + (z - cz).abs() <= radius {
            return Block::Stone;
        }
    }
    Block::Air
}

pub(super) fn bench(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // A small resting spot with a slatted windbreak and a low canopy.
    if (1..=3).contains(&x) && (4..=10).contains(&z) && y == 2
        || x == 2 && (z == 4 || z == 10) && y == 1
        || x == 0 && (4..=11).contains(&z) && (3..=5).contains(&y) && z % 2 == 0
        || (x == 0 || x == 4) && (z == 3 || z == 11) && y <= 8
        || y == 9 && x <= 4 && (3..=11).contains(&z) && z % 3 != 0
    {
        return Block::Wood;
    }
    Block::Air
}

pub(super) fn cart(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if x > 12 && z > 4 {
            Block::Dirt
        } else {
            Block::Grass
        };
    }
    // One wheel has fallen off; broken shafts point back toward the road.
    if (14..=19).contains(&x) && (6..=13).contains(&z) && y == 3
        || (x == 14 || x == 19) && (7..=13).contains(&z) && (4..=5).contains(&y) && (x + z) % 5 != 0
        || z == 13 && (14..=19).contains(&x) && (4..=6).contains(&y)
        || x == 14 && (1..=5).contains(&z) && y == 2
        || x == 19 && (3..=5).contains(&z) && y == 2
        || x == 1 && (8..=10).contains(&z) && y <= 2
    {
        return Block::Wood;
    }
    for (cx, cz) in [(13, 8), (13, 12), (19, 12)] {
        if x == cx && (y - 2).pow(2) + (z - cz).pow(2) <= 4 {
            return if y == 2 && z == cz {
                Block::Stone
            } else {
                Block::Wood
            };
        }
    }
    Block::Air
}

pub(super) fn survey(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // Three separate legs converge below the small sighting instrument.
    for (foot_x, foot_z) in [(0, 8), (4, 8), (2, 13)] {
        if (1..=8).contains(&y)
            && x == foot_x + (2 - foot_x) * y / 8
            && z == foot_z + (10 - foot_z) * y / 8
        {
            return Block::Wood;
        }
    }
    if x == 9 && z == 11 && y <= 11 || x == 0 && z == 2 && y <= 4 {
        return Block::Wood;
    }
    if y == 9 && (1..=4).contains(&x) && z == 10 {
        return Block::Stone;
    }
    if (1..=3).contains(&x) && (6..=8).contains(&z) && y == 1 {
        return Block::Snow;
    }
    Block::Air
}

pub(super) fn snag(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    let dx = x - 2;
    let dz = z - 8;
    if dx * dx + dz * dz <= if y < 4 { 6 } else { 2 } && y <= 20 - (x + z) % 3
        || (3..=7).contains(&x) && z == 8 && y == 12 + (x - 3) / 2
        || x == 2 && (3..=7).contains(&z) && y == 9 + (7 - z) / 2
        || (8..=17).contains(&y) && x == 2 + (y - 8) / 2 && (7..=8).contains(&z)
        || (7..=15).contains(&y) && z == 8 - (y - 7) / 2 && (2..=3).contains(&x)
        || (0..=3).contains(&x) && z == 10 && y <= 2
    {
        return Block::Wood;
    }
    Block::Air
}

pub(super) fn boulder(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Grass;
    }
    // Two eroded halves leave a shoulder-width crack to walk through.
    for (cx, cz, rx, rz, height) in [(2, 9, 4, 6, 12), (13, 10, 3, 5, 9)] {
        let dx = x - cx;
        let dz = z - cz;
        if dx * dx * rz * rz * height * height
            + dz * dz * rx * rx * height * height
            + y * y * rx * rx * rz * rz
            <= rx * rx * rz * rz * height * height
        {
            return Block::Stone;
        }
    }
    Block::Air
}

pub(super) fn cliff_deck(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Wood;
    }
    // Low rails frame the view, with an open front and room around the bench.
    if y <= 2 && (x == 0 || x == 11 || z == 13 || z == 0 && !(4..=8).contains(&x))
        || y == 2 && (1..=2).contains(&x) && (7..=10).contains(&z)
        || y == 1 && x == 1 && (z == 7 || z == 10)
        || x == 10 && z == 10 && y <= 5
    {
        return Block::Wood;
    }
    Block::Air
}
