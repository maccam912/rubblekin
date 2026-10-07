//! Small, explicit rendering budgets. These are quality choices, not FPS promises.
use bevy::{
    light::{
        CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap,
        ShadowFilteringMethod,
    },
    prelude::*,
};

pub const MIN_NEAR_DISTANCE: f32 = 24.0;
pub const MAX_NEAR_DISTANCE: f32 = 1000.0;
pub const MIN_SHADOW_DISTANCE: f32 = 8.0;
pub const MAX_SHADOW_DISTANCE: f32 = 192.0;
pub const DISTANCE_STEP: f32 = 8.0;
pub const MIN_TREE_DISTANCE: f32 = 128.0;
pub const MAX_TREE_DISTANCE: f32 = 2500.0;
pub const TREE_DISTANCE_STEP: f32 = 128.0;
const DEFAULT_NEAR_DISTANCE: f32 = 48.0;
const DEFAULT_TREE_DISTANCE: f32 = MIN_TREE_DISTANCE;
const SETTINGS_FILE: &str = "graphics.json";

/// Client preferences stay independent of the session and authoritative world.
#[derive(Resource, Debug)]
pub struct GraphicsSettings {
    pub quality: GraphicsQuality,
    pub near_distance: f32,
    pub shadow_distance: f32,
    pub tree_distance: f32,
    last_saved: (GraphicsQuality, f32, f32, f32),
    last_attempt: (GraphicsQuality, f32, f32, f32),
}

impl GraphicsSettings {
    pub fn new(quality: GraphicsQuality) -> Self {
        let shadow_distance = quality.default_shadow_distance();
        Self {
            quality,
            near_distance: DEFAULT_NEAR_DISTANCE,
            shadow_distance,
            tree_distance: DEFAULT_TREE_DISTANCE,
            last_saved: (
                quality,
                DEFAULT_NEAR_DISTANCE,
                shadow_distance,
                DEFAULT_TREE_DISTANCE,
            ),
            last_attempt: (
                quality,
                DEFAULT_NEAR_DISTANCE,
                shadow_distance,
                DEFAULT_TREE_DISTANCE,
            ),
        }
    }

    pub fn load(default_quality: GraphicsQuality) -> Self {
        match Self::load_from(std::path::Path::new(SETTINGS_FILE)) {
            Ok(settings) => settings,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::new(default_quality)
            }
            Err(error) => {
                eprintln!("Could not load graphics preferences: {error}; using defaults");
                Self::new(default_quality)
            }
        }
    }

    pub fn set_quality(&mut self, quality: GraphicsQuality) {
        if self.quality == quality {
            return;
        }
        self.quality = quality;
        if quality.shadows() {
            self.shadow_distance = quality.default_shadow_distance();
        }
    }

    /// Return experimental distances to this preset's starting values.
    pub fn reset_distances(&mut self) {
        self.near_distance = DEFAULT_NEAR_DISTANCE;
        self.tree_distance = DEFAULT_TREE_DISTANCE;
        self.shadow_distance = self.quality.default_shadow_distance();
    }

    pub fn adjust_near_distance(&mut self, delta: f32) {
        if delta.is_finite() {
            self.near_distance = (((self.near_distance + delta) / DISTANCE_STEP).round()
                * DISTANCE_STEP)
                .clamp(MIN_NEAR_DISTANCE, MAX_NEAR_DISTANCE);
        }
    }

    pub fn adjust_shadow_distance(&mut self, delta: f32) {
        if delta.is_finite() {
            self.shadow_distance =
                (self.shadow_distance + delta).clamp(MIN_SHADOW_DISTANCE, MAX_SHADOW_DISTANCE);
        }
    }

    pub fn adjust_tree_distance(&mut self, delta: f32) {
        if delta.is_finite() {
            self.tree_distance =
                (self.tree_distance + delta).clamp(MIN_TREE_DISTANCE, MAX_TREE_DISTANCE);
        }
    }

    pub fn near_radius_chunks(&self) -> i32 {
        (self
            .near_distance
            .clamp(MIN_NEAR_DISTANCE, MAX_NEAR_DISTANCE)
            / DISTANCE_STEP)
            .round() as i32
    }

    pub fn cascades(&self) -> CascadeShadowConfig {
        self.quality.cascades_at(self.shadow_distance)
    }

    fn values(&self) -> (GraphicsQuality, f32, f32, f32) {
        (
            self.quality,
            self.near_distance,
            self.shadow_distance,
            self.tree_distance,
        )
    }

    fn load_from(path: &std::path::Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let invalid = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid graphics preferences",
            )
        };
        if value["version"].as_u64() != Some(1) {
            return Err(invalid());
        }
        let quality = match value["quality"].as_str() {
            Some("low") => GraphicsQuality::Low,
            Some("balanced") => GraphicsQuality::Balanced,
            Some("high") => GraphicsQuality::High,
            _ => return Err(invalid()),
        };
        let near_distance = value["near_distance_m"].as_f64().ok_or_else(invalid)? as f32;
        let shadow_distance = value["shadow_distance_m"].as_f64().ok_or_else(invalid)? as f32;
        let tree_distance = match value.get("tree_distance_m") {
            None => DEFAULT_TREE_DISTANCE,
            Some(value) => {
                let distance = value.as_f64().ok_or_else(invalid)?;
                if !(f64::from(MIN_TREE_DISTANCE)..=f64::from(MAX_TREE_DISTANCE))
                    .contains(&distance)
                {
                    return Err(invalid());
                }
                distance as f32
            }
        };
        if !(MIN_NEAR_DISTANCE..=MAX_NEAR_DISTANCE).contains(&near_distance)
            || near_distance % DISTANCE_STEP != 0.0
            || !(MIN_SHADOW_DISTANCE..=MAX_SHADOW_DISTANCE).contains(&shadow_distance)
        {
            return Err(invalid());
        }
        let mut settings = Self::new(quality);
        settings.near_distance = near_distance;
        settings.shadow_distance = shadow_distance;
        settings.tree_distance = tree_distance;
        settings.last_saved = settings.values();
        settings.last_attempt = settings.values();
        Ok(settings)
    }

    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let quality = match self.quality {
            GraphicsQuality::Low => "low",
            GraphicsQuality::Balanced => "balanced",
            GraphicsQuality::High => "high",
        };
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "quality": quality,
            "near_distance_m": self.near_distance,
            "shadow_distance_m": self.shadow_distance,
            "tree_distance_m": self.tree_distance,
        }))?;
        let temporary = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(temporary, path)
    }
}

pub fn save_changed(mut settings: ResMut<GraphicsSettings>) {
    let values = settings.values();
    if values == settings.last_saved || values == settings.last_attempt {
        return;
    }
    settings.last_attempt = values;
    match settings.save_to(std::path::Path::new(SETTINGS_FILE)) {
        Ok(()) => settings.last_saved = values,
        Err(error) => warn!("Could not save graphics preferences: {error}"),
    }
}

/// Menu choices and the F2 shortcut share this live rendering path.
pub fn apply_settings(
    settings: Res<GraphicsSettings>,
    mut session: Option<ResMut<crate::Session>>,
    mut lights: Query<(&mut DirectionalLight, &mut CascadeShadowConfig)>,
    mut cameras: Query<(&mut Msaa, &mut ShadowFilteringMethod), With<crate::GameCamera>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
) {
    if !settings.is_changed() && !session.as_ref().is_some_and(|session| session.is_added()) {
        return;
    }
    if let Some(session) = session.as_mut() {
        session.graphics = settings.quality;
    }
    shadow_map.size = settings.quality.shadow_map_size();
    for (mut light, mut cascades) in &mut lights {
        light.shadow_maps_enabled = settings.quality.shadows();
        *cascades = settings.cascades();
    }
    for (mut msaa, mut filter) in &mut cameras {
        *msaa = settings.quality.msaa();
        *filter = settings.quality.shadow_filter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preference_path() -> std::path::PathBuf {
        static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "rubblekin-graphics-{}-{}.json",
            std::process::id(),
            NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn preferences_roundtrip_preserves_custom_distances_and_quality() {
        let path = preference_path();
        let mut settings = GraphicsSettings::new(GraphicsQuality::High);
        settings.adjust_shadow_distance(8.0);
        for (distance, tree_distance) in [(64.0, 256.0), (1000.0, MAX_TREE_DISTANCE)] {
            settings.adjust_near_distance(distance - settings.near_distance);
            settings.adjust_tree_distance(tree_distance - settings.tree_distance);
            settings.save_to(&path).unwrap();
            let restored = GraphicsSettings::load_from(&path).unwrap();
            assert_eq!(
                restored.values(),
                (GraphicsQuality::High, distance, 98.0, tree_distance)
            );
            assert_eq!(restored.last_saved, restored.values());
            assert!(!path.with_extension("json.tmp").exists());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn existing_preferences_default_tree_distance_without_resetting_other_values() {
        let path = preference_path();
        std::fs::write(
            &path,
            r#"{"version":1,"quality":"high","near_distance_m":1000,"shadow_distance_m":98}"#,
        )
        .unwrap();
        let restored = GraphicsSettings::load_from(&path).unwrap();
        assert_eq!(
            restored.values(),
            (GraphicsQuality::High, 1000.0, 98.0, DEFAULT_TREE_DISTANCE)
        );
        assert_eq!(restored.last_saved, restored.values());
        assert_eq!(restored.last_attempt, restored.values());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_or_unsupported_preferences_are_rejected() {
        let path = preference_path();
        for bytes in [
            "not json",
            r#"{"version":2,"quality":"low","near_distance_m":48,"shadow_distance_m":32}"#,
            r#"{"version":1,"quality":"ultra","near_distance_m":48,"shadow_distance_m":32}"#,
            r#"{"version":1,"quality":"low","near_distance_m":23,"shadow_distance_m":32}"#,
            r#"{"version":1,"quality":"low","near_distance_m":49,"shadow_distance_m":32}"#,
            r#"{"version":1,"quality":"low","near_distance_m":1008,"shadow_distance_m":32}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":193}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":127}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":2501}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":2500.00001}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":1e50}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":null}"#,
            r#"{"version":1,"quality":"low","near_distance_m":48,"shadow_distance_m":32,"tree_distance_m":"far"}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(
                GraphicsSettings::load_from(&path).unwrap_err().kind(),
                std::io::ErrorKind::InvalidData
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tree_distance_reaches_exact_cap_and_is_retained_across_presets() {
        let mut settings = GraphicsSettings::new(GraphicsQuality::Balanced);
        assert_eq!(settings.tree_distance, DEFAULT_TREE_DISTANCE);
        for _ in 0..19 {
            settings.adjust_tree_distance(TREE_DISTANCE_STEP);
        }
        assert_eq!(settings.tree_distance, MAX_TREE_DISTANCE);
        settings.adjust_tree_distance(TREE_DISTANCE_STEP);
        assert_eq!(settings.tree_distance, MAX_TREE_DISTANCE);
        for quality in [
            GraphicsQuality::Low,
            GraphicsQuality::High,
            GraphicsQuality::Balanced,
        ] {
            settings.set_quality(quality);
            assert_eq!(settings.tree_distance, MAX_TREE_DISTANCE);
        }
        for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            settings.adjust_tree_distance(delta);
            assert_eq!(settings.tree_distance, MAX_TREE_DISTANCE);
        }
        settings.adjust_tree_distance(-TREE_DISTANCE_STEP);
        assert_eq!(
            settings.tree_distance,
            MAX_TREE_DISTANCE - TREE_DISTANCE_STEP
        );
        settings.adjust_tree_distance(-10000.0);
        assert_eq!(settings.tree_distance, MIN_TREE_DISTANCE);
    }

    #[test]
    fn distance_limits_and_presets_keep_short_high_cascades_valid() {
        let mut settings = GraphicsSettings::new(GraphicsQuality::Balanced);
        settings.adjust_near_distance(-1000.0);
        assert_eq!(settings.near_radius_chunks(), 3);
        settings.adjust_near_distance(1000.0);
        assert_eq!(settings.near_radius_chunks(), 125);
        settings.adjust_shadow_distance(1000.0);
        assert_eq!(settings.shadow_distance, MAX_SHADOW_DISTANCE);
        settings.set_quality(GraphicsQuality::Low);
        assert_eq!(settings.shadow_distance, MAX_SHADOW_DISTANCE);
        settings.set_quality(GraphicsQuality::High);
        assert_eq!(settings.shadow_distance, 90.0);
        assert_eq!(settings.near_distance, MAX_NEAR_DISTANCE);
        settings.adjust_shadow_distance(-1000.0);
        let cascades = settings.cascades();
        assert_eq!(cascades.bounds.len(), 2);
        assert!(cascades.bounds[0] < cascades.bounds[1]);
        assert_eq!(cascades.bounds[1], MIN_SHADOW_DISTANCE);
        settings.set_quality(GraphicsQuality::Balanced);
        assert_eq!(settings.shadow_distance, 32.0);
        settings.adjust_near_distance(f32::NAN);
        settings.adjust_shadow_distance(f32::INFINITY);
        assert_eq!(
            settings.values(),
            (
                GraphicsQuality::Balanced,
                1000.0,
                32.0,
                DEFAULT_TREE_DISTANCE
            )
        );
    }
}

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

    fn default_shadow_distance(self) -> f32 {
        if self == Self::High { 90.0 } else { 32.0 }
    }

    fn cascades_at(self, distance: f32) -> CascadeShadowConfig {
        // Balanced renders a single nearby shadow view. High is still bounded;
        // distant scenery never casts shadows. Keep the camera's far view intact.
        let num_cascades = if self == Self::High { 2 } else { 1 };
        let maximum_distance = distance.clamp(MIN_SHADOW_DISTANCE, MAX_SHADOW_DISTANCE);
        CascadeShadowConfigBuilder {
            num_cascades,
            maximum_distance,
            // The first cascade must stay inside the last when High is short.
            first_cascade_far_bound: 18.0_f32.min(maximum_distance * 0.5),
            ..default()
        }
        .build()
    }
}
