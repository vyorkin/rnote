# Toolbars float unless the drop is explicitly armed for docking

## Status

accepted (2026-11-05)

## Context

Dragging a toolbar near a window edge always docked it ([0002](0002-toolbar-docking.md)), which turned a request to place
two toolbars next to each other into a dock and a shrunken canvas. Docking was a side effect of a
plain drag, not a choice.

## Decision

- A new preference, `toolbars-dock-on-drop` (GSettings, default `false`), decides whether a plain
  release near a window edge docks the dragged toolbar.
- Shift inverts the preference: with the preference off, holding Shift is the only way to dock;
  with it on, holding Shift keeps the toolbar floating.
- "Near" is `DOCK_SNAP_DISTANCE = 24` px between the toolbar's centre and the edge.
- The dock indicator is shown only while the preference and the modifier state make the current
  release dock. It is a strip as thick as the snap zone, painted by the `dock_indicator` CSS class
  with the theme accent colour instead of a hardcoded wash.
- A release that does not dock places the toolbar at the pointer, clamped inside the overlay.
- The switch applies to every open window at once; the value is saved with the other toolbar
  settings.

## Consequences

- Docking is discovered through the switch or the handle tooltip, not through trying a drag.
- Touch and pen input carry no Shift, so they can dock only with the switch on.
- Toolbars that are already docked stay docked; only later drops change.
- Free positions are persisted per window; see ADR 0006.
