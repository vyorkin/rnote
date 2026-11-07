# Draw-and-Hold snaps a parenthesis to a canonical bracket

## Status

accepted (2026-11-05), amended by [0006](0006-goodnotes-style-curves.md): the
canonical bracket is a circular arc stored as a cubic, not a quadratic, and
by [0009](0009-rounded-brackets.md): the arc is one of two bracket shapes, the
other a straight body rounded at its ends

## Context

Draw-and-Hold fits an open stroke to a quadratic or cubic Bézier whose endpoints
are the stroke's own endpoints (ADR 0004). Every irregularity of the drawing
survives the fit: the ends of a hand-drawn parenthesis rarely lie on one
vertical, and the fitted control point puts the deepest point wherever the pen
happened to bow. Holding at the end of a parenthesis therefore gave a crooked
curve, not the neat bracket GoodNotes produces.

A parenthesis is also the one open stroke whose shape is known in advance: a
single bend, taller than it is deep, on an up-down axis. The open-curve fit
cannot tell it apart from a shallow arc, and the line recognizer, which runs
first, turns a nearly straight parenthesis into a straight line.

After the snap the pen keeps editing the shape until it is lifted. For an open
curve the bend follows the pen and the endpoints stay fixed; on a bracket that
means the bulge grows under a pen that lies on the end of the stroke, and the
end the pen is on does not move at all.

## Decision

`recognize_bracket` recognizes a parenthesis, and `Shape::QuadraticBezier`
expresses the result.

Recognition runs before `recognize_line` in `recognize_shape` and accepts a
stroke when:

- the chord between its ends is within 35° of vertical (`BRACKET_MAX_TILT`).
  A bracket drawn by hand leans: its ends are pulled sideways by the hooks where
  the pen lands and lifts, and the strokes measured from a page lean ten to
  twenty-five degrees. A 15° limit threw away the brackets actually drawn, so
  the limit only rejects an arc that is closer to a chevron than to an upright,

- its height is at least 30 surface pixels (`BRACKET_MIN_HEIGHT_SURFACE`,
  divided by the camera zoom),
- it has one bulge only: the signed distance from the chord does not change
  sign, and the largest such distance — the bulge — stays under 60% of the chord
  and above the line recognizer's own deviation tolerance for that stroke, so a
  stroke that is a line is never a bracket,
- its deepest point sits in the middle half of the stroke
  (`BRACKET_MAX_APEX_OFFSET`): a bracket bends where it is half way along,
- it has no sharp corner, which keeps the existing `max_open_turn` gate.

The ends of the stroke are the pen landing and lifting: they turn sharper than
the bracket and their distance from the chord is not where the bend is. The
first and last 8% of the stroke (`BRACKET_HOOK_RATIO`) are therefore left out
when the bend and the turn are judged, though not out of the shape that is
snapped. A hand-drawn parenthesis is often a deep arc — a half circle is not
uncommon — and is closer to a circular arc than to the quadratic it is snapped
into, so its distance from that quadratic is not what decides whether it is a
bracket.

The snap is canonical:

- both ends move to the mean of their x, so the chord is vertical; their y is
  kept, and so is the height,
- the deepest point is at the middle of the height: the control point is
  `midpoint + 2 × bulge × outward normal`, which puts the peak of a quadratic
  Bézier at `t = 0.5`,
- the bulge is the one the user drew.

The bracket replaces the stroke as a `ShapeStroke` like every other
Draw-and-Hold shape: the width is the stroke's mean width and the gesture
records one history entry.

While the pen is still down, a bracket is adjusted by its **end**, not by its
bend: the end the pen rested at follows the pen, the other end stays where it
is, and the bracket stays symmetric about the chord between those two ends. Its
bulge keeps the ratio `bulge / chord length` it had when it was snapped, so
stretching a bracket keeps its look. A bracket may therefore tilt and change
height while it is dragged, and it is not re-aligned when the pen is lifted.
Open curves keep the bend-follows-the-pen adjustment from ADR 0004.

Only round parentheses are in scope. Square and curly brackets have corners,
which ADR 0004 leaves as corners.

## Consequences

- A parenthesis snaps on the same hold, with the same delay and the same
  settings as a line or a circle; there is no new toggle.
- A snapped bracket is a plain `QuadraticBezier`, so `Shape`, `Shapeable`,
  `Transformable` and the composer gain no variant. The changes are the
  recognizer, the hold path and `ShapeAdjust`.
- A shallow parenthesis no longer becomes a straight line: the bracket test runs
  before the line test, and the line test cannot accept a stroke the bracket
  gate accepted.
- The size of the tilt limit decides whether the snap is reached at all: the
  brackets measured from a page leaned 17°, 25° and 26° off the vertical, and
  the earlier 15° limit recognized none of them.
- The bracket has a single width, as every recognized shape does
  (`style_for_recognized_shape`); the pressure taper of the original stroke is
  not kept.
- Dragging the end can tilt the bracket and change its height; nothing
  re-aligns it on release.
- Square and curly brackets still snap to nothing and remain as drawn.
