//! Fixed console geometry. Hosts bind strip identity to their own typed controls.
//! Coordinates are logical pixels; depth is relative to the faceplate, not metres.
use super::{meter, MeterConfig, Strip};
use crate::{
    topology::{Label, State},
    Primitive, Rect, Scene, ThemeRole,
};
use serde::{Deserialize, Serialize};

/// Console material tokens. Independent of audio snapshots and control geometry.
/// Colors resolve through the host's existing semantic palette, not a second palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsoleAppearance {
    /// Faceplate color role.
    pub face: ThemeRole,
    /// Fader cap color role.
    pub cap: ThemeRole,
    /// Fader slot color role.
    pub slot: ThemeRole,
    /// Scribble-strip color role.
    pub identity: ThemeRole,
    /// Engraved scale and fastener color role.
    pub markings: ThemeRole,
    /// Local directional material highlight, 0..=1.
    pub gloss: f32,
    /// Local fine grain, 0..=0.02 (global renderer material remains host-owned).
    pub grain: f32,
    /// Show edge bevels and fasteners. Does not change hit geometry.
    pub hardware_details: bool,
    /// Show the fader cap's cast shadow.
    pub cast_shadows: bool,
}
impl Default for ConsoleAppearance {
    fn default() -> Self {
        Self {
            face: ThemeRole::Surface,
            cap: ThemeRole::TextMuted,
            slot: ThemeRole::Shadow,
            identity: ThemeRole::SurfaceRecessed,
            markings: ThemeRole::TextMuted,
            gloss: 0.22,
            grain: 0.001,
            hardware_details: true,
            cast_shadows: true,
        }
    }
}
impl ConsoleAppearance {
    /// A quiet flat finish with exactly the same control positions and state cues.
    /// Set global renderer gloss/grain to zero as well for a fully flat wGPU theme.
    pub fn flat() -> Self {
        Self {
            gloss: 0.0,
            grain: 0.0,
            hardware_details: false,
            cast_shadows: false,
            ..Self::default()
        }
    }
    /// Validate external material tokens before any scene mutation.
    pub fn validate(&self) -> Result<(), crate::signals::ViewError> {
        if !self.gloss.is_finite()
            || !(0.0..=1.0).contains(&self.gloss)
            || !self.grain.is_finite()
            || !(0.0..=0.02).contains(&self.grain)
        {
            return Err(crate::signals::ViewError);
        }
        Ok(())
    }
}

/// One render-independent location suitable for a hit target or extrusion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlLocation {
    /// Shared 2D bounds. VR hosts choose the logical-pixel to world-space scale.
    pub bounds: Rect,
    /// Relative extrusion: negative is recessed, positive is raised.
    pub depth: f32,
}

/// Fixed channel-strip zones, shared by painting, hit testing and future VR hosts.
/// This geometry does not invent controls, own parameter values, or issue commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSurface {
    /// Faceplate.
    pub panel: Rect,
    /// Channel identity scribble strip.
    pub identity: Rect,
    /// Recessed meter glass.
    pub meter: ControlLocation,
    /// Fader interaction corridor, separate from the meter.
    pub fader: ControlLocation,
    /// Routing indicator, not an inferred audio-activity lamp.
    pub route: Rect,
    /// Numeric observed gain.
    pub reading: Rect,
}
impl StripSurface {
    /// Derive all zones from one channel cell. Callers validate the containing panel.
    pub fn new(cell: Rect) -> Self {
        let zone = |x, y, w, h| {
            Rect::new(
                cell.x + cell.width * x,
                cell.y + cell.height * y,
                cell.width * w,
                cell.height * h,
            )
        };
        Self {
            panel: zone(0.025, 0.015, 0.95, 0.97),
            identity: zone(0.10, 0.88, 0.80, 0.08),
            meter: ControlLocation {
                bounds: zone(0.10, 0.06, 0.80, 0.27),
                depth: -0.025,
            },
            fader: ControlLocation {
                bounds: zone(0.30, 0.46, 0.40, 0.35),
                depth: -0.018,
            },
            route: zone(0.43, 0.37, 0.14, 0.035),
            reading: zone(0.10, 0.81, 0.80, 0.055),
        }
    }
    /// Y coordinate for an observed gain, with the same taper used by input mapping.
    pub fn fader_y(self, db: f32) -> f32 {
        self.fader.bounds.bottom() - self.fader.bounds.height * gain_fraction(db)
    }
}

/// Display/input taper: -60..+12 dB, unity at five-sixths of the travel.
/// Hosts retain values outside display travel; absence must not become unity.
pub fn gain_fraction(db: f32) -> f32 {
    ((db + 60.0) / 72.0).clamp(0.0, 1.0)
}
/// Inverse of the display taper for bounded host input; does not execute an action.
pub fn gain_from_fraction(value: f32) -> f32 {
    value.clamp(0.0, 1.0) * 72.0 - 60.0
}

pub(super) fn append(
    scene: &mut Scene,
    g: StripSurface,
    strip: &Strip,
    config: &MeterConfig,
    appearance: &ConsoleAppearance,
) {
    let b = g.panel;
    scene.push(
        Primitive::rounded_rect(b, 2.0, appearance.face)
            .with_gloss(appearance.gloss)
            .with_grain(appearance.grain),
    );
    if appearance.hardware_details {
        scene.push(Primitive::line(
            [b.x, b.y],
            [b.x, b.bottom()],
            1.0,
            ThemeRole::Highlight,
        ));
        scene.push(Primitive::line(
            [b.right(), b.y],
            [b.right(), b.bottom()],
            2.0,
            ThemeRole::Shadow,
        ));
        // Countersunk fasteners, not interactive decorations.
        let radius = b.width.min(b.height) * 0.022;
        for x in [b.x + b.width * 0.08, b.right() - b.width * 0.08] {
            let y = b.bottom() - b.height * 0.025;
            scene.push(Primitive::disc(
                Rect::new(x - radius, y - radius, radius * 2.0, radius * 2.0),
                ThemeRole::Shadow,
            ));
            scene.push(Primitive::line(
                [x - radius * 0.5, y],
                [x + radius * 0.5, y],
                0.7,
                appearance.markings,
            ));
        }
    }
    if strip.show_meter {
        meter(scene, g.meter.bounds, config, strip.peak_dbfs);
        if strip.peak_dbfs.is_none() {
            let c = g.meter.bounds.center();
            scene.push(Primitive::line(
                [c.x - 6.0, c.y],
                [c.x + 6.0, c.y],
                2.0,
                ThemeRole::TextMuted,
            ));
        }
    }
    let known = matches!(strip.state, State::Connected | State::Active | State::Fault);
    let side = g.route.width.min(g.route.height);
    scene.push(Primitive::lamp(
        Rect::new(g.route.center().x - side * 0.5, g.route.y, side, side),
        if known { 1.0 } else { 0.0 },
        strip.state.role(),
    ));
    let f = g.fader.bounds;
    let x = f.center().x;
    // A visible empty slot is not a pretend movable control when gain is unknown.
    scene.push(Primitive::rounded_rect(
        Rect::new(x - f.width * 0.08, f.y, f.width * 0.16, f.height),
        f.width * 0.05,
        appearance.slot,
    ));
    for db in [-60.0, -36.0, -24.0, -12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = g.fader_y(db);
        let width = if db == 0.0 {
            f.width * 0.22
        } else {
            f.width * 0.12
        };
        scene.push(Primitive::line(
            [x - f.width * 0.48, y],
            [x - f.width * 0.48 + width, y],
            if db == 0.0 { 2.0 } else { 1.0 },
            appearance.markings,
        ));
    }
    if let Some(db) = strip.gain_db {
        let y = g.fader_y(db);
        let h = b.height * 0.028;
        let cap = Rect::new(x - f.width * 0.36, y - h, f.width * 0.72, h * 2.0);
        if appearance.cast_shadows {
            scene.push(Primitive::rounded_rect(
                Rect::new(cap.x + 2.0, cap.y + 3.0, cap.width, cap.height),
                2.0,
                ThemeRole::Shadow,
            ));
        }
        scene.push(Primitive::rounded_rect(cap, 2.0, appearance.cap).with_gloss(appearance.gloss));
        scene.push(Primitive::line(
            [cap.x + cap.width * 0.12, y],
            [cap.right() - cap.width * 0.12, y],
            2.0,
            appearance.slot,
        ));
    }
    scene.push(Primitive::rounded_rect(
        g.identity,
        1.0,
        appearance.identity,
    ));
}

pub(super) fn labels(g: StripSurface, strip: &Strip) -> Vec<Label> {
    let mut labels = vec![Label {
        text: strip.name.clone(),
        bounds: g.identity,
        role: ThemeRole::Text,
    }];
    if let Some(db) = strip.gain_db {
        labels.push(Label {
            text: format!("{db:+.1} dB"),
            bounds: g.reading,
            role: ThemeRole::TextMuted,
        });
    }
    if let Some(db) = strip.peak_dbfs.filter(|_| strip.show_meter) {
        let b = g.meter.bounds;
        labels.push(Label {
            text: format!("{db:.1} dBFS"),
            bounds: Rect::new(b.x, b.bottom(), b.width, g.panel.height * 0.035),
            role: ThemeRole::TextMuted,
        });
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_zones_do_not_overlap() {
        for (w, h) in [(96.0, 240.0), (180.0, 640.0), (400.0, 240.0)] {
            let g = StripSurface::new(Rect::new(30.0, 20.0, w, h));
            assert!(!g.meter.bounds.overlaps(&g.fader.bounds));
            assert!(!g.fader.bounds.overlaps(&g.identity));
            assert!(g.panel.contains(g.identity.center()));
            assert!(g.panel.contains(g.fader.bounds.center()));
        }
    }
    #[test]
    fn taper_round_trips_and_clamps() {
        for db in [-60.0, -36.0, -12.0, 0.0, 12.0] {
            assert!((gain_from_fraction(gain_fraction(db)) - db).abs() < 0.001);
        }
        assert_eq!(gain_fraction(-120.0), 0.0);
        assert_eq!(gain_fraction(24.0), 1.0);
    }
    #[test]
    fn appearance_is_bounded_and_round_trips() {
        for a in [ConsoleAppearance::default(), ConsoleAppearance::flat()] {
            a.validate().unwrap();
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<ConsoleAppearance>(&json).unwrap(), a);
        }
        let mut invalid = ConsoleAppearance::default();
        invalid.gloss = f32::NAN;
        assert!(invalid.validate().is_err());
        invalid.gloss = 0.0;
        invalid.grain = 0.03;
        assert!(invalid.validate().is_err());
        assert!(serde_json::from_str::<ConsoleAppearance>(r#"{"unrecognized":true}"#).is_err());
    }
    #[test]
    fn missing_gain_has_no_cap_or_numeric_reading() {
        let strip = Strip {
            show_meter: true,
            name: "L".into(),
            detail: String::new(),
            state: State::Unknown,
            peak_dbfs: None,
            gain_db: None,
        };
        let g = StripSurface::new(Rect::new(0.0, 0.0, 180.0, 640.0));
        let mut s = Scene::new();
        append(
            &mut s,
            g,
            &strip,
            &MeterConfig::default(),
            &ConsoleAppearance::default(),
        );
        let count = s.len();
        assert_eq!(labels(g, &strip).len(), 1);
        let mut live = strip;
        live.gain_db = Some(0.0);
        let mut s = Scene::new();
        append(
            &mut s,
            g,
            &live,
            &MeterConfig::default(),
            &ConsoleAppearance::default(),
        );
        assert_eq!(s.len(), count + 3);
        assert!(s.validate());
    }
}
