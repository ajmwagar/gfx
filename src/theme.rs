use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Color;

/// Number of semantic color slots in the stable GPU theme contract.
pub const THEME_ROLE_COUNT: usize = 16;

/// Semantic color roles shared by every renderer and application.
///
/// Views ask for meaning, not a palette index. A host can therefore replace
/// an entire visual identity without rewriting view code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum ThemeRole {
    /// Canvas behind all surfaces.
    Background = 0,
    /// Default faceplate or card surface.
    Surface = 1,
    /// Raised control or panel surface.
    SurfaceRaised = 2,
    /// Recessed well, track, or display surface.
    SurfaceRecessed = 3,
    /// Primary text and markings.
    Text = 4,
    /// Secondary text and markings.
    TextMuted = 5,
    /// Main brand/action accent.
    Primary = 6,
    /// Secondary accent.
    Secondary = 7,
    /// Positive state or safe meter range.
    Success = 8,
    /// Caution state.
    Warning = 9,
    /// Destructive, clipping, or recording state.
    Danger = 10,
    /// Quiet portion of a level display.
    MeterLow = 11,
    /// Hot portion of a level display.
    MeterHigh = 12,
    /// Hairlines and control outlines.
    Outline = 13,
    /// Cast and ambient shadows.
    Shadow = 14,
    /// Specular highlights and glass reflections.
    Highlight = 15,
}

impl ThemeRole {
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// Shape tokens shared across applications.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeTheme {
    /// Small control corner radius in logical pixels.
    pub radius_small: f32,
    /// Panel corner radius in logical pixels.
    pub radius_large: f32,
    /// Default outline width in logical pixels.
    pub outline_width: f32,
}

/// Material behavior shared across applications.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialTheme {
    /// Strength of directional faceplate highlights.
    pub gloss: f32,
    /// Strength of deterministic fine-grain variation.
    pub grain: f32,
    /// Strength of emissive bloom contained within controls.
    pub emission: f32,
}

/// Motion tokens; applications remain responsible for advancing time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionTheme {
    /// Meter decay in normalized units per second.
    pub meter_decay: f32,
    /// Default parameter interpolation time in milliseconds.
    pub control_smoothing_ms: f32,
}

/// A complete semantic visual theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    /// Version of the stable theme schema.
    pub schema_version: u16,
    /// Fixed semantic color table indexed by [`ThemeRole`].
    pub colors: [Color; THEME_ROLE_COUNT],
    /// Shared shape tokens.
    pub shapes: ShapeTheme,
    /// Shared material tokens.
    pub materials: MaterialTheme,
    /// Shared motion tokens.
    pub motion: MotionTheme,
}

impl Theme {
    /// Current theme schema version.
    pub const SCHEMA_VERSION: u16 = 1;

    /// A neutral dark theme suitable for studio and instrumentation views.
    pub fn studio_dark() -> Self {
        let srgb = Color::from_srgb8;
        Self {
            schema_version: Self::SCHEMA_VERSION,
            colors: [
                srgb(10, 12, 16, 255),
                srgb(28, 31, 37, 255),
                srgb(48, 52, 61, 255),
                srgb(15, 17, 21, 255),
                srgb(235, 238, 244, 255),
                srgb(145, 153, 166, 255),
                srgb(77, 139, 255, 255),
                srgb(184, 122, 255, 255),
                srgb(54, 211, 153, 255),
                srgb(247, 183, 49, 255),
                srgb(255, 73, 82, 255),
                srgb(48, 177, 117, 255),
                srgb(255, 78, 50, 255),
                srgb(82, 88, 101, 255),
                srgb(0, 0, 0, 170),
                srgb(255, 255, 255, 210),
            ],
            shapes: ShapeTheme {
                radius_small: 5.0,
                radius_large: 14.0,
                outline_width: 1.0,
            },
            materials: MaterialTheme {
                gloss: 0.22,
                grain: 0.035,
                emission: 0.28,
            },
            motion: MotionTheme {
                meter_decay: 1.8,
                control_smoothing_ms: 80.0,
            },
        }
    }

    /// Returns the color assigned to a semantic role.
    pub const fn color(&self, role: ThemeRole) -> Color {
        self.colors[role.index()]
    }

    /// Replaces a semantic color.
    pub fn set_color(&mut self, role: ThemeRole, color: Color) {
        self.colors[role.index()] = color;
    }

    /// Validates all externally supplied values before they reach the GPU.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError`] when the schema version is unsupported or any
    /// color or numeric design token is invalid.
    pub fn validate(&self) -> Result<(), ThemeError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ThemeError::SchemaVersion(self.schema_version));
        }
        if self.colors.iter().copied().any(|color| !color.valid()) {
            return Err(ThemeError::Color);
        }
        let finite_nonnegative = |value: f32| value.is_finite() && value >= 0.0;
        if ![
            self.shapes.radius_small,
            self.shapes.radius_large,
            self.shapes.outline_width,
            self.materials.gloss,
            self.materials.grain,
            self.materials.emission,
            self.motion.meter_decay,
            self.motion.control_smoothing_ms,
        ]
        .into_iter()
        .all(finite_nonnegative)
        {
            return Err(ThemeError::Token);
        }
        Ok(())
    }

    pub(crate) fn gpu_colors(&self) -> [[f32; 4]; THEME_ROLE_COUNT] {
        self.colors.map(Color::to_array)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::studio_dark()
    }
}

/// A rejected theme supplied by a host or configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ThemeError {
    /// The schema version is not understood by this renderer.
    #[error("unsupported theme schema version {0}")]
    SchemaVersion(u16),
    /// At least one color is non-finite or outside normalized RGBA.
    #[error("theme colors must be finite normalized linear RGBA")]
    Color,
    /// At least one shape, material, or motion token is invalid.
    #[error("theme tokens must be finite and non-negative")]
    Token,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_valid_and_round_trips() {
        let theme = Theme::default();
        theme.validate().unwrap();
        let json = serde_json::to_string(&theme).unwrap();
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), theme);
    }

    #[test]
    fn invalid_external_values_fail_loudly() {
        let mut theme = Theme::default();
        theme.set_color(ThemeRole::Primary, Color::linear(f32::NAN, 0.0, 0.0, 1.0));
        assert_eq!(theme.validate(), Err(ThemeError::Color));
    }
}
