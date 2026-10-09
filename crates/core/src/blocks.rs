//! The creative block library. Stable variant names are also the save/wire IDs.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCategory {
    Natural,
    Stone,
    Masonry,
    Wood,
    Metal,
    Concrete,
    Wool,
    Tile,
    Decor,
}
impl BlockCategory {
    pub const ALL: [Self; 9] = [
        Self::Natural,
        Self::Stone,
        Self::Masonry,
        Self::Wood,
        Self::Metal,
        Self::Concrete,
        Self::Wool,
        Self::Tile,
        Self::Decor,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Natural => "Nature",
            Self::Stone => "Stone",
            Self::Masonry => "Masonry",
            Self::Wood => "Wood",
            Self::Metal => "Metals",
            Self::Concrete => "Colors",
            Self::Wool => "Wool",
            Self::Tile => "Tiles",
            Self::Decor => "Decor",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTexture {
    Grass,
    Earth,
    Rock,
    Sand,
    Bark,
    Leaves,
    Bricks,
    Smooth,
    Snow,
    Ore,
    Veins,
    Tiles,
    Planks,
    Parquet,
    Metal,
    Cloth,
}

macro_rules! blocks {
    ($( $id:ident, $name:literal, [$r:literal, $g:literal, $b:literal], $category:ident, $texture:ident; )*) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Block { #[default] Air, $( $id, )* }
        impl Block {
            pub const ALL: &'static [Self] = &[$(Self::$id,)*];
            pub fn is_solid(self) -> bool { self != Self::Air }
            /// Base tint in sRGB; the renderer adds a material-specific pattern.
            pub fn color(self) -> [f32; 4] { match self { Self::Air => [0.; 4], $(Self::$id => [$r, $g, $b, 1.],)* }}
            pub fn name(self) -> &'static str { match self { Self::Air => "Air", $(Self::$id => $name,)* }}
            pub fn category(self) -> BlockCategory { match self { Self::Air => BlockCategory::Natural, $(Self::$id => BlockCategory::$category,)* }}
            pub fn texture(self) -> BlockTexture { match self { Self::Air => BlockTexture::Smooth, $(Self::$id => BlockTexture::$texture,)* }}
            pub fn catalog_index(self) -> Option<usize> { Self::ALL.iter().position(|block| *block == self) }
        }
    }
}
blocks! {
    Grass, "Grass", [0.40, 0.57, 0.28], Natural, Grass;
    Dirt, "Earth", [0.43, 0.31, 0.20], Natural, Earth;
    Stone, "Stone", [0.54, 0.57, 0.58], Stone, Rock;
    Sand, "Sand", [0.75, 0.68, 0.48], Natural, Sand;
    Wood, "Wood", [0.43, 0.29, 0.16], Wood, Bark;
    Leaves, "Leaves", [0.25, 0.43, 0.23], Natural, Leaves;
    Brick, "Brick", [0.62, 0.32, 0.23], Masonry, Bricks;
    Glass, "Glass", [0.57, 0.78, 0.83], Decor, Smooth;
    Snow, "Snow", [0.87, 0.91, 0.94], Natural, Snow;
    Clay, "Clay", [0.64, 0.47, 0.34], Natural, Earth;
    IronOre, "Iron ore", [0.45, 0.34, 0.29], Metal, Ore;
    Gravel, "Gravel", [0.49, 0.47, 0.43], Natural, Earth;
    Mud, "Mud", [0.28, 0.23, 0.16], Natural, Earth;
    RichSoil, "Rich soil", [0.24, 0.17, 0.12], Natural, Earth;
    RedSand, "Red sand", [0.74, 0.39, 0.22], Natural, Earth;
    Peat, "Peat", [0.21, 0.20, 0.15], Natural, Earth;
    Moss, "Moss", [0.30, 0.42, 0.16], Natural, Leaves;
    DryGrass, "Dry grass", [0.65, 0.59, 0.29], Natural, Earth;
    PineLeaves, "Pine leaves", [0.14, 0.30, 0.23], Natural, Leaves;
    AutumnLeaves, "Autumn leaves", [0.83, 0.39, 0.12], Natural, Leaves;
    GoldenLeaves, "Golden leaves", [0.79, 0.65, 0.18], Natural, Leaves;
    CherryLeaves, "Cherry leaves", [0.90, 0.53, 0.63], Natural, Leaves;
    Granite, "Granite", [0.68, 0.53, 0.49], Stone, Rock;
    PolishedGranite, "Polished granite", [0.68, 0.53, 0.49], Stone, Veins;
    GraniteBricks, "Granite bricks", [0.68, 0.53, 0.49], Masonry, Bricks;
    Basalt, "Basalt", [0.24, 0.27, 0.29], Stone, Rock;
    PolishedBasalt, "Polished basalt", [0.24, 0.27, 0.29], Stone, Veins;
    BasaltBricks, "Basalt bricks", [0.24, 0.27, 0.29], Masonry, Bricks;
    Limestone, "Limestone", [0.76, 0.74, 0.60], Stone, Rock;
    PolishedLimestone, "Polished limestone", [0.76, 0.74, 0.60], Stone, Veins;
    LimestoneBricks, "Limestone bricks", [0.76, 0.74, 0.60], Masonry, Bricks;
    Sandstone, "Sandstone", [0.78, 0.65, 0.43], Stone, Rock;
    PolishedSandstone, "Polished sandstone", [0.78, 0.65, 0.43], Stone, Veins;
    SandstoneBricks, "Sandstone bricks", [0.78, 0.65, 0.43], Masonry, Bricks;
    Slate, "Slate", [0.34, 0.41, 0.46], Stone, Rock;
    PolishedSlate, "Polished slate", [0.34, 0.41, 0.46], Stone, Veins;
    SlateBricks, "Slate bricks", [0.34, 0.41, 0.46], Masonry, Bricks;
    Marble, "Marble", [0.88, 0.86, 0.81], Stone, Rock;
    PolishedMarble, "Polished marble", [0.88, 0.86, 0.81], Stone, Veins;
    MarbleBricks, "Marble bricks", [0.88, 0.86, 0.81], Masonry, Bricks;
    Obsidian, "Obsidian", [0.16, 0.12, 0.22], Stone, Rock;
    PolishedObsidian, "Polished obsidian", [0.16, 0.12, 0.22], Stone, Veins;
    ObsidianBricks, "Obsidian bricks", [0.16, 0.12, 0.22], Masonry, Bricks;
    Quartz, "Quartz", [0.91, 0.88, 0.84], Stone, Rock;
    PolishedQuartz, "Polished quartz", [0.91, 0.88, 0.84], Stone, Veins;
    QuartzBricks, "Quartz bricks", [0.91, 0.88, 0.84], Masonry, Bricks;
    RedSandstone, "Red sandstone", [0.73, 0.40, 0.27], Stone, Rock;
    PolishedRedSandstone, "Polished red sandstone", [0.73, 0.40, 0.27], Stone, Veins;
    RedSandstoneBricks, "Red sandstone bricks", [0.73, 0.40, 0.27], Masonry, Bricks;
    Bluestone, "Bluestone", [0.36, 0.48, 0.57], Stone, Rock;
    PolishedBluestone, "Polished bluestone", [0.36, 0.48, 0.57], Stone, Veins;
    BluestoneBricks, "Bluestone bricks", [0.36, 0.48, 0.57], Masonry, Bricks;
    Cobblestone, "Cobblestone", [0.49, 0.51, 0.48], Masonry, Rock;
    MossyCobblestone, "Mossy cobblestone", [0.39, 0.46, 0.32], Masonry, Rock;
    StoneBricks, "Stone bricks", [0.51, 0.54, 0.54], Masonry, Bricks;
    MossyStoneBricks, "Mossy stone bricks", [0.40, 0.48, 0.35], Masonry, Bricks;
    CrackedStoneBricks, "Cracked stone bricks", [0.45, 0.46, 0.43], Masonry, Bricks;
    DarkBricks, "Dark bricks", [0.32, 0.22, 0.20], Masonry, Bricks;
    CreamBricks, "Cream bricks", [0.83, 0.74, 0.57], Masonry, Bricks;
    Terracotta, "Terracotta", [0.76, 0.40, 0.27], Masonry, Bricks;
    RoofTiles, "Red roof tiles", [0.59, 0.24, 0.16], Masonry, Tiles;
    BlueRoofTiles, "Blue roof tiles", [0.25, 0.37, 0.49], Masonry, Tiles;
    GreenRoofTiles, "Green roof tiles", [0.29, 0.43, 0.33], Masonry, Tiles;
    OakLog, "Oak log", [0.62, 0.44, 0.25], Wood, Bark;
    OakPlanks, "Oak planks", [0.62, 0.44, 0.25], Wood, Planks;
    OakParquet, "Oak parquet", [0.62, 0.44, 0.25], Wood, Parquet;
    PineLog, "Pine log", [0.72, 0.56, 0.33], Wood, Bark;
    PinePlanks, "Pine planks", [0.72, 0.56, 0.33], Wood, Planks;
    PineParquet, "Pine parquet", [0.72, 0.56, 0.33], Wood, Parquet;
    BirchLog, "Birch log", [0.84, 0.77, 0.58], Wood, Bark;
    BirchPlanks, "Birch planks", [0.84, 0.77, 0.58], Wood, Planks;
    BirchParquet, "Birch parquet", [0.84, 0.77, 0.58], Wood, Parquet;
    CedarLog, "Cedar log", [0.52, 0.29, 0.19], Wood, Bark;
    CedarPlanks, "Cedar planks", [0.52, 0.29, 0.19], Wood, Planks;
    CedarParquet, "Cedar parquet", [0.52, 0.29, 0.19], Wood, Parquet;
    WalnutLog, "Walnut log", [0.31, 0.20, 0.15], Wood, Bark;
    WalnutPlanks, "Walnut planks", [0.31, 0.20, 0.15], Wood, Planks;
    WalnutParquet, "Walnut parquet", [0.31, 0.20, 0.15], Wood, Parquet;
    CherryLog, "Cherry log", [0.67, 0.36, 0.28], Wood, Bark;
    CherryPlanks, "Cherry planks", [0.67, 0.36, 0.28], Wood, Planks;
    CherryParquet, "Cherry parquet", [0.67, 0.36, 0.28], Wood, Parquet;
    AshLog, "Ash log", [0.73, 0.67, 0.52], Wood, Bark;
    AshPlanks, "Ash planks", [0.73, 0.67, 0.52], Wood, Planks;
    AshParquet, "Ash parquet", [0.73, 0.67, 0.52], Wood, Parquet;
    EbonyLog, "Ebony log", [0.20, 0.19, 0.21], Wood, Bark;
    EbonyPlanks, "Ebony planks", [0.20, 0.19, 0.21], Wood, Planks;
    EbonyParquet, "Ebony parquet", [0.20, 0.19, 0.21], Wood, Parquet;
    Iron, "Iron", [0.68, 0.72, 0.73], Metal, Metal;
    Copper, "Copper", [0.76, 0.43, 0.27], Metal, Metal;
    Bronze, "Bronze", [0.61, 0.45, 0.23], Metal, Metal;
    Gold, "Gold", [0.92, 0.72, 0.24], Metal, Metal;
    Silver, "Silver", [0.82, 0.86, 0.89], Metal, Metal;
    Steel, "Steel", [0.43, 0.49, 0.54], Metal, Metal;
    RustedIron, "Rusted iron", [0.57, 0.30, 0.18], Metal, Metal;
    PatinaCopper, "Patina copper", [0.25, 0.61, 0.49], Metal, Metal;
    CoalOre, "Coal ore", [0.35, 0.37, 0.38], Metal, Ore;
    CopperOre, "Copper ore", [0.59, 0.45, 0.34], Metal, Ore;
    GoldOre, "Gold ore", [0.63, 0.56, 0.34], Metal, Ore;
    Crystal, "Violet crystal", [0.58, 0.40, 0.77], Decor, Veins;
    WhiteConcrete, "White concrete", [0.91, 0.91, 0.87], Concrete, Smooth;
    CreamConcrete, "Cream concrete", [0.91, 0.83, 0.65], Concrete, Smooth;
    YellowConcrete, "Yellow concrete", [0.94, 0.76, 0.20], Concrete, Smooth;
    OrangeConcrete, "Orange concrete", [0.92, 0.46, 0.16], Concrete, Smooth;
    RedConcrete, "Red concrete", [0.72, 0.20, 0.18], Concrete, Smooth;
    RoseConcrete, "Rose concrete", [0.87, 0.39, 0.48], Concrete, Smooth;
    PinkConcrete, "Pink concrete", [0.94, 0.64, 0.71], Concrete, Smooth;
    MagentaConcrete, "Magenta concrete", [0.69, 0.25, 0.60], Concrete, Smooth;
    PurpleConcrete, "Purple concrete", [0.46, 0.28, 0.65], Concrete, Smooth;
    BlueConcrete, "Blue concrete", [0.23, 0.39, 0.72], Concrete, Smooth;
    SkyConcrete, "Sky concrete", [0.39, 0.68, 0.86], Concrete, Smooth;
    CyanConcrete, "Cyan concrete", [0.20, 0.68, 0.70], Concrete, Smooth;
    TealConcrete, "Teal concrete", [0.16, 0.46, 0.45], Concrete, Smooth;
    GreenConcrete, "Green concrete", [0.30, 0.56, 0.26], Concrete, Smooth;
    LimeConcrete, "Lime concrete", [0.61, 0.76, 0.25], Concrete, Smooth;
    BrownConcrete, "Brown concrete", [0.42, 0.29, 0.22], Concrete, Smooth;
    GrayConcrete, "Gray concrete", [0.48, 0.51, 0.53], Concrete, Smooth;
    BlackConcrete, "Black concrete", [0.14, 0.16, 0.18], Concrete, Smooth;
    WhiteWool, "White wool", [0.91, 0.91, 0.87], Wool, Cloth;
    CreamWool, "Cream wool", [0.91, 0.83, 0.65], Wool, Cloth;
    YellowWool, "Yellow wool", [0.94, 0.76, 0.20], Wool, Cloth;
    OrangeWool, "Orange wool", [0.92, 0.46, 0.16], Wool, Cloth;
    RedWool, "Red wool", [0.72, 0.20, 0.18], Wool, Cloth;
    RoseWool, "Rose wool", [0.87, 0.39, 0.48], Wool, Cloth;
    PinkWool, "Pink wool", [0.94, 0.64, 0.71], Wool, Cloth;
    MagentaWool, "Magenta wool", [0.69, 0.25, 0.60], Wool, Cloth;
    PurpleWool, "Purple wool", [0.46, 0.28, 0.65], Wool, Cloth;
    BlueWool, "Blue wool", [0.23, 0.39, 0.72], Wool, Cloth;
    SkyWool, "Sky wool", [0.39, 0.68, 0.86], Wool, Cloth;
    CyanWool, "Cyan wool", [0.20, 0.68, 0.70], Wool, Cloth;
    TealWool, "Teal wool", [0.16, 0.46, 0.45], Wool, Cloth;
    GreenWool, "Green wool", [0.30, 0.56, 0.26], Wool, Cloth;
    LimeWool, "Lime wool", [0.61, 0.76, 0.25], Wool, Cloth;
    BrownWool, "Brown wool", [0.42, 0.29, 0.22], Wool, Cloth;
    GrayWool, "Gray wool", [0.48, 0.51, 0.53], Wool, Cloth;
    BlackWool, "Black wool", [0.14, 0.16, 0.18], Wool, Cloth;
    WhiteTile, "White tile", [0.91, 0.91, 0.87], Tile, Tiles;
    CreamTile, "Cream tile", [0.91, 0.83, 0.65], Tile, Tiles;
    YellowTile, "Yellow tile", [0.94, 0.76, 0.20], Tile, Tiles;
    OrangeTile, "Orange tile", [0.92, 0.46, 0.16], Tile, Tiles;
    RedTile, "Red tile", [0.72, 0.20, 0.18], Tile, Tiles;
    RoseTile, "Rose tile", [0.87, 0.39, 0.48], Tile, Tiles;
    PinkTile, "Pink tile", [0.94, 0.64, 0.71], Tile, Tiles;
    MagentaTile, "Magenta tile", [0.69, 0.25, 0.60], Tile, Tiles;
    PurpleTile, "Purple tile", [0.46, 0.28, 0.65], Tile, Tiles;
    BlueTile, "Blue tile", [0.23, 0.39, 0.72], Tile, Tiles;
    SkyTile, "Sky tile", [0.39, 0.68, 0.86], Tile, Tiles;
    CyanTile, "Cyan tile", [0.20, 0.68, 0.70], Tile, Tiles;
    TealTile, "Teal tile", [0.16, 0.46, 0.45], Tile, Tiles;
    GreenTile, "Green tile", [0.30, 0.56, 0.26], Tile, Tiles;
    LimeTile, "Lime tile", [0.61, 0.76, 0.25], Tile, Tiles;
    BrownTile, "Brown tile", [0.42, 0.29, 0.22], Tile, Tiles;
    GrayTile, "Gray tile", [0.48, 0.51, 0.53], Tile, Tiles;
    BlackTile, "Black tile", [0.14, 0.16, 0.18], Tile, Tiles;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_catalog_entry_has_a_unique_stable_id_and_valid_metadata() {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        assert!(Block::ALL.len() >= 150);
        for &block in Block::ALL {
            assert!(block.is_solid());
            let id = serde_json::to_string(&block).unwrap();
            assert!(ids.insert(id.clone()));
            assert!(names.insert(block.name()));
            assert_eq!(serde_json::from_str::<Block>(&id).unwrap(), block);
            assert!(
                block
                    .color()
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            );
        }
        for category in BlockCategory::ALL {
            assert!(Block::ALL.iter().any(|b| b.category() == category));
        }
        assert_eq!(
            serde_json::from_str::<Block>("\"IronOre\"").unwrap(),
            Block::IronOre
        );
    }
}
