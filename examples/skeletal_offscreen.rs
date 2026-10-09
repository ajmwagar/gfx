//! Agent-friendly, fixed-camera native pose capture. No window or application UI.
use fpl_gfx::{mesh, skeletal::Asset, RenderContext};
use glam::{Mat4, Vec3};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, io::Write, path::Path};
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() -> Result<(), Box<dyn Error>> {
    pollster::block_on(run())
}
#[allow(
    clippy::too_many_lines,
    reason = "Linear offscreen capture runbook keeps GPU resource lifetimes and receipt emission together."
)]
async fn run() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 5 || args.len() > 68 {
        return Err(
            "usage: skeletal_offscreen INPUT.glb NEW_DIR CLIP SECONDS... (up to 64 frames)".into(),
        );
    }
    let input = Path::new(&args[1]);
    if fs::metadata(input)?.len() > 128 * 1024 * 1024 {
        return Err("GLB exceeds 128 MiB".into());
    }
    let bytes = fs::read(input)?;
    let asset = Asset::from_glb(&bytes)?;
    let clip = args[3].parse::<usize>()?;
    let frames = args[4..]
        .iter()
        .map(|t| Ok(asset.sample(Some(clip), t.parse::<f32>()?)?))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for frame in &frames {
        for mesh in &frame.meshes {
            for v in &mesh.vertices {
                let p = Vec3::from_array(v.position);
                low = low.min(p);
                high = high.max(p);
            }
        }
    }
    let center = (low + high) * 0.5;
    let extent = (high - low).max(Vec3::splat(0.001));
    let distance = extent.max_element().max(0.1);
    let eye = Vec3::new(center.x, high.y + distance, center.z);
    let view = Mat4::look_at_rh(eye, center, Vec3::NEG_Z);
    let projection = Mat4::orthographic_rh(
        -extent.x * 0.6,
        extent.x * 0.6,
        -extent.z * 0.6,
        extent.z * 0.6,
        0.001,
        distance + extent.y + 1.,
    );
    let mvp = projection * view;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await?;
    let info = adapter.get_info();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("gfx skeletal capture"),
            ..Default::default()
        })
        .await?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let renderer = mesh::Renderer::new(&device, format);
    let size = wgpu::Extent3d {
        width: 256,
        height: 256,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("skeletal color"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("skeletal depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("skeletal readback"),
        size: 256 * 256 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut images = Vec::new();
    let mut records = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let draws = frame
            .meshes
            .iter()
            .map(|m| mesh::Draw {
                vertices: &m.vertices,
                indices: &m.indices,
                uniforms: mesh::Uniforms {
                    mvp: mvp.to_cols_array_2d(),
                    model: Mat4::IDENTITY.to_cols_array_2d(),
                    color: [0.25, 0.85, 1., 1.],
                    params: [frame.seconds, 0., 1., 0.],
                    field_params: [0.; 4],
                    eye: [eye.x, eye.y, eye.z, 0.],
                },
            })
            .collect::<Vec<_>>();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.render(
            RenderContext {
                device: &device,
                queue: &queue,
                encoder: &mut encoder,
                target: &target,
            },
            &depth,
            &draws,
        )?;
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256 * 4),
                    rows_per_image: Some(256),
                },
            },
            size,
        );
        queue.submit(Some(encoder.finish()));
        let (send, receive) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receive.recv()??;
        let rgba = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 256, 256);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&rgba)?;
        }
        let file = format!("pose-{i:03}.png");
        records.push(json!({"seconds":frame.seconds,"clip":frame.clip,"png":file,"png_sha256":digest(&png),"rgba_sha256":digest(&rgba),"meshes":frame.meshes.iter().map(|m|json!({"node":m.node,"positions":m.vertices.iter().map(|v|v.position).collect::<Vec<_>>(),"normals":m.vertices.iter().map(|v|v.normal).collect::<Vec<_>>(),"indices":m.indices})).collect::<Vec<_>>()}));
        images.push((file, png));
    }
    let receipt = json!({"schema":"gfx.skeletal-capture.v1","source_sha256":digest(&bytes),"shader_sha256":digest(mesh::SHADER.as_bytes()),"sampling":"per-channel clamp; STEP and LINEAR TRS","skinning":"CPU world-space linear blend; inverse-transpose provided normals or deformed-face generated normals","material":"shared illustrative mesh shader, cyan; no source textures/PBR","adapter":{"name":info.name,"backend":format!("{:?}",info.backend),"vendor":info.vendor,"device":info.device,"driver":info.driver},"size":[256,256],"mvp":mvp.to_cols_array_2d(),"frames":records});
    let output = Path::new(&args[2]);
    fs::create_dir(output)?;
    for (file, data) in images {
        fs::File::create_new(output.join(file))?.write_all(&data)?;
    }
    fs::File::create_new(output.join("receipt.json"))?
        .write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "{}",
        serde_json::to_string(
            &json!({"output":output,"frames":frames.len(),"adapter":receipt["adapter"]})
        )?
    );
    Ok(())
}
