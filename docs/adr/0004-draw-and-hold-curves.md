# Draw-and-Hold snaps open curves

## Status

accepted (2026-11-04), superseded by [0006](0006-goodnotes-style-curves.md):
the one-segment Bézier fit is replaced by a chain of cubic segments, and a
parenthesis now becomes a canonical bracket rather than the raw fitted Bézier
([0005](0005-bracket-snap.md))

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

`recognize_curve` resamples the stroke points, then fits both a quadratic and a
cubic Bézier to them with fixed endpoints. A fit is accepted when its mean
distance to the stroke stays under 4% of the chord and no single point is
farther than 10% of it. A quadratic is chosen unless the cubic mean residual is
at least 40% lower (`CURVE_CUBIC_IMPROVEMENT` = 0.6); both are added to `Shape`
as `QuadraticBezier` and `CubicBezier`.

The fit is rejected for strokes whose largest turn angle, measured three
samples to each side along the resampled points, exceeds 30° (`CURVE_MAX_TURN`),
so a sharp corner stays a corner. Both distances have a floor of
`CURVE_MIN_TOLERANCE_SURFACE` (in surface coordinates, divided by the camera
zoom), so the jitter of a short stroke is tolerated.

## Consequences

- Holding still over an S-curve or a bracket now gives a clean Bézier, and the
  endpoints stay where the pen landed.
- `Shape` grows two variants; shape export, bounds, and adjust switch on them.
  The quadratic control point follows the pen, the cubic control points move
  the two bend points.
- A stroke that is already a good quadratic is not promoted to a cubic: the
  residual has to improve enough to justify the extra degree.