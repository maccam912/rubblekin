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
