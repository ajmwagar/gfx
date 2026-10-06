# Patchbay annotations

`patchbay::Patchbay` supplies renderer-neutral paired-jack geometry and label
rectangles. Hosts paint the existing `Scene` and text anchors through their own
renderer. No GPU resources, fonts, studio maps or audio state live in this module.

Supply one-based column IDs, upper/lower labels and `Mode::{Normal, HalfNormal,
Thru}`. Geometry and text sort by physical column, regardless of input ordering.
Use several short strips for a dense bay rather than squeezing 48 labels into a
small panel. Empty labels denote unassigned jacks. Semantic theme roles let each
host inject its palette.

The normal line and mode caption describe declared wiring, not a sensed cable.
Half-normal explicitly means top taps and bottom breaks the normal. Never infer
current signal continuity from this drawing. Synesthesia owns the machine-readable
physical map and any operator patch declarations; Canvas owns presentation.

This first slice is a core primitive, not yet a new transported SignalFrame view.
