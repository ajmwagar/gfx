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
mod geometry;
mod path;
#[cfg(feature = "wgpu")]
mod renderer;
mod scene;
mod surface;
mod theme;

pub use clock::AnalogClock;
pub use color::Color;
pub use geometry::{Point, Rect, Size};
pub use path::{PathCommand, PathError, VectorPath};
#[cfg(feature = "wgpu")]
pub use renderer::{
    RenderContext, RenderError, RenderOptions, Renderer, RendererConfig, RendererStats, Viewport,
};
pub use scene::{Primitive, PrimitiveKind, Scene};
pub use surface::{
    NativeSurfaceHandle, PixelFormat, SurfaceDescriptor, SurfaceError, SurfaceLease,
    SURFACE_DESCRIPTOR_VERSION,
};
pub use theme::{MaterialTheme, MotionTheme, ShapeTheme, Theme, ThemeError, ThemeRole};
