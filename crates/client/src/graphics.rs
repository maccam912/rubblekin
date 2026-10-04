//! Small, explicit rendering budgets. These are quality choices, not FPS promises.
use bevy::{
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder, ShadowFilteringMethod},
    prelude::*,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphicsQuality {
    Low,
    #[default]
    Balanced,
    High,
}

impl GraphicsQuality {
    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::Balanced,
            Self::Balanced => Self::High,
            Self::High => Self::Low,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low · shaded",
            Self::Balanced => "Balanced · nearby shadows",
            Self::High => "High · shadows + MSAA",
        }
    }

    pub fn shadows(self) -> bool {
        self != Self::Low
    }

    pub fn msaa(self) -> Msaa {
        if self == Self::High {
            Msaa::Sample4
        } else {
            Msaa::Off
        }
    }

    pub fn shadow_map_size(self) -> usize {
        if self == Self::High { 2048 } else { 1024 }
    }

    pub fn shadow_filter(self) -> ShadowFilteringMethod {
        if self == Self::High {
            ShadowFilteringMethod::Gaussian
        } else {
            ShadowFilteringMethod::Hardware2x2
        }
    }

    pub fn cascades(self) -> CascadeShadowConfig {
        // Balanced renders a single nearby shadow view. High is still bounded;
        // distant scenery never casts shadows. Keep the camera's far view intact.
        let (num_cascades, maximum_distance) = if self == Self::High {
            (2, 90.0)
        } else {
            (1, 32.0)
        };
        CascadeShadowConfigBuilder {
            num_cascades,
            maximum_distance,
            first_cascade_far_bound: 18.0,
            ..default()
        }
        .build()
    }
}
