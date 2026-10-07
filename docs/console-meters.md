# Console strips and peak meter faces

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
