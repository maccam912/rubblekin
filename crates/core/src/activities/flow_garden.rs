//! The first authored flow garden: two elbows and a straight channel.
//! These are local prop connections; they do not alter terrain hydrology.

/// Local grid, with north along negative Z. Adjacent centers are one tile apart.
pub const CENTERS: [[i32; 2]; 3] = [[0, 0], [0, -1], [1, -1]];
pub const SOURCE: [i32; 2] = [-1, 0];
pub const BED: [i32; 2] = [2, -1];
pub const START_FACES: [u8; 3] = [0, 0, 0];
pub const SOLVED_FACES: [u8; 3] = [3, 1, 1];
pub const TILE: f32 = 1.;
/// Keep the small prop meshes clear of voxel walls and adjacent higher ground.
/// Walking-point clearance alone can hide a spring behind a causeway pier.
pub fn props_clear(
    world: &crate::world::World,
    centers: [[f32; 3]; 3],
    anchors: [[f32; 3]; 3],
) -> bool {
    use crate::world::{BlockPos, CELL_SIZE};
    let volume = |p: [f32; 3], radius: f32, height: f32| {
        let min = [
            (p[0] - radius) / CELL_SIZE,
            (p[1] + 0.03) / CELL_SIZE,
            (p[2] - radius) / CELL_SIZE,
        ]
        .map(|n| n.floor() as i32);
        let max = [
            (p[0] + radius) / CELL_SIZE,
            (p[1] + height) / CELL_SIZE,
            (p[2] + radius) / CELL_SIZE,
        ]
        .map(|n| n.floor() as i32);
        (min[0]..=max[0]).all(|x| {
            (min[1]..=max[1])
                .all(|y| (min[2]..=max[2]).all(|z| !world.block(BlockPos::new(x, y, z)).is_solid()))
        })
    };
    centers.into_iter().all(|p| volume(p, 0.82, 0.65))
        && volume(anchors[0], 0.9, 1.1)
        && volume(anchors[1], 0.82, 1.1)
        && super::can_interact(world, anchors[2], anchors[0])
}

/// Accept the authored right-angle network at any horizontal rotation, with a
/// level source and bed. Terrain fitting must not distort the connections.
pub fn valid_layout(centers: [[f32; 3]; 3], anchors: [[f32; 3]; 3]) -> bool {
    if centers
        .iter()
        .chain(&anchors)
        .flatten()
        .any(|v| !v.is_finite())
    {
        return false;
    }
    let x: [f32; 3] = std::array::from_fn(|i| centers[2][i] - centers[1][i]);
    let z: [f32; 3] = std::array::from_fn(|i| centers[0][i] - centers[1][i]);
    let length = |v: [f32; 3]| v.into_iter().map(|n| n * n).sum::<f32>().sqrt();
    let close = |a: [f32; 3], b: [f32; 3]| length(std::array::from_fn(|i| a[i] - b[i])) < 0.05;
    (length(x) - TILE).abs() < 0.05
        && (length(z) - TILE).abs() < 0.05
        && x.into_iter().zip(z).map(|(a, b)| a * b).sum::<f32>().abs() < 0.05
        && (x[0] * z[2] - x[2] * z[0] - TILE * TILE).abs() < 0.1
        && centers
            .iter()
            .chain(&anchors)
            .all(|p| (p[1] - centers[0][1]).abs() < 0.05)
        && close(anchors[0], std::array::from_fn(|i| centers[0][i] - x[i]))
        && close(anchors[1], std::array::from_fn(|i| centers[2][i] + x[i]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    East,
    South,
    West,
}
impl Direction {
    pub fn offset(self) -> [i32; 2] {
        match self {
            Self::North => [0, -1],
            Self::East => [1, 0],
            Self::South => [0, 1],
            Self::West => [-1, 0],
        }
    }
    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }
    fn from_face(face: u8) -> Self {
        match face % 4 {
            0 => Self::North,
            1 => Self::East,
            2 => Self::South,
            _ => Self::West,
        }
    }
}
/// Visible open ends, including the straight channel's equivalent half turn.
pub fn ports(piece: usize, face: u8) -> Option<[Direction; 2]> {
    if piece >= CENTERS.len() || face >= 4 {
        return None;
    }
    let separation = if piece == 2 { 2 } else { 1 };
    Some([
        Direction::from_face(face),
        Direction::from_face(face + separation),
    ])
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spill {
    Source,
    Channel { piece: usize, edge: Direction },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    /// Only pieces reached from the real source receive water.
    pub wet: [bool; 3],
    pub spill: Option<Spill>,
    pub garden_watered: bool,
}
/// Trace the three actual port connections. Matching an answer array is not the
/// completion rule: both east/west straight orientations work.
pub fn flow(faces: [u8; 3]) -> Option<Flow> {
    if faces.iter().any(|face| *face >= 4) {
        return None;
    }
    let mut flow = Flow {
        wet: [false; 3],
        spill: Some(Spill::Source),
        garden_watered: false,
    };
    let mut piece = 0;
    let mut incoming = Direction::West;
    for _ in 0..CENTERS.len() {
        let ends = ports(piece, faces[piece])?;
        if flow.wet[piece] || !ends.contains(&incoming) {
            return Some(flow);
        }
        flow.wet[piece] = true;
        let outgoing = *ends.iter().find(|edge| **edge != incoming)?;
        flow.spill = Some(Spill::Channel {
            piece,
            edge: outgoing,
        });
        let center = CENTERS[piece];
        let offset = outgoing.offset();
        let next = [center[0] + offset[0], center[1] + offset[1]];
        if next == BED {
            flow.garden_watered = true;
            flow.spill = None;
            return Some(flow);
        }
        let Some(next_piece) = CENTERS.iter().position(|center| *center == next) else {
            return Some(flow);
        };
        piece = next_piece;
        incoming = outgoing.opposite();
    }
    Some(flow)
}
/// Point at the blocked inlet, or the wet channel spilling in the wrong
/// direction. The first dry piece alone is not always the piece to turn.
pub fn next_piece(faces: [u8; 3]) -> Option<usize> {
    let flow = flow(faces)?;
    match flow.spill? {
        Spill::Source => Some(0),
        Spill::Channel { piece, edge } => {
            let center = CENTERS[piece];
            let offset = edge.offset();
            let next = [center[0] + offset[0], center[1] + offset[1]];
            Some(
                CENTERS
                    .iter()
                    .position(|p| *p == next)
                    .filter(|i| !flow.wet[*i])
                    .unwrap_or(piece),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_follows_connected_ports_and_stops_at_each_real_break() {
        assert_eq!(
            flow(START_FACES),
            Some(Flow {
                wet: [false; 3],
                spill: Some(Spill::Source),
                garden_watered: false,
            })
        );
        assert_eq!(
            flow([3, 0, 0]),
            Some(Flow {
                wet: [true, false, false],
                spill: Some(Spill::Channel {
                    piece: 0,
                    edge: Direction::North,
                }),
                garden_watered: false,
            })
        );
        assert_eq!(
            flow([3, 1, 0]),
            Some(Flow {
                wet: [true, true, false],
                spill: Some(Spill::Channel {
                    piece: 1,
                    edge: Direction::East,
                }),
                garden_watered: false,
            })
        );
        assert_eq!(
            flow(SOLVED_FACES),
            Some(Flow {
                wet: [true; 3],
                spill: None,
                garden_watered: true,
            })
        );
        assert_eq!(flow([3, 1, 3]), flow(SOLVED_FACES));
        assert_eq!(flow([3, 1, 2]).unwrap().wet, [true, true, false]);
    }
    #[test]
    fn every_orientation_is_reversible_and_only_connected_solutions_water_the_bed() {
        let mut solutions = Vec::new();
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    let faces = [a, b, c];
                    let water = flow(faces).unwrap();
                    if water.garden_watered {
                        solutions.push(faces);
                        assert!(water.wet.iter().all(|wet| *wet));
                        assert!(water.spill.is_none());
                    } else {
                        assert!(water.spill.is_some());
                    }
                    assert!(!water.wet[2] || water.wet[1]);
                    assert!(!water.wet[1] || water.wet[0]);
                    for piece in 0..3 {
                        let mut turned = faces;
                        for _ in 0..4 {
                            turned[piece] = (turned[piece] + 1) % 4;
                            assert!(flow(turned).is_some());
                        }
                        assert_eq!(flow(turned), Some(water));
                    }
                }
            }
        }
        assert_eq!(solutions, [[3, 1, 1], [3, 1, 3]]);
        // Disconnecting the inlet after solving removes flow immediately.
        assert_eq!(flow([0, 1, 1]).unwrap().wet, [false; 3]);
    }
    #[test]
    fn malformed_faces_and_missing_pieces_are_rejected() {
        for piece in 0..3 {
            for invalid in [4, 255] {
                let mut faces = SOLVED_FACES;
                faces[piece] = invalid;
                assert!(flow(faces).is_none());
                assert!(ports(piece, invalid).is_none());
            }
        }
        assert!(ports(3, 0).is_none());
        assert_eq!(ports(2, 1), Some([Direction::East, Direction::West]));
        assert_eq!(ports(2, 3), Some([Direction::West, Direction::East]));
    }
    #[test]
    fn hints_follow_the_actual_break_including_a_wet_wrong_outlet() {
        assert_eq!(next_piece([0, 0, 0]), Some(0));
        assert_eq!(next_piece([2, 0, 0]), Some(0));
        assert_eq!(next_piece([3, 0, 0]), Some(1));
        assert_eq!(next_piece([3, 2, 0]), Some(1));
        assert_eq!(next_piece([3, 1, 0]), Some(2));
        assert_eq!(next_piece([3, 1, 2]), Some(2));
        assert_eq!(next_piece(SOLVED_FACES), None);
        assert_eq!(next_piece([3, 1, 3]), None);
        assert_eq!(next_piece([4, 0, 0]), None);
    }
    #[test]
    fn terrain_fitting_keeps_level_connected_geometry_and_rejects_mirrors() {
        let centers = [[0., 0., 0.], [0., 0., -TILE], [TILE, 0., -TILE]];
        let anchors = [[-TILE, 0., 0.], [2. * TILE, 0., -TILE], [0., 0., TILE]];
        assert!(valid_layout(centers, anchors));
        for angle in [0., 0.7, 1.2, 3.1] {
            let transform = |p: [f32; 3]| {
                [
                    100. + p[0] * f32::cos(angle) + p[2] * f32::sin(angle),
                    20.,
                    -50. - p[0] * f32::sin(angle) + p[2] * f32::cos(angle),
                ]
            };
            assert!(valid_layout(centers.map(transform), anchors.map(transform)));
        }
        let mut distorted = centers;
        distorted[2][0] += 0.3;
        assert!(!valid_layout(distorted, anchors));
        let mut tilted = anchors;
        tilted[1][1] += 0.3;
        assert!(!valid_layout(centers, tilted));
        let mirror = |p: [f32; 3]| [-p[0], p[1], p[2]];
        assert!(!valid_layout(centers.map(mirror), anchors.map(mirror)));
    }
}
