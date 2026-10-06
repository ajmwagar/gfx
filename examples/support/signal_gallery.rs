//! Explicit synthetic fixtures, not live acquisition or production UI state.
use fpl_gfx::{
    signals::{self, Envelope, Note, PianoViewport, SpectrumBand, ValueRange},
    Rect, Scene, ThemeRole,
};

pub fn append(scene: &mut Scene) -> Result<(), signals::ViewError> {
    let audio = ValueRange {
        minimum: -1.0,
        maximum: 1.0,
    };
    let peaks: Vec<_> = (0..280)
        .map(|i| {
            let amplitude = (i as f32 * 0.08).sin().abs() * 0.8;
            Envelope {
                minimum: -amplitude,
                maximum: amplitude,
            }
        })
        .collect();
    signals::waveform(
        scene,
        Rect::new(20.0, 20.0, 280.0, 70.0),
        audio,
        &peaks,
        ThemeRole::Secondary,
    )?;
    let trace: Vec<_> = (0..280).map(|i| (i as f32 * 0.04).sin() * 0.8).collect();
    signals::scope(
        scene,
        Rect::new(320.0, 20.0, 300.0, 70.0),
        audio,
        &trace,
        ThemeRole::Success,
    )?;
    let notes: Vec<_> = (0..12)
        .map(|i| Note {
            start: f64::from(i) * 0.5,
            end: f64::from(i) * 0.5 + 0.4,
            key: 60 + (i % 5) * 2,
            velocity: 85 + i * 3,
            channel: i % 2,
        })
        .collect();
    signals::piano_roll(
        scene,
        PianoViewport {
            bounds: Rect::new(20.0, 110.0, 600.0, 130.0),
            start: 0.0,
            end: 8.0,
            low_key: 48,
            high_key: 72,
        },
        &notes,
        Some(3.2),
    )?;
    let bands: Vec<_> = (0..64)
        .map(|i| {
            let lo = 20.0 * 1000.0_f32.powf(i as f32 / 64.0);
            let hi = 20.0 * 1000.0_f32.powf((i + 1) as f32 / 64.0);
            SpectrumBand {
                low_hz: lo,
                high_hz: hi,
                db: -20.0 - (i as f32 * 0.3).sin().abs() * 40.0,
            }
        })
        .collect();
    signals::spectrum(
        scene,
        Rect::new(20.0, 260.0, 600.0, 80.0),
        ValueRange {
            minimum: 20.0,
            maximum: 20000.0,
        },
        ValueRange {
            minimum: -90.0,
            maximum: 0.0,
        },
        &bands,
        ThemeRole::Primary,
    )?;
    Ok(())
}
