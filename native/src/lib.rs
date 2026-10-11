//! Platform ownership and GPU targets, isolated from GFX's safe rendering code.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::IoSurfaceTarget;
