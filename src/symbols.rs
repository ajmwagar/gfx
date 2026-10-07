//! Small backend-independent outlines shared by instrument and spatial hosts.
//! Points are in caller-scaled local coordinates, with a maximum of 14 points.
//! The caller owns placement, semantic colors, line rendering and interaction.

/// Upper bound for one outline; hosts can retain a fixed scratch buffer.
pub const MAX_POINTS: usize = 14;

/// Look up an immutable outline. Contact outlines use a 0.11-unit maximum radius;
/// UI outlines use a unit radius. A forward contact points toward positive Y.
pub fn outline(name: &str) -> Option<&'static [[f32; 2]]> {
    Some(match name {
        "meshcore" => &[
            [0., 0.08],
            [-0.065, 0.],
            [0., -0.08],
            [0.065, 0.],
            [0., 0.08],
        ],
        "meshtastic" => &[
            [-0.06, -0.055],
            [-0.06, 0.055],
            [0.06, 0.055],
            [0.06, -0.055],
            [-0.06, -0.055],
        ],
        "ais" => &[
            [0., 0.085],
            [-0.045, 0.03],
            [-0.038, -0.075],
            [0.038, -0.075],
            [0.045, 0.03],
            [0., 0.085],
        ],
        "adsb" => &[
            [0., 0.11],
            [-0.017, 0.025],
            [-0.10, -0.02],
            [-0.10, -0.045],
            [-0.018, -0.035],
            [-0.018, -0.085],
            [-0.046, -0.105],
            [0.046, -0.105],
            [0.018, -0.085],
            [0.018, -0.035],
            [0.10, -0.045],
            [0.10, -0.02],
            [0.017, 0.025],
            [0., 0.11],
        ],
        "close" => &[[-1., -1.], [1., 1.], [0., 0.], [-1., 1.], [1., -1.]],
        "canvas" => &[[-1., -0.7], [-1., 0.7], [1., 0.7], [1., -0.7], [-1., -0.7]],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outlines_are_bounded_and_finite() {
        assert!(outline("unknown").is_none());
        for name in ["meshcore", "meshtastic", "ais", "adsb", "close", "canvas"] {
            let points = outline(name).unwrap();
            assert!((2..=MAX_POINTS).contains(&points.len()));
            assert!(points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 1.));
        }
        assert_ne!(outline("ais"), outline("adsb"));
        assert_eq!(
            outline("ais").unwrap().first(),
            outline("ais").unwrap().last()
        );
        assert_eq!(
            outline("adsb").unwrap().first(),
            outline("adsb").unwrap().last()
        );
    }
}
