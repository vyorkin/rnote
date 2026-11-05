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

## Amendment (2026-11-05): one bend is one quadratic

The chain cleans a stroke but leaves an ordinary curve as the hand-drawn wobble
it was, and a bracket could come out of any deep bend: a `C` on its side, an
arch, the bowl of a letter were brackets, because nothing but the bulge was
judged. Two rules now hold.

**A bracket is an upright bend.** The parenthesis gates stay as they are — the
chord within `BRACKET_MAX_TILT` (35°) of the vertical, so the top end is above
the bottom one, the deepest point at the middle of the height, the ends left
out as hooks — and a stroke that does not pass them is not a bracket, whatever
else it is.

**A bend that is not a bracket is a single quadratic.** When the stroke stays on
one side of its chord and does not cross it, it has one bend and no inflection:
it is snapped to the quadratic through its own endpoints whose deepest point is
the stroke's, provided the stroke lies within `BEND_MAX_DEV_RATIO` (3.5% of the
chord, at least 2 surface pixels) of it. A stroke that crosses its chord bends
twice, and keeps the chain. The control point comes from the deepest point
rather than from a fit, so the bend lands where the pen drew it.

The canonical bracket's arc no longer mirrors: the sagitta of a deep arc makes
the signs of its end tangents a near tie, and a bracket that deep came out bent
to the other side of its chord. `CubicBezier::new_circular_arc` now checks which
side of the chord the middle of its arc lands on and mirrors both handles when
it is not the side it was asked for.

Consequences:

- `QuadraticBezier` is produced by recognition again, for a single bend; the
  quadratic branch of `ShapeAdjust` edits its bend by the control point, while a
  bracket is still edited by an end.
- Deep brackets bulge to the side they were drawn on, at any tilt, hook and
  handwriting noise.
