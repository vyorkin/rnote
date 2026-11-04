# The fountain pen bakes its width into the stroke

## Status

accepted (2026-11-04)

## Context

The fountain pen has to be thinner the faster the pen moves, thicker the harder
it presses, and tapered at both ends. The stroke format stores one element per
point, and an element has only a `pressure` scalar — there is no time, velocity,
or per-point width field. A renderer that varies the width of a finished stroke
must therefore read the variation from that scalar.

Two things could produce the variation. The renderer could recompute width from
a stored velocity/width field, which needs a format change and a new smoothing
pass; or the pen could write the final width into the stroke while it is drawn,
leaving the existing smooth style to draw it. The second keeps every reader
(export, SVG, PDF, shape adjust) working unchanged.

## Decision

`BrushStyle::Fountain` is a brush style, not a new pen. While a fountain stroke
is drawn, every point's `pressure` is replaced by a width factor between 0 and
1: the pressure of the input scaled by a speed factor (a smoothed pen speed in
surface units per second, computed from the time between input events and
converted with `camera.total_zoom()` so it is zoom-invariant). When the stroke
finishes, both ends are tapered along the stroke length with the same factor.

`FountainOptions` holds the smooth style options plus `pressure_influence`,
`speed_influence`, and `taper_length`. The style the stroke is drawn with is
`Smooth` with a `Linear` pressure curve and the style's `stroke_width`, so
`pressure × stroke_width` is drawn width. `stroke_width_for_pressure` returns
the same product when a recognized shape replaces the stroke, and shapes are
drawn with a constant width equal to the drawn mean.

## Consequences

- The stroke format does not change; files with fountain strokes open in every
  version and render with the baked taper.
- Speed is measured only while drawing, so the speed influence of a stored
  stroke can't be recomputed later. The stroke keeps the width it was drawn
  with, which is what the user sees.
- A `Smooth` stroke is indistinguishable from a fountain stroke after saving;
  the fountain behaviour is a drawing-time model, not a per-stroke attribute.
- The taper is a post-pass over the finished path, so it applies to whatever the
  path builder produced (`Simple`, `Curved`, `Modeled`).