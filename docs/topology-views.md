# Topology views

`fpl_gfx::topology::Graph` is a bounded, renderer-neutral signal-flow primitive.
It builds themed node cards, ports, curved cables and optional meter rails into
a caller-owned `Scene`. Hosts render text at the shared `Graph::labels` anchors.
`signals::SignalFrame::Topology` carries the same view across process boundaries.

Nodes declare stable IDs, labels, lane ordering, observation state and optional
normalized levels. Edges reference declared node IDs. There are no device names,
network clients, clocks or routing mutations in gfx. A DAW, radio app or network
operator can supply its own graph through the same boundary.

- Lane and node order are deterministic; transport arrival order is irrelevant.
- Configured-only edges are dashed. Connected and active edges are solid.
- Activity and levels must come from the owner; connection state is not signal.
- The host owns freshness, fonts, ellipsis, clipping and input handling.
- Limits are 64 nodes, 128 edges and eight lane ranks. Invalid data fails before
  scene mutation. Rendering is bounded by 16 line segments per cable.
- Reuse `Scene` allocations with `clear`, and cache geometry/labels until the
  snapshot or viewport changes. Do not build this view in an audio callback.

Synesthesia derives its graph from `/v1/daw`. Canvas consumes the portable frame;
the desktop Iced/wGPU adapter and Android native-paint adapter share geometry
and label placement, not duplicated application-specific layout.
