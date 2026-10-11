# Signal and timeline views

`fpl_gfx::signals` is a rendering-only core shared by DAWs, radio dashboards
and instrumentation. No new windowing toolkit, FFT implementation or transport
dependency is required. All colors are semantic theme roles.

| Builder | Input | Owner of input |
| --- | --- | --- |
| `piano_roll` | note intervals, timeline window, optional playhead | DAW sequencer or MIDI observer |
| `waveform` | min/max envelope columns, value range | recording/peak-cache owner |
| `scope` | uniformly spaced values and explicit scale | audio tap, LFO, automation or calibrated CV source |
| `spectrum` | sorted frequency bands and amplitudes in dB | existing FFT/analyzer owner |
| `SignalFrame::Plot` | physical X/Y samples, labeled unit domains, legends, optional viewport | telemetry or experiment owner |

## Rich chart widgets

`fpl_gfx::plot::Plot` adds axes, tick labels, a grid, theme-role legend swatches
and clipped line traces. Hosts paint the returned geometry and glyph placements;
no PDF, browser or remote screenshot is involved. Desktop Canvas uses iced/wGPU;
FireTV paints the same geometry through its hardware-accelerated Android Canvas.
Geometry preparation is CPU work; accelerated painting is not a claim of GPU DSP.

The strict `SignalFrame` payload uses `view: "plot"` and a `plot` object with
`x`, `y`, `series` and optional `viewport`. Each axis declares `label`, `unit`,
`minimum` and `maximum`; each series declares a distinct `id`, `label`, semantic
`role` and explicit `{x, y}` points. A null Y is a gap, not zero. At most four
series and 4096 points per series are accepted. X coordinates must increase.
The optional viewport is a valid subset of the physical domains; hosts own
revisioned zoom/pan state and input, not gfx. This first slice does not implement
hover inspection, linked panels, dashboards or Grafana/Tableau embedding.

Lua composes a Canvas `signal_view` widget with `view = "plot"`; providers still
own acquisition, units, identity, provenance and freshness. Completed experiment
results remain labeled samples, never masquerade as live telemetry. Cache by
frame, resolved theme and bounds; unchanged data should not regenerate paths.

Live MIDI is the same piano-roll view with held notes ending at the current
observation time. The app must handle note-off, sustain, duplicate notes, channel
filtering and disconnect/all-notes-off; the graphics library must not infer them.
Pointer-to-time/key mapping lives in `PianoViewport::position` so hosts can
share geometry without embedding a Canvas or DAW command API in gfx.

## Ownership

- **gfx:** bounded view geometry, theme roles and reusable GPU rendering.
- **Synesthesia:** MIDI/recording state, FFT normalization, automation semantics,
  editing commands and observed-state contracts.
- **Radioman:** tuner/demodulator signal acquisition and spectrum analysis; it can
  render the same scope/spectrum snapshots without depending on Synesthesia.
- **Canvas/Dock:** layout, surface leases, stream subscriptions, visibility,
  theme injection and routing pointer events. It does not become a DSP service.

## Performance and correctness

Each view accepts at most 4096 items, validates before mutating the scene, and
appends O(items) primitives. Reuse caller-owned `Scene` capacity; retained static
panels can be separated from dynamic data. Reduce to visible pixel columns
upstream. Peak reduction must preserve min/max transients; sampling every Nth
sample is not a waveform envelope. Rendering remains independent of take length.
The builders allocate no temporary vectors and never access an audio callback.
Scene growth may allocate unless the caller reserves capacity.

Scope snapshots are uniformly spaced; timestamps, discontinuities and trigger
alignment remain producer-owned. dBFS, dBm and volts are different units: pass an
explicit scale, source identity and freshness in the host contract. Physical CV
requires suitable calibrated hardware; these views do not imply Scarlett DC/CV
support. Empty input means empty display, never a fabricated signal. Reject NaN,
infinity, malformed intervals or invalid ranges rather than hiding them.

Legacy builders' limits: rendering/hit geometry only; no text labels, beat-grid editing,
drag commands, trigger acquisition or live stream adapters. Apps own labels and
stale/no-data overlays. A scope trace is not a spectrogram or a full FFT engine.

## Exercise the shared GPU path

```sh
cargo run --example offscreen -- --signals
# Optional pixel readback (refuses to overwrite an existing file):
cargo run --example offscreen -- --signals --output /tmp/gfx-signals.ppm
```

This uses explicitly synthetic fixtures in the existing offscreen example and
the normal instanced wGPU renderer. It does not connect to studio hardware or
change playback. CPU validation also works with `--no-default-features`.
