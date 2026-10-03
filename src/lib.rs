//! Fast, high-quality, themeable 2D rendering for caller-owned wGPU targets.
//!
//! `fpl-gfx` owns neither windows nor application state. A host supplies its
//! [`wgpu::Device`], [`wgpu::Queue`], command encoder, and target texture. The
//! application builds a reusable [`Scene`], and [`Renderer`] submits every
//! primitive in one instanced draw call.
//!
//! The initial primitive set is intentionally small. It is designed for dense,
//! animated control surfaces without becoming an application framework.

mod clock;
mod color;
#[cfg(feature = "wgpu")]
mod renderer;
mod scene;
mod theme;

pub use clock::AnalogClock;
pub use color::Color;
#[cfg(feature = "wgpu")]
pub use renderer::{
    RenderContext, RenderError, RenderOptions, Renderer, RendererConfig, RendererStats, Viewport,
};
pub use scene::{Primitive, PrimitiveKind, Rect, Scene};
pub use theme::{MaterialTheme, MotionTheme, ShapeTheme, Theme, ThemeError, ThemeRole};
