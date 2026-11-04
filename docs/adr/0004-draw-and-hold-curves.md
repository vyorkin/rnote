# Draw-and-Hold snaps open curves

## Status

accepted (2026-11-04)

## Context

Draw-and-Hold replaces a rough stroke with a fitted shape when the pen is held
still at the end of the stroke. It recognized only lines, ellipses, rectangles,
and polygons. A whole class of strokes — a bracket, a single bend, a shallow
arc — is an open curve that is neither a line nor a closed shape, so the hold
did nothing.

Snapping these to a polyline would keep the corners of the rough stroke.
Snapping them to an ellipse would wrap the stroke into a loop. The natural
targets are quadratic and cubic Béziers, which can express one or two bends
while keeping the endpoints fixed.

## Decision

`recognize_curve` fits both a quadratic and a cubic Bézier to the stroke points
with fixed endpoints, and accepts the fit only when it is dense enough (the
longest gap between points is small relative to the stroke) and straight enough
not to be a line. A quadratic is chosen unless the cubic residual is more than
6% lower; both are added to `Shape` as `QuadraticBezier` and `CubicBezier`.

The fit is rejected for strokes whose largest turn angle exceeds `max_turn_angle`
(30° by default), so a sharp corner stays a corner, and for strokes thinner
than `CURVE_MIN_TOLERANCE_SURFACE` so that it does not fight the pen. The
tolerances are in surface coordinates divided by the camera zoom.

## Consequences

- Holding still over an S-curve or a bracket now gives a clean Bézier, and the
  endpoints stay where the pen landed.
- `Shape` grows two variants; shape export, bounds, and adjust switch on them.
  The quadratic control point follows the pen, the cubic control points move
  the two bend points.
- A stroke that is already a good quadratic is not promoted to a cubic: the
  residual has to improve enough to justify the extra degree.