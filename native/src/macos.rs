//! Immutable-frame IOSurface target. Never reuse a published target for writes.
#![allow(unexpected_cfgs)]
use core_foundation::{
    base::TCFType, dictionary::CFDictionary, number::CFNumber, string::CFString,
};
use metal::{
    foreign_types::ForeignType, objc::runtime::Object, MTLPixelFormat, MTLStorageMode,
    MTLTextureType, MTLTextureUsage, TextureDescriptor,
};
use objc::{msg_send, sel, sel_impl};
use std::{ffi::c_void, ptr::NonNull};

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceCreate(properties: *const c_void) -> *mut c_void;
    fn IOSurfaceGetID(surface: *mut c_void) -> u32;
    fn IOSurfaceGetWidth(surface: *mut c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *mut c_void) -> usize;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}

/// Owns the native storage and its Metal/wgpu render target together.
/// Publish only after GPU completion. Once published, never write it again.
pub struct IoSurfaceTarget {
    texture: wgpu::Texture,
    surface: NonNull<c_void>,
    width: u32,
    height: u32,
}
impl IoSurfaceTarget {
    /// Allocate a non-global BGRA/sRGB surface and import it as a GPU target.
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Result<Self, String> {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 8_388_608
        {
            return Err("native target exceeds pixel budget".into());
        }
        let properties = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("IOSurfaceWidth"),
                CFNumber::from(i64::from(width)),
            ),
            (
                CFString::new("IOSurfaceHeight"),
                CFNumber::from(i64::from(height)),
            ),
            (
                CFString::new("IOSurfaceBytesPerElement"),
                CFNumber::from(4_i64),
            ),
            (
                CFString::new("IOSurfacePixelFormat"),
                CFNumber::from(i64::from(u32::from_be_bytes(*b"BGRA"))),
            ),
        ]);
        // SAFETY: the typed CF dictionary remains alive during the call; the
        // Create rule gives this wrapper one owned reference on success.
        let surface =
            NonNull::new(unsafe { IOSurfaceCreate(properties.as_concrete_TypeRef().cast()) })
                .ok_or("IOSurface allocation failed")?;
        let texture = import_target(device, surface.as_ptr(), width, height);
        match texture {
            Ok(texture) => Ok(Self {
                texture,
                surface,
                width,
                height,
            }),
            Err(error) => {
                unsafe { CFRelease(surface.as_ptr()) };
                Err(error)
            }
        }
    }
    /// Native handle is valid only while this owner is alive.
    pub fn as_ptr(&self) -> *mut c_void {
        self.surface.as_ptr()
    }
    /// Correlation ID, not an access capability (transfer its Mach right).
    pub fn id(&self) -> u32 {
        unsafe { IOSurfaceGetID(self.surface.as_ptr()) }
    }
    /// Actual native dimensions, verified against the texture before publish.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    /// Render into this view before publishing. No CPU pixel access is exposed.
    pub fn view(&self) -> wgpu::TextureView {
        self.texture.create_view(&Default::default())
    }
}
impl Drop for IoSurfaceTarget {
    fn drop(&mut self) {
        // The Metal texture owns a separate reference; consumers retain theirs.
        unsafe { CFRelease(self.surface.as_ptr()) };
    }
}
fn import_target(
    device: &wgpu::Device,
    surface: *mut c_void,
    width: u32,
    height: u32,
) -> Result<wgpu::Texture, String> {
    // SAFETY: freshly allocated packed BGRA surface owned by the caller.
    if unsafe { IOSurfaceGetWidth(surface) } != width as usize
        || unsafe { IOSurfaceGetHeight(surface) } != height as usize
    {
        return Err("native extent mismatch".into());
    }
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm_sRGB);
    descriptor.set_width(u64::from(width));
    descriptor.set_height(u64::from(height));
    descriptor.set_depth(1);
    descriptor.set_array_length(1);
    descriptor.set_mipmap_level_count(1);
    descriptor.set_sample_count(1);
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    // SAFETY: the backend guard keeps the device alive; the native target is
    // exclusively written by this producer before its publication.
    let hal =
        unsafe { device.as_hal::<wgpu_hal::api::Metal>() }.ok_or("native target requires Metal")?;
    let metal_device = hal.raw_device().lock();
    let raw: *mut Object = unsafe {
        msg_send![metal_device.as_ptr(), newTextureWithDescriptor: descriptor.as_ptr() iosurface: surface plane: 0_u64]
    };
    if raw.is_null() {
        return Err("Metal refused native target".into());
    }
    let texture = unsafe { metal::Texture::from_ptr(raw.cast()) };
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let texture = unsafe {
        wgpu_hal::metal::Device::texture_from_raw(
            texture,
            format,
            MTLTextureType::D2,
            1,
            1,
            wgpu_hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
        )
    };
    Ok(unsafe {
        device.create_texture_from_hal::<wgpu_hal::api::Metal>(
            texture,
            &wgpu::TextureDescriptor {
                label: Some("GFX immutable IOSurface target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gpu_target_retains_exact_extent_and_renders_without_readback() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        assert!(IoSurfaceTarget::new(&device, 0, 24).is_err());
        let target = IoSurfaceTarget::new(&device, 257, 129).unwrap();
        assert_eq!(target.dimensions(), (257, 129));
        assert_ne!(target.id(), 0);
        let view = target.view();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        let submission = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .unwrap();
    }
}
