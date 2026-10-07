//! Regional ground plants share the terrain batch and have no collision.
use bevy::prelude::*;
use rubblekin_core::geography::Biome;

use crate::terrain::Geometry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Detail {
    Mushroom,
    Fern,
    DryShrub,
    Cushion,
}

/// The caller already checked density, supporting grass and empty space.
/// A detail replaces one existing grass tuft, keeping the same cell budget.
pub(crate) fn detail(biome: Option<Biome>, chance: f32) -> Option<Detail> {
    match biome {
        Some(Biome::Forest | Biome::Rainforest) if chance < 0.005 => Some(Detail::Mushroom),
        Some(Biome::Forest | Biome::Rainforest) if chance < 0.024 => Some(Detail::Fern),
        Some(Biome::PineForest) if chance < 0.003 => Some(Detail::Mushroom),
        Some(Biome::PineForest) if chance < 0.008 => Some(Detail::Fern),
        Some(Biome::Shrubland) => Some(Detail::DryShrub),
        Some(Biome::Tundra) if chance < 0.004 => Some(Detail::Cushion),
        _ => None,
    }
}

pub(crate) fn add(mesh: &mut Geometry, base: Vec3, kind: Detail) {
    // Every plant uses three cuboids, equal to an ordinary three-stem tuft.
    let parts = match kind {
        Detail::Mushroom => [
            ([0., 0.09, 0.], [0.055, 0.18, 0.055], [0.76, 0.72, 0.59, 1.]),
            ([0., 0.20, 0.], [0.23, 0.08, 0.23], [0.57, 0.29, 0.25, 1.]),
            (
                [0.03, 0.244, -0.02],
                [0.065, 0.012, 0.045],
                [0.86, 0.82, 0.67, 1.],
            ),
        ],
        Detail::Fern => [
            ([0., 0.10, 0.], [0.025, 0.20, 0.025], [0.24, 0.37, 0.19, 1.]),
            ([0., 0.15, 0.], [0.36, 0.045, 0.11], [0.32, 0.49, 0.26, 1.]),
            ([0., 0.22, 0.], [0.11, 0.045, 0.32], [0.39, 0.56, 0.29, 1.]),
        ],
        Detail::DryShrub => [
            ([0., 0.10, 0.], [0.035, 0.20, 0.035], [0.42, 0.34, 0.22, 1.]),
            (
                [-0.04, 0.23, 0.],
                [0.24, 0.16, 0.14],
                [0.53, 0.52, 0.30, 1.],
            ),
            (
                [0.06, 0.18, 0.04],
                [0.18, 0.14, 0.23],
                [0.60, 0.57, 0.33, 1.],
            ),
        ],
        Detail::Cushion => [
            ([0., 0.045, 0.], [0.30, 0.09, 0.24], [0.44, 0.52, 0.36, 1.]),
            (
                [0.055, 0.11, 0.01],
                [0.065, 0.075, 0.065],
                [0.82, 0.81, 0.67, 1.],
            ),
            (
                [-0.06, 0.09, -0.03],
                [0.05, 0.055, 0.05],
                [0.75, 0.73, 0.61, 1.],
            ),
        ],
    };
    for (offset, size, color) in parts {
        mesh.cuboid(
            base + Vec3::from_array(offset),
            Vec3::from_array(size),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn regional_details_stay_inside_their_supported_cell_with_the_existing_tuft_budget() {
        for kind in [
            Detail::Mushroom,
            Detail::Fern,
            Detail::DryShrub,
            Detail::Cushion,
        ] {
            let base = Vec3::new(-17.25, 42., 21.75);
            let mut geometry = Geometry::default();
            add(&mut geometry, base, kind);
            let mesh = geometry.into_mesh();
            assert_eq!(mesh.count_vertices(), 72);
            assert_eq!(mesh.indices().unwrap().len(), 108);
            let VertexAttributeValues::Float32x3(points) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
            else {
                panic!("positions");
            };
            assert!(points.iter().all(|point| {
                let p = Vec3::from_array(*point) - base;
                p.is_finite()
                    && p.x.abs() <= 0.25
                    && p.z.abs() <= 0.25
                    && (0.0..=0.32).contains(&p.y)
            }));
        }
        assert_eq!(
            detail(Some(Biome::Rainforest), 0.001),
            Some(Detail::Mushroom)
        );
        assert_eq!(detail(Some(Biome::Forest), 0.01), Some(Detail::Fern));
        assert_eq!(
            detail(Some(Biome::Shrubland), 0.009),
            Some(Detail::DryShrub)
        );
        assert_eq!(detail(Some(Biome::Tundra), 0.003), Some(Detail::Cushion));
        assert_eq!(detail(Some(Biome::PineForest), 0.01), None);
        assert_eq!(detail(Some(Biome::Grassland), 0.001), None);
        assert_eq!(detail(Some(Biome::Desert), 0.001), None);
    }
}
