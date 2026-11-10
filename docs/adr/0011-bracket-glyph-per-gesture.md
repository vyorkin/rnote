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
a parenthesis with a corner radius: the setting stops at 2% of the height, and at radius zero
`Curve::new_rounded_bracket` refuses the shape outright (`radius <= EPSILON`), so a square bracket
with sharp corners could not be drawn.

## Decision

Two settings, one per gesture:

- `gestures.bracket_style`: `ParenthesisStyle`, `Arc` | `Rounded` | `Square` — the glyph a
  smooth-bend stroke is snapped to.
- `gestures.brace_style`: `BraceStyle`, `Curly` | `Rounded` | `Square` — the glyph a cusp stroke
  is snapped to.

The stroke still decides the family. One smooth bend is a parenthesis and a cusp between two
curled tips is a brace; no setting turns one into the other. What the family becomes is the
setting's: a parenthesis is an arc, a rounded parenthesis or a square bracket, a brace is the brace
of type, the same with a filleted cusp, or the same with right angles at its cusp.

The one recognition the settings do not decide is a square bracket drawn by hand. A stroke that
turns sharply at the end of a straight body is a square bracket whatever the parenthesis style
says: its corners turn at one point of the stroke (`SQUARE_CORNER_MIN_TURN`, 30°), where a drawn
parenthesis turns gradually through an arc at each end. That is the gesture for `[`; the
parenthesis style decides what a *smooth* stroke becomes, which is why `Square` there makes a bowed
parenthesis a square bracket as well.

Three canonical shapes are new. `Curve::new_square_bracket(start, end, depth, outward)` is a
straight body parallel to the chord at `depth` and two straight arms from the ends out to it, so
each corner is a right angle and the shape is three straight cubics; its depth is
`SQUARE_DEPTH_RATIO`, 15% of its height, as a rounded bracket's corner radius is set by its height
and not by how deeply the hand bowed the stroke. `Curve::new_rounded_brace` is the brace of type
with the cusp met by arms replaced by a circular fillet, `CURLY_CUSP_FILLET_SHARE` of the depth.
`Curve::new_square_brace` is the same skeleton with every corner a right angle: a mesa of
`SQUARE_BRACE_MESA_SHARE` of the depth on each side of the middle.

`BracketKind` stays two gestures deep — `Parenthesis(ParenthesisStyle)`, `Brace(BraceStyle)`,
`SquareBracket` — and `RecognizedShape::bracket` carries it. While the pen is down, `ShapeAdjust`'s
`RoundedBracketEnd` becomes a `BracketBodyEnd` with a `BracketBody::{Rounded, Square}`: the same
drag of the end the pen rested at, the same `depth / height` ratio kept while stretching, only the
constructor differs. The brace's drag keeps the style it was recognized in, and divides out
`Curve::rounded_cusp_shortfall()` first, so a filleted cusp is not drawn shallower on every drag.

The serde names keep an old config readable: `bracket_style` keeps its name and its values
`"arc"`/`"rounded"`, gains `"square"`; `brace_style` is new and defaults to `"curly"`.

## Consequences

- An old config opens unchanged. A config written by this version carries `brace_style`, which an
  older version ignores, and a `bracket_style` of `"square"`, which it refuses — the same
  forward-compatibility the earlier enum additions have.
- ADR 0010's "the settings gain no control" no longer holds: the cusp still decides that a stroke
  is a brace, and the setting decides what the brace becomes.
- The corner radius spin stays sensitive only while the parenthesis style is `Rounded`: a rounded
  brace turns its cusp over a fixed fillet, so the ratio does not reach it.
- A parenthesis drawn with sharp corners is a square bracket whatever the parenthesis style says;
  the style applies to the smooth strokes only. The two gestures still read the one style, so a
  writer who wants every bracket square sets both to `Square`.
- The drawn bulge of a square bracket is discarded as it is for a rounded one; `Arc` remains the
  only glyph that keeps the drawn depth. A rounded or square brace keeps the depth drawn only in
  that its height comes from the stroke: the depth itself is canonical, `CURLY_DEPTH_RATIO`.
- `BracketStyle` is gone; `ParenthesisStyle` and `BraceStyle` are the settings' enums, their combo
  rows carry a third item each, and the rows are titled by the gesture they apply to.
- No angle bracket. A chevron `<`/`>` fails the smooth-bend gate, and a `V` is refused as a brace
  by ADR 0010's curled-tip rule, so a chevron glyph would need a gate of its own and is left out.
