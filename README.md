# gfx

[![CI](https://github.com/ajmwagar/gfx/actions/workflows/ci.yml/badge.svg)](https://github.com/ajmwagar/gfx/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`fpl-gfx` is a fast, high-quality, themeable 2D graphics layer for
caller-owned [wGPU](https://wgpu.rs/) targets.

It is designed for dense, animated interfaces—audio tools, instrumentation,
control surfaces, dashboards, and compositors—without owning a window, event
loop, application model, or GPU device.

## Priorities

1. **Performance:** stable allocations, geometric buffer growth, compact
   64-byte instances, and one draw call for the complete primitive batch.
2. **Quality:** linear-light colors, analytic antialiasing, material highlights,
   controlled grain, and deterministic rendering.
3. **Theming:** versioned semantic roles and shared shape, material, and motion
   tokens rather than application-specific palette indexes.
4. **Portability:** caller-owned resources and conservative wGPU requirements
   suitable for Apple Silicon, desktop Linux, and Raspberry Pi-class Vulkan.

## What it owns

- Retained pipelines and geometrically growing GPU buffers.
- Reusable CPU scenes with stable allocations.
- Instanced rounded panels, discs, meters, knobs, and lamps.
- Semantic themes serialized independently from application state.
- Portable geometry and retained vector-path commands for layout producers.
- Validated IOSurface/DMA-BUF descriptors with explicit lease lifetimes.
- Rendering into a texture view supplied by a compositor or application.
- Observable per-frame work statistics.

It deliberately does **not** own windows, input, layout policy, audio/MIDI,
business state, or application lifecycle.

## Example

```rust,no_run
use fpl_gfx::{
    Color, Primitive, Rect, RenderOptions, Renderer, RendererConfig, Scene,
    Theme, ThemeRole, Viewport,
};

# fn render(
#     device: &wgpu::Device,
#     queue: &wgpu::Queue,
#     encoder: &mut wgpu::CommandEncoder,
#     target: &wgpu::TextureView,
# ) -> Result<(), fpl_gfx::RenderError> {
let mut renderer = Renderer::new(
    device,
    RendererConfig::new(wgpu::TextureFormat::Rgba8UnormSrgb),
);
let mut scene = Scene::with_capacity(64);

scene.push(
    Primitive::rounded_rect(
        Rect::new(24.0, 24.0, 592.0, 312.0),
        18.0,
        ThemeRole::Surface,
    )
    .with_gloss(0.2)
    .with_grain(0.025),
);
scene.push(Primitive::knob(
    Rect::new(84.0, 92.0, 96.0, 96.0),
    0.68,
));
scene.push(Primitive::meter(
    Rect::new(242.0, 72.0, 24.0, 216.0),
    0.73,
));

renderer.render(
    fpl_gfx::RenderContext {
        device,
        queue,
        encoder,
        target,
    },
    Viewport::new(640.0, 360.0),
    &Theme::studio_dark(),
    &scene,
    RenderOptions {
        clear: Some(Color::linear(0.0, 0.0, 0.0, 1.0)),
    },
)?;
# Ok(())
# }
```

See [`examples/offscreen.rs`](examples/offscreen.rs) for device creation and a
complete offscreen frame.

## Theming contract

Views select [`ThemeRole`](https://docs.rs/fpl-gfx/latest/fpl_gfx/enum.ThemeRole.html)
values such as `SurfaceRaised`, `Primary`, `Warning`, and `MeterHigh`. The host
injects the actual colors and material behavior through a validated `Theme`.

This keeps a mixer, plugin host, and system dashboard visually consistent while
allowing each application to retain its own state and domain components.

Theme colors are linear RGBA. `Color::from_srgb8` converts authored sRGB colors
correctly for an sRGB render target.

## Performance contract

- A scene retains CPU storage across `clear()` and rebuilds.
- Renderer staging storage and GPU instance buffers grow geometrically.
- Stable-size frames perform no renderer-owned heap allocation.
- Every primitive uses one 64-byte instance.
- A non-empty scene is submitted in one instanced draw call.
- The shader requires no compute, bindless resources, or large attachment set.
- `RendererStats` exposes primitive count, draw calls, bytes uploaded, and
  retained instance capacity.

Run the CPU scene benchmark with:

```sh
cargo bench --bench scene
```

## Relationship to host applications

`fpl-gfx` is the common rendering layer. Higher-level controls remain with the
consumer:

- PedalKernel builds plugin controls and captures native editor surfaces while
  sharing geometry, path, and frame contracts from this crate.
- Synesthesia can build mixers, tape machines, meters, and patch matrices.
- Canvas can own the wGPU device, target textures, composition, and theme.

The default `wgpu` feature supplies the retained GPU renderer. Disable default
features for lightweight geometry, theming, vector-path, and external-surface
contracts in processes that do not own a GPU device.

## Status

The API is intentionally small and pre-1.0. The initial contract establishes
resource ownership, batching, theme semantics, and portable GPU limits before
adding text atlases, vector-path tessellation, clipping, and richer materials.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
