//! Fast, high-quality, themeable rendering for caller-owned wGPU targets.
//!
//! `fpl-gfx` owns neither windows nor application state. A host supplies its
//! [`wgpu::Device`], [`wgpu::Queue`], command encoder, and target texture. The
//! application can build a reusable [`Scene`], present retained frames, or
//! encode a camera-relative [`GlobeRenderer`] directly into its render graph.
//!
//! The initial primitive set is intentionally small. It is designed for dense,
//! animated control surfaces without becoming an application framework.

mod clock;
mod color;
pub mod signals;
pub mod topology;
pub mod patchbay;
pub mod console;
#[cfg(feature = "wgpu")]
mod frame;
mod geometry;
#[cfg(feature = "wgpu")]
mod globe;
mod orbit;
mod orientation;
mod path;
#[cfg(feature = "wgpu")]
mod renderer;
mod scene;
mod spatial;
mod surface;
mod theme;

pub use clock::AnalogClock;
pub use color::Color;
#[cfg(feature = "wgpu")]
pub use frame::{
    FramePresentation, FramePresentationError, FramePresenter, FramePresenterConfig, FrameTexture,
};
pub use geometry::{Point, Rect, Size};
#[cfg(feature = "wgpu")]
pub use globe::{
    GlobeFrame, GlobePoint, GlobeRenderError, GlobeRenderStats, GlobeRenderer, GlobeRendererConfig,
    GlobeStyle,
};
pub use orbit::{
    named_orientation, orientation_at, OrbitAction, OrbitController, OrbitLimits, OrbitPose,
    DEFAULT_ORBIT_LIMITS, MAX_ORBIT_DISTANCE, MAX_ORBIT_PITCH_DEGREES, MIN_ORBIT_DISTANCE,
    NAMED_ORIENTATIONS,
};
pub use orientation::{
    OrientationCube, OrientationRegion, OrientationRegionKind, ProjectedOrientationRegion,
};
pub use path::{PathCommand, PathError, VectorPath};
#[cfg(feature = "wgpu")]
pub use renderer::{
    RenderContext, RenderError, RenderOptions, Renderer, RendererConfig, RendererStats, Viewport,
};
pub use scene::{Primitive, PrimitiveKind, Scene};
pub use spatial::{
    geodetic_to_ecef, relative_to_origin_f32, wgs84_ellipsoid_mesh, EllipsoidMesh, EllipsoidVertex,
    SpatialError, WGS84_SEMI_MAJOR_M, WGS84_SEMI_MINOR_M,
};
pub use surface::{
    NativeSurfaceHandle, PixelFormat, SurfaceDescriptor, SurfaceError, SurfaceLease,
    SURFACE_DESCRIPTOR_VERSION,
};
pub use theme::{MaterialTheme, MotionTheme, ShapeTheme, Theme, ThemeError, ThemeRole};
