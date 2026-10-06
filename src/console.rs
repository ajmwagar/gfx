//! Read-only mixing-console strips and calibrated peak-meter faces.
//! Hosts own measurements, ballistics, controls and theme colors.
use crate::{
    Primitive, Rect, Scene, ThemeRole,
    signals::ViewError,
    topology::{Label, State},
};
use serde::{Deserialize, Serialize};

/// Meter face variant. Both display caller-provided dB, not synthesized VU data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeterStyle {
    /// Wide analog scale with needle, bezel and red-zone markings.
    Needle,
    /// Segmented level bank with caution/clipping colors.
    Led,
}

/// Calibration and geometry, independent of the host's semantic color palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterConfig {
    /// Vertical LED orientation for console channel strips.
    #[serde(default)]
    pub vertical_led: bool,
    /// Analog needle or segmented LED display.
    pub style: MeterStyle,
    /// Lower scale bound in caller's dB convention.
    pub minimum_db: f32,
    /// Upper scale bound, including any requested headroom.
    pub maximum_db: f32,
    /// Beginning of caution markings.
    pub warning_db: f32,
    /// Beginning of clipping markings.
    pub clip_db: f32,
    /// LED segment count, 8..=48.
    pub segments: u8,
    /// Faceplate role; e.g. Highlight for a light vintage face.
    pub face: ThemeRole,
    /// Scale ink role, independently configurable for light/dark faces.
    pub ink: ThemeRole,
    /// Optional host-side visual interpolation time. Does not implement VU ballistics.
    #[serde(default)]
    pub smoothing_ms: u16,
}
impl Default for MeterConfig {
    fn default() -> Self {
        Self {
            vertical_led: false,
            style: MeterStyle::Led,
            minimum_db: -60.0,
            maximum_db: 6.0,
            warning_db: -12.0,
            clip_db: 0.0,
            segments: 24,
            face: ThemeRole::SurfaceRecessed,
            ink: ThemeRole::Text,
            smoothing_ms: 0,
        }
    }
}
impl MeterConfig {
    fn valid(&self) -> bool {
        [
            self.minimum_db,
            self.maximum_db,
            self.warning_db,
            self.clip_db,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.minimum_db >= -160.0
            && self.maximum_db <= 60.0
            && self.minimum_db < self.warning_db
            && self.warning_db < self.clip_db
            && self.clip_db < self.maximum_db
            && (8..=48).contains(&self.segments)
            && self.smoothing_ms <= 2000
    }
    fn fraction(&self, db: f32) -> f32 {
        ((db - self.minimum_db) / (self.maximum_db - self.minimum_db)).clamp(0.0, 1.0)
    }
    fn role(&self, db: f32) -> ThemeRole {
        if db >= self.clip_db {
            ThemeRole::Danger
        } else if db >= self.warning_db {
            ThemeRole::Warning
        } else {
            ThemeRole::MeterLow
        }
    }
}

/// One channel's display snapshot. None is missing data, never fabricated silence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strip {
    /// False for routing-only/controller/rack cards without an audio meter.
    #[serde(default = "default_show_meter")]
    pub show_meter: bool,
    /// Human-facing source/bus name.
    pub name: String,
    /// Destination, send/return annotation or provenance.
    pub detail: String,
    /// Observed routing state, not audio activity inferred from a cable.
    pub state: State,
    /// Peak dBFS for this first slice; hosts must not pass RMS as peak.
    pub peak_dbfs: Option<f32>,
    /// Actual observed gain; absent disables the visual fader reference.
    pub gain_db: Option<f32>,
}
fn default_show_meter() -> bool {
    true
}

/// Bounded read-only strip panel. Interaction belongs to the host's typed API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Console {
    /// At most sixteen channel strips.
    pub strips: Vec<Strip>,
    /// True for side-by-side master strips; false for compact source rows.
    pub columns: bool,
    /// Shared meter geometry and calibration.
    pub meter: MeterConfig,
}
impl Console {
    fn validate(&self, b: Rect) -> Result<(), ViewError> {
        if !b.is_valid()
            || b.width < 2.0
            || b.height < 2.0
            || b.width > 16384.0
            || b.height > 16384.0
            || b.x.abs() > 1_000_000.0
            || b.y.abs() > 1_000_000.0
            || self.strips.len() > 16
            || !self.meter.valid()
            || self.strips.iter().any(|s| {
                s.name.is_empty()
                    || s.name.chars().count() > 96
                    || s.detail.chars().count() > 160
                    || s.peak_dbfs
                        .is_some_and(|v| !v.is_finite() || !(-160.0..=60.0).contains(&v))
                    || s.gain_db
                        .is_some_and(|v| !v.is_finite() || !(-120.0..=24.0).contains(&v))
            })
        {
            return Err(ViewError);
        }
        Ok(())
    }
    fn cells(&self, b: Rect) -> Result<Vec<Rect>, ViewError> {
        self.validate(b)?;
        let count = f32::from(u8::try_from(self.strips.len().max(1)).map_err(|_| ViewError)?);
        self.strips
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let i = f32::from(u8::try_from(i).map_err(|_| ViewError)?);
                Ok(if self.columns {
                    Rect::new(b.x + i * b.width / count, b.y, b.width / count, b.height)
                } else {
                    Rect::new(b.x, b.y + i * b.height / count, b.width, b.height / count)
                })
            })
            .collect()
    }
    /// Append bounded bezel, meters, status rail and observed gain reference.
    /// # Errors
    /// Invalid measurements/configuration fail before the scene is modified.
    pub fn append(&self, scene: &mut Scene, b: Rect) -> Result<(), ViewError> {
        let cells = self.cells(b)?;
        for (strip, cell) in self.strips.iter().zip(cells) {
            let card = Rect::new(
                cell.x + cell.width * if self.columns { 0.0 } else { 0.02 },
                cell.y + cell.height * 0.03,
                cell.width * if self.columns { 1.0 } else { 0.96 },
                cell.height * 0.94,
            );
            scene.push(Primitive::rounded_rect(
                card,
                if self.columns {
                    0.0
                } else {
                    6.0_f32.min(card.height * 0.08)
                },
                ThemeRole::SurfaceRaised,
            ));
            scene.push(Primitive::rounded_rect(
                Rect::new(card.x, card.y, 2.0_f32.min(card.width), card.height),
                1.0,
                strip.state.role(),
            ));
            if strip.show_meter && strip.peak_dbfs.is_some() {
                let face = Rect::new(
                    card.x + card.width * 0.07,
                    card.y + card.height * if self.columns { 0.27 } else { 0.52 },
                    card.width * if self.columns { 0.86 } else { 0.60 },
                    card.height * if self.columns { 0.48 } else { 0.18 },
                );
                meter(scene, face, &self.meter, strip.peak_dbfs);
            }
            if let Some(gain) = strip.gain_db.filter(|_| self.columns) {
                if self.columns {
                    let x = card.x + card.width * 0.88;
                    let top = card.y + card.height * 0.33;
                    let height = card.height * 0.38;
                    scene.push(Primitive::line(
                        [x, top],
                        [x, top + height],
                        2.0,
                        ThemeRole::Outline,
                    ));
                    let y = top + height * (1.0 - ((gain + 60.0) / 72.0).clamp(0.0, 1.0));
                    scene.push(Primitive::rounded_rect(
                        Rect::new(x - 6.0, y - 4.0, 12.0, 8.0),
                        2.0,
                        ThemeRole::Text,
                    ));
                    continue;
                }
                let y = card.y + card.height * 0.85;
                let start = card.x + card.width * 0.1;
                let width = card.width * 0.8;
                scene.push(Primitive::line(
                    [start, y],
                    [start + width, y],
                    2.0,
                    ThemeRole::Outline,
                ));
                let x = start + width * ((gain + 60.0) / 72.0).clamp(0.0, 1.0);
                scene.push(Primitive::rounded_rect(
                    Rect::new(x - 3.0, y - 4.0, 6.0, 8.0),
                    2.0,
                    ThemeRole::Text,
                ));
            }
        }
        Ok(())
    }
    /// Shared text anchors with explicit peak units and missing-data labels.
    /// # Errors
    /// Uses the same validation/layout as geometry.
    pub fn labels(&self, b: Rect) -> Result<Vec<Label>, ViewError> {
        let cells = self.cells(b)?;
        let mut labels = vec![];
        for (strip, cell) in self.strips.iter().zip(cells) {
            let label = |text: String, y: f32, h: f32, role: ThemeRole| Label {
                text,
                bounds: Rect::new(
                    cell.x + cell.width * 0.08,
                    cell.y + cell.height * y,
                    cell.width * 0.84,
                    cell.height * h,
                ),
                role,
            };
            labels.push(label(
                strip.name.clone(),
                0.07,
                if self.columns { 0.13 } else { 0.27 },
                ThemeRole::Text,
            ));
            labels.push(label(
                strip.detail.clone(),
                if self.columns { 0.21 } else { 0.35 },
                if self.columns { 0.10 } else { 0.18 },
                ThemeRole::TextMuted,
            ));
            if let Some(db) = strip.peak_dbfs.filter(|_| strip.show_meter) {
                let mut reading = label(
                    format!("{db:.1} dBFS"),
                    if self.columns { 0.77 } else { 0.55 },
                    if self.columns { 0.10 } else { 0.18 },
                    ThemeRole::Text,
                );
                if !self.columns {
                    reading.bounds.x = cell.x + cell.width * 0.70;
                    reading.bounds.width = cell.width * 0.24;
                }
                labels.push(reading);
                if self.meter.style == MeterStyle::Needle {
                    for (x, value) in [
                        (0.12, self.meter.minimum_db),
                        (0.65, 0.0),
                        (0.84, self.meter.maximum_db),
                    ] {
                        labels.push(Label {
                            text: format!("{value:.0}"),
                            bounds: Rect::new(
                                cell.x + cell.width * x,
                                cell.y + cell.height * 0.40,
                                cell.width * 0.12,
                                cell.height * 0.08,
                            ),
                            role: self.meter.ink,
                        });
                    }
                }
            } else if strip.show_meter {
                labels.push(label(
                    "Level unavailable".into(),
                    if self.columns { 0.49 } else { 0.55 },
                    if self.columns { 0.11 } else { 0.18 },
                    ThemeRole::TextMuted,
                ));
            }
            labels.push(label(
                match strip.gain_db {
                    Some(gain) => format!("{gain:+0.1} dB"),
                    None => String::new(),
                },
                if self.columns { 0.90 } else { 0.79 },
                if self.columns { 0.10 } else { 0.18 },
                strip.state.role(),
            ));
        }
        Ok(labels)
    }
}

fn meter(scene: &mut Scene, b: Rect, config: &MeterConfig, db: Option<f32>) {
    scene.push(Primitive::rounded_rect(
        b,
        6.0_f32.min(b.height * 0.08),
        ThemeRole::Shadow,
    ));
    let inner = Rect::new(
        b.x + b.width * 0.02,
        b.y + b.height * 0.04,
        b.width * 0.96,
        b.height * 0.92,
    );
    scene.push(Primitive::rounded_rect(
        inner,
        4.0_f32.min(b.height * 0.05),
        config.face,
    ));
    match config.style {
        MeterStyle::Led => {
            let count = f32::from(config.segments);
            let segment = inner.width * 0.9 / count;
            for i in 0..config.segments {
                let threshold = config.minimum_db
                    + (config.maximum_db - config.minimum_db) * f32::from(i + 1) / count;
                let role = if db.is_some_and(|db| db >= threshold) {
                    config.role(threshold)
                } else {
                    ThemeRole::Outline
                };
                let rect = if config.vertical_led {
                    let height = inner.height * 0.9 / count;
                    Rect::new(
                        inner.x + inner.width * 0.3,
                        inner.bottom() - inner.height * 0.05 - f32::from(i + 1) * height,
                        inner.width * 0.3,
                        height * 0.7,
                    )
                } else {
                    Rect::new(
                        inner.x + inner.width * 0.05 + f32::from(i) * segment,
                        inner.y + inner.height * 0.3,
                        segment * 0.7,
                        inner.height * 0.4,
                    )
                };
                scene.push(Primitive::rounded_rect(rect, 1.0, role));
            }
        }
        MeterStyle::Needle => {
            let pivot = [inner.x + inner.width * 0.5, inner.y + inner.height * 0.88];
            let radius = (inner.width * 0.44).min(inner.height * 0.76);
            let point = |t: f32, r: f32| {
                let a = (-150.0 + t * 120.0).to_radians();
                [pivot[0] + a.cos() * r, pivot[1] + a.sin() * r]
            };
            for i in 0_u8..=32 {
                let t = f32::from(i) / 32.0;
                if i > 0 {
                    scene.push(Primitive::line(
                        point(t - 1.0 / 32.0, radius),
                        point(t, radius),
                        1.0,
                        config.ink,
                    ));
                }
                if i % 2 == 0 {
                    scene.push(Primitive::line(
                        point(t, radius),
                        point(t, radius * 0.9),
                        1.2,
                        config
                            .role(config.minimum_db + t * (config.maximum_db - config.minimum_db)),
                    ));
                }
            }
            if let Some(db) = db {
                scene.push(Primitive::line(
                    pivot,
                    point(config.fraction(db), radius * 0.95),
                    2.0,
                    ThemeRole::Danger,
                ));
            }
            scene.push(Primitive::rounded_rect(
                Rect::new(pivot[0] - 4.0, pivot[1] - 4.0, 8.0, 8.0),
                4.0,
                config.ink,
            ));
            scene.push(Primitive::line(
                [inner.x + inner.width * 0.08, inner.y + inner.height * 0.08],
                [inner.x + inner.width * 0.92, inner.y + inner.height * 0.08],
                1.0,
                ThemeRole::Highlight,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panel(style: MeterStyle) -> Console {
        Console {
            strips: vec![Strip {
                show_meter: true,
                name: "L".into(),
                detail: "Monitor output".into(),
                state: State::Connected,
                peak_dbfs: Some(-12.0),
                gain_db: Some(-6.0),
            }],
            columns: true,
            meter: MeterConfig {
                style,
                ..Default::default()
            },
        }
    }
    #[test]
    fn both_faces_are_bounded_and_have_truthful_units() {
        for style in [MeterStyle::Needle, MeterStyle::Led] {
            let p = panel(style);
            let mut s = Scene::new();
            p.append(&mut s, Rect::new(0.0, 0.0, 400.0, 240.0)).unwrap();
            assert!(s.validate());
            assert!(s.len() < 100);
            assert!(
                p.labels(Rect::new(0.0, 0.0, 400.0, 240.0))
                    .unwrap()
                    .iter()
                    .any(|l| l.text.contains("dBFS"))
            );
        }
    }
    #[test]
    fn compact_rows_prioritize_readable_names_and_gain() {
        let mut p = panel(MeterStyle::Led);
        p.columns = false;
        let labels = p.labels(Rect::new(0.0, 0.0, 400.0, 80.0)).unwrap();
        assert!(labels[0].bounds.height >= 20.0);
        assert!(labels.iter().any(|l| l.text == "-6.0 dB"));
        assert!(!labels.iter().any(|l| l.text.contains("gain")));
        let mut scene = Scene::new();
        p.append(&mut scene, Rect::new(0.0, 0.0, 400.0, 80.0))
            .unwrap();
        assert!(scene.validate());
    }
    #[test]
    fn nan_is_atomic_and_missing_is_not_silence() {
        let mut p = panel(MeterStyle::Needle);
        p.strips[0].peak_dbfs = Some(f32::NAN);
        let mut s = Scene::new();
        assert!(p.append(&mut s, Rect::new(0.0, 0.0, 400.0, 240.0)).is_err());
        assert!(s.is_empty());
        p.strips[0].peak_dbfs = None;
        assert!(
            p.labels(Rect::new(0.0, 0.0, 400.0, 240.0))
                .unwrap()
                .iter()
                .any(|l| l.text == "Level unavailable")
        );
    }
}
