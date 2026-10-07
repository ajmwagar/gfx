# Console strips and peak meter faces

Tall `columns` now use a fixed recording-console surface rather than card layout:
recessed meter glass above a separate long fader corridor, observed-routing lamp,
scribble strip, countersunk fasteners, unity marks and raised fader cap. Compact
rows and icon banks retain their own layouts. No audio/control schema changed.

`console::StripSurface` is the shared geometry for painting and host hit testing.
`ControlLocation` adds relative depth hints (not world-space units) for a future
extruded VR surface. Hosts bind their stable channel/control identities, choose
physical scale, and own all interaction. `gain_fraction`/`gain_from_fraction`
share one display/input taper. A missing gain has an empty rail, never a fake cap.
Peak meters remain peak dBFS, not calibrated RMS VU measurements.

`Console::append_with_appearance` accepts a separate host-owned
`ConsoleAppearance`: semantic face/cap/slot/identity/marking roles, bounded local
gloss/grain, hardware details and cast shadows. `ConsoleAppearance::flat()` removes
hardware ornament and shadows without changing the control positions or meter
semantics. Disable global renderer gloss/grain too for a fully flat wGPU finish.
The existing host `Theme` supplies all colors; there is no duplicated palette.
Appearance is validated before the scene changes. It is not part of a DAW's
measurement snapshot, so the renderer can change themes independently of audio.

The default procedural studio finish needs no asset downloads. Pedalkernel
filmstrips/backplates can be host-owned optional packs, but are not bundled in this
MIT/Apache library. Check each asset's provenance; imported models may carry
different licenses. Upload/decode once, not per meter update. A 128×16384 RGBA knob
strip costs 8 MiB decoded; its 256×32768 variant costs 32 MiB. Choose atlases within
the device's maximum texture dimension or tile/extract frames first. File size is
not GPU memory size, and a desktop filmstrip is not automatically TV-safe.

The first pass uses themeable procedural geometry, no baked image dependency or
3D runtime. It can be rendered into a cached texture with the existing offscreen
renderer; live needles, lamps and caps remain separate primitives. A true mesh
renderer and asset-baking pipeline are not implemented here.

Preview: `cargo run --release --example offscreen -- --console --output console.ppm`.
Add `--flat` to compare material finishes using the exact same layout and values.
This geometry-only preview uses explicit demo values, not studio telemetry. Hosts
draw the channel identity/value glyphs from `Console::labels` with their existing
text renderer. Never substitute those demo levels for missing live readings.

Compact banks can select an optional `icon` (`play`, `keyboard`, `ports`). These
are bounded vector primitives, not bitmap logos; the provider owns their semantic
assignment. Icon banks render only identities and routing lamps. Standard strips
have inset separators so adjacent channels don't share cramped edges.

`console::Console` is a bounded, read-only strip panel; `SignalFrame::Console`
transports it through existing renderer adapters. `Console::append` produces the
same Scene primitives used by wGPU hosts and `Console::labels` supplies shared
glyph anchors. No window, network client, audio callback, FFT or measurement DSP
is owned here.

`MeterConfig` selects `needle` or `led`, calibrated dB bounds, warning/clipping
thresholds, 8–48 LED segments, horizontal/vertical LED orientation, semantic face
and ink roles, and optional host-side visual smoothing (0–2000 ms). Light vintage
faces can use `highlight` with `shadow` ink; dark faces use `surface_recessed` with
`text`. Hosts inject the actual colors. Android defaults its light meter material
to a warm paper face and dark scale ink, with optional `meter_face`/`meter_ink`
palette overrides.

Needle appearance is not measurement semantics. This contract carries **peak
dBFS**. It must not be described as a calibrated RMS VU meter. Visual interpolation
does not implement VU ballistics; hosts retain exact numeric readouts from the
source snapshot. Missing levels are `null`, not zero or a synthetic animation.
`show_meter: false` is for non-audio routing/controller/rack cards.

Routing lamps use both shape and color: hollow means configured, unknown or
idle; filled means observed connection/activity or fault (semantic color still
distinguishes these). They do not infer audio activity from routing. Missing
levels draw two dashes, never a zero reading or a fabricated lit meter.

Observed gain draws a read-only fader reference. An absent gain never draws a
pretend control. Typed interactions, scaling gains, meter acquisition and peak
hold/reset remain host responsibilities.
