use bevy::prelude::*;
use rubblekin_core::{
    building::Selection,
    world::{BlockPos, CELL_SIZE},
};

#[derive(Default)]
pub(crate) struct Tools {
    pub enabled: bool,
    first: Option<BlockPos>,
    second: Option<BlockPos>,
}
impl Tools {
    pub fn clear(&mut self) {
        self.first = None;
        self.second = None;
    }
    pub fn corner(&mut self, p: BlockPos) {
        if self.first.is_none() || self.second.is_some() {
            self.first = Some(p);
            self.second = None;
        } else {
            self.second = Some(p);
        }
    }
    pub fn corners(&self) -> Option<(BlockPos, BlockPos)> {
        self.first.zip(self.second)
    }
    pub fn hint(&self, block: rubblekin_core::world::Block, touch: bool) -> String {
        if !self.enabled {
            return String::new();
        }
        let action = if touch {
            "Select corners · Fill · Menu: Building to exit"
        } else {
            "Shift-click corners · Enter fill · Backspace clear · K exit"
        };
        let step = match self.corners() {
            Some((a, b)) => Selection::new(a, b).map_or_else(
                |e| e.into(),
                |s| format!("{} blocks with {}", s.count, block.name()),
            ),
            None if self.first.is_some() => "First corner set · choose the opposite corner".into(),
            None => "Choose the first corner".into(),
        };
        format!("{step}\n{action}")
    }
}
pub(crate) fn preview(tools: &Tools, target: Option<BlockPos>, gizmos: &mut Gizmos) {
    if !tools.enabled {
        return;
    }
    if let Some(a) = tools.first {
        let b = tools.second.or(target).unwrap_or(a);
        let min = Vec3::new(
            a.x.min(b.x) as f32,
            a.y.min(b.y) as f32,
            a.z.min(b.z) as f32,
        ) * CELL_SIZE;
        let max = Vec3::new(
            a.x.max(b.x) as f32 + 1.,
            a.y.max(b.y) as f32 + 1.,
            a.z.max(b.z) as f32 + 1.,
        ) * CELL_SIZE;
        let color = if Selection::new(a, b).is_ok() {
            Color::srgb(1., 0.78, 0.25)
        } else {
            Color::srgb(1., 0.25, 0.2)
        };
        gizmos.cube(
            Transform::from_translation((min + max) * 0.5)
                .with_scale(max - min + Vec3::splat(0.015)),
            color,
        );
        gizmos.cube(
            Transform::from_translation(
                Vec3::new(a.x as f32 + 0.5, a.y as f32 + 0.5, a.z as f32 + 0.5) * CELL_SIZE,
            )
            .with_scale(Vec3::splat(CELL_SIZE + 0.03)),
            Color::srgb(0.35, 1., 0.65),
        );
    }
}
