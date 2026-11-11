# A parenthesis can be one smooth bow of a set depth

## Status

accepted (2026-11-11), amends [0011](0011-bracket-glyph-per-gesture.md)

## Context

The round bracket has three glyphs: `Arc` keeps the bulge the hand drew on a circular arc, `Rounded`
is a straight body turned out into quarter-circle tips, `Square` is the same body turned out at a
right angle. Two of them are the ends of a spectrum and the middle of it is empty: an arc of a drawn
bulge reads as a bulging curve rather than as a parenthesis, and a rounded bracket has a body that is
straight all the way with a corner you can see the circle of at each end. The parenthesis of type is
neither — it is one continuous bow, tightest at the tips and nearly straight in the middle, and
nothing in the settings draws it.

## Decision

`ParenthesisStyle` gains `Elliptical` (`"elliptical"`), the glyph a smooth-bend stroke is snapped to:
a half ellipse whose chord is the diameter between the ends. `Curve::new_elliptical_bracket(start,
end, depth, outward)` builds it as two quarters of the ellipse, one cubic each, so the curve has a
tangent at both tips (a right angle to the chord, as the corners of a rounded bracket leave it) and
no corner anywhere.

Its depth is `gestures.bracket_corner_radius_ratio` of the drawn height, the same share a rounded
bracket spends on its corners: switching between `Elliptical` and `Rounded` keeps the depth and
changes only the ends, and the spin row is now titled "Bracket Depth (%)" and is sensitive for both.
`Arc` stays the only glyph that keeps the drawn bulge.

The combo row lists `Arc`, `Elliptical`, `Rounded corners`, `Square corners`; the item indices in
`settingspanel` shift with it. While the pen is still down the bracket is adjusted by the end the pen
rested at, as a rounded one is: `BracketBody` gains `Elliptical`, and `BracketBodyEnd` keeps the
`depth / height` ratio it found at the deepest point of the first cubic.

No recognition gate changes. A smooth bend is a parenthesis as before, and the style decides what it
becomes; a stroke drawn with a sharp corner at each end is still a square bracket whatever the style
says.

## Consequences

- An old config opens unchanged. A config written by this version carries `"elliptical"`, which an
  older version refuses, as it refuses `"square"`.
- The parenthesis shapes are now four: drawn-depth arc, smooth bow, straight body with round corners,
  straight body with right-angled corners. The middle of the spectrum the settings lacked is filled
  without a second gesture or a second gate.
- The corner-radius spin no longer applies to rounded corners alone, which is why it speaks of depth:
  for a rounded bracket the depth is exactly its corner radius, since the body runs one radius from
  the vertical of the tips.
- `Elliptical` and `Rounded` differ only in the ends and share a depth, so a writer who wants the
  parenthesis of type but a deeper or slimmer bow can tune it, where an arc would also change the
  depth the hand drew.
