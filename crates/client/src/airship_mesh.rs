//! One shared exterior mesh gives every airship a stepped voxel envelope.
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use std::{collections::HashSet, sync::OnceLock};

struct Envelope {
    cells: Vec<[i32; 3]>,
    occupied: HashSet<[i32; 3]>,
    boxes: Vec<(Vec3, Vec3)>,
}
fn envelope() -> &'static Envelope {
    static SHAPE: OnceLock<Envelope> = OnceLock::new();
    SHAPE.get_or_init(|| {
        let mut cells = Vec::new();
        let mut boxes = Vec::new();
        for y in 5..14 {
            for z in -11..11 {
                let mut first = None;
                let mut last = 0;
                for x in -6..6 {
                    let p = (Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)
                        - Vec3::new(0.0, 9.4, 0.0))
                        / Vec3::new(5.4, 4.0, 11.0);
                    if p.length_squared() <= 1.0 {
                        cells.push([x, y, z]);
                        first.get_or_insert(x);
                        last = x + 1;
                    }
                }
                if let Some(first) = first {
                    boxes.push((
                        Vec3::new(first as f32, y as f32, z as f32),
                        Vec3::new(last as f32, y as f32 + 1.0, z as f32 + 1.0),
                    ));
                }
            }
        }
        Envelope {
            occupied: cells.iter().copied().collect(),
            cells,
            boxes,
        }
    })
}

pub(super) fn camera_boxes() -> &'static [(Vec3, Vec3)] {
    &envelope().boxes
}

pub(super) fn mesh() -> Mesh {
    let shape = envelope();
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for cell in &shape.cells {
        for (normal, corners) in FACES {
            let neighbor = std::array::from_fn(|axis| cell[axis] + normal[axis]);
            if shape.occupied.contains(&neighbor) {
                continue;
            }
            let base = positions.len() as u32;
            let shade = if normal[1] > 0 {
                1.0
            } else if normal[1] < 0 {
                0.63
            } else {
                0.82
            };
            let stripe = matches!(cell[2], -7 | 6);
            let tint = if stripe {
                [0.92 * shade, 0.64 * shade, 0.28 * shade, 1.0]
            } else {
                [shade; 4]
            };
            for corner in corners {
                positions.push(std::array::from_fn(|axis| cell[axis] as f32 + corner[axis]));
                normals.push(normal.map(|v| v as f32));
                colors.push([tint[0], tint[1], tint[2], 1.0]);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let uvs = vec![[0.0_f32; 2]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

const FACES: [([i32; 3], [[f32; 3]; 4]); 6] = [
    (
        [1, 0, 0],
        [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
    ),
    (
        [-1, 0, 0],
        [[0., 0., 1.], [0., 1., 1.], [0., 1., 0.], [0., 0., 0.]],
    ),
    (
        [0, 1, 0],
        [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
    ),
    (
        [0, -1, 0],
        [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
    ),
    (
        [0, 0, 1],
        [[1., 0., 1.], [1., 1., 1.], [0., 1., 1.], [0., 0., 1.]],
    ),
    (
        [0, 0, -1],
        [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    #[test]
    fn voxel_envelope_has_only_exterior_faces_and_bounded_axis_aligned_geometry() {
        let mesh = mesh();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!()
        };
        let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
        assert!(indices.len() / 3 < 5000);
        assert!(positions.iter().flatten().all(|v| v.fract() == 0.0));
        let mut top_widths = HashSet::new();
        for (min, max) in camera_boxes() {
            top_widths.insert((max.x - min.x) as i32);
        }
        assert!(top_widths.len() > 3);
        for triangle in indices.as_chunks::<3>().0 {
            let a = Vec3::from_array(positions[triangle[0]]);
            let b = Vec3::from_array(positions[triangle[1]]);
            let c = Vec3::from_array(positions[triangle[2]]);
            assert!(
                (b - a)
                    .cross(c - a)
                    .dot(Vec3::from_array(normals[triangle[0]]))
                    > 0.0
            );
        }
    }
}
