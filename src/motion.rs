//! Retargetable, frame-rate-independent motion for host-owned rendering loops.
//! No timers, allocations, threads, or redraws are owned by GFX.

/// A finite scalar transition. Retargeting starts at the currently displayed value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
}

impl Tween {
    /// Create a settled value; invalid input is rejected.
    pub fn new(value: f32) -> Option<Self> {
        value.is_finite().then_some(Self {
            from: value,
            to: value,
            elapsed: 0.0,
            duration: 0.0,
        })
    }

    /// Retarget without a jump. Zero duration implements reduced motion.
    /// Invalid targets or durations leave the transition unchanged.
    pub fn retarget(&mut self, target: f32, seconds: f32) -> bool {
        if !target.is_finite() || !seconds.is_finite() || seconds < 0.0 {
            return false;
        }
        self.from = self.value();
        self.to = target;
        self.elapsed = 0.0;
        self.duration = seconds;
        true
    }

    /// Advance using elapsed host time, not frame count. Invalid deltas are ignored.
    pub fn advance(&mut self, seconds: f32) -> f32 {
        if seconds.is_finite() && seconds >= 0.0 {
            self.elapsed = (self.elapsed + seconds).min(self.duration);
        }
        self.value()
    }

    /// Current ease-out cubic value, bounded by the endpoints.
    pub fn value(&self) -> f32 {
        let t = if self.duration == 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        };
        let weight = 1.0 - (1.0 - t).powi(3);
        // Weighted endpoints avoid overflowing a finite endpoint difference.
        self.from * (1.0 - weight) + self.to * weight
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
}
