//! Semantic finish tokens independent of snapshot identity and view geometry.
//! Hosts supply colors through the existing theme; effects are not data.

use crate::{MaterialTheme, Primitive, ThemeRole};
use serde::{Deserialize, Serialize};

/// Shared panel finish for code, graphs and future instrumentation views.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewAppearance {
    /// Main panel well.
    pub face: ThemeRole,
    /// Selected row or node face; selection does not imply execution.
    pub selection: ThemeRole,
    /// Separators and panel edge.
    pub outline: ThemeRole,
    /// Main text.
    pub ink: ThemeRole,
    /// Source, addresses and annotations.
    pub muted: ThemeRole,
    /// Inspection selection marker.
    pub accent: ThemeRole,
    /// Existing shared material tokens; no second palette or shader ownership.
    pub material: MaterialTheme,
}

impl Default for ViewAppearance {
    fn default() -> Self {
        Self::clean()
    }
}

impl ViewAppearance {
    /// Flat finish with semantic state cues and no material effects.
    pub const fn clean() -> Self {
        Self {
            face: ThemeRole::SurfaceRecessed,
            selection: ThemeRole::SurfaceRaised,
            outline: ThemeRole::Outline,
            ink: ThemeRole::Text,
            muted: ThemeRole::TextMuted,
            accent: ThemeRole::Primary,
            material: MaterialTheme {
                gloss: 0.0,
                grain: 0.0,
                emission: 0.0,
            },
        }
    }

    /// Restrained terminal finish. The host palette still chooses actual colors.
    /// Unsupported renderer materials can be omitted without changing the data.
    pub const fn terminal() -> Self {
        Self {
            accent: ThemeRole::Success,
            material: MaterialTheme {
                gloss: 0.0,
                grain: 0.006,
                emission: 0.12,
            },
            ..Self::clean()
        }
    }

    /// Validate externally supplied tokens before changing any scene.
    ///
    /// # Errors
    /// Rejects nonfinite or out-of-range material strengths.
    pub fn validate(&self) -> Result<(), crate::signals::ViewError> {
        if [
            self.material.gloss,
            self.material.grain,
            self.material.emission,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        {
            Ok(())
        } else {
            Err(crate::signals::ViewError)
        }
    }

    /// Apply finish only; dimensions, shape, value and rotation stay unchanged.
    pub const fn apply(&self, primitive: Primitive) -> Primitive {
        primitive
            .with_gloss(self.material.gloss)
            .with_grain(self.material.grain)
            .with_emission(self.material.emission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    #[test]
    fn finish_changes_no_geometry_or_observed_value() {
        let primitive = Primitive::meter(Rect::new(2.0, 3.0, 20.0, 80.0), 0.4);
        let finished = ViewAppearance::terminal().apply(primitive);
        assert_eq!(finished.rect, primitive.rect);
        assert_eq!(finished.kind, primitive.kind);
        assert_eq!(finished.value, primitive.value);
        assert_eq!(finished.rotation, primitive.rotation);
        assert_eq!(finished.fill, primitive.fill);
        assert!(ViewAppearance::clean().validate().is_ok());
        assert!(ViewAppearance::terminal().validate().is_ok());
    }

    #[test]
    fn invalid_materials_are_rejected() {
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut appearance = ViewAppearance::clean();
            appearance.material.grain = value;
            assert!(appearance.validate().is_err());
        }
    }
}
