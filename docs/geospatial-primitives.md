# Shared geospatial rendering

## What is implemented

`gfx::geo` is the coordinate/geometry boundary, not a GIS data service:

- Checked WGS84 positions and ECEF metres, using the existing `gfx::spatial`
  implementation rather than another Earth model.
- Reusable `LocalFrame`: subtract ECEF positions in f64, then rotate into
  **East, North, Up** metres. Hosts adapt axes explicitly; ENU must not quietly
  mean Bevy's `[east, up, north]` or LÖVR's `[east, up, south]`.
- `Extent`: linear latitude/longitude layout for bounded charts and existing
  baked terrain tables, including antimeridian-crossing extents. This is **not**
  a Mercator projection, a geodesic distance, or a globe camera.
- `MarkerView`: bounded, theme-role marker batches into the existing GPU
  `Scene` renderer, plus nearest-marker picking using precisely the same
  projection and edge visibility. The returned index maps back to the owner's
  stable feature identity. Nothing acquires feeds or invents observation age.

Geographic positions use f64 until the deliberate GPU-local conversion. Height
is explicitly ellipsoidal, not mean sea level. Unknown height is not zero.
The two-dimensional extent projection needs no height at all. Invalid inputs
fail before mutating a scene. Marker batches are bounded to 4096 observations;
larger scenes must be culled/reduced by their owner before this view boundary.
Reuse retained scenes/buffers and rebuild only on data, projection or theme
changes—not every camera repaint. CPU coordinate/geometry preparation feeds
GPU painting; this module does not claim GPU coordinate computation.

ENU is tested against the published [PROJ topocentric example](https://proj.org/en/stable/operations/conversions/topocentric.html),
including its axis order, and separately at the date line, poles and centimetre
height increments. It is a local tangent frame, not a globally flat map.

## Existing owners reviewed

`gfx::geo_map::Map`, exposed as `SignalFrame::GeoMap`, is a bounded regional
Canvas view with theme-role markers, clipped boundary/trail lines, coordinate
grid labels and a collision-limited caption budget. It accepts at most 512
markers, 128 paths and 8192 path positions; validates whole revisions before
painting; and handles longitude wrapping without drawing a date-line segment
across the world. The host retains/caches its existing GPU scene. This is not
the continuous globe, a tile service or an interactive GIS catalog. Source
timestamps, expiry and omission counts stay with the provider/composition.

Panopticon's `panopticon-render-contract` already owns stable feature IDs,
sources, observation timestamps, confidence, affiliation and semantic feature
classes. Keep that contract there; GFX must not grow a second competing feature
catalog or know what ADS-B/Meshtastic/TAK transport delivers.

Galileo's `primitives.rs` already compiles symbols, infrastructure, surfaces,
tracks and coverage into a fixed set of batches. `batch_mesh.rs` adapts those
batches to Bevy. Its current local coordinate helper approximates longitude/
latitude differences, while polygon conversion retains only the first ring
and surface filling uses a fan. These are migration targets, not capabilities
to copy blindly: holes and concavity must survive or fail visibly.

Holodeck's `app/render/terrain-table.lua` already combines bounded contacts,
observed trails, selection and geographic tables. Terrain shaders and contact
outlines already come from GFX. Its linear table mapping must remain aligned
with the baked terrain assets; replacing it with ENU without rebaking would
misalign contacts. Preserve Lua composition, room anchors, consent, freshness
and selection. Expose shared geometry through the existing bounded native GFX
API, not a second headset service or wGPU device.

GFX's existing `GlobeRenderer` owns retained ellipsoid/raster/point GPU resources;
the host supplies its camera, target and theme. Reuse it for globe views rather
than introducing a second Earth renderer. `MarkerView` is a local 2D overlay,
not a replacement for that globe renderer.

## Next migration slices

1. Adapt Galileo and Quest to the shared coordinate boundary, with named axis
   conversions and baked-table alignment tests. This document does not imply
   either running application has been upgraded yet.
2. Lift renderer-neutral **geometry** batches: heading-oriented symbols,
   clipped trails/roads, range rings/sectors and observed uncertainty. Keep
   extrapolated tracks explicitly distinct from observed paths. Preserve full
   source IDs for picking; do not rely solely on collision-prone short hashes.
3. Add proper polygon triangulation with holes, shared bounds/culling and
   tests for antimeridian splits. No silent first-ring fallback or convex fan
   for arbitrary geographic polygons.
4. Add retained GPU instancing/line ribbons, revision-aware partial uploads,
   projected label budgets and shared picking. Bevy, wGPU and LÖVR remain thin
   resource/input adapters. Measure upload bytes, draw calls, CPU preparation
   and GPU frame time separately; accelerated does not mean cost-free.
5. Connect Canvas map widgets and Quest tables to the same provider projection
   over existing Unibus contracts. Panopticon owns querying and attribution;
   Mycelium owns discovery; Unibus owns access/transport; consumers own views.

Tiles, raster caching/attribution, terrain baking, source freshness, camera
navigation and access grants remain explicit host/provider responsibilities.
This first slice has no new server, transport, map tile downloader, polygon
renderer, live app deployment or claim of matching desktop/Quest pixels.
