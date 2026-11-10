# Each bracket gesture snaps to the glyph its own setting names

## Status

accepted (2026-11-10), amends [0009](0009-rounded-brackets.md) and [0010](0010-curly-braces.md)

## Context

ADR 0009 gave the round bracket a shape setting, `Arc` or `Rounded`, and ADR 0010 left the curly
brace with no control at all, on the ground that a brace is a different glyph from a parenthesis
rather than another style of one. Both are true of the *stroke*: a smooth bend and a cusp are
different families, and the settings cannot make a parenthesis out of a brace stroke. What the
stroke becomes after it is recognized is a separate question, and it is the writer's: a block of
content is often grouped with square brackets, and `[` had no gesture at all. `Rounded corners` is
a parenthesis with a corner radius, and at radius zero `Curve::new_rounded_bracket` refuses the
shape (`radius <= EPSILON`), so a square bracket with sharp corners could not be drawn.

## Decision

Two settings, one per gesture:

- `gestures.bracket_style`: `ParenthesisStyle`, `Arc` | `Rounded` | `Square` — the glyph a
  smooth-bend stroke is snapped to.
- `gestures.brace_style`: `BraceStyle`, `Curly` | `Rounded` | `Square` — the glyph a cusp stroke
  is snapped to.

The recognition gates are untouched. The stroke decides the family — one smooth bend, or a cusp
between two curled tips — and the setting only decides the glyph the family becomes. A square
bracket is recognized by the same gates as a rounded one: a shallow smooth bow, or a straight
middle with a corner at each end.

A square bracket is a new canonical shape, `Curve::new_square_bracket(start, end, depth, outward)`:
a straight body parallel to the chord at `depth`, and two straight arms from the ends out to it,
so each corner is a right angle and the shape is three straight cubics. Its depth is canonical,
`SQUARE_DEPTH_RATIO`, 15% of its height, as the corner radius of a rounded bracket is: the arms of
a bracket are set by the height it covers, not by how deeply the hand bowed the stroke. A brace
snapped to a rounded bracket reuses `gestures.bracket_corner_radius_ratio`, the same shape a
parenthesis snaps to, on the side its cusp points to.

`BracketKind` becomes flat — `Arc`, `Rounded`, `Square`, `Curly` — and `RecognizedShape::bracket`
carries it. While the pen is down, `ShapeAdjust`'s `RoundedBracketEnd` becomes a `BracketBodyEnd`
with a `BracketBody::{Rounded, Square}`: the same drag of the end the pen rested at, the same
`depth / height` ratio kept while stretching, only the constructor differs.

The serde names keep an old config readable: `bracket_style` keeps its name and its values
`"arc"`/`"rounded"`, gains `"square"`; `brace_style` is new and defaults to `"curly"`.

## Consequences

- An old config opens unchanged. A config written by this version carries `brace_style`, which an
  older version ignores, and a `bracket_style` of `"square"`, which it refuses — the same
  forward-compatibility the earlier enum additions have.
- ADR 0010's "the settings gain no control" no longer holds: the cusp still decides that a stroke
  is a brace, and the setting decides what the brace becomes.
- The corner radius spin is sensitive while either gesture snaps to a rounded bracket, since both
  read the one ratio.
- The drawn bulge of a square bracket is discarded as it is for a rounded one; `Arc` remains the
  only glyph that keeps the drawn depth.
- `BracketStyle` is gone; `ParenthesisStyle` and `BraceStyle` are the settings' enums, their combo
  rows carry a third item each, and the rows are titled by the gesture they apply to.
- No angle bracket. A chevron `<`/`>` fails the smooth-bend gate, and a `V` is refused as a brace
  by ADR 0010's curled-tip rule, so a chevron glyph would need a gate of its own and is left out.
