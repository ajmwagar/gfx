//! Bounded CPU raster backend. Coverage is deliberately limited; unsupported GPU semantics error.
use crate::{PrimitiveKind, Scene, Theme};
use thiserror::Error;

/// Maximum software output extent.
pub const MAX_WIDTH: u32 = 480;
/// Maximum software output extent.
pub const MAX_HEIGHT: u32 = 640;
/// Maximum accepted scene size.
pub const MAX_PRIMITIVES: usize = 4096;

/// RGBA8 image in sRGB channel encoding, row-major with a four-byte stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Row-major RGBA8 samples, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

/// Failure modes of the bounded software rasterizer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RasterError {
    /// Output dimensions were zero or exceeded the 480x640 bound.
    #[error("dimensions must be nonzero and at most 480x640")]
    Dimensions,
    /// The scene held more than [`MAX_PRIMITIVES`] primitives.
    #[error("scene exceeds primitive limit")]
    TooManyPrimitives,
    /// A primitive or its color carried non-finite or otherwise invalid input.
    #[error("primitive contains non-finite or invalid input")]
    InvalidInput,
    /// A primitive kind or style field is outside the supported subset.
    #[error("primitive kind or style is unsupported by software rasterizer")]
    Unsupported,
    /// A buffer dimension overflowed `usize`.
    #[error("allocation size overflow")]
    Overflow,
}

/// Render only solid rounded rectangles and discs. Rounded rectangles with outlines,
/// nonzero material effects, or other style fields are explicitly rejected. No blending:
/// each accepted primitive overwrites its covered pixels outright, so alpha is carried
/// through verbatim and later primitives fully replace earlier ones.
pub fn render(scene: &Scene, theme: &Theme, width: u32, height: u32) -> Result<Image, RasterError> {
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(RasterError::Dimensions);
    }
    if scene.len() > MAX_PRIMITIVES {
        return Err(RasterError::TooManyPrimitives);
    }
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(RasterError::Overflow)?;
    let mut pixels = vec![0; len];
    for p in scene.iter() {
        if !p.rect.is_valid()
            || [
                p.radius,
                p.outline_width,
                p.value,
                p.rotation,
                p.gloss,
                p.grain,
                p.emission,
            ]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(RasterError::InvalidInput);
        }
        if !matches!(p.kind, PrimitiveKind::RoundedRect | PrimitiveKind::Disc) {
            return Err(RasterError::Unsupported);
        }
        if p.kind == PrimitiveKind::RoundedRect && p.outline_width != 0.0
            || p.gloss != 0.0
            || p.grain != 0.0
            || p.emission != 0.0
            || p.value != 0.0
            || p.rotation != 0.0
        {
            return Err(RasterError::Unsupported);
        }
        if p.kind == PrimitiveKind::RoundedRect && p.outline != crate::ThemeRole::Outline {
            return Err(RasterError::Unsupported);
        }
        let color = theme.colors[p.fill as usize];
        if !color.valid() {
            return Err(RasterError::InvalidInput);
        }
        let [r, g, b, a] = color.to_srgb8();
        let x0 = p.rect.x.floor().max(0.0).min(width as f32) as u32;
        let y0 = p.rect.y.floor().max(0.0).min(height as f32) as u32;
        let x1 = (p.rect.x + p.rect.width).ceil().max(0.0).min(width as f32) as u32;
        let y1 = (p.rect.y + p.rect.height)
            .ceil()
            .max(0.0)
            .min(height as f32) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let inside = if p.kind == PrimitiveKind::Disc {
                    let cx = p.rect.x + p.rect.width / 2.0;
                    let cy = p.rect.y + p.rect.height / 2.0;
                    let rx = p.rect.width / 2.0;
                    let ry = p.rect.height / 2.0;
                    ((px - cx) / rx).powi(2) + ((py - cy) / ry).powi(2) <= 1.0
                } else {
                    let radius = p.radius.max(0.0).min(p.rect.width.min(p.rect.height) / 2.0);
                    let cx = px.clamp(p.rect.x + radius, p.rect.x + p.rect.width - radius);
                    let cy = py.clamp(p.rect.y + radius, p.rect.y + p.rect.height - radius);
                    (px - cx).powi(2) + (py - cy).powi(2) <= radius * radius || radius == 0.0
                };
                if inside {
                    let i = ((y * width + x) * 4) as usize;
                    pixels[i..i + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

impl Image {
    /// Convert RGBA to tightly packed BGR24 rows (stride exactly width * 3).
    pub fn to_bgr24(&self) -> Result<Vec<u8>, RasterError> {
        let row = (self.width as usize)
            .checked_mul(3)
            .ok_or(RasterError::Overflow)?;
        if self.pixels.len()
            != (self.width as usize)
                .checked_mul(self.height as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or(RasterError::Overflow)?
        {
            return Err(RasterError::Overflow);
        }
        let mut out = vec![
            0;
            row.checked_mul(self.height as usize)
                .ok_or(RasterError::Overflow)?
        ];
        for (src, dst) in self.pixels.chunks_exact(4).zip(out.chunks_exact_mut(3)) {
            dst.copy_from_slice(&[src[2], src[1], src[0]]);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Primitive, Rect, ThemeRole};
    #[test]
    fn dimensions_and_unsupported() {
        let s = Scene::new();
        let t = Theme::studio_dark();
        assert_eq!(render(&s, &t, 481, 1), Err(RasterError::Dimensions));
        let mut s = Scene::new();
        s.push(Primitive::knob(Rect::new(0., 0., 2., 2.), 0.));
        assert_eq!(render(&s, &t, 2, 2), Err(RasterError::Unsupported));
    }
    #[test]
    fn clipping_colors_and_bgr_stride() {
        let mut s = Scene::new();
        s.push(Primitive::disc(
            Rect::new(-2., 0., 4., 4.),
            ThemeRole::Primary,
        ));
        let i = render(&s, &Theme::studio_dark(), 4, 4).unwrap();
        assert_eq!(i.pixels.len(), 64);
        let fill = Theme::studio_dark().colors[ThemeRole::Primary as usize].to_srgb8();
        assert_eq!(i.pixels[0..4], fill);
        assert_eq!(i.pixels[8..12], [0, 0, 0, 0]);
        let b = i.to_bgr24().unwrap();
        assert_eq!(b.len(), 48);
        assert_eq!(&b[0..3], &[fill[2], fill[1], fill[0]]);
    }
    #[test]
    fn zero_dimension_and_alpha() {
        let s = Scene::new();
        let t = Theme::studio_dark();
        assert_eq!(render(&s, &t, 0, 4), Err(RasterError::Dimensions));
        assert_eq!(render(&s, &t, 4, 0), Err(RasterError::Dimensions));
        let mut s = Scene::new();
        s.push(Primitive::disc(
            Rect::new(0., 0., 2., 2.),
            ThemeRole::Primary,
        ));
        let i = render(&s, &t, 2, 2).unwrap();
        assert_eq!(i.pixels[3], 255);
    }
    #[test]
    fn nan_rejected() {
        let mut s = Scene::new();
        s.push(Primitive::disc(
            Rect::new(f32::NAN, 0., 1., 1.),
            ThemeRole::Primary,
        ));
        assert_eq!(
            render(&s, &Theme::studio_dark(), 1, 1),
            Err(RasterError::InvalidInput)
        );
    }
}
