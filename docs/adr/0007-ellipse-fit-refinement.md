# An ellipse is fitted where the pen drew it

## Status

accepted (2026-11-06), amends the ellipse fit of
[0004](0004-draw-and-hold-curves.md)

## Context

Draw-and-Hold snaps a closed stroke to a circle or an ellipse in
`recognize_ellipse`: the stroke is resampled to `N_RESAMPLE` points, an ellipse
is fitted to them by their second moments, and the fit is kept when the points
stray from the outline by less than `ELLIPSE_MAX_MEAN_RESIDUAL` (0.10) on
average and no point by more than `ELLIPSE_MAX_RESIDUAL` (0.25), both as a
share of the radius. Two things about a real circle defeat that fit:

- Circles are not closed exactly. The closed gate accepts a gap of up to 15%
  of the perimeter, which is about 50 degrees of a circle, and
  `recognize_closed` resamples the loop with `wrap = true`, so the missing part
  is bridged by a straight chord the pen never drew. The chord joins the
  stroke's two ends and pulls the moment fit: the center moves into the drawn
  arc, the radii with it, and the residual rises.
- The pen often turns back inside the rim before it lifts. Those points belong
  to the stroke and carry their own residual, which the max-residual gate
  counts.

Measured on a generated hand-drawn circle (radius 100, 20 degrees of overdraw
past the start, ends lifted 12% of the radius inside the rim): the old fit's
largest residual was 0.277 against the 0.25 gate, so the stroke was not
recognized at all and stayed freehand. Over a grid of generated circles
(radius 40-150, openings 0-40 degrees, end hooks 0-25% of the radius, tremor,
an egg-shaped third harmonic) the old pipeline rejected 408 of the cases; the
one below rejects 124, and every remaining rejection is 25% tremor on a 24 px
circle, which is not a circle a hand draws.

## Decision

`fit_ellipse` fits the points the pen drew and then refines the fit:

- The ellipse is fitted to `resample(points, N_RESAMPLE, false)`, without the
  closing chord, and that is what both the ellipse branch and the polygon
  fallback of `recognize_closed` use. The closed/smoothed points stay the input
  of the corner and turn-angle test, which needs the loop to be a loop.
- The starting fit is still the one by the second moments. It is exact for
  points spread evenly around a whole ellipse, and it is the starting point,
  not the answer.
- `ELLIPSE_FIT_STEPS` (32) Gauss-Newton steps then move the center, the two
  semi-axes and the tilt to where the points sit closest to the outline. The
  radii are parameterized in log, so a step cannot cross zero; the step is
  Levenberg-damped by the diagonal, capped at `ELLIPSE_FIT_MAX_STEP` (4), and
  kept only if it lowers the sum of squared residuals, which is also the
  stopping condition: a step that does not help ends the refinement and the
  previous fit stands.
- The residuals are measured against the mean radius of the points, not
  against radius 1: the algebraic objective the moments came from has a scale
  ambiguity, so a fit that sits a little small is not a fit that is off the
  shape.
- The axes are kept canonical (the semi-axis along the major direction first),
  because `CIRCLE_RADII_RATIO` reads `radii.y / radii.x` and a quarter-turned
  fit would read a long ellipse as a circle.

The gates themselves are unchanged: `ELLIPSE_MAX_MEAN_RESIDUAL` 0.10,
`ELLIPSE_MAX_RESIDUAL` 0.25, `CLOSED_GAP_PERIMETER_RATIO` 0.15,
`CIRCLE_RADII_RATIO` 0.75.

## Consequences

- A circle drawn a little past its start, or left open by up to ~50 degrees, is
  now recognized, and its center is within about 1% of the radius of where it
  was drawn (0.4 px on a radius of 100).
- A stroke that is not an ellipse is not made round by the refinement: the
  residual gates are the same, and the step acceptance returns to the starting
  fit whenever the refinement does not lower the residual. The existing
  rejections of `rectangle`, `triangle`, `square`, `scribble`, `spiral`,
  `letter-o` and `egg` stay rejections.
- A closed stroke costs up to 32 passes over 96 points when it is snapped. The
  fit runs on every closed stroke that reaches it, including rectangles: the
  step count is what bounds the cost, not the shape.
- The closed gate is deliberately still 15%: loosening it would also turn a
  deep arc or a parenthesis whose ends nearly meet into a circle. Strokes that
  miss each other by more than that are still not circles.
