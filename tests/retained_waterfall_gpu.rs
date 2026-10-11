#![cfg(feature = "wgpu")]
//! Explicit native pixel qualification using the production waterfall pipeline.
use fpl_gfx::retained_waterfall::{Pipeline, Uniforms};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "Requires a native GPU adapter"]
fn ring_history_palette_and_scaling_pixels() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let values: [f32; 16] = [
            0., 0.1, 0.5, 1., 1., 0.5, 0.1, 0., 0.5, 0., 1., 0.1, 0.1, 1., 0., 0.5,
        ];
        queue.write_texture(
            source.as_image_copy(),
            bytemuck::cast_slice(&values),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(4),
            },
            source.size(),
        );
        let source_view = source.create_view(&Default::default());
        let pipeline = Pipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        // Independent palette interpolation, with observed versus absent history.
        let stops = [
            [0.002, 0.006, 0.035],
            [0., 0.02, 0.24],
            [0., 0.72, 0.96],
            [0.98, 0.92, 0.05],
            [0.96, 0.08, 0.01],
            [1., 1., 1.],
        ];
        let palette = |value: f32| -> [u8; 4] {
            let scaled = f64::from(value).clamp(0., 1.).powf(0.72) * 5.;
            let index = (scaled.floor() as usize).min(4);
            let fraction = scaled - index as f64;
            let mut pixel = [255; 4];
            for c in 0..3 {
                pixel[c] = ((stops[index][c] * (1. - fraction) + stops[index + 1][c] * fraction)
                    * 255.)
                    .round() as u8;
            }
            pixel
        };
        for (width, height) in [(64u32, 32u32), (128, 64)] {
            for head in [0u32, 1, 3] {
                for filled in [0u32, 2, 4] {
                    for top in [0f32, 0.39] {
                        let target = device.create_texture(&wgpu::TextureDescriptor {
                            label: None,
                            size: wgpu::Extent3d {
                                width,
                                height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_SRC,
                            view_formats: &[],
                        });
                        let uniform =
                            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: None,
                                contents: &Uniforms {
                                    viewport: [width as f32, height as f32],
                                    texture_size: [4, 4],
                                    head,
                                    filled,
                                    waterfall_top: top,
                                    padding: 0.,
                                }
                                .to_bytes(),
                                usage: wgpu::BufferUsages::UNIFORM,
                            });
                        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &pipeline.bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(&source_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: uniform.as_entire_binding(),
                                },
                            ],
                        });
                        let readback = device.create_buffer(&wgpu::BufferDescriptor {
                            label: None,
                            size: u64::from(width * height * 4),
                            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                            mapped_at_creation: false,
                        });
                        let view = target.create_view(&Default::default());
                        let mut encoder = device.create_command_encoder(&Default::default());
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: None,
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&pipeline.render_pipeline);
                            pass.set_bind_group(0, &bind, &[]);
                            pass.draw(0..3, 0..1);
                        }
                        encoder.copy_texture_to_buffer(
                            target.as_image_copy(),
                            wgpu::TexelCopyBufferInfo {
                                buffer: &readback,
                                layout: wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(width * 4),
                                    rows_per_image: Some(height),
                                },
                            },
                            target.size(),
                        );
                        queue.submit(Some(encoder.finish()));
                        let (tx, rx) = std::sync::mpsc::channel();
                        readback
                            .slice(..)
                            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                        rx.recv().unwrap().unwrap();
                        let pixels = readback.slice(..).get_mapped_range();
                        for y in 0..height {
                            for x in 0..width {
                                let uv_y = (f64::from(y) + 0.5) / f64::from(height);
                                let expected = if uv_y < f64::from(top) {
                                    [0; 4]
                                } else {
                                    let logical =
                                        (((uv_y - f64::from(top)) / (1. - f64::from(top))) * 4.)
                                            .floor() as u32;
                                    if logical >= filled {
                                        [1, 2, 6, 255]
                                    } else {
                                        let column = (((f64::from(x) + 0.5) / f64::from(width))
                                            * 4.)
                                            .floor()
                                            as u32;
                                        palette(
                                            values[(((head + logical) % 4) * 4 + column) as usize],
                                        )
                                    }
                                };
                                let offset = ((y * width + x) * 4) as usize;
                                for c in 0..4 {
                                    assert!((i16::from(pixels[offset+c])-i16::from(expected[c])).abs()<=1,
                                        "pixel ({x},{y}) head={head} filled={filled} top={top}: {:?} != {expected:?}", &pixels[offset..offset+4]);
                                }
                            }
                        }
                        drop(pixels);
                        readback.unmap();
                    }
                }
            }
        }
    });
}
