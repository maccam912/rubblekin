//! Sparse GeographyV5 trail landmarks, built from ordinary editable cells.
use super::front_door;
use crate::world::Block;

pub(super) fn ruin(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if (3..=20).contains(&x) && (3..=20).contains(&z) && (x + z) % 4 != 0 {
            Block::Dirt
        } else {
            Block::Stone
        };
    }
    if front_door(x, y, z, 24) || (z >= 21 && (11..=13).contains(&x) && y <= 5) {
        return Block::Air;
    }
    // Four worn corner piers, two open doorways, and roofless broken walls.
    if ((1..=2).contains(&x) || (21..=22).contains(&x))
        && ((1..=2).contains(&z) || (21..=22).contains(&z))
    {
        return if y <= 9 + (x + z) % 3 {
            Block::Stone
        } else {
            Block::Air
        };
    }
    let wall =
        (x == 2 || x == 21 || z == 2 || z == 21) && (2..=21).contains(&x) && (2..=21).contains(&z);
    if wall {
        if (z == 2 || z == 21) && (11..=13).contains(&x) && y <= 5 {
            return Block::Air;
        }
        if (x == 2 || x == 21) && (9..=13).contains(&z) && y >= 2 {
            return Block::Air;
        }
        return if y <= 4 + (x * 7 + z * 3) % 4 {
            Block::Stone
        } else {
            Block::Air
        };
    }
    // A surviving hearth and two benches suggest an old roadside resting place.
    if (4..=6).contains(&x) && (17..=19).contains(&z) && y <= 3 {
        return Block::Brick;
    }
    if y == 1 && ((x == 5 && (5..=8).contains(&z)) || (x == 18 && (15..=18).contains(&z))) {
        return Block::Wood;
    }
    if y == 1 && ((x == 3 && (15..=16).contains(&z)) || (x == 20 && (4..=6).contains(&z))) {
        return Block::Leaves;
    }
    Block::Air
}

pub(super) fn waystone(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Stone;
    }
    if (4..=5).contains(&x) && (4..=5).contains(&z) && y <= 9 {
        return if y == 3 || y == 7 {
            Block::Brick
        } else {
            Block::Stone
        };
    }
    if (3..=6).contains(&x) && (3..=6).contains(&z) && y == 10 {
        return Block::Stone;
    }
    if x == 1 && (4..=7).contains(&z) && y == 1 {
        return Block::Wood;
    }
    Block::Air
}

/// Open timber resting shelter. Four sides and the center aisle remain clear.
pub(super) fn pavilion(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if x == 0 || x == 23 || z == 0 || z == 19 {
            Block::Stone
        } else {
            Block::Wood
        };
    }
    let roof = 10 + x.min(23 - x) / 3;
    if y == roof && (1..=18).contains(&z) {
        return if x % 4 == 0 {
            Block::Brick
        } else {
            Block::Wood
        };
    }
    if ((x == 2 || x == 21) && (z == 3 || z == 16) && y <= 10)
        || (y == 9
            && ((z == 3 || z == 16) && (2..=21).contains(&x)
                || (x == 2 || x == 21) && (3..=16).contains(&z)))
    {
        return Block::Wood;
    }
    // Side benches and a low table, off the entrance-to-exit walking aisle.
    if y == 1 && (x == 4 || x == 19) && (5..=14).contains(&z) {
        return Block::Wood;
    }
    if (6..=8).contains(&x) && (8..=11).contains(&z) && y == 2 {
        return Block::Wood;
    }
    if x == 7 && (z == 8 || z == 11) && y == 1 {
        return Block::Wood;
    }
    // A cold stone hearth and piled timber suggest a rest stop, without fire
    // simulation, loot containers, or a new job role.
    if (16..=18).contains(&x) && (15..=17).contains(&z) && y == 1 {
        return if x == 17 && z == 16 {
            Block::Brick
        } else {
            Block::Stone
        };
    }
    Block::Air
}

/// A small stone-cutting terrace beside a real stone-rich area.
pub(super) fn quarry(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if !(2..=25).contains(&x) || z > 21 || (x + z) % 7 == 0 {
            Block::Stone
        } else {
            Block::Dirt
        };
    }
    // Rear rock terraces have 1 m treads and half-meter rises. A broad central
    // route reaches the top; taller irregular shoulders frame the cut face.
    if (16..=22).contains(&z) && (2..=25).contains(&x) {
        let height = if (11..=16).contains(&x) {
            1 + (z - 16) / 2
        } else {
            5 + (z - 16) / 2 + (x * 3 + z) % 3
        };
        if y <= height {
            return Block::Stone;
        }
    }
    // Covered cutting bench at one side and squared blocks at the other.
    if (4..=8).contains(&x) && (7..=10).contains(&z) {
        if y == 2 {
            return Block::Stone;
        }
        if y == 1 && (x == 4 || x == 8) {
            return Block::Wood;
        }
    }
    if ((x == 3 || x == 9) && (z == 6 || z == 11) && y <= 8)
        || y == 8 && (3..=9).contains(&x) && (6..=11).contains(&z)
    {
        return Block::Wood;
    }
    if (20..=23).contains(&x) && (5..=11).contains(&z) && (z - 5) % 3 < 2 && y <= 2 + (x - 20) / 2 {
        return Block::Stone;
    }
    Block::Air
}
