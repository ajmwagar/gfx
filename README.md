# gfx

[![CI](https://github.com/ajmwagar/gfx/actions/workflows/ci.yml/badge.svg)](https://github.com/ajmwagar/gfx/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`fpl-gfx` is a fast, high-quality, themeable graphics layer for
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
- Toolkit-independent orbit cameras and projected, pickable orientation cubes.
- Retained aspect-fit presentation for decoded or externally imported frames.
- WGS84/ECEF geometry and a retained raster-plus-points globe renderer.
- Validated IOSurface/DMA-BUF descriptors with explicit lease lifetimes.
- Rendering into a texture view supplied by a compositor or application.
- Observable per-frame work statistics.
- Renderer-neutral assistant reactor geometry and deterministic animation via
  `reactor::visit`, available without the `wgpu` feature.
- Rendering-only [DAW signal views](docs/signal-views.md): piano roll/live notes,
  peak waveforms, scope/automation/LFO traces and log-frequency spectra.

It deliberately does **not** own windows, input, layout policy, audio/MIDI,
business state, or application lifecycle.

## Coherence design language

Coherence owns the optional native adapter in
[`coherence-native`](https://github.com/FuturePresentLabs/coherence/tree/master/crates/coherence-native).
Use its `gfx` feature to map canonical Coherence colors and shapes into this
crate's existing `Theme`; egui clients can enable the companion `egui` feature
and share the same resolved token snapshot. Neither gfx nor independent apps
need a Canvas dependency.

```rust,ignore
let tokens = coherence_native::Tokens::new(
    coherence_native::Mode::Dark,
    coherence_native::Density::Compact,
);
coherence_native::gfx::apply(&tokens, &mut host_theme)?;
```

Pin a reviewed Coherence Git revision. Apply on appearance changes, not per
frame. The adapter performs the sRGB-to-linear conversion and rejects invalid
geometry before mutation. Coherence's flat profile replaces materials/colors;
host-owned motion and outline width remain unchanged. Hosts read the same token
metrics for typography and spacing. `Theme::studio_dark()` remains a standalone
gfx fallback, not a second implementation of Coherence.

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

### Assistant reactor

`reactor::visit(Activity::Thinking, seconds, |mark| { /* draw mark */ })`
emits at most 38 vector marks with no allocations. Uniformly fit the canonical
`reactor::SIZE` into your bounds, centered on `reactor::CENTER`. Adapters draw
discs, rings, clockwise arcs and bars; hosts resolve surface/accent/ink colors.
The native JARVIS overlay consumes this same geometry. Canvas and Holodeck can
consume it without depending on JARVIS or Iced.

The caller supplies a clock and visual activity. Hidden emits no marks. Thinking
uses counter-rotating rings; speaking uses a faster pulse, not a fabricated audio
level. Real audio levels, presence, control targets and lifecycle belong to the
host. No timers, windows, network subscriptions or assistant routing live here.
Non-finite clocks render the static zero frame. Hosts may freeze the clock for
reduced motion and should stop scheduling frames while hidden.

`reactor::visit_with_level` accepts an optional measured playback envelope;
`Some(0.0)` means real silence, while `None` preserves activity-only animation.
It does not measure audio, predict phonemes, or subscribe to a transport.
`reactor::band_mesh` extrudes a ring/arc into a bounded, closed triangle mesh
for 3D hosts. Build and retain these meshes once; animate their transforms and
colors, not their topology. Holodeck's bounded Rust/Lua bridge consumes both.

Views select [`ThemeRole`](https://docs.rs/fpl-gfx/latest/fpl_gfx/enum.ThemeRole.html)
values such as `SurfaceRaised`, `Primary`, `Warning`, and `MeterHigh`. The host
injects the actual colors and material behavior through a validated `Theme`.

This keeps a mixer, plugin host, and system dashboard visually consistent while
allowing each application to retain its own state and domain components.

Theme colors are linear RGBA. `Color::from_srgb8` converts authored sRGB colors
correctly for an sRGB render target.

Zero-gloss, zero-grain flat fills preserve their authored color exactly; there
is no implicit material darkening. Explicit primitive or theme gloss retains
the established directional lighting response. The GPU regression can be run
on a render host with `cargo test --test flat_material -- --ignored`.

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
  Its Transmog-facing model view now delegates orbit, orientation-cube, and
  retained-frame work here; its Panopticon-facing globe is a thin domain and
  palette adapter over the shared globe renderer.

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
