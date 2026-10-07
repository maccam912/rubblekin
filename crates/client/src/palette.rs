//! Two fixed pages of unlimited building blocks, separate from traded cargo.
use rubblekin_core::world::Block;

pub const QUICK_SLOTS: usize = 6;
// Keep the original six indices stable for existing quick-key behavior.
pub const MATERIALS: [Block; 11] = [
    Block::Grass,
    Block::Dirt,
    Block::Stone,
    Block::Wood,
    Block::Brick,
    Block::Glass,
    Block::Sand,
    Block::Leaves,
    Block::Snow,
    Block::Clay,
    Block::IronOre,
];
pub const PAGE_COUNT: usize = MATERIALS.len().div_ceil(QUICK_SLOTS);

pub fn page(selected: usize) -> usize {
    selected.min(MATERIALS.len() - 1) / QUICK_SLOTS
}

/// Changing pages also selects its first block, keeping the build material
/// visible and highlighted in the six quick slots.
pub fn next_page(selected: usize) -> usize {
    ((page(selected) + 1) % PAGE_COUNT) * QUICK_SLOTS
}

pub fn index_for_slot(selected: usize, slot: usize) -> Option<usize> {
    if slot >= QUICK_SLOTS {
        return None;
    }
    let index = page(selected) * QUICK_SLOTS + slot;
    (index < MATERIALS.len()).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn both_pages_cover_every_existing_solid_and_preserve_the_original_quick_keys() {
        assert_eq!(
            &MATERIALS[..QUICK_SLOTS],
            &[
                Block::Grass,
                Block::Dirt,
                Block::Stone,
                Block::Wood,
                Block::Brick,
                Block::Glass,
            ]
        );
        let mut reachable = HashSet::new();
        let mut selected = 0;
        for _ in 0..PAGE_COUNT {
            for slot in 0..QUICK_SLOTS {
                if let Some(index) = index_for_slot(selected, slot) {
                    assert!(MATERIALS[index].is_solid());
                    assert!(reachable.insert(MATERIALS[index]));
                }
            }
            selected = next_page(selected);
        }
        assert_eq!(selected, 0);
        assert_eq!(
            reachable,
            HashSet::from([
                Block::Grass,
                Block::Dirt,
                Block::Stone,
                Block::Sand,
                Block::Wood,
                Block::Leaves,
                Block::Brick,
                Block::Glass,
                Block::Snow,
                Block::Clay,
                Block::IronOre,
            ])
        );
        assert!(!reachable.contains(&Block::Air));
    }

    #[test]
    fn short_second_page_has_no_sixth_selection_and_never_indexes_past_the_catalog() {
        let second = next_page(5);
        assert_eq!(MATERIALS[second], Block::Sand);
        assert_eq!(
            index_for_slot(second, 4).map(|index| MATERIALS[index]),
            Some(Block::IronOre)
        );
        assert_eq!(index_for_slot(second, 5), None);
        assert_eq!(index_for_slot(second, usize::MAX), None);
        assert_eq!(next_page(10), 0);
        assert!(next_page(usize::MAX) < MATERIALS.len());
    }
}
