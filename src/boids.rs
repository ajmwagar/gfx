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
        if let Some(shark) = &mut self.shark {
            // Pursue the nearest real fish, not a scripted orbit.
            let prey = self
                .scratch
                .iter()
                .min_by(|a, b| {
                    let distance = |bird: &Bird| {
                        wrap_delta(bird.x - shark.x, self.aspect)
                            .hypot(wrap_delta(bird.y - shark.y, 1.0))
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .ok_or("Empty school")?;
            let dx = wrap_delta(prey.x - shark.x, self.aspect);
            let dy = wrap_delta(prey.y - shark.y, 1.0);
            let distance = dx.hypot(dy).max(0.001);
            shark.vx += (dx / distance * 0.11 - shark.vx) * dt * 1.4;
            shark.vy += (dy / distance * 0.11 - shark.vy) * dt * 1.4;
            shark.x = (shark.x + shark.vx * dt).rem_euclid(self.aspect);
            shark.y = (shark.y + shark.vy * dt).rem_euclid(1.0);
        }
        for (i, bird) in self.birds.iter_mut().enumerate() {
            let original = self.scratch[i];
            let (mut neighbours, mut cx, mut cy, mut ax, mut ay, mut sx, mut sy) =
                (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            for (j, other) in self.scratch.iter().enumerate() {
                if i == j {
                    continue;
                }
                let dx = wrap_delta(other.x - original.x, self.aspect);
                let dy = wrap_delta(other.y - original.y, 1.0);
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
                let dx = wrap_delta(original.x - shark.x, self.aspect);
                let dy = wrap_delta(original.y - shark.y, 1.0);
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
            bird.x = (original.x + bird.vx * dt).rem_euclid(self.aspect);
            bird.y = (original.y + bird.vy * dt).rem_euclid(1.0);
        }
        Ok(())
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
            let shape = [
                (5.0, 0.0),
                (1.0, -1.5),
                (-1.0, -1.2),
                (-3.0, -3.0),
                (-2.5, 0.0),
                (-3.0, 3.0),
                (-1.0, 1.2),
                (0.0, 3.0),
                (1.0, 1.5),
                (5.0, 0.0),
            ];
            for pair in shape.windows(2) {
                scene.push(Primitive::line(
                    point(pair[0].0, pair[0].1),
                    point(pair[1].0, pair[1].1),
                    size * 0.3,
                    ThemeRole::Text,
                ));
            }
        }
    }
}
fn wrap_delta(delta: f32, extent: f32) -> f32 {
    (delta + extent * 0.5).rem_euclid(extent) - extent * 0.5
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
        assert_eq!(scene.iter().count(), 137);
    }
    #[test]
    fn nearby_fish_turn_away_from_predator() {
        let mut hunted = Flock::aquarium(1,1.0).unwrap();
        let mut safe = Flock::new(1,1.0).unwrap();
        let fish = Bird { x:0.55,y:0.5,vx:0.0,vy:0.09 };
        hunted.birds[0]=fish;
        safe.birds[0]=fish;
        hunted.shark=Some(Bird { x:0.5,y:0.5,vx:0.0,vy:0.0 });
        hunted.step(0.1).unwrap();
        safe.step(0.1).unwrap();
        assert!(hunted.birds[0].vx > safe.birds[0].vx);
        assert!(hunted.shark.unwrap().vx > 0.0);
    }
}
