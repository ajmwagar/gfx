//! Small backend-independent outlines shared by instrument and spatial hosts.
//! Points are in caller-scaled local coordinates, with a maximum of 14 points.
//! The caller owns placement, semantic colors, line rendering and interaction.

/// Upper bound for one outline; hosts can retain a fixed scratch buffer.
pub const MAX_POINTS: usize = 14;

/// Bounded shared catalog, used by deterministic host exports.
pub const NAMES: &[&str] = &[
    "range-reset",
    "range-next",
    "range-target",
    "range-paper",
    "meshcore",
    "meshtastic",
    "ais",
    "adsb",
    "close",
    "canvas",
];

/// Export an owner outline as SVG with inherited color and an upright axis.
/// Hosts retain dimensions, semantics, placement and interaction.
pub fn svg(name: &str) -> Option<String> {
    use std::fmt::Write;
    let points = outline(name)?;
    let radius = points.iter().flatten().fold(0f32, |r, v| r.max(v.abs())) * 1.12;
    let mut data = String::new();
    for [x, y] in points {
        write!(data, "{x},{} ", -y).ok()?;
    }
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" aria-hidden=\"true\" focusable=\"false\"><polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" vector-effect=\"non-scaling-stroke\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/></svg>",
        -radius,-radius,radius*2.,radius*2.,data.trim_end()))
}

/// Look up an immutable outline. Contact outlines use a 0.11-unit maximum radius;
/// UI outlines use a unit radius. A forward contact points toward positive Y.
pub fn outline(name: &str) -> Option<&'static [[f32; 2]]> {
    Some(match name {
        "range-reset" => &[
            [-0.6, 0.5],
            [0., 0.8],
            [0.7, 0.4],
            [0.8, -0.3],
            [0.3, -0.8],
            [-0.5, -0.6],
            [-0.8, 0.],
            [-0.6, 0.5],
            [-0.9, 0.4],
            [-0.6, 0.5],
            [-0.5, 0.1],
        ],
        "range-next" => &[[-0.4, -0.7], [0.4, 0.], [-0.4, 0.7]],
        "range-target" => &[
            [-0.7, -0.7],
            [-0.7, 0.7],
            [0.7, 0.7],
            [0.7, -0.7],
            [-0.7, -0.7],
            [0., 0.],
            [-0.3, 0.],
            [0.3, 0.],
            [0., 0.],
            [0., -0.3],
            [0., 0.3],
        ],
        "range-paper" => &[
            [-0.5, -0.7],
            [-0.5, 0.7],
            [0.5, 0.7],
            [0.5, -0.7],
            [-0.5, -0.7],
            [0., 0.],
            [-0.25, 0.],
            [0.25, 0.],
            [0., 0.],
            [0., -0.25],
            [0., 0.25],
        ],
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

    #[test]
    fn svg_uses_only_owner_geometry_and_rejects_unknown_names() {
        assert!(svg("<script>").is_none());
        for &name in NAMES {
            let markup = svg(name).unwrap();
            assert!(
                markup.contains("stroke=\"currentColor\"")
                    && markup.contains("aria-hidden=\"true\"")
            );
            let points = markup
                .split("points=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let parsed = points
                .split_whitespace()
                .map(|p| {
                    let (x, y) = p.split_once(',').unwrap();
                    [x.parse::<f32>().unwrap(), -y.parse::<f32>().unwrap()]
                })
                .collect::<Vec<_>>();
            assert_eq!(parsed, outline(name).unwrap());
        }
    }
}
