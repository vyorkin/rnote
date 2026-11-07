# A bracket snaps to a rounded shape as well as to an arc

## Status

accepted (2026-11-07), amends [0005](0005-bracket-snap.md)

## Context

A bracket drawn to group a block of content is tall. The arc snap of ADR 0005 keeps the bulge the
user drew and spreads its bend evenly over the whole height, so a tall bracket reads as a bulging
arc rather than as a near-vertical line with rounded ends — the shape a hand actually wants for a
tall group. And a stroke drawn as that shape is not recognized as a bracket at all: the gate wants
one smooth bend whose deepest point is in the middle of the stroke, while a straight body between
two corners fails both tests. Measured on a body 260 pixels tall with corners of radius 20, the
recognizer returns a `Shape::Line` from (120, 100) to (120, 400) and the corners are discarded —
worse than a bulge, since the stroke the user meant to be a bracket becomes a plain line.

## Decision

A bracket has two shapes, `BracketStyle`:

- `Arc` is the snap of ADR 0005: a circular arc through the ends, its bulge the one drawn.
- `Rounded` is a straight body, parallel to the chord at one corner radius to its side, turned out
  into a short tip at each end. A tip stays on the vertical of the ends, so the bracket is as deep
  as its corner radius, and each end turns with a quarter of a circle of that radius. The shape is
  stored as `Shape::Curve` of three cubics — a corner, the body and a corner — built by
  `Curve::new_rounded_bracket`. A radius of half the height or more leaves no body: the corners
  meet, and the bracket is the half circle through both ends.

The drawn bulge does not shape a rounded bracket. Its radius is the configured share of its height
(`gestures.bracket_corner_radius_ratio`, 12% by default, `BRACKET_CORNER_RADIUS_RATIO_MIN`..`MAX`
of 2%..40%), so every upright bracket of a given height snaps to the same shape however deeply it
was drawn. That is the point of the shape: a tall grouping bracket must not be as deep as the hand
happened to bow.

Recognition keeps the smooth-bend gate of ADR 0005 for both shapes, but requires the drawn bulge to
exceed the line recognizer's tolerance. A rounded bracket is additionally accepted when the stroke
is drawn as one: the middle half of it is straight within a tolerance, and both ends turn the same
way by at least 60°. A line has no corners, and a smooth bend has no straight middle, so the two
signatures do not overlap.

The gesture settings carry the shape and the radius: the "Bracket Shape" combo between "Arc" and
"Rounded corners", and the "Bracket Corner Radius" spin, which follows only the rounded shape.

While the pen is still down a rounded bracket is adjusted by its end like an arc bracket: the end
the pen rested at follows the pen, the other stays, and the bracket keeps the ratio
`radius / height` it was snapped with, so it rounds in proportion as it is stretched and stays on
the side it bulged to.

## Consequences

- `Rounded` is the default: the hold that used to give an arc now gives a rounded bracket. The arc
  shape remains in the settings and its recognition is unchanged, which the tests that pass
  `BracketOptions::default()` (an arc) show.
- `RecognizedShape::bracket` is `Option<BracketStyle>` rather than a boolean, since the two shapes
  are adjusted by different parts of different shapes; `ShapeAdjust` gains a
  `RoundedBracketEnd`.
- A bracket and an ordinary open curve are both `Shape::Curve` now; only the recognized bracket
  style tells them apart while the pen is down. Nothing else in `Shape`, `Shapeable`,
  `Transformable` or the composer changes.
- A tall bracket drawn as a straight body with corners no longer becomes a line, and a shallow
  smooth bow still does: the corner signature needs a corner at each end, which a bow does not
  have.
- A rounded bracket ignores the drawn depth, so a deliberately deep parenthesis snaps to a slim
  bracket when the rounded shape is selected. The arc shape is the way to keep the drawn depth.
- A square bracket drawn as a straight body with turned-down tips now snaps too, which ADR 0005
  left as drawn: the corner signature cannot tell a corner of 60° from one of 90°, and the snap
  rounds it. Curly brackets still bend to both sides of their chord and are left alone, and a
  stroke whose bulge is under the corner depth stays as drawn.
