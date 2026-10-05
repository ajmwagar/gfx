//! Pixel-level regression for caller-authored flat semantic colors.
#![cfg(feature = "wgpu")]

use fpl_gfx::{
    Color, MaterialTheme, Primitive, Rect, RenderOptions, Renderer, RendererConfig, Scene, Theme,
    ThemeRole, Viewport,
};

#[test]
#[ignore = "requires a working Vulkan/Metal adapter; run explicitly on a render host"]
fn flat_fill_preserves_color_and_nonzero_gloss_keeps_material_response() {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("flat-fill regression"),
            size: wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = Renderer::new(
            &device,
            RendererConfig::new(wgpu::TextureFormat::Rgba8UnormSrgb),
        );
        let mut theme = Theme::studio_dark();
        theme.set_color(ThemeRole::Surface, Color::from_srgb8(255, 255, 255, 255));
        let mut scene = Scene::with_capacity(1);
        scene.push(Primitive::rounded_rect(
            Rect::new(0.0, 0.0, 32.0, 32.0),
            0.0,
            ThemeRole::Surface,
        ));
        for (flat, gloss) in [(true, 0.0), (false, 0.2)] {
            theme.materials = MaterialTheme {
                gloss,
                grain: 0.0,
                emission: 0.0,
            };
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("flat-fill readback"),
                size: 256 * 32,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            renderer
                .render(
                    fpl_gfx::RenderContext {
                        device: &device,
                        queue: &queue,
                        encoder: &mut encoder,
                        target: &view,
                    },
                    Viewport::new(32.0, 32.0),
                    &theme,
                    &scene,
                    RenderOptions {
                        clear: Some(Color::linear(0.0, 0.0, 0.0, 1.0)),
                    },
                )
                .unwrap();
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = send.send(result);
                });
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            receive.recv().unwrap().unwrap();
            let bytes = buffer.slice(..).get_mapped_range();
            let pixel = &bytes[16 * 256 + 16 * 4..16 * 256 + 16 * 4 + 4];
            assert_pixel(pixel, flat);
        }
    });
}

fn assert_pixel(pixel: &[u8], flat: bool) {
    if flat {
        assert_eq!(
            pixel,
            [255, 255, 255, 255],
            "flat authored white must remain white"
        );
    } else {
        // Legacy shader response at the center: 0.86 + .2 * ~.5 * .3.
        assert!(
            pixel[..3].iter().all(|v| v.abs_diff(242) <= 2),
            "legacy gloss changed: {pixel:?}"
        );
        assert_eq!(pixel[3], 255);
    }
}
