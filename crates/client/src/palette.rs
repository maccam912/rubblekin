//! Unlimited creative building slots, separate from the player's traded cargo.
use rubblekin_core::world::Block;
pub const QUICK_SLOTS: usize = 6;
pub const MATERIALS: &[Block] = Block::ALL;
pub const DEFAULT_HOTBAR: [Block; QUICK_SLOTS] = [
    Block::Grass,
    Block::Dirt,
    Block::Stone,
    Block::Wood,
    Block::Brick,
    Block::Glass,
];
const FILE: &str = "creative-hotbar.json";
pub fn load() -> [Block; QUICK_SLOTS] {
    load_from(std::path::Path::new(FILE))
}
fn load_from(path: &std::path::Path) -> [Block; QUICK_SLOTS] {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() < 4096)
        .and_then(|bytes| serde_json::from_slice::<[Block; QUICK_SLOTS]>(&bytes).ok())
        .filter(|slots| slots.iter().all(|b| *b != Block::Air))
        .unwrap_or(DEFAULT_HOTBAR)
}
pub fn save(slots: &[Block; QUICK_SLOTS]) -> std::io::Result<()> {
    save_to(std::path::Path::new(FILE), slots)
}
fn save_to(path: &std::path::Path, slots: &[Block; QUICK_SLOTS]) -> std::io::Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec(slots)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edited_slots_survive_restart_and_invalid_slots_fall_back() {
        let directory =
            std::env::temp_dir().join(format!("rubblekin-hotbar-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(FILE);
        assert_eq!(load_from(&path), DEFAULT_HOTBAR);
        let mut slots = DEFAULT_HOTBAR;
        slots[2] = Block::PurpleWool;
        slots[5] = Block::OakPlanks;
        slots[0] = Block::Torch;
        save_to(&path, &slots).unwrap();
        assert_eq!(load_from(&path), slots);
        slots[0] = Block::Air;
        save_to(&path, &slots).unwrap();
        assert_eq!(load_from(&path), DEFAULT_HOTBAR);
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(load_from(&path), DEFAULT_HOTBAR);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
