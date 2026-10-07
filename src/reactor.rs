//! Small renderer-neutral assistant reactor. Hosts own state, colors and clocks.
use std::f32::consts::{PI, TAU};

/// Visual activity only; not an assistant lifecycle or wire protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Hidden, with no geometry or animation work.
    Hidden,
    /// Quiet presence.
    Resting,
    /// Slow acquisition pulse.
    Listening,
    /// Fast counter-rotating rings.
    Thinking,
    /// Fast speaking pulse. This is not an audio level measurement.
    Speaking,
}

/// Semantic paint selected by the host's theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Reactor housing.
    Surface,
    /// State-dependent accent.
    Accent,
    /// Dark foreground bars.
    Ink,
}

/// Logical geometry in the canonical 176 by 166 coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// Filled circle.
    Disc {
        /// Logical radius about [`CENTER`].
        radius: f32,
        /// Host-resolved semantic color.
        paint: Paint,
        /// Opacity in the inclusive unit interval.
        alpha: f32,
    },
    /// Circle outline.
    Ring {
        /// Logical radius about [`CENTER`].
        radius: f32,
        /// Logical stroke width.
        width: f32,
        /// Accent opacity.
        alpha: f32,
    },
    /// Clockwise arc, in radians.
    Arc {
        /// Logical radius about [`CENTER`].
        radius: f32,
        /// Start angle in clockwise radians from the right.
        start: f32,
        /// End angle in clockwise radians from the right.
        end: f32,
        /// Logical stroke width.
        width: f32,
        /// Accent opacity.
        alpha: f32,
    },
    /// Centered vertical bar, relative to reactor center.
    Bar {
        /// Left edge relative to [`CENTER`].
        x: f32,
        /// Height, centered vertically on [`CENTER`]; use [`Paint::Ink`].
        height: f32,
        /// Logical bar width.
        width: f32,
        /// Ink opacity.
        alpha: f32,
    },
}

/// Canonical logical size. Scale uniformly and center within the host bounds.
pub const SIZE: [f32; 2] = [176.0, 166.0];
/// Canonical center.
pub const CENTER: [f32; 2] = [88.0, 83.0];
/// Maximum emitted marks per frame; suitable for fixed-capacity adapters.
pub const MAX_MARKS: usize = 38;

/// Builds a shallow, closed 3D ring/arc triangle mesh once, for host retention.
/// Coordinates are centered on the origin in the XY plane, +Z toward the viewer.
/// At most 64 angular segments (516 triangles), including capped arc ends.
/// Invalid geometry returns `None`; animation should rotate the retained mesh,
/// not rebuild it. `span` is in radians and must be in `(0, TAU]`.
pub fn band_mesh(radius: f32, width: f32, depth: f32, span: f32) -> Option<Vec<[f32; 3]>> {
    if ![radius, width, depth, span].into_iter().all(f32::is_finite)
        || radius <= 0.0
        || width <= 0.0
        || width >= radius * 2.0
        || depth <= 0.0
        || span <= 0.0
        || span > TAU
    {
        return None;
    }
    let segments = (span / TAU * 64.0).ceil().max(1.0) as u8;
    let mut vertices = Vec::with_capacity(usize::from(segments) * 24 + 12);
    let point = |angle: f32, r: f32, z: f32| [angle.cos() * r, angle.sin() * r, z];
    let inner = radius - width * 0.5;
    let outer = radius + width * 0.5;
    let z = depth * 0.5;
    if !inner.is_finite() || !outer.is_finite() || z <= 0.0 {
        return None;
    }
    let mut quad = |a, b, c, d| vertices.extend_from_slice(&[a, b, c, a, c, d]);
    for i in 0..segments {
        let a = span * f32::from(i) / f32::from(segments);
        let b = if i + 1 == segments && span == TAU {
            0.0 // Close the full-circle seam exactly, not approximately at sin(TAU).
        } else {
            span * (f32::from(i) + 1.0) / f32::from(segments)
        };
        quad(
            point(a, inner, z),
            point(a, outer, z),
            point(b, outer, z),
            point(b, inner, z),
        );
        quad(
            point(b, inner, -z),
            point(b, outer, -z),
            point(a, outer, -z),
            point(a, inner, -z),
        );
        quad(
            point(a, outer, z),
            point(a, outer, -z),
            point(b, outer, -z),
            point(b, outer, z),
        );
        quad(
            point(b, inner, z),
            point(b, inner, -z),
            point(a, inner, -z),
            point(a, inner, z),
        );
    }
    if span < TAU {
        quad(
            point(0.0, inner, -z),
            point(0.0, outer, -z),
            point(0.0, outer, z),
            point(0.0, inner, z),
        );
        quad(
            point(span, inner, z),
            point(span, outer, z),
            point(span, outer, -z),
            point(span, inner, -z),
        );
    }
    Some(vertices)
}

/// Emits one deterministic frame without allocating, scheduling or owning input.
/// Non-finite clocks use the static zero frame; hidden activity emits nothing.
pub fn visit(activity: Activity, seconds: f32, mut emit: impl FnMut(Mark)) {
    visit_with_level(activity, seconds, None, &mut emit);
}

/// Emits a frame with an optional measured playback envelope in `0..=1`.
/// A level of zero means actual silence, not missing telemetry. Levels only
/// affect speaking geometry; invalid levels fall back to activity animation.
pub fn visit_with_level(
    activity: Activity,
    seconds: f32,
    level: Option<f32>,
    mut emit: impl FnMut(Mark),
) {
    if activity == Activity::Hidden {
        return;
    }
    let t = if seconds.is_finite() { seconds } else { 0.0 };
    emit(Mark::Disc {
        radius: 67.0,
        paint: Paint::Surface,
        alpha: 1.0,
    });
    for (radius, alpha) in [(67.0, 0.22), (61.0, 0.38), (41.0, 0.35)] {
        emit(Mark::Ring {
            radius,
            width: 1.0,
            alpha,
        });
    }
    let speed = if activity == Activity::Thinking {
        2.9
    } else {
        0.45
    };
    for (radius, count, direction) in [(53.0, 12_u8, 1.0), (34.0, 3, -1.4)] {
        let rotation =
            (f64::from(t) * f64::from(speed) * direction).rem_euclid(f64::from(TAU)) as f32;
        for index in 0..count {
            let start = f32::from(index) * TAU / f32::from(count) + rotation;
            let strong = index % 3 == 0 || count == 3;
            emit(Mark::Arc {
                radius,
                start,
                end: start + TAU / f32::from(count) * 0.72,
                width: if strong { 3.5 } else { 1.5 },
                alpha: if strong { 0.95 } else { 0.4 },
            });
        }
    }
    let frequency = if activity == Activity::Speaking {
        12.0
    } else {
        3.0
    };
    let wave =
        |rate: f32, phase: f32| (f64::from(t) * f64::from(rate) + f64::from(phase)).sin() as f32;
    let measured = level
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0))
        .filter(|_| activity == Activity::Speaking);
    let pulse = measured.unwrap_or_else(|| wave(frequency, 0.0) * 0.5 + 0.5);
    for radius in (16_u8..29).rev() {
        emit(Mark::Disc {
            radius: f32::from(radius),
            paint: Paint::Accent,
            alpha: 0.012 + pulse * 0.004,
        });
    }
    emit(Mark::Disc {
        radius: 16.0,
        paint: Paint::Accent,
        alpha: if activity == Activity::Thinking {
            0.3 + pulse * 0.3
        } else {
            0.7 + pulse * 0.3
        },
    });
    if activity != Activity::Thinking {
        for index in 0_u8..5 {
            let i = f32::from(index);
            emit(Mark::Bar {
                x: (i - 2.0) * 5.0 - 1.5,
                height: 4.0
                    + 15.0
                        * measured.map_or_else(
                            || wave(frequency + i * 0.7, i * PI / 3.0).abs(),
                            |v| v * (1.0 - (i - 2.0).abs() * 0.15),
                        ),
                width: 3.0,
                alpha: 0.95,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shallow_mesh_is_bounded_and_rejects_invalid_geometry() {
        for span in [TAU, TAU * 0.06, TAU * 0.24] {
            let mesh = band_mesh(53.0, 3.5, 2.4, span).unwrap();
            assert!(mesh.len() <= 1548 && mesh.len().is_multiple_of(3));
            assert!(mesh.iter().flatten().all(|v| v.is_finite()));
            assert!(mesh.iter().all(|v| v[2].abs() == 1.2));
        }
        assert!(band_mesh(1.0, 2.0, 1.0, TAU).is_none());
        assert!(band_mesh(1.0, 1.0, 1.0, f32::NAN).is_none());
        assert!(band_mesh(f32::MAX, f32::MAX, 1.0, TAU).is_none());
        assert!(band_mesh(1.0, 1.0, 1.0, 0.0).is_none());
    }
    #[test]
    fn measured_speech_distinguishes_silence_from_missing_levels() {
        let frame = |level| {
            let mut out = Vec::new();
            visit_with_level(Activity::Speaking, 1.0, level, |m| out.push(m));
            out
        };
        assert_ne!(frame(None), frame(Some(0.0)));
        assert_eq!(frame(Some(f32::NAN)), frame(None));
        assert_eq!(frame(Some(2.0)), frame(Some(1.0)));
        assert_eq!(frame(Some(-1.0)), frame(Some(0.0)));
        assert!(frame(Some(0.0))
            .iter()
            .filter_map(|m| match m {
                Mark::Bar { height, .. } => Some(*height),
                _ => None,
            })
            .all(|h| h == 4.0));
    }
    fn frame(activity: Activity, time: f32) -> Vec<Mark> {
        let mut marks = Vec::new();
        visit(activity, time, |mark| marks.push(mark));
        marks
    }
    #[test]
    fn hidden_is_empty_and_frames_are_bounded() {
        assert!(frame(Activity::Hidden, 1.0).is_empty());
        for activity in [
            Activity::Resting,
            Activity::Listening,
            Activity::Thinking,
            Activity::Speaking,
        ] {
            for time in [0.0, -10.0, 1.25, f32::MAX] {
                let marks = frame(activity, time);
                assert!(marks.len() <= MAX_MARKS);
                assert_eq!(marks, frame(activity, time));
                for mark in marks {
                    match mark {
                        Mark::Disc { radius, alpha, .. } => {
                            assert!(radius.is_finite());
                            assert!((0.0..=1.0).contains(&alpha));
                        }
                        Mark::Ring {
                            radius,
                            width,
                            alpha,
                        } => {
                            assert!(radius > 0.0 && width > 0.0);
                            assert!((0.0..=1.0).contains(&alpha));
                        }
                        Mark::Arc { start, end, .. } => {
                            assert!(start.is_finite() && end.is_finite() && end > start)
                        }
                        Mark::Bar { height, .. } => assert!((4.0..=19.0).contains(&height)),
                    }
                }
            }
        }
    }
    #[test]
    fn invalid_clock_is_static_and_activity_changes_motion() {
        assert_eq!(
            frame(Activity::Speaking, f32::NAN),
            frame(Activity::Speaking, 0.0)
        );
        assert_ne!(
            frame(Activity::Speaking, 1.0),
            frame(Activity::Listening, 1.0)
        );
        assert_eq!(frame(Activity::Thinking, 1.0).len(), 33);
        assert_eq!(frame(Activity::Speaking, 1.0).len(), MAX_MARKS);
    }
}
