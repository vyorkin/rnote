# A toolbar layout is stored for the last active window, as fractions

## Status

accepted (2026-11-05)

## Context

ADR 0002 stored only a dock name per toolbar panel in GSettings, so a floating toolbar always came
back to its default corner. ADR 0005 made floating the normal state, which makes an arrangement of
two toolbars placed side by side worth keeping. Windows are independent of each other while a
session runs, but the settings they share are global.

## Decision

- One window's toolbar layout is stored under the new string key `toolbars-layout` as a JSON
  object keyed by `ToolbarPanel::name` (`penpicker`, `colorpicker`, `penssidebar`). Each entry has
  a `dock` and, for a floating panel, a `position`.
- `position` is a pair of fractions of the distance the panel can travel inside the overlay:
  `fx = x / (overlay_width - panel_width)`. On a resize the fractions are applied again, so a panel
  keeps its relative place and can never leave the window.
- Positions are applied from the window surface's "layout" signal, once the panels have been
  allocated for that size. This is the only hook that fires for every size: `size_allocate` on the
  overlays widget is bypassed once the template's box layout manages it, and `GtkWidget`'s
  "width"/"height" don't notify in GTK4. Applying the fractions before the first allocation would
  put every panel at the overlay's top-left corner, where they would stay.
- The signal only *queues* an application, and the queue waits for the size to settle for 100 ms.
  A window opens at its default size and is then maximized, so several sizes arrive in a row; an
  application at each of them makes the panels glide down the window after every start, because
  each panel's position moves with the window's height. Only the size the window keeps is used.
- The size that has to settle is the *window surface's*, not the overlay's. A placement changes the
  overlay's size whenever the overlay measures a floating panel, so comparing the overlay's size
  makes every apply a new size to apply for, and the panels slide out of the window one reapply at
  a time.
- A floating panel is never measured into the overlay (`set_measure_overlay(panel, false)`). Its
  margin is part of the overlay's size while it is measured, and the panel is placed inside the
  overlay by that margin: the overlay grows by the panel's position, the travel range grows with
  it, and the next apply places the panel further out. The overlay takes its size from the canvas
  alone.
- The layout is written when a panel drag is released, when a window stops being active, and when
  an active window saves its settings. A release writes it directly, because a window that is
  closed without ever reporting itself as inactive — killed, or closed by the compositor — would
  otherwise keep the layout from before the drag. Only the last active window's layout is restored
  on the next start.
- On start, a stored layout restores docks and positions. If `toolbars-layout` is empty or
  malformed, the per-panel `*-dock` keys are read as before.
- The `*-dock` keys are still written, as the fallback for an older version reading the same
  settings.

## Consequences

- Two windows opened in one session remember their own layouts, but a restart restores one layout,
  because a single window is created at startup.
- A panel in its default corner stores no position; its margins still come from the show-scrollbars
  setting. A stored position replaces those default margins.
- Positions are fractions of a measured travel range, so they are only meaningful once the panel
  and the overlay have been allocated a size. Until then the panels keep the position the layout
  gave them.
