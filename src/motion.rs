//! Retargetable, frame-rate-independent motion for host-owned rendering loops.
//! No timers, allocations, threads, or redraws are owned by GFX.

use serde::{Deserialize, Serialize};

/// Interpolation curve a [`Tween`] applies between its endpoints.
///
/// The curve is deterministic and frame-rate-independent: the same host time
/// always yields the same displayed value regardless of the selected curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    /// Ease-out cubic. Smooth and decelerating — organic controls.
    Cubic,
    /// Constant-velocity linear ramp. Neutral and mechanical.
    Linear,
    /// Discrete stepped ramp reaching `steps` equal increments one at a time
    /// over the duration — the classic LED/LCD readout or terminal bar feel.
    /// `steps == 0` degrades to a settled value.
    Stepped(u16),
}

/// A finite scalar transition. Retargeting starts at the currently displayed value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    /// Interpolation curve applied between the endpoints.
    ease: Ease,
    /// Optional output grid in logical units that snaps the displayed value to
    /// its nearest multiple; `0.0` disables snapping (terminal/pixel readouts).
    quantum: f32,
}

impl Tween {
    /// Create a settled value; invalid input is rejected.
    pub fn new(value: f32) -> Option<Self> {
        value.is_finite().then_some(Self {
            from: value,
            to: value,
            elapsed: 0.0,
            duration: 0.0,
            ease: Ease::Cubic,
            quantum: 0.0,
        })
    }

    /// Create a settled value with a chosen easing curve and optional snap grid.
    pub fn with_style(value: f32, ease: Ease, quantum: f32) -> Option<Self> {
        Self::new(value).map(|tween| tween.style(ease, quantum))
    }

    /// Returns a copy with the easing curve and snap grid set.
    /// `quantum` is clamped to be non-negative; negative values disable snapping.
    pub fn style(mut self, ease: Ease, quantum: f32) -> Self {
        self.ease = ease;
        self.quantum = quantum.max(0.0);
        self
    }

    /// Sets the easing curve in place.
    pub fn set_ease(&mut self, ease: Ease) {
        self.ease = ease;
    }

    /// Sets the output snap grid in place (logical units). Non-finite or
    /// negative values disable snapping.
    pub fn set_quantum(&mut self, quantum: f32) {
        self.quantum = if quantum.is_finite() && quantum >= 0.0 {
            quantum
        } else {
            0.0
        };
    }

    /// Retarget without a jump. Zero duration implements reduced motion.
    /// Invalid targets or durations leave the transition unchanged. The current
    /// easing curve and snap grid are preserved across retargets.
    pub fn retarget(&mut self, target: f32, seconds: f32) -> bool {
        self.retarget_eased(target, seconds, self.ease)
    }

    /// Retarget with an explicit easing curve, preserving any snap grid.
    /// Invalid targets or durations leave the transition unchanged.
    pub fn retarget_eased(&mut self, target: f32, seconds: f32, ease: Ease) -> bool {
        if !target.is_finite() || !seconds.is_finite() || seconds < 0.0 {
            return false;
        }
        self.from = self.value();
        self.to = target;
        self.elapsed = 0.0;
        self.duration = seconds;
        self.ease = ease;
        true
    }

    /// Advance using elapsed host time, not frame count. Invalid deltas are ignored.
    pub fn advance(&mut self, seconds: f32) -> f32 {
        if seconds.is_finite() && seconds >= 0.0 {
            self.elapsed = (self.elapsed + seconds).min(self.duration);
        }
        self.value()
    }

    /// Current displayed value for the selected easing curve and snap grid,
    /// bounded by the endpoints before snapping.
    pub fn value(&self) -> f32 {
        let t = if self.duration == 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        };
        let weight = self.ease_weight(t);
        // Weighted endpoints avoid overflowing a finite endpoint difference.
        let value = self.from * (1.0 - weight) + self.to * weight;
        self.snap(value)
    }

    /// Normalized [0, 1] progress weight for the active easing curve.
    fn ease_weight(self: &Tween, t: f32) -> f32 {
        match self.ease {
            Ease::Cubic => 1.0 - (1.0 - t).powi(3),
            Ease::Linear => t,
            Ease::Stepped(steps) => {
                if steps == 0 {
                    return 1.0;
                }
                let total = steps as f32;
                // Held step index, clamped so the final step lands exactly on 1.0.
                let step = (t * total).floor().clamp(0.0, total);
                step / total
            }
        }
    }

    /// Snaps a value to the configured output grid; identity when disabled.
    fn snap(&self, value: f32) -> f32 {
        if self.quantum > 0.0 {
            (value / self.quantum).round() * self.quantum
        } else {
            value
        }
    }

    /// Whether the host needs another animation frame.
    pub fn is_active(&self) -> bool {
        self.elapsed < self.duration && self.from != self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retarget_is_continuous_and_settles() {
        let mut tween = Tween::new(0.0).unwrap();
        assert!(tween.retarget(1.0, 0.2));
        let displayed = tween.advance(0.05);
        assert!(tween.retarget(0.0, 0.1));
        assert_eq!(tween.value(), displayed);
        assert_eq!(tween.advance(1.0), 0.0);
        assert!(!tween.is_active());
    }
    #[test]
    fn reduced_motion_and_invalid_input() {
        let mut tween = Tween::new(1.0).unwrap();
        assert!(!tween.retarget(f32::NAN, 0.1));
        assert!(!tween.retarget(2.0, -1.0));
        assert_eq!(tween.advance(f32::INFINITY), 1.0);
        assert!(tween.retarget(2.0, 0.0));
        assert_eq!(tween.value(), 2.0);
        assert!(!tween.is_active());
    }
    #[test]
    fn sampling_is_frame_rate_independent() {
        let mut a = Tween::new(0.0).unwrap();
        a.retarget(10.0, 1.0);
        let mut b = a;
        a.advance(0.25);
        for _ in 0..4 {
            b.advance(0.0625);
        }
        assert_eq!(a.value(), b.value());
    }

    #[test]
    fn linear_is_a_constant_velocity_ramp() {
        let mut tween = Tween::new(0.0).unwrap();
        tween.retarget_eased(10.0, 1.0, Ease::Linear);
        assert!((tween.advance(0.5) - 5.0).abs() < 1e-6);
        assert!((tween.advance(0.5) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn stepped_holds_discrete_increments_and_settles_exactly() {
        let mut tween = Tween::with_style(0.0, Ease::Stepped(4), 0.0).unwrap();
        tween.retarget(1.0, 1.0);
        // Below the first step boundary it still reads zero.
        assert_eq!(tween.advance(0.24), 0.0);
        // First step ladders up in held increments.
        assert_eq!(tween.advance(0.01), 0.25);
        assert_eq!(tween.advance(0.24), 0.25);
        assert_eq!(tween.advance(0.25), 0.5);
        // The final step lands exactly on the target, never short.
        assert_eq!(tween.advance(1.0), 1.0);
        assert!(!tween.is_active());
    }

    #[test]
    fn quantum_snaps_the_output_to_a_grid() {
        let mut tween = Tween::with_style(0.0, Ease::Linear, 1.0).unwrap();
        tween.retarget(10.0, 1.0);
        // 0.49 progress -> 4.9 -> snaps to 5; 0.10 -> 1.0 stays.
        assert_eq!(tween.advance(0.49), 5.0);
        assert_eq!(tween.advance(0.11), 6.0);
    }

    #[test]
    fn style_survives_retarget_and_invalid_quantum_disables() {
        let mut tween = Tween::with_style(0.0, Ease::Stepped(2), 0.5).unwrap();
        tween.retarget(1.0, 1.0);
        // Ease + grid survive a retarget: stepped(2) at half -> 0.5, on the 0.5 grid.
        assert_eq!(tween.advance(0.5), 0.5);
        tween.set_quantum(f32::NAN);
        assert_eq!(tween.quantum, 0.0);
        tween.set_quantum(-1.0);
        assert_eq!(tween.quantum, 0.0);
    }

    #[test]
    fn ease_is_serializable() {
        let json = serde_json::to_string(&Ease::Stepped(8)).unwrap();
        assert_eq!(serde_json::from_str::<Ease>(&json).unwrap(), Ease::Stepped(8));
    }
}
