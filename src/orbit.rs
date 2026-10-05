use serde::{Deserialize, Serialize};

use crate::{Point, Size};

/// Pitch limit that avoids the undefined up vector at a camera pole.
pub const MAX_ORBIT_PITCH_DEGREES: f32 = 85.0;
/// Smallest supported multiplier on a scene's resting camera distance.
pub const MIN_ORBIT_DISTANCE: f32 = 0.45;
/// Largest supported multiplier on a scene's resting camera distance.
pub const MAX_ORBIT_DISTANCE: f32 = 3.0;

/// Named CAD orientations shared by key bindings, view cubes, and renderers.
pub const NAMED_ORIENTATIONS: [(&str, f32, f32); 26] = [
    ("front", 0.0, 0.0),
    ("back", 180.0, 0.0),
    ("right", 90.0, 0.0),
    ("left", -90.0, 0.0),
    ("top", 0.0, MAX_ORBIT_PITCH_DEGREES),
    ("bottom", 0.0, -MAX_ORBIT_PITCH_DEGREES),
    ("iso", 45.0, 35.264),
    ("iso_rear", -135.0, 35.264),
    ("iso_front_left", -45.0, 35.264),
    ("iso_back_right", 135.0, 35.264),
    ("iso_bottom", 45.0, -35.264),
    ("iso_bottom_left", -45.0, -35.264),
    ("iso_bottom_back_right", 135.0, -35.264),
    ("iso_bottom_rear", -135.0, -35.264),
    ("front_right", 45.0, 0.0),
    ("front_left", -45.0, 0.0),
    ("back_right", 135.0, 0.0),
    ("back_left", -135.0, 0.0),
    ("top_front", 0.0, 45.0),
    ("top_right", 90.0, 45.0),
    ("top_back", 180.0, 45.0),
    ("top_left", -90.0, 45.0),
    ("bottom_front", 0.0, -45.0),
    ("bottom_right", 90.0, -45.0),
    ("bottom_back", 180.0, -45.0),
    ("bottom_left", -90.0, -45.0),
];

/// Angles for a named CAD orientation.
pub fn named_orientation(name: &str) -> Option<(f32, f32)> {
    NAMED_ORIENTATIONS
        .iter()
        .find(|(candidate, _, _)| *candidate == name)
        .map(|(_, yaw, pitch)| (*yaw, *pitch))
}

/// Named orientation matching a pose within one degree.
pub fn orientation_at(yaw_degrees: f32, pitch_degrees: f32) -> Option<&'static str> {
    NAMED_ORIENTATIONS
        .iter()
        .find(|(_, yaw, pitch)| {
            angular_difference(yaw_degrees, *yaw).abs() < 1.0 && (pitch_degrees - pitch).abs() < 1.0
        })
        .map(|(name, _, _)| *name)
}

/// An absolute orbit-camera pose independent of input toolkit or transport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitPose {
    /// Heading in degrees, normalized to `-180..=180`.
    pub yaw_degrees: f32,
    /// Elevation in degrees, clamped short of the poles.
    pub pitch_degrees: f32,
    /// Multiplier on the host scene's resting camera distance.
    pub distance: f32,
}

impl Default for OrbitPose {
    fn default() -> Self {
        let (yaw_degrees, pitch_degrees) = named_orientation("iso").unwrap_or((45.0, 35.264));
        Self {
            yaw_degrees,
            pitch_degrees,
            distance: 1.0,
        }
    }
}

impl OrbitPose {
    /// Creates a finite pose using the standard CAD orbit limits.
    pub fn new(yaw_degrees: f32, pitch_degrees: f32, distance: f32) -> Self {
        Self::with_limits(yaw_degrees, pitch_degrees, distance, DEFAULT_ORBIT_LIMITS)
    }

    fn with_limits(
        yaw_degrees: f32,
        pitch_degrees: f32,
        distance: f32,
        limits: OrbitLimits,
    ) -> Self {
        Self {
            yaw_degrees: wrap_degrees(yaw_degrees),
            pitch_degrees: finite_or(pitch_degrees, 0.0)
                .clamp(-limits.max_pitch_degrees, limits.max_pitch_degrees),
            distance: finite_or(distance, 1.0).clamp(limits.min_distance, limits.max_distance),
        }
    }

    /// Unit eye direction in a right-handed coordinate system with `+Y` up.
    pub fn direction(self) -> [f32; 3] {
        let yaw = self.yaw_degrees.to_radians();
        let pitch = self.pitch_degrees.to_radians();
        [
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ]
    }

    /// Converts a direction into orbit angles.
    pub fn angles_of(direction: [f32; 3]) -> (f32, f32) {
        let [x, y, z] = direction;
        let length = (x * x + y * y + z * z).sqrt();
        if !length.is_finite() || length < 1e-6 {
            return (0.0, 0.0);
        }
        let (x, y, z) = (x / length, y / length, z / length);
        (
            x.atan2(z).to_degrees(),
            y.clamp(-1.0, 1.0).asin().to_degrees(),
        )
    }

    /// Name of the orientation represented by this pose, when exact enough.
    pub fn named_orientation(self) -> Option<&'static str> {
        orientation_at(self.yaw_degrees, self.pitch_degrees)
    }
}

/// Interaction tuning and safety limits for an orbit controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitLimits {
    /// Yaw traversed by dragging across the full surface width.
    pub yaw_per_width: f32,
    /// Pitch traversed by dragging across the full surface height.
    pub pitch_per_height: f32,
    /// Degrees applied by one keyboard nudge.
    pub nudge_degrees: f32,
    /// Multiplicative zoom base applied per input step.
    pub zoom_step: f32,
    /// Absolute pitch limit.
    pub max_pitch_degrees: f32,
    /// Minimum distance multiplier.
    pub min_distance: f32,
    /// Maximum distance multiplier.
    pub max_distance: f32,
}

/// Standard mouse and key behavior used by CAD-style model viewers.
pub const DEFAULT_ORBIT_LIMITS: OrbitLimits = OrbitLimits {
    yaw_per_width: 270.0,
    pitch_per_height: 180.0,
    nudge_degrees: 15.0,
    zoom_step: 1.12,
    max_pitch_degrees: MAX_ORBIT_PITCH_DEGREES,
    min_distance: MIN_ORBIT_DISTANCE,
    max_distance: MAX_ORBIT_DISTANCE,
};

/// Toolkit-independent state machine for orbit camera interaction.
#[derive(Debug, Clone, PartialEq)]
pub struct OrbitController {
    pose: OrbitPose,
    limits: OrbitLimits,
    enabled: bool,
    anchor: Option<Point>,
}

impl Default for OrbitController {
    fn default() -> Self {
        Self::new(OrbitPose::default(), DEFAULT_ORBIT_LIMITS)
    }
}

impl OrbitController {
    /// Creates an orbit controller from an absolute pose and explicit limits.
    pub fn new(pose: OrbitPose, limits: OrbitLimits) -> Self {
        let pose =
            OrbitPose::with_limits(pose.yaw_degrees, pose.pitch_degrees, pose.distance, limits);
        Self {
            pose,
            limits,
            enabled: true,
            anchor: None,
        }
    }

    /// Current absolute pose.
    pub const fn pose(&self) -> OrbitPose {
        self.pose
    }

    /// Whether authored idle orbit is enabled.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether a pointer drag is in progress.
    pub const fn is_grabbed(&self) -> bool {
        self.anchor.is_some()
    }

    /// Toggles authored idle orbit and releases any pointer capture when disabled.
    pub fn toggle_enabled(&mut self) -> bool {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.anchor = None;
        }
        self.enabled
    }

    /// Disables authored idle orbit.
    pub fn stop_auto_orbit(&mut self) {
        self.enabled = false;
    }

    /// Begins a pointer drag.
    pub fn press(&mut self, at: Point) {
        self.anchor = Some(at);
    }

    /// Ends a pointer drag.
    pub fn release(&mut self) {
        self.anchor = None;
    }

    /// Applies one pointer movement relative to the previous movement.
    pub fn drag(&mut self, to: Point, surface: Size) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        if !surface.is_valid() || surface.width <= 1.0 || surface.height <= 1.0 {
            return false;
        }
        let dx = (to.x - anchor.x) / surface.width;
        let dy = (to.y - anchor.y) / surface.height;
        self.anchor = Some(to);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.pose.yaw_degrees =
            wrap_degrees(self.pose.yaw_degrees - dx * self.limits.yaw_per_width);
        self.pose.pitch_degrees = (self.pose.pitch_degrees + dy * self.limits.pitch_per_height)
            .clamp(
                -self.limits.max_pitch_degrees,
                self.limits.max_pitch_degrees,
            );
        true
    }

    /// Applies a wheel or pinch zoom. Positive values zoom in.
    pub fn zoom(&mut self, steps: f32) -> bool {
        if !steps.is_finite() || steps == 0.0 {
            return false;
        }
        let before = self.pose.distance;
        self.pose.distance = (self.pose.distance / self.limits.zoom_step.powf(steps))
            .clamp(self.limits.min_distance, self.limits.max_distance);
        self.pose.distance.to_bits() != before.to_bits()
    }

    /// Jumps to a named orientation.
    pub fn snap(&mut self, name: &str) -> bool {
        let Some((yaw_degrees, pitch_degrees)) = named_orientation(name) else {
            return false;
        };
        self.pose.yaw_degrees = yaw_degrees;
        self.pose.pitch_degrees = pitch_degrees;
        true
    }

    /// Turns by configured keyboard-nudge increments.
    pub fn nudge(&mut self, yaw_steps: f32, pitch_steps: f32) -> bool {
        if !yaw_steps.is_finite()
            || !pitch_steps.is_finite()
            || (yaw_steps == 0.0 && pitch_steps == 0.0)
        {
            return false;
        }
        self.pose.yaw_degrees =
            wrap_degrees(self.pose.yaw_degrees + yaw_steps * self.limits.nudge_degrees);
        self.pose.pitch_degrees =
            (self.pose.pitch_degrees + pitch_steps * self.limits.nudge_degrees).clamp(
                -self.limits.max_pitch_degrees,
                self.limits.max_pitch_degrees,
            );
        true
    }

    /// Restores the default isometric pose and releases pointer capture.
    pub fn reset(&mut self) -> bool {
        self.pose = OrbitPose::with_limits(
            OrbitPose::default().yaw_degrees,
            OrbitPose::default().pitch_degrees,
            OrbitPose::default().distance,
            self.limits,
        );
        self.anchor = None;
        true
    }
}

/// Discrete camera actions used by key bindings and orientation controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrbitAction {
    /// Jump to one named orientation.
    Snap(&'static str),
    /// Turn by yaw and pitch step counts.
    Nudge(f32, f32),
    /// Zoom by a signed number of steps.
    Zoom(f32),
    /// Restore the default pose.
    Reset,
}

impl OrbitAction {
    /// Applies this action and reports whether the pose changed.
    pub fn apply(self, controller: &mut OrbitController) -> bool {
        match self {
            Self::Snap(name) => controller.snap(name),
            Self::Nudge(yaw, pitch) => controller.nudge(yaw, pitch),
            Self::Zoom(steps) => controller.zoom(steps),
            Self::Reset => controller.reset(),
        }
    }
}

fn angular_difference(left: f32, right: f32) -> f32 {
    wrap_degrees(left - right)
}

fn wrap_degrees(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let wrapped = value % 360.0;
    if wrapped > 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_orientations_round_trip_through_directions() {
        for (name, yaw, pitch) in NAMED_ORIENTATIONS {
            let pose = OrbitPose::new(yaw, pitch, 1.0);
            let (round_trip_yaw, round_trip_pitch) = OrbitPose::angles_of(pose.direction());
            assert!((round_trip_pitch - pitch).abs() < 0.01, "{name}");
            if pitch.abs() < MAX_ORBIT_PITCH_DEGREES - 1.0 {
                assert!(
                    angular_difference(round_trip_yaw, yaw).abs() < 0.01,
                    "{name}"
                );
            }
            assert_eq!(pose.named_orientation(), Some(name));
        }
    }

    #[test]
    fn drag_is_surface_relative_and_bounded() {
        let mut small = OrbitController::default();
        small.press(Point::new(0.0, 0.0));
        assert!(small.drag(Point::new(200.0, 150.0), Size::new(200.0, 150.0)));

        let mut large = OrbitController::default();
        large.press(Point::new(0.0, 0.0));
        assert!(large.drag(Point::new(1_920.0, 1_080.0), Size::new(1_920.0, 1_080.0),));
        assert_eq!(small.pose(), large.pose());
        // Clamping must return the exact published limit.
        assert_eq!(
            small.pose().pitch_degrees.to_bits(),
            MAX_ORBIT_PITCH_DEGREES.to_bits()
        );
    }

    #[test]
    fn invalid_input_cannot_escape_as_non_finite_state() {
        let pose = OrbitPose::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        assert!(pose.yaw_degrees.is_finite());
        assert!(pose.pitch_degrees.is_finite());
        assert!(pose.distance.is_finite());
        let mut controller = OrbitController::default();
        assert!(!controller.zoom(f32::NAN));
        assert!(!controller.nudge(f32::INFINITY, 0.0));
    }
}
