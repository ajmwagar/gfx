use serde::{Deserialize, Serialize};

/// A linear-light RGBA color.
///
/// Most authored colors are sRGB. Use [`Color::from_srgb8`] to convert them
/// before rendering to an sRGB wGPU target.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    /// Linear red component.
    pub r: f32,
    /// Linear green component.
    pub g: f32,
    /// Linear blue component.
    pub b: f32,
    /// Linear alpha component.
    pub a: f32,
}

impl Color {
    /// Transparent black.
    pub const TRANSPARENT: Self = Self::linear(0.0, 0.0, 0.0, 0.0);

    /// Constructs a linear color.
    pub const fn linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Converts 8-bit sRGB channels to linear light.
    pub fn from_srgb8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a: f32::from(a) / 255.0,
        }
    }

    /// Returns normalized linear RGBA components.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub(crate) fn valid(self) -> bool {
        self.to_array()
            .into_iter()
            .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
    }

    pub(crate) fn as_wgpu(self) -> wgpu::Color {
        wgpu::Color {
            r: f64::from(self.r),
            g: f64::from(self.g),
            b: f64::from(self.b),
            a: f64::from(self.a),
        }
    }
}

fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_srgb_endpoints_and_midpoint() {
        assert_eq!(Color::from_srgb8(0, 0, 0, 0), Color::TRANSPARENT);
        assert_eq!(
            Color::from_srgb8(255, 255, 255, 255),
            Color::linear(1.0, 1.0, 1.0, 1.0)
        );
        let gray = Color::from_srgb8(128, 128, 128, 255);
        assert!((gray.r - 0.215_86).abs() < 0.000_1);
    }
}
