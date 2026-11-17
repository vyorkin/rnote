# A picked color recolors only a selection

## Status

accepted (2026-11-17)

## Context

[ADR-0015](0015-typed-text-keeps-the-chosen-color.md) made a color picked in the toolbar the ink for
what is typed, and left `Engine::text_change_color()` repainting the whole box when nothing was
selected. That second half fired without the writer asking: the toolbar's palette pads follow the
ink, `refresh_ui` calls `RnColorPicker::set_stroke_color()` whenever the typewriter page is shown or
the text editing state changes, and a GObject property set emits `notify` even when the boxed value
is unchanged. The palette's `notify` handler runs `text_change_color()`, so merely clicking into a
box written in another color repainted it in the ink. Picking a color while editing with nothing
selected did the same, and recorded an undo step for it.

## Decision

`Engine::text_change_color()` recolors the selected text range and does nothing to strokes otherwise.
With nothing selected, a picked color only sets `TypewriterConfig::text_style.color` — the ink for the
next typed characters.

Existing text changes color through a selection and only through it: the box picked with the
Selector (a whole-stroke repaint), or a range selected in the typewriter with a color applied as a
`TextAttribute::TextColor` (Ctrl+A included). Focusing a box, and the palette syncing itself to the
ink, never repaint it.

## Consequences

- The focus-triggered palette sync is inert on strokes: opening an old box keeps its color whatever
  the palette shows.
- Picking a color with nothing selected creates no store record, so it leaves no undo step behind.
- Repainting a whole box in one action is still available: select it with the Selector and pick the
  color, which goes through `change_selection_stroke_colors()`.
- This reverses the consequence in [ADR-0015](0015-typed-text-keeps-the-chosen-color.md) that
  `text_change_color` still recolors the base color of the whole stroke.
