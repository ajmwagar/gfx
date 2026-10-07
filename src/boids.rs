//! Caller-owned deterministic flock dynamics and ordinary scene primitives.
use crate::{Primitive, Rect, Scene, ThemeRole};

/// Hard cap for the quadratic neighbourhood search.
pub const MAX_BOIDS: usize = 128;

#[derive(Clone, Copy, Debug)]
struct Bird {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

/// A bounded flock. The host owns its clock, lifecycle, viewport and theme.
pub struct Flock {
    birds: Vec<Bird>,
    scratch: Vec<Bird>,
    aspect: f32,
    shark: Option<Bird>,
    prey: usize,
    retarget_in: f32,
    bite_remaining: f32,
    catches: u32,
}
impl Flock {
    /// Seed a deterministic flock in a toroidal, aspect-correct world.
    pub fn new(count: usize, aspect: f32) -> Result<Self, &'static str> {
        if count == 0 || count > MAX_BOIDS || !aspect.is_finite() || !(0.25..=4.0).contains(&aspect)
        {
            return Err("Invalid flock count or aspect");
        }
        let mut seed = 0x4f50454e_u32;
        let mut unit = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let birds: Vec<_> = (0..count)
            .map(|_| {
                let x = unit() * aspect;
                let y = unit();
                let angle = unit() * std::f32::consts::TAU;
                Bird {
                    x,
                    y,
                    vx: angle.cos() * 0.09,
                    vy: angle.sin() * 0.09,
                }
            })
            .collect();
        Ok(Self {
            scratch: birds.clone(),
            birds,
            aspect,
            shark: None,
            prey: 0,
            retarget_in: 0.0,
            bite_remaining: 0.0,
            catches: 0,
        })
    }
    /// Enable a predator; school members react to its actual position.
    pub fn aquarium(count: usize, aspect: f32) -> Result<Self, &'static str> {
        let mut flock = Self::new(count, aspect)?;
        // Start as a school rather than an evenly scattered flock.
        for bird in &mut flock.birds {
            bird.x = aspect * 0.55 + (bird.x / aspect - 0.5) * 0.35;
            bird.y = 0.5 + (bird.y - 0.5) * 0.35;
        }
        flock.shark = Some(Bird {
            x: aspect * 0.2,
            y: 0.5,
            vx: 0.1,
            vy: 0.0,
        });
        Ok(flock)
    }
    /// Advance at most 100ms; paused hosts must not accumulate catch-up work.
    pub fn step(&mut self, dt: f32) -> Result<(), &'static str> {
        if !dt.is_finite() || !(0.0..=0.1).contains(&dt) {
            return Err("Invalid flock timestep");
        }
        self.scratch.clone_from(&self.birds);
        self.bite_remaining = (self.bite_remaining - dt).max(0.0);
        if let Some(shark) = &mut self.shark {
            // Sticky target, straggler preference, and bounded intercept prediction.
            self.retarget_in -= dt;
            if self.retarget_in <= 0.0 {
                let score = |index: usize| {
                    let fish = self.scratch[index];
                    let neighbours = self
                        .scratch
                        .iter()
                        .filter(|other| (other.x - fish.x).hypot(other.y - fish.y) < 0.12)
                        .count();
                    (fish.x - shark.x).hypot(fish.y - shark.y) * (1.0 + neighbours as f32 * 0.06)
                };
                let best = (0..self.scratch.len())
                    .min_by(|a, b| score(*a).total_cmp(&score(*b)))
                    .ok_or("Empty school")?;
                if self.retarget_in <= -dt || score(best) < score(self.prey) * 0.65 {
                    self.prey = best;
                }
                self.retarget_in = 0.8;
            }
            let prey = self.scratch[self.prey];
            let lead = ((prey.x - shark.x).hypot(prey.y - shark.y) / 0.24).clamp(0.0, 1.2);
            let dx = (prey.x + prey.vx * lead).clamp(0.11, self.aspect - 0.11) - shark.x;
            let dy = (prey.y + prey.vy * lead).clamp(0.11, 0.89) - shark.y;
            let distance = dx.hypot(dy).max(0.001);
            let speed = if distance < 0.2 { 0.32 } else { 0.24 };
            shark.vx += (dx / distance * speed - shark.vx) * dt * 2.5;
            shark.vy += (dy / distance * speed - shark.vy) * dt * 2.5;
            shark.x += shark.vx * dt;
            shark.y += shark.vy * dt;
            contain(shark, self.aspect, 0.11);
        }
        for (i, bird) in self.birds.iter_mut().enumerate() {
            let original = self.scratch[i];
            let (mut neighbours, mut cx, mut cy, mut ax, mut ay, mut sx, mut sy) =
                (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            for (j, other) in self.scratch.iter().enumerate() {
                if i == j {
                    continue;
                }
                let dx = if self.shark.is_some() {
                    other.x - original.x
                } else {
                    wrap_delta(other.x - original.x, self.aspect)
                };
                let dy = if self.shark.is_some() {
                    other.y - original.y
                } else {
                    wrap_delta(other.y - original.y, 1.0)
                };
                let distance2 = dx * dx + dy * dy;
                if distance2 < 0.18 * 0.18 {
                    neighbours += 1.0;
                    cx += dx;
                    cy += dy;
                    ax += other.vx;
                    ay += other.vy;
                    if distance2 < 0.055 * 0.055 {
                        let weight = 1.0 / distance2.max(0.0001);
                        sx -= dx * weight;
                        sy -= dy * weight;
                    }
                }
            }
            let (mut fx, mut fy) = if neighbours > 0.0 {
                (
                    cx / neighbours * 0.9 + (ax / neighbours - original.vx) * 1.4 + sx * 0.002,
                    cy / neighbours * 0.9 + (ay / neighbours - original.vy) * 1.4 + sy * 0.002,
                )
            } else {
                (0.0, 0.0)
            };
            if let Some(shark) = self.shark {
                let dx = original.x - shark.x;
                let dy = original.y - shark.y;
                let distance = dx.hypot(dy).max(0.001);
                if distance < 0.3 {
                    let panic = (1.0 - distance / 0.3) * 0.8;
                    fx += dx / distance * panic;
                    fy += dy / distance * panic;
                }
            }
            let acceleration = fx.hypot(fy).max(0.25);
            bird.vx += fx * (0.25 / acceleration) * dt;
            bird.vy += fy * (0.25 / acceleration) * dt;
            let speed = bird.vx.hypot(bird.vy).max(0.0001);
            let scale = speed.clamp(0.045, 0.14) / speed;
            bird.vx *= scale;
            bird.vy *= scale;
            bird.x = original.x + bird.vx * dt;
            bird.y = original.y + bird.vy * dt;
            if self.shark.is_some() {
                contain(bird, self.aspect, 0.018);
            } else {
                bird.x = bird.x.rem_euclid(self.aspect);
                bird.y = bird.y.rem_euclid(1.0);
            }
        }
        if let Some(shark) = self.shark {
            let speed = shark.vx.hypot(shark.vy).max(0.001);
            let mouth = [
                shark.x + shark.vx / speed * 0.09,
                shark.y + shark.vy / speed * 0.09,
            ];
            if self.bite_remaining == 0.0 {
                if let Some(fish) = self
                    .birds
                    .iter_mut()
                    .find(|fish| (fish.x - mouth[0]).hypot(fish.y - mouth[1]) < 0.032)
                {
                    self.bite_remaining = 0.65;
                    self.catches = self.catches.saturating_add(1);
                    // Replenish on the opposite side, preserving a bounded school.
                    fish.x = self.aspect - shark.x;
                    fish.y = 1.0 - shark.y;
                    contain(fish, self.aspect, 0.018);
                    self.retarget_in = 0.0;
                }
            }
        }
        Ok(())
    }
    /// Brief catch feedback; the host owns its overlay and accessibility policy.
    pub fn catch_flash(&self) -> f32 {
        ((self.bite_remaining - 0.4) / 0.25).clamp(0.0, 1.0)
    }
    /// Append two oriented, themeable strokes per bird; no raster allocation.
    pub fn append(&self, scene: &mut Scene, bounds: Rect, ink: ThemeRole) {
        let size = (bounds.height * 0.006).clamp(3.0, 12.0);
        for bird in &self.birds {
            let center = [
                bounds.x + bird.x / self.aspect * bounds.width,
                bounds.y + bird.y * bounds.height,
            ];
            let length = bird.vx.hypot(bird.vy).max(0.0001);
            let direction = [bird.vx / length, bird.vy / length];
            let nose = [
                center[0] + direction[0] * size,
                center[1] + direction[1] * size,
            ];
            for sign in [-1.0, 1.0] {
                let tail = [
                    center[0] - direction[0] * size * 0.6 - direction[1] * size * sign * 0.65,
                    center[1] - direction[1] * size * 0.6 + direction[0] * size * sign * 0.65,
                ];
                scene.push(Primitive::line(nose, tail, (size * 0.16).max(1.0), ink));
            }
        }
        if let Some(shark) = self.shark {
            let size = size * 3.0;
            let center = [
                bounds.x + shark.x / self.aspect * bounds.width,
                bounds.y + shark.y * bounds.height,
            ];
            let speed = shark.vx.hypot(shark.vy).max(0.001);
            let dir = [shark.vx / speed, shark.vy / speed];
            let point = |x: f32, y: f32| {
                [
                    center[0] + (dir[0] * x - dir[1] * y) * size,
                    center[1] + (dir[1] * x + dir[0] * y) * size,
                ]
            };
            // Distinct shark silhouette: pointed head, broad body, dorsal fin and forked tail.
            let jaw = if self.bite_remaining > 0.0 {
                (self.bite_remaining / 0.65 * std::f32::consts::PI).sin() * 1.3
            } else {
                0.0
            };
            let shape = [
                (5.0, -jaw),
                (1.0, -1.5),
                (-1.0, -1.2),
                (-3.0, -3.0),
                (-2.5, 0.0),
                (-3.0, 3.0),
                (-1.0, 1.2),
                (0.0, 3.0),
                (1.0, 1.5),
                (5.0, jaw),
            ];
            for pair in shape.windows(2) {
                scene.push(Primitive::line(
                    point(pair[0].0, pair[0].1),
                    point(pair[1].0, pair[1].1),
                    size * 0.3,
                    ThemeRole::Text,
                ));
            }
            if jaw > 0.0 {
                for side in [-1.0, 1.0] {
                    scene.push(Primitive::line(
                        point(5.0, side * jaw),
                        point(3.0, 0.0),
                        size * 0.22,
                        ThemeRole::Text,
                    ));
                }
            }
        }
    }
}
fn wrap_delta(delta: f32, extent: f32) -> f32 {
    (delta + extent * 0.5).rem_euclid(extent) - extent * 0.5
}

fn contain(bird: &mut Bird, aspect: f32, margin: f32) {
    if bird.x < margin {
        bird.x = margin;
        bird.vx = bird.vx.abs();
    }
    if bird.x > aspect - margin {
        bird.x = aspect - margin;
        bird.vx = -bird.vx.abs();
    }
    if bird.y < margin {
        bird.y = margin;
        bird.vy = bird.vy.abs();
    }
    if bird.y > 1.0 - margin {
        bird.y = 1.0 - margin;
        bird.vy = -bird.vy.abs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_flock_stays_finite_bounded_and_renders_shared_primitives() {
        let mut a = Flock::new(64, 16.0 / 9.0).unwrap();
        let mut b = Flock::new(64, 16.0 / 9.0).unwrap();
        for _ in 0..600 {
            a.step(1.0 / 30.0).unwrap();
            b.step(1.0 / 30.0).unwrap();
        }
        for (a, b) in a.birds.iter().zip(&b.birds) {
            assert_eq!(a.x, b.x);
            assert!((0.0..16.0 / 9.0).contains(&a.x) && (0.0..1.0).contains(&a.y));
            assert!((0.044..0.141).contains(&a.vx.hypot(a.vy)));
        }
        let mut scene = Scene::with_capacity(128);
        a.append(
            &mut scene,
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            ThemeRole::Primary,
        );
        assert_eq!(scene.iter().count(), 128);
    }
    #[test]
    fn limits_and_neighbour_rules() {
        assert!(Flock::new(0, 1.0).is_err());
        assert!(Flock::new(MAX_BOIDS + 1, 1.0).is_err());
        assert!(Flock::new(64, f32::NAN).is_err());
        let mut flock = Flock::new(2, 1.0).unwrap();
        assert!(flock.step(f32::NAN).is_err());
        assert!(flock.step(1.0).is_err());
        flock.birds = vec![
            Bird {
                x: 0.49,
                y: 0.5,
                vx: 0.09,
                vy: 0.0,
            },
            Bird {
                x: 0.51,
                y: 0.5,
                vx: 0.09,
                vy: 0.0,
            },
        ];
        flock.step(0.1).unwrap();
        assert!(
            flock.birds[0].vx < flock.birds[1].vx,
            "Close neighbours separate"
        );
        assert!((wrap_delta(0.98, 1.0) + 0.02).abs() < 0.0001);
    }
    #[test]
    fn predator_pursues_and_school_escapes() {
        let mut aquarium = Flock::aquarium(64, 16.0 / 9.0).unwrap();
        let first = aquarium.shark.unwrap();
        for _ in 0..3600 {
            aquarium.step(1.0 / 60.0).unwrap();
        }
        let shark = aquarium.shark.unwrap();
        assert_ne!(first.x, shark.x);
        assert!(shark.x.is_finite() && (0.0..1.0).contains(&shark.y));
        assert!(aquarium
            .birds
            .iter()
            .all(|b| b.x.is_finite() && b.y.is_finite()));
        let mut scene = Scene::with_capacity(140);
        aquarium.append(
            &mut scene,
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            ThemeRole::Primary,
        );
        assert!((137..=139).contains(&scene.iter().count()));
        assert!(aquarium
            .birds
            .iter()
            .all(|fish| (0.018..=aquarium.aspect - 0.018).contains(&fish.x)
                && (0.018..=0.982).contains(&fish.y)));
        assert!((0.11..=aquarium.aspect - 0.11).contains(&shark.x));
    }
    #[test]
    fn nearby_fish_turn_away_from_predator() {
        let mut hunted = Flock::aquarium(1, 1.0).unwrap();
        let mut safe = Flock::new(1, 1.0).unwrap();
        let fish = Bird {
            x: 0.55,
            y: 0.5,
            vx: 0.0,
            vy: 0.09,
        };
        hunted.birds[0] = fish;
        safe.birds[0] = fish;
        hunted.shark = Some(Bird {
            x: 0.5,
            y: 0.5,
            vx: 0.0,
            vy: 0.0,
        });
        hunted.step(0.1).unwrap();
        safe.step(0.1).unwrap();
        assert!(hunted.birds[0].vx > safe.birds[0].vx);
        assert!(hunted.shark.unwrap().vx > 0.0);
    }
    #[test]
    fn catch_chomps_flashes_and_replenishes_without_unbounded_growth() {
        let mut tank = Flock::aquarium(1, 1.0).unwrap();
        tank.shark = Some(Bird {
            x: 0.4,
            y: 0.5,
            vx: 0.24,
            vy: 0.0,
        });
        tank.birds[0] = Bird {
            x: 0.49,
            y: 0.5,
            vx: 0.09,
            vy: 0.0,
        };
        tank.step(0.0).unwrap();
        assert_eq!(tank.catches, 1);
        assert_eq!(tank.birds.len(), 1);
        assert!((tank.catch_flash() - 1.0).abs() < 0.00001);
        assert!(tank.birds[0].x > 0.5);
        for _ in 0..4 {
            tank.step(0.1).unwrap();
        }
        assert_eq!(tank.catch_flash(), 0.0);
        let mut edge = Bird {
            x: -1.0,
            y: 2.0,
            vx: -0.24,
            vy: 0.24,
        };
        contain(&mut edge, 1.0, 0.11);
        assert_eq!(edge.x, 0.11);
        assert_eq!(edge.y, 0.89);
        assert!(edge.vx > 0.0 && edge.vy < 0.0);
    }
}
