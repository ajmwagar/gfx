//! Bounded, theme-role chart geometry. Providers own acquisition and reduction;
//! hosts own glyphs, input, and revisioned viewport state.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use crate::{topology::Label, Primitive, Rect, Scene, ThemeRole};
use serde::{Deserialize, Serialize};

/// Maximum independently colored traces.
pub const MAX_SERIES: usize = 4;
/// Maximum observations per trace. Reduce source data upstream, preserving peaks.
pub const MAX_POINTS: usize = 4096;

/// A physical axis domain, independent of the visible viewport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    /// Human-readable physical quantity.
    pub label: String,
    /// Explicit unit, e.g. s, V, Hz, or percent; empty for dimensionless values.
    pub unit: String,
    /// Lower domain edge.
    pub minimum: f64,
    /// Upper domain edge.
    pub maximum: f64,
}

/// One observation. `None` means a gap; never connect across missing data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Physical horizontal position, not an array index.
    pub x: f64,
    /// Physical value; missing is not zero.
    pub y: Option<f64>,
}

/// A stable trace identity and themeable legend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    /// Stable series ID, distinct within this plot.
    pub id: String,
    /// Legend caption, e.g. U1 pin 3 output.
    pub label: String,
    /// Semantic color shared by the trace and legend swatch.
    pub role: ThemeRole,
    /// Strictly increasing physical X coordinates, including explicit gaps.
    pub points: Vec<Sample>,
}

/// Visible subset of the declared domains. Application state, not source data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    /// Visible left edge.
    pub x_minimum: f64,
    /// Visible right edge.
    pub x_maximum: f64,
    /// Visible lower edge.
    pub y_minimum: f64,
    /// Visible upper edge.
    pub y_maximum: f64,
}

/// Shared chart contract: axes, physical samples, grid and trace legend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plot {
    /// Horizontal domain and quantity.
    pub x: Axis,
    /// Vertical domain and quantity.
    pub y: Axis,
    /// Bounded traces, including empty traces without fabricated observations.
    pub series: Vec<Trace>,
    /// Optional zoom/pan window; absent means the full physical domain.
    #[serde(default)]
    pub viewport: Option<Viewport>,
}

/// Invalid geometry or chart data, surfaced before any scene mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid plot geometry, domain, or observations")]
pub struct PlotError;

fn range(lo: f64, hi: f64) -> bool {
    lo.is_finite() && hi.is_finite() && hi > lo && (hi - lo).is_finite()
}
fn caption(axis: &Axis) -> String {
    if axis.unit.is_empty() {
        axis.label.clone()
    } else {
        format!("{} ({})", axis.label, axis.unit)
    }
}
fn tick(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if value.abs() < 0.001 || value.abs() >= 10000.0 {
        format!("{value:.2e}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

impl Plot {
    /// Validate the complete input before generating graphics or labels.
    /// # Errors
    /// Rejects invalid units, identities, ranges, gaps, ordering and bounds.
    pub fn validate(&self) -> Result<Viewport, PlotError> {
        for a in [&self.x, &self.y] {
            if !range(a.minimum, a.maximum)
                || a.label.is_empty()
                || a.label.chars().count() > 64
                || a.unit.chars().count() > 16
            {
                return Err(PlotError);
            }
        }
        if self.series.is_empty() || self.series.len() > MAX_SERIES {
            return Err(PlotError);
        }
        for (i, s) in self.series.iter().enumerate() {
            if s.id.is_empty()
                || s.id.len() > 64
                || s.label.is_empty()
                || s.label.chars().count() > 96
                || s.points.len() > MAX_POINTS
                || self.series[..i].iter().any(|other| other.id == s.id)
                || s.points.windows(2).any(|p| p[1].x <= p[0].x)
                || s.points.iter().any(|p| {
                    !p.x.is_finite()
                        || p.x < self.x.minimum
                        || p.x > self.x.maximum
                        || p.y.is_some_and(|y| {
                            !y.is_finite()
                                || !((y - self.y.minimum) / (self.y.maximum - self.y.minimum))
                                    .is_finite()
                        })
                })
            {
                return Err(PlotError);
            }
        }
        let v = self.viewport.unwrap_or(Viewport {
            x_minimum: self.x.minimum,
            x_maximum: self.x.maximum,
            y_minimum: self.y.minimum,
            y_maximum: self.y.maximum,
        });
        if !range(v.x_minimum, v.x_maximum)
            || !range(v.y_minimum, v.y_maximum)
            || v.x_minimum < self.x.minimum
            || v.x_maximum > self.x.maximum
            || v.y_minimum < self.y.minimum
            || v.y_maximum > self.y.maximum
        {
            return Err(PlotError);
        }
        // Coordinate conversion must remain finite even at a deeply zoomed view.
        if self.series.iter().flat_map(|s| &s.points).any(|p| {
            !((p.x - v.x_minimum) / (v.x_maximum - v.x_minimum)).is_finite()
                || p.y
                    .is_some_and(|y| !((y - v.y_minimum) / (v.y_maximum - v.y_minimum)).is_finite())
        }) {
            return Err(PlotError);
        }
        Ok(v)
    }

    fn placement(&self, bounds: Rect) -> Result<(Viewport, Rect, usize), PlotError> {
        let viewport = self.validate()?;
        if !bounds.is_valid()
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
            || bounds.width < 240.0
            || bounds.height < 120.0
            || bounds.width > 16384.0
            || bounds.height > 16384.0
        {
            return Err(PlotError);
        }
        let columns = if bounds.width >= 640.0 { 4 } else { 2 };
        let rows = self.series.len().div_ceil(columns);
        let top = 26.0 + rows as f32 * 22.0;
        let inner = Rect::new(
            bounds.x + 62.0,
            bounds.y + top,
            bounds.width - 76.0,
            bounds.height - top - 42.0,
        );
        if inner.height < 24.0 {
            return Err(PlotError);
        }
        Ok((viewport, inner, columns))
    }

    /// Append retained GPU-friendly grid, axes, swatches and clipped trace lines.
    /// # Errors
    /// Invalid inputs leave the scene untouched.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), PlotError> {
        let (v, inner, columns) = self.placement(bounds)?;
        scene.push(Primitive::rounded_rect(
            bounds,
            0.0,
            ThemeRole::SurfaceRecessed,
        ));
        for i in 0..=5 {
            let y = inner.bottom() - i as f32 * inner.height / 5.0;
            scene.push(Primitive::line(
                [inner.x, y],
                [inner.right(), y],
                0.5,
                ThemeRole::Outline,
            ));
        }
        let divisions = if inner.width >= 460.0 { 10 } else { 5 };
        for i in 0..=divisions {
            let x = inner.x + i as f32 * inner.width / divisions as f32;
            scene.push(Primitive::line(
                [x, inner.y],
                [x, inner.bottom()],
                0.5,
                ThemeRole::Outline,
            ));
        }
        for (i, trace) in self.series.iter().enumerate() {
            let lx = bounds.x + 8.0 + (i % columns) as f32 * (bounds.width - 16.0) / columns as f32;
            let ly = bounds.y + 30.0 + (i / columns) as f32 * 22.0;
            scene.push(Primitive::line(
                [lx, ly + 8.0],
                [lx + 18.0, ly + 8.0],
                2.0,
                trace.role,
            ));
            for pair in trace.points.windows(2) {
                let (Some(a), Some(b)) = (pair[0].y, pair[1].y) else {
                    continue;
                };
                let p0 = [
                    (pair[0].x - v.x_minimum) / (v.x_maximum - v.x_minimum),
                    (a - v.y_minimum) / (v.y_maximum - v.y_minimum),
                ];
                let p1 = [
                    (pair[1].x - v.x_minimum) / (v.x_maximum - v.x_minimum),
                    (b - v.y_minimum) / (v.y_maximum - v.y_minimum),
                ];
                if let Some((a, b)) = clip(p0, p1) {
                    let map = |p: [f64; 2]| {
                        [
                            inner.x + p[0] as f32 * inner.width,
                            inner.bottom() - p[1] as f32 * inner.height,
                        ]
                    };
                    scene.push(Primitive::line(map(a), map(b), 1.5, trace.role));
                }
            }
        }
        Ok(())
    }

    /// Axis/tick/legend glyph placement shared by all native paint adapters.
    /// # Errors
    /// Rejects the same inputs as geometry generation.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, PlotError> {
        let (v, inner, columns) = self.placement(bounds)?;
        let mut labels = vec![
            Label {
                text: caption(&self.y),
                bounds: Rect::new(bounds.x + 8.0, bounds.y + 4.0, bounds.width - 16.0, 20.0),
                role: ThemeRole::Text,
            },
            Label {
                text: caption(&self.x),
                bounds: Rect::new(
                    inner.x + inner.width * 0.35,
                    bounds.bottom() - 20.0,
                    inner.width * 0.65,
                    20.0,
                ),
                role: ThemeRole::Text,
            },
        ];
        for (i, s) in self.series.iter().enumerate() {
            let width = (bounds.width - 16.0) / columns as f32;
            labels.push(Label {
                text: s.label.clone(),
                bounds: Rect::new(
                    bounds.x + 30.0 + (i % columns) as f32 * width,
                    bounds.y + 28.0 + (i / columns) as f32 * 22.0,
                    width - 24.0,
                    20.0,
                ),
                role: s.role,
            });
        }
        for i in 0..=5 {
            let y = inner.bottom() - i as f32 * inner.height / 5.0;
            labels.push(Label {
                text: tick(v.y_minimum + (v.y_maximum - v.y_minimum) * f64::from(i) / 5.0),
                bounds: Rect::new(bounds.x + 4.0, y - 8.0, 54.0, 16.0),
                role: ThemeRole::TextMuted,
            });
        }
        let divisions = if inner.width >= 460.0 { 10 } else { 5 };
        for i in 0..=divisions {
            let x = inner.x + i as f32 * inner.width / divisions as f32;
            labels.push(Label {
                text: tick(
                    v.x_minimum + (v.x_maximum - v.x_minimum) * f64::from(i) / f64::from(divisions),
                ),
                bounds: Rect::new(
                    (x - 15.0).clamp(bounds.x, bounds.right() - 42.0),
                    inner.bottom() + 3.0,
                    42.0,
                    16.0,
                ),
                role: ThemeRole::TextMuted,
            });
        }
        Ok(labels)
    }
}

// Liang–Barsky in normalized coordinates; clipping precedes f32 conversion.
fn clip(a: [f64; 2], b: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let d = [b[0] - a[0], b[1] - a[1]];
    if d.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for (p, q) in [
        (-d[0], a[0]),
        (d[0], 1.0 - a[0]),
        (-d[1], a[1]),
        (d[1], 1.0 - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                lo = lo.max(t);
            } else {
                hi = hi.min(t);
            }
            if lo > hi {
                return None;
            }
        }
    }
    let at = |t: f64| {
        [
            (a[0] + d[0] * t).clamp(0.0, 1.0),
            (a[1] + d[1] * t).clamp(0.0, 1.0),
        ]
    };
    Some((at(lo), at(hi)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Plot {
        Plot {
            x: Axis {
                label: "Time".into(),
                unit: "s".into(),
                minimum: 0.,
                maximum: 1.,
            },
            y: Axis {
                label: "Voltage".into(),
                unit: "V".into(),
                minimum: 0.,
                maximum: 5.,
            },
            series: vec![Trace {
                id: "output".into(),
                label: "U1 pin 3 output".into(),
                role: ThemeRole::Primary,
                points: vec![Sample { x: 0., y: Some(0.) }, Sample { x: 1., y: Some(4.) }],
            }],
            viewport: None,
        }
    }
    #[test]
    fn labels_include_units_ticks_and_legend_with_fitting_bounds() {
        let p = fixture();
        let b = Rect::new(0., 0., 600., 180.);
        let labels = p.labels(b).unwrap();
        for text in ["Time (s)", "Voltage (V)", "U1 pin 3 output", "0", "5", "1"] {
            assert!(labels.iter().any(|l| l.text == text));
        }
        assert!(labels.iter().all(|l| l.bounds.height >= 16.
            && l.bounds.x >= 0.
            && l.bounds.y >= 0.
            && l.bounds.right() <= b.right()
            && l.bounds.bottom() <= b.bottom()));
        let mut scene = Scene::new();
        p.append(&mut scene, b).unwrap();
        assert!(!scene.is_empty());
    }
    #[test]
    fn gaps_do_not_connect_and_viewport_clips() {
        let mut p = fixture();
        p.series[0].points.insert(1, Sample { x: 0.5, y: None });
        let mut scene = Scene::new();
        p.append(&mut scene, Rect::new(0., 0., 600., 180.)).unwrap();
        assert_eq!(
            scene
                .iter()
                .filter(|q| q.fill == ThemeRole::Primary)
                .count(),
            1
        ); // legend only
        assert_eq!(clip([-1., 0.5], [2., 0.5]), Some(([0., 0.5], [1., 0.5])));
        p.viewport = Some(Viewport {
            x_minimum: 0.2,
            x_maximum: 0.8,
            y_minimum: 1.,
            y_maximum: 4.,
        });
        assert!(p.validate().is_ok());
    }
    #[test]
    fn invalid_inputs_fail_before_mutation() {
        let mut p = fixture();
        p.series[0].points[1].x = 0.;
        let mut scene = Scene::new();
        assert!(p.append(&mut scene, Rect::new(0., 0., 600., 180.)).is_err());
        assert!(scene.is_empty());
        p = fixture();
        p.series[0].points[1].y = Some(f64::NAN);
        assert!(p.validate().is_err());
        p = fixture();
        p.series.push(p.series[0].clone());
        assert!(p.validate().is_err());
    }
    #[test]
    fn serde_contract_roundtrip_is_strict() {
        let p = fixture();
        assert_eq!(
            serde_json::from_value::<Plot>(serde_json::to_value(&p).unwrap()).unwrap(),
            p
        );
        let mut v = serde_json::to_value(p).unwrap();
        v["surprise"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Plot>(v).is_err());
    }
    #[test]
    fn four_series_legends_and_clipped_geometry_fit_compact_bounds() {
        let mut p = fixture();
        for (id, role) in [
            ("timing", ThemeRole::Secondary),
            ("supply", ThemeRole::Text),
            ("load", ThemeRole::TextMuted),
        ] {
            let mut trace = p.series[0].clone();
            trace.id = id.into();
            trace.label = id.into();
            trace.role = role;
            trace.points[0].y = Some(-5.);
            trace.points[1].y = Some(10.);
            p.series.push(trace);
        }
        let bounds = Rect::new(12., 24., 360., 160.);
        let labels = p.labels(bounds).unwrap();
        assert!(labels.iter().all(|l| l.bounds.x >= bounds.x
            && l.bounds.y >= bounds.y
            && l.bounds.right() <= bounds.right()
            && l.bounds.bottom() <= bounds.bottom()));
        let mut scene = Scene::new();
        p.append(&mut scene, bounds).unwrap();
        for q in scene.iter() {
            if q.kind == crate::PrimitiveKind::Line {
                let center = q.rect.center();
                for sign in [-1.0, 1.0] {
                    let x = center.x + sign * q.value * q.rotation.cos();
                    let y = center.y + sign * q.value * q.rotation.sin();
                    assert!(x >= bounds.x && x <= bounds.right());
                    assert!(y >= bounds.y && y <= bounds.bottom());
                }
            } else {
                assert!(
                    q.rect.x >= bounds.x
                        && q.rect.y >= bounds.y
                        && q.rect.right() <= bounds.right()
                        && q.rect.bottom() <= bounds.bottom()
                );
            }
        }
    }
}
