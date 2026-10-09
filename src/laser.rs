//! Shared energy-bolt colors, independent of the rendering backend.

/// Named linear-renderer input colors in sRGB. Unknown names are rejected.
pub fn color(name: &str) -> Option<[f32; 3]> {
    Some(match name {
        "blue" => [0.12, 0.65, 1.0],
        "red" => [1.0, 0.12, 0.06],
        "green" => [0.15, 1.0, 0.25],
        "purple" => [0.72, 0.25, 1.0],
        "amber" => [1.0, 0.62, 0.08],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_is_bounded_and_rejects_unknown_colors() {
        for name in ["blue", "red", "green", "purple", "amber"] {
            let rgb = color(name).unwrap();
            assert!(rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            assert!(rgb.iter().any(|v| *v == 1.0));
        }
        assert!(color("unknown").is_none());
    }
}
