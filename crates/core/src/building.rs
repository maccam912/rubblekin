//! Inclusive box selections and discoveries derived from saved player edits.
use crate::world::{Block, BlockEdit, BlockPos, CELL_SIZE};
use std::collections::{HashMap, HashSet};

pub const MAX_FILL_BLOCKS: usize = 4096;
pub const FILL_REACH: f32 = 64.0;
pub const BUILD_MIN_BLOCKS: usize = 64;
pub const BUILD_MIN_SPAN: i32 = 8;

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub min: BlockPos,
    pub max: BlockPos,
    pub count: usize,
}
impl Selection {
    pub fn new(a: BlockPos, b: BlockPos) -> Result<Self, &'static str> {
        let min = BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
        let max = BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
        let mut count = 1u64;
        for (lo, hi) in [(min.x, max.x), (min.y, max.y), (min.z, max.z)] {
            let side = (i64::from(hi) - i64::from(lo) + 1) as u64;
            count = count.saturating_mul(side);
        }
        if count > MAX_FILL_BLOCKS as u64 {
            return Err("Select at most 4096 blocks; fill a larger build in sections.");
        }
        Ok(Self {
            min,
            max,
            count: count as usize,
        })
    }
    pub fn cells(self) -> impl Iterator<Item = BlockPos> {
        (self.min.x..=self.max.x).flat_map(move |x| {
            (self.min.y..=self.max.y)
                .flat_map(move |y| (self.min.z..=self.max.z).map(move |z| BlockPos::new(x, y, z)))
        })
    }
}

#[derive(Debug, Clone)]
pub struct Build {
    pub min: BlockPos,
    pub max: BlockPos,
    pub cells: Vec<BlockEdit>,
}
impl Build {
    pub fn position(&self) -> [f32; 3] {
        [
            (self.min.x as f32 + self.max.x as f32 + 1.) * CELL_SIZE * 0.5,
            (self.max.y as f32 + 1.) * CELL_SIZE,
            (self.min.z as f32 + self.max.z as f32 + 1.) * CELL_SIZE * 0.5,
        ]
    }
}
/// Six-neighbour connected components exclude excavation (Air). Sorting makes
/// markers and meshes reproducible on all peers and after reload.
pub fn substantial_builds(edits: &[BlockEdit]) -> Vec<Build> {
    let blocks: HashMap<_, _> = edits
        .iter()
        .filter(|e| e.block != Block::Air)
        .map(|e| (e.position, e.block))
        .collect();
    let mut unseen: HashSet<_> = blocks.keys().copied().collect();
    let mut builds = Vec::new();
    for e in edits {
        if !unseen.remove(&e.position) {
            continue;
        }
        let mut cells = vec![*e];
        let mut index = 0;
        let mut min = e.position;
        let mut max = min;
        while index < cells.len() {
            let p = cells[index].position;
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                // Saved edits have finite world bounds far from i32 extremes.
                if let (Some(x), Some(y), Some(z)) = (
                    p.x.checked_add(dx),
                    p.y.checked_add(dy),
                    p.z.checked_add(dz),
                ) {
                    let n = BlockPos::new(x, y, z);
                    if unseen.remove(&n) {
                        cells.push(BlockEdit {
                            position: n,
                            block: blocks[&n],
                        });
                    }
                }
            }
            index += 1;
        }
        let span = (max.x - min.x + 1)
            .max(max.y - min.y + 1)
            .max(max.z - min.z + 1);
        if cells.len() >= BUILD_MIN_BLOCKS && span >= BUILD_MIN_SPAN {
            cells.sort_unstable_by_key(|e| (e.position.x, e.position.y, e.position.z));
            builds.push(Build { min, max, cells });
        }
    }
    builds.sort_unstable_by_key(|b| (b.min.x, b.min.y, b.min.z));
    builds
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversed_inclusive_boxes_and_overflow_are_bounded() {
        let s = Selection::new(BlockPos::new(7, 3, 7), BlockPos::new(0, 0, 0)).unwrap();
        assert_eq!(s.count, 256);
        assert_eq!(s.cells().count(), 256);
        assert!(
            Selection::new(BlockPos::new(i32::MIN, 0, 0), BlockPos::new(i32::MAX, 0, 0)).is_err()
        );
    }
    #[test]
    fn shared_builds_appear_grow_and_disappear_when_broken_up() {
        let mut edits: Vec<_> = Selection::new(BlockPos::new(0, 0, 0), BlockPos::new(7, 0, 7))
            .unwrap()
            .cells()
            .map(|position| BlockEdit {
                position,
                block: Block::Brick,
            })
            .collect();
        assert_eq!(substantial_builds(&edits).len(), 1);
        edits.reverse();
        assert_eq!(substantial_builds(&edits)[0].position(), [2., 0.5, 2.]);
        edits.retain(|e| e.position.x != 3);
        assert!(substantial_builds(&edits).is_empty());
        for e in &mut edits {
            e.block = Block::Air;
        }
        assert!(substantial_builds(&edits).is_empty());
    }
}
