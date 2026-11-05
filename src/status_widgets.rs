//! Renderer-neutral read-only status cards. Hosts own permission and freshness.
pub mod model_estimate;
pub mod volume;
pub use volume::VolumeView;
use crate::ThemeRole;
pub use model_estimate::ModelEstimateView;

/// Availability is explicit; cached numbers never imply freshness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Availability {
    /// Provider declares this reading current.
    Fresh,
    /// No usable observation.
    #[default]
    Unknown,
    /// Expired observation; numbers are hidden.
    Stale,
    /// Access was denied; numbers are hidden.
    Denied,
}

/// Caller-owned telemetry, already normalized to microvolts by its provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatteryView {
    /// Reported SOC only; never derived from voltage.
    pub percent: Option<u8>,
    /// Measured microvolts, without a UI accuracy claim.
    pub voltage_uv: Option<u32>,
    /// Host-supplied permission/freshness classification.
    pub availability: Availability,
}
impl BatteryView {
    /// Reject out-of-range SOC without modifying any state.
    pub fn validate(self) -> Result<(), String> {
        if self.percent.is_some_and(|p| p > 100) {
            return Err("battery percent must be 0..100".into());
        }
        Ok(())
    }
    /// Human-readable availability without numeric leakage.
    pub fn state_label(self) -> &'static str {
        match self.availability {
            Availability::Fresh => "Fresh",
            Availability::Unknown => "Unknown",
            Availability::Stale => "Stale",
            Availability::Denied => "Denied",
        }
    }
}

/// Geometry in card-local [0,1] coordinates, uniformly projected by the host.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    /// Bounded normalized vector icon stroke.
    Line {
        /// Connected normalized points.
        points: Vec<[f32; 2]>,
        /// Semantic stroke color.
        role: ThemeRole,
    },
    /// Filled semantic rectangle.
    Rect {
        /// x, y, width, height in normalized card coordinates.
        bounds: [f32; 4],
        /// Theme color role.
        role: ThemeRole,
    },
    /// Single bounded line, host clips/elides to card width.
    Text {
        /// Normalized top-left anchor.
        at: [f32; 2],
        /// Literal display content.
        text: String,
        /// Theme color role.
        role: ThemeRole,
    },
}
/// A generic bounded progress indicator. Unknown has no filled portion.
pub fn visit_progress(fraction: Option<f32>, mut emit: impl FnMut(Mark)) {
    emit(Mark::Rect {
        bounds: [0.04, 0.86, 0.92, 0.05],
        role: ThemeRole::SurfaceRaised,
    });
    if let Some(f) = fraction.filter(|f| f.is_finite() && (0.0..=1.0).contains(f)) {
        emit(Mark::Rect {
            bounds: [0.04, 0.86, 0.92 * f, 0.05],
            role: ThemeRole::Primary,
        });
    }
}
/// Battery card; unknown/stale/denied numbers are deliberately not emitted.
pub fn visit_battery(
    view: BatteryView,
    compact: bool,
    emit: impl FnMut(Mark),
) -> Result<(), String> {
    visit_battery_composed(view, compact, false, emit)
}
/// Composition opt-in: omit unknown SOC only when a separate model card exists.
/// Fresh measured SOC is never hidden; retained readings are never altered.
pub fn visit_battery_composed(
    view: BatteryView,
    compact: bool,
    hide_unknown_soc: bool,
    mut emit: impl FnMut(Mark),
) -> Result<(), String> {
    view.validate()?;
    emit(Mark::Rect {
        bounds: [0.0, 0.0, 1.0, 1.0],
        role: ThemeRole::Surface,
    });
    let fresh = view.availability == Availability::Fresh;
    let percent = if fresh { view.percent } else { None };
    // Vector battery housing, no font/emoji dependence.
    emit(Mark::Line {
        points: vec![
            [0.04, 0.12],
            [0.16, 0.12],
            [0.16, 0.38],
            [0.04, 0.38],
            [0.04, 0.12],
        ],
        role: ThemeRole::Primary,
    });
    emit(Mark::Line {
        points: vec![[0.08, 0.08], [0.12, 0.08]],
        role: ThemeRole::Primary,
    });
    let detail = if fresh {
        view.voltage_uv
            .map(|v| format!("{}.{:03} V", v / 1_000_000, (v % 1_000_000) / 1000))
            .unwrap_or_else(|| "— V".into())
    } else {
        "— V".into()
    };
    emit(Mark::Text {
        at: [0.23, 0.05],
        text: detail,
        role: ThemeRole::Text,
    });
    if percent.is_some() || !hide_unknown_soc {
        // Gauge for measured SOC; no voltage-derived fill.
        emit(Mark::Line {
            points: vec![
                [0.04, 0.72],
                [0.04, 0.60],
                [0.10, 0.48],
                [0.16, 0.60],
                [0.16, 0.72],
            ],
            role: ThemeRole::TextMuted,
        });
        emit(Mark::Line {
            points: vec![[0.10, 0.68], [0.14, 0.57]],
            role: ThemeRole::TextMuted,
        });
        emit(Mark::Text {
            at: [0.23, 0.48],
            text: percent.map_or_else(|| "—%".into(), |p| format!("{p}%")),
            role: ThemeRole::Text,
        });
    }
    if !fresh {
        // Clock=stale, lock=denied, warning=unknown.
        let points = match view.availability {
            Availability::Stale => vec![
                [0.87, 0.50],
                [0.94, 0.50],
                [0.98, 0.61],
                [0.94, 0.73],
                [0.87, 0.73],
                [0.83, 0.61],
                [0.87, 0.50],
                [0.90, 0.61],
                [0.94, 0.61],
            ],
            Availability::Denied => vec![
                [0.85, 0.60],
                [0.85, 0.73],
                [0.97, 0.73],
                [0.97, 0.60],
                [0.85, 0.60],
                [0.87, 0.50],
                [0.95, 0.50],
                [0.97, 0.60],
            ],
            _ => vec![
                [0.83, 0.73],
                [0.90, 0.48],
                [0.98, 0.73],
                [0.83, 0.73],
                [0.90, 0.56],
                [0.90, 0.64],
            ],
        };
        emit(Mark::Line {
            points,
            role: ThemeRole::Warning,
        });
    }
    let _ = compact;
    if percent.is_some() || !hide_unknown_soc {
        visit_progress(percent.map(|p| p as f32 / 100.0), emit);
    }
    Ok(())
}
/// Honest source-status card, not synthetic health or activity telemetry.
pub fn visit_status(view: BatteryView, mut emit: impl FnMut(Mark)) {
    emit(Mark::Rect {
        bounds: [0.0, 0.0, 1.0, 1.0],
        role: ThemeRole::Surface,
    });
    emit(Mark::Text {
        at: [0.04, 0.12],
        text: "Read-only battery".into(),
        role: ThemeRole::Text,
    });
    emit(Mark::Text {
        at: [0.04, 0.55],
        text: format!("Source: {}", view.state_label()),
        role: ThemeRole::TextMuted,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_composition_hides_only_unknown_measured_placeholder() {
        for percent in [None, Some(42)] {
            let mut marks = Vec::new();
            visit_battery_composed(
                BatteryView {
                    percent,
                    voltage_uv: Some(3_879_000),
                    availability: Availability::Fresh,
                },
                true,
                true,
                |m| marks.push(m),
            )
            .unwrap();
            assert!(
                !marks
                    .iter()
                    .any(|m| matches!(m,Mark::Text{text,..} if text=="—%"))
            );
            assert_eq!(
                marks
                    .iter()
                    .any(|m| matches!(m,Mark::Text{text,..} if text=="42%")),
                percent.is_some()
            );
            assert!(
                marks
                    .iter()
                    .any(|m| matches!(m,Mark::Text{text,..} if text=="3.879 V"))
            );
        }
    }
    #[test]
    fn range_and_unknown_states() {
        assert!(
            BatteryView {
                percent: Some(101),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        for availability in [
            Availability::Unknown,
            Availability::Stale,
            Availability::Denied,
        ] {
            let mut marks = Vec::new();
            visit_battery(
                BatteryView {
                    percent: Some(79),
                    voltage_uv: Some(3_879_000),
                    availability,
                },
                true,
                |m| marks.push(m),
            )
            .unwrap();
            let text = format!("{marks:?}");
            assert!(!text.contains("79%") && !text.contains("3.879"));
            assert!(marks.iter().any(|m| matches!(
                m,
                Mark::Line {
                    role: ThemeRole::Warning,
                    ..
                }
            )));
        }
    }
    #[test]
    fn voltage_does_not_invent_soc_and_geometry_is_bounded() {
        let mut marks = Vec::new();
        visit_battery(
            BatteryView {
                percent: None,
                voltage_uv: Some(3_879_000),
                availability: Availability::Fresh,
            },
            true,
            |m| marks.push(m),
        )
        .unwrap();
        let text = format!("{marks:?}");
        assert!(text.contains("3.879 V") && text.contains("—%"));
        for mark in marks {
            match mark {
                Mark::Line { points, .. } => assert!(
                    points
                        .iter()
                        .all(|[x, y]| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y))
                ),
                Mark::Rect {
                    bounds: [x, y, w, h],
                    ..
                } => assert!(x >= 0.0 && y >= 0.0 && x + w <= 1.0 && y + h <= 1.0),
                Mark::Text { at: [x, y], .. } => {
                    assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
                }
            }
        }
    }
}
