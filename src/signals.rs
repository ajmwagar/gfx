//! Transport-independent DAW and instrumentation views.
//!
//! Callers own acquisition, timestamps, FFTs, note state and retention. These
//! builders borrow display snapshots and append bounded, theme-role primitives.
//! No audio callback, window, network client or DSP dependency lives here.

use crate::{Point, Primitive, Rect, Scene, ThemeRole};
use serde::{Deserialize, Serialize};

/// Maximum columns or notes accepted by one view.
pub const MAX_ITEMS: usize = 4096;

/// Invalid view geometry or snapshot data. Validation happens before scene mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid signal view geometry or snapshot")]
pub struct ViewError;

/// One min/max envelope column, in caller-selected signal units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// Lowest observed value.
    pub minimum: f32,
    /// Highest observed value.
    pub maximum: f32,
}

/// Linear vertical scale; use volts, normalized audio, automation units or dB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueRange {
    /// Bottom of the display.
    pub minimum: f32,
    /// Top of the display.
    pub maximum: f32,
}

impl ValueRange {
    fn valid(self) -> bool {
        self.minimum.is_finite()
            && self.maximum.is_finite()
            && self.maximum > self.minimum
            && (self.maximum - self.minimum).is_finite()
    }

    fn y(self, bounds: Rect, value: f32) -> f32 {
        bounds.bottom()
            - ((value.clamp(self.minimum, self.maximum) - self.minimum)
                / (self.maximum - self.minimum))
                * bounds.height
    }
}

fn validate(bounds: Rect, range: ValueRange, count: usize) -> Result<(), ViewError> {
    if !bounds.is_valid()
        || !bounds.right().is_finite()
        || !bounds.bottom().is_finite()
        || !range.valid()
        || count > MAX_ITEMS
    {
        return Err(ViewError);
    }
    Ok(())
}

fn panel(scene: &mut Scene, bounds: Rect) {
    scene.push(
        Primitive::rounded_rect(bounds, 0.0, ThemeRole::SurfaceRecessed)
            .with_outline(ThemeRole::Outline, 0.0),
    );
}

fn column(index: usize) -> f32 {
    // Validated callers bound every count/index to MAX_ITEMS (4096).
    f32::from(u16::try_from(index).expect("validated display item count"))
}

/// Append peak-preserving waveform/envelope columns. Empty data draws an empty well.
///
/// Work and geometry are O(columns), not O(recording length). Reduce source audio
/// upstream to the visible range; never pass an entire recording here.
///
/// # Errors
/// Returns `ViewError` for invalid geometry, scale, peaks or excessive columns.
pub fn waveform(
    scene: &mut Scene,
    bounds: Rect,
    range: ValueRange,
    peaks: &[Envelope],
    role: ThemeRole,
) -> Result<(), ViewError> {
    validate(bounds, range, peaks.len())?;
    if peaks
        .iter()
        .any(|p| !p.minimum.is_finite() || !p.maximum.is_finite() || p.minimum > p.maximum)
    {
        return Err(ViewError);
    }
    panel(scene, bounds);
    if range.minimum <= 0.0 && range.maximum >= 0.0 {
        let y = range.y(bounds, 0.0);
        let height = bounds.height.min(0.5);
        scene.push(Primitive::rounded_rect(
            Rect::new(
                bounds.x,
                y.clamp(bounds.y, bounds.bottom() - height),
                bounds.width,
                height,
            ),
            0.0,
            ThemeRole::Outline,
        ));
    }
    let width = bounds.width / column(peaks.len().max(1));
    for (i, peak) in peaks.iter().enumerate() {
        let top = range.y(bounds, peak.maximum);
        let bottom = range.y(bounds, peak.minimum);
        let height = (bottom - top).max(0.5).min(bounds.height);
        scene.push(Primitive::rounded_rect(
            Rect::new(
                bounds.x + column(i) * width,
                top.min(bounds.bottom() - height),
                width,
                height,
            ),
            0.0,
            role,
        ));
    }
    Ok(())
}

/// Append an evenly spaced scope, LFO or automation trace, clamped to its scale.
/// Missing/nonfinite samples are errors, not fabricated zeros. Empty input is valid.
///
/// # Errors
/// Returns `ViewError` for invalid geometry, scale, samples or excessive points.
pub fn scope(
    scene: &mut Scene,
    bounds: Rect,
    range: ValueRange,
    samples: &[f32],
    role: ThemeRole,
) -> Result<(), ViewError> {
    validate(bounds, range, samples.len())?;
    if samples.iter().any(|v| !v.is_finite()) {
        return Err(ViewError);
    }
    panel(scene, bounds);
    let inset = bounds.height.min(bounds.width).min(2.0) * 0.5;
    let inner = Rect::new(
        bounds.x + inset,
        bounds.y + inset,
        bounds.width - 2.0 * inset,
        bounds.height - 2.0 * inset,
    );
    if !inner.is_valid() || samples.len() < 2 {
        return Ok(());
    }
    let step = inner.width / column(samples.len() - 1);
    for (i, pair) in samples.windows(2).enumerate() {
        scene.push(Primitive::line(
            [inner.x + column(i) * step, range.y(inner, pair[0])],
            [inner.x + column(i + 1) * step, range.y(inner, pair[1])],
            inset * 2.0,
            role,
        ));
    }
    Ok(())
}

/// One precomputed spectral display band. FFT normalization remains caller-owned.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectrumBand {
    /// Lower band edge in Hz, strictly positive for the logarithmic axis.
    pub low_hz: f32,
    /// Upper band edge in Hz.
    pub high_hz: f32,
    /// Band amplitude in the caller's dB convention (e.g. dBFS).
    pub db: f32,
}

/// Append a log-frequency spectrum. Bands must be sorted and nonoverlapping.
///
/// # Errors
/// Returns `ViewError` for invalid scales, geometry, bands or excessive items.
pub fn spectrum(
    scene: &mut Scene,
    bounds: Rect,
    hz: ValueRange,
    db: ValueRange,
    bands: &[SpectrumBand],
    role: ThemeRole,
) -> Result<(), ViewError> {
    validate(bounds, db, bands.len())?;
    if !hz.valid() || hz.minimum <= 0.0 {
        return Err(ViewError);
    }
    let span = hz.maximum.ln() - hz.minimum.ln();
    if !span.is_finite() || span <= 0.0 {
        return Err(ViewError);
    }
    let mut previous = 0.0;
    for band in bands {
        if !band.low_hz.is_finite()
            || !band.high_hz.is_finite()
            || !band.db.is_finite()
            || band.low_hz <= 0.0
            || band.high_hz <= band.low_hz
            || band.low_hz < previous
        {
            return Err(ViewError);
        }
        previous = band.high_hz;
    }
    panel(scene, bounds);
    for band in bands {
        let lo = band.low_hz.max(hz.minimum);
        let hi = band.high_hz.min(hz.maximum);
        if lo >= hi {
            continue;
        }
        let x = bounds.x + (lo.ln() - hz.minimum.ln()) / span * bounds.width;
        let right = bounds.x + (hi.ln() - hz.minimum.ln()) / span * bounds.width;
        let y = db.y(bounds, band.db);
        if right > x && bounds.bottom() > y {
            scene.push(Primitive::rounded_rect(
                Rect::new(x, y, right - x, bounds.bottom() - y),
                0.0,
                role,
            ));
        }
    }
    Ok(())
}

/// Caller-owned recorded or currently held note. Times use one consistent unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    /// Start in beats or seconds, matching the viewport.
    pub start: f64,
    /// End in the same unit; live callers use their current observation time.
    pub end: f64,
    /// MIDI note number.
    pub key: u8,
    /// Note-on velocity, 1..=127.
    pub velocity: u8,
    /// MIDI channel, zero-based.
    pub channel: u8,
}

/// Shared piano-roll geometry for rendering and host hit testing.
#[derive(Debug, Clone, Copy)]
pub struct PianoViewport {
    /// View bounds.
    pub bounds: Rect,
    /// Left timeline edge.
    pub start: f64,
    /// Right timeline edge.
    pub end: f64,
    /// Lowest visible key.
    pub low_key: u8,
    /// Highest visible key, inclusive.
    pub high_key: u8,
}

/// Renderer-neutral, bounded snapshot payload. Units/provenance/freshness belong
/// to the surrounding application contract, not to the renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case", deny_unknown_fields)]
pub enum SignalFrame {
    /// Symbolic owner-observed agent attention; no provider parsing or control.
    Attention {
        /// Bounded counts and explicit freshness.
        panel: crate::attention::Attention,
    },
    /// Read-only channel strips with caller-owned peak levels and gain references.
    Console {
        /// Bounded strips, configured meter face and calibration.
        panel: crate::console::Console,
    },
    /// A bounded directed signal-flow graph; no device ownership lives here.
    Topology {
        /// Nodes, directed cables and caller-owned observation states.
        graph: crate::topology::Graph,
    },
    /// Min/max peak envelope in explicit value units.
    Waveform {
        /// Explicit vertical signal-unit scale.
        range: ValueRange,
        /// Peak-preserving visible columns.
        peaks: Vec<Envelope>,
    },
    /// Uniformly spaced time-domain or automation samples.
    Scope {
        /// Explicit vertical signal-unit scale.
        range: ValueRange,
        /// Uniformly spaced visible samples.
        samples: Vec<f32>,
    },
    /// Precomputed dB spectral bands, never raw FFT work.
    Spectrum {
        /// Positive frequency bounds in Hz.
        hz: ValueRange,
        /// Vertical amplitude scale in the caller's dB convention.
        db: ValueRange,
        /// Sorted, nonoverlapping spectral bands.
        bands: Vec<SpectrumBand>,
    },
    /// Note intervals in a caller-defined common timebase.
    PianoRoll {
        /// Left edge in the caller's timeline units.
        start: f64,
        /// Right edge in the same units.
        end: f64,
        /// Lowest visible MIDI key, inclusive.
        low_key: u8,
        /// Highest visible MIDI key, inclusive.
        high_key: u8,
        /// Recorded or held note intervals.
        notes: Vec<Note>,
        /// Optional cursor in the common timeline units.
        playhead: Option<f64>,
    },
}

impl SignalFrame {
    /// Shared label placement for hosts with their own glyph renderer.
    ///
    /// # Errors
    /// Rejects invalid topology data or geometry.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<crate::topology::Label>, ViewError> {
        match self {
            Self::Attention { panel } => panel.labels(bounds),
            Self::Console { panel } => panel.labels(bounds),
            Self::Topology { graph } => graph.labels(bounds).map_err(|_| ViewError),
            _ => Ok(vec![]),
        }
    }
    /// Append to caller-owned scene; parsing alone does not imply validation.
    ///
    /// # Errors
    /// Returns `ViewError` for invalid payloads or view geometry.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), ViewError> {
        match self {
            Self::Attention { panel } => panel.append(scene, bounds),
            Self::Console { panel } => panel.append(scene, bounds),
            Self::Topology { graph } => graph.append(scene, bounds).map_err(|_| ViewError),
            Self::Waveform { range, peaks } => {
                waveform(scene, bounds, *range, peaks, ThemeRole::Secondary)
            }
            Self::Scope { range, samples } => {
                scope(scene, bounds, *range, samples, ThemeRole::Success)
            }
            Self::Spectrum { hz, db, bands } => {
                spectrum(scene, bounds, *hz, *db, bands, ThemeRole::Primary)
            }
            Self::PianoRoll {
                start,
                end,
                low_key,
                high_key,
                notes,
                playhead,
            } => piano_roll(
                scene,
                PianoViewport {
                    bounds,
                    start: *start,
                    end: *end,
                    low_key: *low_key,
                    high_key: *high_key,
                },
                notes,
                *playhead,
            ),
        }
    }

    /// Validate a snapshot independently of its eventual host placement.
    ///
    /// # Errors
    /// Returns `ViewError` for an invalid or oversized payload.
    pub fn validate(&self) -> Result<(), ViewError> {
        self.append(&mut Scene::new(), Rect::new(0.0, 0.0, 1024.0, 512.0))
    }
}

impl PianoViewport {
    fn valid(self) -> bool {
        self.bounds.is_valid()
            && self.bounds.right().is_finite()
            && self.bounds.bottom().is_finite()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.end > self.start
            && (self.end - self.start).is_finite()
            && self.high_key <= 127
            && self.low_key <= self.high_key
    }

    /// Resolve a host pointer to timeline position and MIDI key. Outside is None.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Row is floored and clamped to 0..=127.
    pub fn position(self, point: Point) -> Option<(f64, u8)> {
        if !self.valid() || !point.is_finite() || !self.bounds.contains(point) {
            return None;
        }
        let rows = u16::from(self.high_key) - u16::from(self.low_key) + 1;
        let row = (((self.bounds.bottom() - point.y) / self.bounds.height) * f32::from(rows))
            .floor()
            .clamp(0.0, f32::from(rows - 1)) as u8;
        Some((
            self.start
                + f64::from((point.x - self.bounds.x) / self.bounds.width)
                    * (self.end - self.start),
            self.low_key + row,
        ))
    }
}

/// Append piano-roll rows, clipped notes and optional playhead. Live MIDI uses the
/// same renderer; note pairing/sustain/channel filtering are source responsibilities.
///
/// # Errors
/// Returns `ViewError` for invalid viewport, notes, playhead or excessive notes.
#[allow(clippy::cast_possible_truncation)] // Time ratio is clipped to 0..=1 before conversion to logical pixels.
pub fn piano_roll(
    scene: &mut Scene,
    view: PianoViewport,
    notes: &[Note],
    playhead: Option<f64>,
) -> Result<(), ViewError> {
    if !view.valid()
        || notes.len() > MAX_ITEMS
        || playhead.is_some_and(|v| !v.is_finite())
        || notes.iter().any(|n| {
            !n.start.is_finite()
                || !n.end.is_finite()
                || n.end < n.start
                || n.key > 127
                || n.velocity == 0
                || n.velocity > 127
                || n.channel > 15
        })
    {
        return Err(ViewError);
    }
    panel(scene, view.bounds);
    let height =
        view.bounds.height / f32::from(u16::from(view.high_key) - u16::from(view.low_key) + 1);
    for key in view.low_key..=view.high_key {
        if matches!(key % 12, 1 | 3 | 6 | 8 | 10) {
            scene.push(Primitive::rounded_rect(
                Rect::new(
                    view.bounds.x,
                    view.bounds.bottom()
                        - f32::from(u16::from(key) - u16::from(view.low_key) + 1) * height,
                    view.bounds.width,
                    height,
                ),
                0.0,
                ThemeRole::Surface,
            ));
        }
    }
    let x = |time: f64| {
        view.bounds.x + ((time - view.start) / (view.end - view.start)) as f32 * view.bounds.width
    };
    for note in notes {
        if note.key < view.low_key || note.key > view.high_key {
            continue;
        }
        let left = note.start.max(view.start);
        let right = note.end.min(view.end);
        if right <= left {
            continue;
        }
        let role = if note.channel % 2 == 0 {
            ThemeRole::Primary
        } else {
            ThemeRole::Secondary
        };
        let left_x = x(left);
        let right_x = x(right);
        if right_x <= left_x {
            continue; // Interval is below representable logical-pixel precision.
        }
        scene.push(
            Primitive::rounded_rect(
                Rect::new(
                    left_x,
                    view.bounds.bottom()
                        - f32::from(u16::from(note.key) - u16::from(view.low_key) + 1) * height,
                    right_x - left_x,
                    height,
                ),
                height.min(3.0),
                role,
            )
            .with_gloss(f32::from(note.velocity) / 127.0 * 0.4),
        );
    }
    if let Some(time) = playhead.filter(|t| *t >= view.start && *t <= view.end) {
        scene.push(Primitive::rounded_rect(
            Rect::new(
                x(time).min(view.bounds.right() - view.bounds.width.min(1.0)),
                view.bounds.y,
                view.bounds.width.min(1.0),
                view.bounds.height,
            ),
            0.0,
            ThemeRole::Warning,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds() -> Rect {
        Rect::new(10.0, 20.0, 800.0, 200.0)
    }
    fn range() -> ValueRange {
        ValueRange {
            minimum: -1.0,
            maximum: 1.0,
        }
    }
    #[test]
    fn validation_is_atomic() {
        let mut scene = Scene::new();
        assert!(
            scope(
                &mut scene,
                bounds(),
                range(),
                &[0.0, f32::NAN],
                ThemeRole::Primary
            )
            .is_err()
        );
        assert!(
            waveform(
                &mut scene,
                bounds(),
                range(),
                &[Envelope {
                    minimum: 1.0,
                    maximum: -1.0
                }],
                ThemeRole::Primary
            )
            .is_err()
        );
        assert!(scene.is_empty());
    }
    #[test]
    fn geometry_is_bounded_and_themeable() {
        let mut scene = Scene::new();
        waveform(
            &mut scene,
            bounds(),
            range(),
            &[Envelope {
                minimum: -5.0,
                maximum: 5.0,
            }; 800],
            ThemeRole::Secondary,
        )
        .unwrap();
        assert!(scene.validate());
        assert_eq!(scene.len(), 802);
        assert!(scene.iter().all(|p| p.rect.x >= bounds().x
            && p.rect.right() <= bounds().right() + 0.01
            && p.rect.y >= bounds().y
            && p.rect.bottom() <= bounds().bottom() + 0.01));
        scene.clear();
        scope(
            &mut scene,
            bounds(),
            range(),
            &[0.0; MAX_ITEMS],
            ThemeRole::Success,
        )
        .unwrap();
        assert_eq!(scene.len(), MAX_ITEMS);
        assert!(scene.validate());
        assert!(
            scope(
                &mut scene,
                bounds(),
                range(),
                &vec![0.0; MAX_ITEMS + 1],
                ThemeRole::Primary
            )
            .is_err()
        );
    }
    #[test]
    fn piano_clips_notes_and_shares_hit_geometry() {
        let view = PianoViewport {
            bounds: bounds(),
            start: 4.0,
            end: 8.0,
            low_key: 60,
            high_key: 71,
        };
        assert_eq!(view.position(Point::new(410.0, 220.0)), Some((6.0, 60)));
        assert_eq!(view.position(Point::new(410.0, 20.0)), Some((6.0, 71)));
        assert!(view.position(Point::new(0.0, 0.0)).is_none());
        let mut scene = Scene::new();
        piano_roll(
            &mut scene,
            view,
            &[Note {
                start: 0.0,
                end: 10.0,
                key: 60,
                velocity: 100,
                channel: 0,
            }],
            Some(6.0),
        )
        .unwrap();
        assert!(scene.validate());
        assert_eq!(scene.iter().nth(scene.len() - 2).unwrap().rect.width, 800.0);
    }
    #[test]
    fn logarithmic_spectrum_rejects_unsorted_data() {
        let hz = ValueRange {
            minimum: 20.0,
            maximum: 20000.0,
        };
        let db = ValueRange {
            minimum: -90.0,
            maximum: 0.0,
        };
        let mut scene = Scene::new();
        spectrum(
            &mut scene,
            bounds(),
            hz,
            db,
            &[SpectrumBand {
                low_hz: 100.0,
                high_hz: 1000.0,
                db: -12.0,
            }],
            ThemeRole::Primary,
        )
        .unwrap();
        assert!(scene.validate());
        let len = scene.len();
        assert!(
            spectrum(
                &mut scene,
                bounds(),
                hz,
                db,
                &[SpectrumBand {
                    low_hz: 0.0,
                    high_hz: 1.0,
                    db: 0.0
                }],
                ThemeRole::Primary
            )
            .is_err()
        );
        assert_eq!(scene.len(), len);
    }

    #[test]
    fn full_midi_range_and_tiny_views_do_not_overflow() {
        let mut scene = Scene::new();
        piano_roll(
            &mut scene,
            PianoViewport {
                bounds: bounds(),
                start: 0.0,
                end: 4.0,
                low_key: 0,
                high_key: 127,
            },
            &[Note {
                start: 0.0,
                end: 1.0,
                key: 127,
                velocity: 127,
                channel: 15,
            }],
            None,
        )
        .unwrap();
        waveform(
            &mut scene,
            Rect::new(0.0, 0.0, 0.1, 0.1),
            range(),
            &[Envelope {
                minimum: 0.0,
                maximum: 0.0,
            }],
            ThemeRole::Primary,
        )
        .unwrap();
        assert!(scene.validate());
    }
}
