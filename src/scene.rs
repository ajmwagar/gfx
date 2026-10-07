#[cfg(feature = "wgpu")]
use bytemuck::{Pod, Zeroable};

use crate::{Rect, ThemeRole};

/// GPU-rendered primitive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PrimitiveKind {
    /// Axis-aligned antialiased rounded rectangle.
    RoundedRect = 0,
    /// Antialiased circular control.
    Disc = 1,
    /// Vertical normalized level display.
    Meter = 2,
    /// Rotary control with an indicator.
    Knob = 3,
    /// Circular status lamp with contained emission.
    Lamp = 4,
    /// Antialiased round-capped line segment.
    Line = 5,
    /// Antialiased circular arc.
    Arc = 6,
    /// Procedural grooved vinyl with a paper label and transparent spindle hole.
    Vinyl = 7,
}

/// One compact, instance-rendered visual primitive.
#[must_use = "a primitive must be added to a scene to be rendered"]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primitive {
    /// Primitive bounds in logical pixels.
    pub rect: Rect,
    /// Shape implementation.
    pub kind: PrimitiveKind,
    /// Main semantic color.
    pub fill: ThemeRole,
    /// Outline or track color.
    pub outline: ThemeRole,
    /// Indicator, meter, or emission color.
    pub accent: ThemeRole,
    /// Corner radius or disc inset in logical pixels.
    pub radius: f32,
    /// Outline width in logical pixels.
    pub outline_width: f32,
    /// Normalized control value.
    pub value: f32,
    /// Indicator rotation in radians.
    pub rotation: f32,
    /// Directional highlight strength.
    pub gloss: f32,
    /// Fine-grain material strength.
    pub grain: f32,
    /// Contained emission strength.
    pub emission: f32,
}

impl Primitive {
    /// Constructs a rounded rectangle.
    pub const fn rounded_rect(rect: Rect, radius: f32, fill: ThemeRole) -> Self {
        Self::base(rect, PrimitiveKind::RoundedRect, fill)
            .with_radius(radius)
            .with_outline(ThemeRole::Outline, 1.0)
    }

    /// Constructs a circular primitive fitted to the rectangle.
    pub const fn disc(rect: Rect, fill: ThemeRole) -> Self {
        Self::base(rect, PrimitiveKind::Disc, fill)
    }

    /// One-instance record material. `rotation` is a host-owned angle in radians;
    /// GFX never infers that a selected record is playing.
    pub const fn vinyl(rect: Rect, label: ThemeRole, rotation: f32) -> Self {
        Self::base(rect, PrimitiveKind::Vinyl, ThemeRole::SurfaceRecessed)
            .with_accent(label)
            .with_value(0.32)
            .with_radius(0.018)
            .with_rotation(rotation)
            .with_gloss(0.65)
    }

    /// Constructs a vertical level meter.
    pub const fn meter(rect: Rect, value: f32) -> Self {
        Self::base(rect, PrimitiveKind::Meter, ThemeRole::SurfaceRecessed)
            .with_accent(ThemeRole::MeterLow)
            .with_value(value)
            .with_radius(3.0)
    }

    /// Constructs a rotary control.
    pub const fn knob(rect: Rect, value: f32) -> Self {
        Self::base(rect, PrimitiveKind::Knob, ThemeRole::SurfaceRaised)
            .with_accent(ThemeRole::Primary)
            .with_value(value)
            .with_gloss(0.5)
    }

    /// Constructs a status lamp.
    pub const fn lamp(rect: Rect, active: f32, accent: ThemeRole) -> Self {
        Self::base(rect, PrimitiveKind::Lamp, ThemeRole::SurfaceRecessed)
            .with_accent(accent)
            .with_value(active)
            .with_emission(0.5)
    }

    /// Constructs a round-capped line from two logical-pixel points.
    ///
    /// The line remains one compact GPU instance. Its axis-aligned quad is
    /// intentionally conservative so rotating it never clips an endpoint.
    pub fn line(start: [f32; 2], end: [f32; 2], width: f32, fill: ThemeRole) -> Self {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let length = dx.hypot(dy);
        let extent = length + width;
        let center = [start[0].midpoint(end[0]), start[1].midpoint(end[1])];
        let mut primitive = Self::base(
            Rect::new(
                center[0] - extent * 0.5,
                center[1] - extent * 0.5,
                extent,
                extent,
            ),
            PrimitiveKind::Line,
            fill,
        );
        primitive.radius = width * 0.5;
        primitive.value = length * 0.5;
        primitive.rotation = dy.atan2(dx);
        primitive
    }

    /// Constructs an antialiased arc inside `rect`.
    ///
    /// Angles use radians in screen coordinates: zero points right and positive
    /// values rotate clockwise. A sweep at or above one turn draws a full ring.
    pub const fn arc(
        rect: Rect,
        start_radians: f32,
        sweep_radians: f32,
        width: f32,
        fill: ThemeRole,
    ) -> Self {
        let mut primitive = Self::base(rect, PrimitiveKind::Arc, fill);
        primitive.radius = width;
        primitive.value = start_radians;
        primitive.rotation = sweep_radians;
        primitive
    }

    const fn base(rect: Rect, kind: PrimitiveKind, fill: ThemeRole) -> Self {
        Self {
            rect,
            kind,
            fill,
            outline: ThemeRole::Outline,
            accent: ThemeRole::Primary,
            radius: 0.0,
            outline_width: 0.0,
            value: 0.0,
            rotation: 0.0,
            gloss: 0.0,
            grain: 0.0,
            emission: 0.0,
        }
    }

    /// Sets the corner radius.
    pub const fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Sets the outline role and width.
    pub const fn with_outline(mut self, role: ThemeRole, width: f32) -> Self {
        self.outline = role;
        self.outline_width = width;
        self
    }

    /// Sets the accent role.
    pub const fn with_accent(mut self, role: ThemeRole) -> Self {
        self.accent = role;
        self
    }

    /// Sets and clamps the normalized value while building the GPU instance.
    pub const fn with_value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// Sets the indicator rotation.
    pub const fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// Sets material highlight strength.
    pub const fn with_gloss(mut self, gloss: f32) -> Self {
        self.gloss = gloss;
        self
    }

    /// Sets material grain strength.
    pub const fn with_grain(mut self, grain: f32) -> Self {
        self.grain = grain;
        self
    }

    /// Sets contained emission strength.
    pub const fn with_emission(mut self, emission: f32) -> Self {
        self.emission = emission;
        self
    }

    pub(crate) fn valid(self) -> bool {
        self.rect.is_valid()
            && [
                self.radius,
                self.outline_width,
                self.value,
                self.rotation,
                self.gloss,
                self.grain,
                self.emission,
            ]
            .into_iter()
            .all(f32::is_finite)
            && self.radius >= 0.0
            && self.outline_width >= 0.0
            && self.gloss >= 0.0
            && self.grain >= 0.0
            && self.emission >= 0.0
            && (!matches!(self.kind, PrimitiveKind::Line) || self.radius > 0.0)
            && (!matches!(self.kind, PrimitiveKind::Arc) || self.radius > 0.0)
    }

    #[cfg(feature = "wgpu")]
    pub(crate) fn gpu(self) -> GpuPrimitive {
        let value = match self.kind {
            PrimitiveKind::Line | PrimitiveKind::Arc => self.value,
            _ => self.value.clamp(0.0, 1.0),
        };
        GpuPrimitive {
            rect: [self.rect.x, self.rect.y, self.rect.width, self.rect.height],
            shape: [self.radius, self.outline_width, value, self.rotation],
            style: [
                self.fill as u32,
                self.outline as u32,
                self.accent as u32,
                self.kind as u32,
            ],
            material: [self.gloss, self.grain, self.emission, 0.0],
        }
    }
}

/// A reusable collection of primitives.
///
/// Call [`Scene::clear`] and repopulate it each frame to retain the allocation.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    primitives: Vec<Primitive>,
}

impl Scene {
    /// Creates an empty scene.
    pub const fn new() -> Self {
        Self {
            primitives: Vec::new(),
        }
    }

    /// Creates an empty scene with reserved capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            primitives: Vec::with_capacity(capacity),
        }
    }

    /// Appends one primitive in painter's order.
    pub fn push(&mut self, primitive: Primitive) {
        self.primitives.push(primitive);
    }

    /// Removes every primitive without releasing capacity.
    pub fn clear(&mut self) {
        self.primitives.clear();
    }

    /// Returns the number of primitives.
    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    /// Returns true when the scene contains no primitives.
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// Returns scene capacity for performance diagnostics.
    pub fn capacity(&self) -> usize {
        self.primitives.capacity()
    }

    /// Borrows primitives in painter order without exposing mutable storage.
    /// Useful for diagnostics, filtering, and composing a subset into a scene.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Primitive> {
        self.primitives.iter()
    }

    /// Validates every primitive before a scene crosses a process or renderer
    /// boundary. GPU hosts also perform this check during submission.
    pub fn validate(&self) -> bool {
        self.primitives.iter().copied().all(Primitive::valid)
    }

    #[cfg(feature = "wgpu")]
    pub(crate) fn primitives(&self) -> &[Primitive] {
        &self.primitives
    }
}

#[cfg(feature = "wgpu")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuPrimitive {
    pub rect: [f32; 4],
    pub shape: [f32; 4],
    pub style: [u32; 4],
    pub material: [f32; 4],
}

#[cfg(feature = "wgpu")]
impl GpuPrimitive {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Uint32x4, 3 => Float32x4];

    pub const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_clear_retains_storage() {
        let mut scene = Scene::with_capacity(32);
        scene.push(Primitive::knob(Rect::new(0.0, 0.0, 40.0, 40.0), 0.75));
        let capacity = scene.capacity();
        scene.clear();
        assert!(scene.is_empty());
        assert_eq!(scene.capacity(), capacity);
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn gpu_instances_are_fixed_and_values_are_bounded() {
        assert_eq!(std::mem::size_of::<GpuPrimitive>(), 64);
        let instance = Primitive::meter(Rect::new(0.0, 0.0, 8.0, 100.0), 2.0).gpu();
        // GPU saturation is exact, not a tolerance-based approximation.
        assert_eq!(instance.shape[2].to_bits(), 1.0_f32.to_bits());
    }
}
