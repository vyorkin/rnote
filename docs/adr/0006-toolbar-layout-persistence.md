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
- The layout is written when a window stops being active, and when an active window saves its
  settings. Only the last active window's layout is restored on the next start.
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
  and the overlay have a size; before the first resize the panel is placed at its stored fraction
  of whatever range the layout has at that point.
