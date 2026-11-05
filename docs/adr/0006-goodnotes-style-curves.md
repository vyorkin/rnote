# Curves and brackets follow GoodNotes, not a Bézier fit

## Status

accepted (2026-11-05), supersedes the fit of
[0004](0004-draw-and-hold-curves.md), amends the bracket geometry of
[0005](0005-bracket-snap.md)

## Context

ADR 0004 fitted one quadratic and one cubic Bézier to an open stroke and kept
the better fit, and ADR 0005 snapped a parenthesis to a canonical
`QuadraticBezier`. In real drawings both went wrong the same way:

- Holding on a parenthesis gave a slightly tilted, lopsided arc: the canonical
  quadratic puts the bulge wherever the drawing bowed, and its ends are the
  pen's own landing and lifting points.
- Every open stroke that was not a bracket became a Bézier that swallowed the
  stroke — a fast, slightly crooked line, a `√`, the stem of a letter — and the
  result was rarely what the pen drew.
- The pen still moved the shape after the snap, but the control points of a
  one-segment Bézier are not a handle the eye understands; dragging the end of
  a bracket rotated it, because the bulge was applied perpendicular to a chord
  that had tilted.

The target is the behaviour of GoodNotes: holding on a bracket straightens it
into a typeset-looking bracket, and other curves are either left alone or made
smooth, never reshaped into a single sweeping arc.

## Decision

**A curve is a chain of cubic segments.** `Shape::Curve` holds
`Vec<CubicBezier>` whose segments share endpoints. `recognize_curve` simplifies
the stroke with Douglas–Peucker at `CURVE_KNOT_TOLERANCE_SURFACE` (1.5 surface
pixels), builds the chain through the knots with Catmull–Rom, and measures how
far that chain leaves the resampled stroke. The curve is accepted when the
stroke stays within `CURVE_MAX_DEV_RATIO` (6%) of the chord or
`CURVE_MIN_TOLERANCE_SURFACE` (2.0 surface pixels) of it, whichever is larger,
and is not a corner (`CURVE_MAX_TURN`).

**Curve recognition is optional, bracket recognition is not.** A new gesture
setting, `curve_recognition_enabled` ("Smooth Curves" in the settings panel,
off by default), gates the curve pass only: with it off a held stroke that is
only a curve is not recognized at all, while lines, brackets and closed shapes
still snap.

**A canonical bracket is a circular arc.** A parenthesis is snapped to the arc
through the two ends of its chord with the drawn bulge as the sagitta, both
ends at the mean of their x, keeping the height and the deepest point at the
middle of it. The shape is stored as the cubic approximation
(`CubicBezier::new_circular_arc`), so a parenthesis that is a half circle is
drawn as the half circle the pen drew instead of the flatter quadratic. The
gates are apex-centric: the bulge must not sit near an end, and the ends of the
stroke — sharp hooks that the pen writes at landing and lifting — are excluded
from the bend and turn judgement.

**The pen edits a bracket by an end, not by a bend.** While the pen is down,
the end it rested at follows the pen and the other end stays; the bracket is
rebuilt as the circular arc between them with the bulge-to-chord ratio it was
snapped with, so it keeps its look and does not re-align on release. Other open
curves are scaled from the corner opposite the pen.

## Consequences

- A bracket looks like a bracket at any depth: the quadratic's flattening of
  deep arcs is gone, and its ends no longer tilt.
- Holding on an ordinary curve with Smooth Curves off now does nothing, which
  is what the user asked for; with it on, the stroke becomes a smooth
  multi-segment curve that passes through it.
- The one-segment quadratic and cubic fits, `CURVE_CUBIC_IMPROVEMENT`, and the
  quadratic branch of the hold path are removed.
- `Shape` grows a variant that everywhere else is treated like any other
  shape: bounds, transforms, hitboxes and the smooth and rough composers all
  handle the chain, and the export to SVG follows the segments.
- Documents saved with a `quadraticBezier` or `cubicBezier` shape still load;
  the variants stay, but recognition no longer produces them.
- The bracket uses the same mean stroke width as every recognized shape
  (`style_for_recognized_shape`); the pressure taper of the original stroke is
  not kept.
