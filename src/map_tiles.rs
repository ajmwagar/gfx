//! Retained GPU raster tiles for a caller-owned linear regional map target.
use crate::{geo::Extent, geo_map::raster::Asset, Rect};
use wgpu::util::DeviceExt;
/// One validated and decoded provider tile, uploaded once by its owner.
pub struct Tile {
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}
/// Independent of fetches, cache paths, IPC and native target ownership.
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}
impl Renderer {
    /// Create a retained raster pipeline for the caller's GPU target format.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map tile"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map tile shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map_tiles.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map tiles"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }
    /// Upload one bounded RGBA tile. Digest verification belongs to its owner.
    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        asset: &Asset,
        rgba: &[u8],
    ) -> Result<Tile, &'static str> {
        if asset.width != 256 || asset.height != 256 || rgba.len() != 256 * 256 * 4 {
            return Err("tile extent/stride mismatch");
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map tile pixels"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: Some(256),
            },
            texture.size(),
        );
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map tile view"),
            contents: &[0; 48],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Ok(Tile { bind, uniform })
    }
    /// Update only view uniforms; retained tile textures are never re-uploaded.
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        tile: &Tile,
        asset: &Asset,
        extent: Extent,
        plot: Rect,
        size: [u32; 2],
    ) -> Result<(), &'static str> {
        extent.validate().map_err(|_| "invalid tile view")?;
        if size.contains(&0)
            || !plot.is_valid()
            || extent.west_deg >= extent.east_deg
            || asset.zoom > 19
        {
            return Err("invalid tile plot");
        }
        let values = [
            extent.west_deg as f32,
            extent.east_deg as f32,
            extent.south_deg as f32,
            extent.north_deg as f32,
            plot.x / size[0] as f32,
            plot.y / size[1] as f32,
            plot.width / size[0] as f32,
            plot.height / size[1] as f32,
            asset.x as f32,
            asset.y as f32,
            (1u32 << asset.zoom) as f32,
            0.,
        ];
        queue.write_buffer(&tile.uniform, 0, bytemuck::cast_slice(&values));
        Ok(())
    }
    /// Draw retained tiles into the host's existing pass and scissor.
    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        tiles: impl IntoIterator<Item = &'a Tile>,
    ) {
        pass.set_pipeline(&self.pipeline);
        for tile in tiles {
            pass.set_bind_group(0, &tile.bind, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("map_tiles.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
