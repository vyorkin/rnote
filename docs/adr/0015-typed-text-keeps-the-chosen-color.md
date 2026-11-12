# The color chosen in the toolbar is the ink, not the box's color

## Status

accepted (2026-11-12)

## Context

Typing into a text box adopted the box's style whole: `Typewriter::update_state()` copied the
edited `TextStroke::text_style` into `TypewriterConfig::text_style`, color included. Opening a box
written in white therefore replaced the color chosen in the toolbar with white, and the words typed
into it came out white. The palette made the replacement hard to see: its pad followed the copy, so
the pad said white, while a swatch still carried the checked state of the last preset clicked — a
color that was picked but no longer in effect.

A color picked in the toolbar is a choice about the text about to be written. Silently discarding it
on the way into a box is the defect; recoloring the whole box on entry would be a second one, since
a writer opening an old box to fix a typo does not expect it to change color.

## Decision

Entering a text box adopts its font — family, size, weight, style, alignment, width — and keeps the
color: `TypewriterConfig::text_style.color` stays what the toolbar set, and the palette's pad keeps
showing it.

Characters typed into a stroke of another color take the ink. `TextStroke::color_text_in_range()`
wraps the inserted range in a `TextAttribute::TextColor` unless the characters already render in
that color, and extends the attribute before it instead of starting a new one, so a typed run stays
one range rather than one range per keystroke. Existing text keeps its color; recoloring it stays a
selection action (`replace_text_attribute_current_selection`).

Every insertion path goes through that rule: `PenEvent::Text` in both the caret and the selecting
state, key presses (`KeyboardKey::Unicode`, tab, newline) and `Typewriter::insert_text()` (IME
commits, paste). Typing over a selection colors the replacement too.

## Consequences

- The palette no longer contradicts itself while typing: the pad and the checked swatch both describe
  the ink, and entering a box does not change either.
- A box can now hold more than one color without a selection ever being made — that is the price of
  typing in the chosen color into existing text, and it is what a text editor does.
- `text_change_color` still recolors the base color of the whole stroke and the selection, so a
  writer can still repaint a box in one action; ranges written by typing are left alone.
- Font changes still flow from the box into the config, which is what makes editing an old box keep
  its look; only the color stopped following.
