# A curly brace snaps with its cusp

## Status

accepted (2026-11-10), extends [0005](0005-bracket-snap.md) and [0009](0009-rounded-brackets.md)

## Context

A curly brace groups a block of content as a bracket does, and it is drawn in one stroke, but no
snap takes it. ADR 0009 leaves it as drawn, and both of its gates refuse it on purpose: the smooth
bend gate wants one bend over the whole height, while a brace turns sharply at its cusp, and the
bulge test wants the stroke on one side of its chord, while a brace written as an S bulges to both.

The two ways a brace is written by hand disagree with each other. Drawn as `{` the body stays on
one side of the chord between the tips, and the cusp juts out from it; drawn as `}` the stroke
crosses the chord, bulging one way above the cusp and the other below. The one thing both have is
a cusp: a sharp turn in the middle of the height, with the tips curling back over the chord at
the ends. The side the cusp points to is the glyph: left is `{`, right is `}`.

## Decision

A brace is a new canonical shape, `Curve::new_curly_brace(start, end, depth, outward)`, six cubics:

- each tip is a quarter of a circle of half the depth, turning towards the cusp and into the body;
- the body runs straight, half the depth to the side of the chord between the tips;
- the cusp is a point in the middle of the body, reached by two arms that leave the body along it
  and arrive at the cusp at 55° to it, so the point is about 70°, as it is in type.

The depth is canonical: `CURLY_DEPTH_RATIO`, 18% of the height, from the tips to the cusp, whatever
was drawn. A brace's size is set by the block it groups, and a hand bowing the stroke deep is
drawing an S, not a wider brace. It is not the configured corner radius of a rounded bracket: a
brace is a different glyph, not another style of a parenthesis, and its depth must not follow a
setting meant for brackets. The shape is adjusted like a rounded bracket: the end the pen rested at
follows the pen, the other stays, the share of the depth to the height is kept, and the brace keeps
opening to the side of its chord it did.

Recognition, `recognize_curly_brace`, is judged before the bracket, and requires:

- the chord upright within `BRACKET_MAX_TILT`, and the height at least `BRACKET_MIN_HEIGHT_SURFACE`;
- a turn of at least 45° over a window of 6% of the stroke at each side, and that turn at the
  middle of the height, between 30% and 70% of it — the cusp, where a brace turns and nothing
  else does. The middle is measured by height, not by length: the two ends of a brace are drawn to
  different lengths, and the hand often sweeps the lower one out into a long straight tail;
- one of the outer 15% ends turning by at least 30°: a tip curls over the chord, and a stroke that
  runs straight to both its ends is a `V`, whose point would otherwise pass for a cusp. One curled
  tip is enough — the other end is frequently that long tail;
- the cusp at least `BRACKET_MIN_DEPTH_SURFACE` off the chord, and no deeper than 60% of it: a
  stroke that wraps further bends back towards itself, which is a hook.

The stroke is snapped onto one vertical, its tips on the mean of their drawn x, its height the
drawn height, its cusp to the side the drawn cusp points to. The direction the stroke was drawn in
does not matter.

## Consequences

- `RecognizedShape::bracket` is `Option<BracketKind>` — `Round(BracketStyle)` for a parenthesis,
  `Curly` for a brace — and `ShapeAdjust::new` takes that kind. A brace and a rounded bracket are
  both `Shape::Curve`, so the kind is what tells the two ends to adjust apart.
- A brace written as an S snaps even though its bulges are on both sides of the chord, which the
  bracket gate of ADR 0009 refuses: the cusp signature does not look at the sides of the bulge, and
  the tips are what tell a brace from a `V`.
- A parenthesis keeps its snap: its bend never turns 45° over that window, and its ends do not curl
  as sharply as a brace's tips do within the outer 15% of the stroke.
- A brace drawn with a soft point is not a brace to the recognizer, and the round bracket gate may
  take it if its bulge is monotone: it snaps to a curly-less bracket. The sharp turn is the whole
  signature, and a stroke that does not have one is the shape it looks like.
- The drawn depth is discarded, as it is for a rounded bracket: a deep S becomes a slim brace. That
  is what a canonical brace is for.
- A tall stroke that turns sharply in its middle, like a handwritten `s`, `з` or `∫`, could snap
  when the pen is held at its end. The hold is the guard, as it is for every other shape.
- Nothing in `Shape`, `Shapeable`, `Transformable` or the composer changes, and the settings gain
  no control.
