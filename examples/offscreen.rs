//! Render a small control surface into a caller-owned offscreen texture.

use fpl_gfx::{
    Color, Primitive, Rect, RenderOptions, Renderer, RendererConfig, Scene, Theme, ThemeRole,
    Viewport,
};

#[path = "support/signal_gallery.rs"]
mod signal_gallery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pollster::block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("fpl-gfx example"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fpl-gfx example target"),
        size: wgpu::Extent3d {
            width: 640,
            height: 360,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut scene = Scene::with_capacity(16);
    scene.push(
        Primitive::rounded_rect(
            Rect::new(24.0, 24.0, 592.0, 312.0),
            18.0,
            ThemeRole::Surface,
        )
        .with_gloss(0.2)
        .with_grain(0.025),
    );
    scene.push(Primitive::knob(Rect::new(84.0, 92.0, 96.0, 96.0), 0.68));
    scene.push(Primitive::meter(Rect::new(242.0, 72.0, 24.0, 216.0), 0.73));
    scene.push(Primitive::lamp(
        Rect::new(334.0, 104.0, 28.0, 28.0),
        1.0,
        ThemeRole::Success,
    ));

    let signals = std::env::args().any(|arg| arg == "--signals");
    let mut theme = Theme::default();
    if signals {
        theme.materials.grain = 0.0;
        theme.materials.gloss = 0.0;
        scene.clear();
        signal_gallery::append(&mut scene)?;
    }

    let mut renderer = Renderer::new(&device, RendererConfig::new(format));
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("fpl-gfx example encoder"),
    });
    let stats = renderer.render(
        fpl_gfx::RenderContext {
            device: &device,
            queue: &queue,
            encoder: &mut encoder,
            target: &target,
        },
        Viewport::new(640.0, 360.0),
        &theme,
        &scene,
        RenderOptions {
            clear: Some(Color::linear(0.0, 0.0, 0.0, 1.0)),
        },
    )?;
    let output = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| pair[1].clone());
    let readback = output.as_ref().map(|_| {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("example image readback"),
            size: 640 * 360 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(640 * 4),
                    rows_per_image: Some(360),
                },
            },
            wgpu::Extent3d {
                width: 640,
                height: 360,
                depth_or_array_layers: 1,
            },
        );
        buffer
    });
    queue.submit(Some(encoder.finish()));
    if let (Some(path), Some(buffer)) = (output, readback) {
        use std::io::Write;
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receive.recv()??;
        let bytes = buffer.slice(..).get_mapped_range();
        let mut file = std::io::BufWriter::new(std::fs::File::create_new(path)?);
        file.write_all(b"P6\n640 360\n255\n")?;
        for pixel in bytes.chunks_exact(4) {
            file.write_all(&pixel[..3])?;
        }
        file.flush()?;
    }
    println!(
        "rendered {} primitives in {} draw call",
        stats.primitives, stats.draw_calls
    );
    Ok(())
}
