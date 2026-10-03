//! Portable metadata and lifetime contract for externally produced GPU frames.
//!
//! A descriptor identifies importable image metadata; the corresponding
//! [`SurfaceLease`] owns the platform resource for at least as long as a
//! consumer imports or displays it. Actual capture, encoding, and wGPU import
//! remain host responsibilities.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current external-surface descriptor version.
pub const SURFACE_DESCRIPTOR_VERSION: u16 = 1;

/// Packed pixel formats accepted by the external-surface contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    /// Eight-bit blue, green, red, and alpha channels.
    Bgra8Unorm,
    /// Eight-bit blue, green, and red channels plus an ignored byte.
    Bgrx8Unorm,
}

impl PixelFormat {
    /// Returns packed bytes per pixel.
    pub const fn bytes_per_pixel(self) -> u32 {
        4
    }
}

/// Platform handle metadata for a leased image.
///
/// This descriptor is not ownership: the [`SurfaceLease`] retains the actual
/// `IOSurface` or DMA-BUF file descriptor and controls its lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeSurfaceHandle {
    /// macOS `IOSurface` registry identifier.
    IoSurface {
        /// Process-visible `IOSurface` identifier.
        id: u32,
    },
    /// Linux DMA-BUF image-plane metadata.
    DmaBuf {
        /// DRM fourcc describing channel layout.
        drm_fourcc: u32,
        /// Tiling/compression modifier, or `None` when the producer cannot
        /// report one. Importers must not interpret `None` as linear.
        modifier: Option<u64>,
        /// Byte offset of the packed image plane.
        offset: u32,
    },
}

/// Immutable metadata for one externally produced image frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceDescriptor {
    /// Descriptor protocol version.
    pub protocol: u16,
    /// Producer-monotonic frame sequence.
    pub sequence: u64,
    /// Capture timestamp in Unix nanoseconds.
    pub captured_at_unix_ns: u64,
    /// Image width in physical pixels.
    pub width: u32,
    /// Image height in physical pixels.
    pub height: u32,
    /// Bytes between adjacent image rows.
    pub stride: u32,
    /// Packed pixel representation.
    pub pixel_format: PixelFormat,
    /// Platform import metadata.
    pub handle: NativeSurfaceHandle,
}

impl SurfaceDescriptor {
    /// Validates metadata before an importer uses platform handles.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] for an unsupported protocol, empty image,
    /// arithmetic overflow, or a stride smaller than one packed row.
    pub fn validate(&self) -> Result<(), SurfaceError> {
        if self.protocol != SURFACE_DESCRIPTOR_VERSION {
            return Err(SurfaceError::Protocol(self.protocol));
        }
        if self.width == 0 || self.height == 0 {
            return Err(SurfaceError::EmptyImage);
        }
        let packed_stride = self
            .width
            .checked_mul(self.pixel_format.bytes_per_pixel())
            .ok_or(SurfaceError::StrideOverflow)?;
        if self.stride < packed_stride {
            return Err(SurfaceError::ShortStride {
                stride: self.stride,
                packed_stride,
            });
        }
        Ok(())
    }
}

/// A retained external frame whose resource outlives every descriptor borrow.
pub trait SurfaceLease {
    /// Returns immutable import metadata for this frame.
    fn descriptor(&self) -> &SurfaceDescriptor;
}

/// Invalid external-surface metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SurfaceError {
    /// The descriptor uses an unsupported protocol version.
    #[error("unsupported external-surface protocol {0}")]
    Protocol(u16),
    /// One or both image dimensions are zero.
    #[error("external-surface dimensions must be non-zero")]
    EmptyImage,
    /// Packed row-size calculation overflowed.
    #[error("external-surface packed stride overflowed")]
    StrideOverflow,
    /// The declared stride cannot contain one packed row.
    #[error("stride {stride} is smaller than packed row size {packed_stride}")]
    ShortStride {
        /// Declared row stride.
        stride: u32,
        /// Minimum packed row stride.
        packed_stride: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor {
            protocol: SURFACE_DESCRIPTOR_VERSION,
            sequence: 7,
            captured_at_unix_ns: 11,
            width: 960,
            height: 640,
            stride: 3_840,
            pixel_format: PixelFormat::Bgra8Unorm,
            handle: NativeSurfaceHandle::IoSurface { id: 99 },
        }
    }

    #[test]
    fn descriptors_validate_and_round_trip() {
        let descriptor = descriptor();
        assert_eq!(descriptor.validate(), Ok(()));
        let json = serde_json::to_string(&descriptor).unwrap();
        assert_eq!(
            serde_json::from_str::<SurfaceDescriptor>(&json).unwrap(),
            descriptor
        );
    }

    #[test]
    fn torn_or_misdescribed_rows_are_rejected() {
        let mut descriptor = descriptor();
        descriptor.stride = descriptor.width * 4 - 1;
        assert!(matches!(
            descriptor.validate(),
            Err(SurfaceError::ShortStride { .. })
        ));
    }
}
