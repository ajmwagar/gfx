# Embedded LÖVR rendering

The `lovr` module is a source catalog, available with `default-features = false`.
A caller looks up a bounded shader name, compiles the returned vertex/fragment
pair with LÖVR's built-in shader prelude, and retains the shader. GFX owns the
source; the caller owns GPU resources, stereo passes, transforms and data.

Catalog version 1 includes model finish inspection, geographic relief,
anisotropic Gaussian splats, reversed-Z aperture clearing, and cached stereo
portal projection. These are LÖVR GLSL dialect sources, not standalone Vulkan
GLSL or wGPU WGSL. Test compilation in LÖVR when changing them.

`symbols::outline` supplies bounded line outlines independently of rendering
backend. Contact outlines point forward along positive Y; UI hosts or map hosts
choose their local plane and scale. The host retains the returned geometry and
supplies semantic theme colors. No network acquisition or app state lives here.
