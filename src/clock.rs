//! Themeable instrument scenes shared by compositors and standalone widgets.

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::{Primitive, Rect, Scene, ThemeRole};

/// State and geometry for the canonical analog clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogClock {
    /// Hour on a 24-hour clock.
    pub hour: u8,
    /// Minute in the current hour.
    pub minute: u8,
    /// Fractional second, allowing a smooth sweep.
    pub second: f32,
}

impl AnalogClock {
    /// Appends the clock to an existing retained scene.
    ///
    /// Geometry intentionally matches Canvas's established clock: a 39% face,
    /// sixty ticks, 48/69/76% hands, and the original stroke hierarchy.
    pub fn append(self, scene: &mut Scene, bounds: Rect) {
        let center = [
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        ];
        let radius = bounds.width.min(bounds.height) * 0.39;
        let face = Rect::new(
            center[0] - radius,
            center[1] - radius,
            radius * 2.0,
            radius * 2.0,
        );
        scene.push(
            Primitive::disc(face, ThemeRole::Surface).with_outline(ThemeRole::TextMuted, 2.5),
        );

        for tick in 0_u8..60 {
            let angle = f32::from(tick) * TAU / 60.0 - FRAC_PI_2;
            let major = tick % 5 == 0;
            let inner_scale = if major { 0.76 } else { 0.84 };
            scene.push(Primitive::line(
                radial(center, radius * inner_scale, angle),
                radial(center, radius * 0.90, angle),
                if major { 2.8 } else { 1.35 },
                ThemeRole::TextMuted,
            ));
        }

        let minute = f32::from(self.minute) + self.second / 60.0;
        let hour = f32::from(self.hour % 12) + minute / 60.0;
        for (fraction, divisions, length, width, role) in [
            (hour, 12.0, 0.48, 4.5, ThemeRole::Text),
            (minute, 60.0, 0.69, 3.0, ThemeRole::Text),
            (self.second, 60.0, 0.76, 1.8, ThemeRole::Primary),
        ] {
            let angle = fraction * TAU / divisions - FRAC_PI_2;
            scene.push(Primitive::line(
                center,
                radial(center, radius * length, angle),
                width,
                role,
            ));
        }
        scene.push(Primitive::disc(
            Rect::new(center[0] - 4.0, center[1] - 4.0, 8.0, 8.0),
            ThemeRole::Primary,
        ));
    }
}

fn radial(center: [f32; 2], radius: f32, angle: f32) -> [f32; 2] {
    [
        center[0] + angle.cos() * radius,
        center[1] + angle.sin() * radius,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_clock_is_one_batch_of_bounded_primitives() {
        let mut scene = Scene::with_capacity(65);
        AnalogClock {
            hour: 10,
            minute: 9,
            second: 30.5,
        }
        .append(&mut scene, Rect::new(0.0, 0.0, 400.0, 300.0));

        assert_eq!(scene.len(), 65);
        assert!(scene.validate());
    }

    #[test]
    fn clock_reuses_retained_scene_capacity() {
        let mut scene = Scene::with_capacity(65);
        let capacity = scene.capacity();
        for second in 0_u8..60 {
            scene.clear();
            AnalogClock {
                hour: 3,
                minute: 14,
                second: f32::from(second),
            }
            .append(&mut scene, Rect::new(0.0, 0.0, 320.0, 320.0));
            assert_eq!(scene.capacity(), capacity);
        }
    }
}
