# Toolbars dock into layout slots, not into the window chrome

## Status

accepted (2026-11-04); amended (2026-11-05): docking is opt-in, see
[0012](0012-toolbars-float-unless-the-drop-docks.md).

## Context

The pen picker, the color picker and the pen options panel are `child type="overlay"` of one
`GtkOverlay` inside `RnOverlays` (`overlays.ui:25-87`), positioned by static `halign`/`valign`
and margins. They float over the canvas. Docking them to a window edge must not let a panel cover
the page, and the window chrome (`AdwOverlaySplitView` for the workspace sidebar, `AdwToolbarView`
for the header) must stay as it is.

## Decision

Each panel gets its own `ToolbarDock` (top, bottom, left, right, floating), stored in GSettings and
shared by all windows. A docked panel is reparented into one of four slots of a `GtkBox` that wraps
the canvas overlay inside `RnOverlays`, so it takes layout space and shrinks the canvas. A floating
panel stays an overlay child with its current anchors. Docking never reparents into
`AdwToolbarView` or a second split view.

## Consequences

- The canvas area owns the whole toolbar layout; window-level containers are untouched, and
  right-handed flips and focus mode keep working on the same widgets.
- A docked panel cannot cover content, but reduces the canvas; on a narrow window the canvas
  shrinks rather than the panel floating away.
- The horizontal slots (top, bottom) sit inside a `GtkScrolledWindow`: when the docked panels are
  wider than the window, the row scrolls instead of clipping the last panel. The vertical slots
  keep a single column, which the window height accommodates.
- `set_measure_overlay` and the margin overrides in `settingspanel` apply to floating panels only.
- The panel drag gesture sits on `RnOverlays`, not on the overlay (nor on each handle): a docked
  panel is a child of a dock slot, a sibling of the overlay, so a gesture on the overlay never sees
  the handle. The gesture hit-tests the handles in `RnOverlays` coordinates.
- Reparenting a panel that is on screen is deferred to the next main loop iteration: unparenting a
  mapped widget from inside `drag-begin` trips GTK's mapped-state assertion and aborts the app.
- The dragged panel is not moved while the pointer is down. A moved widget whose snapshot is
  unchanged keeps its render node, and the frame's damage then covers only where the node now is:
  the position it left keeps its pixels, and a pen, which moves the panel far from frame to frame,
  leaves a ghost trail that redrawing the panel does not clear. Instead, the panel's picture is
  taken at the press (while it is still on screen), the panel is hidden for the drag, and the canvas
  draws the picture at the pointer: the canvas repaints itself as a whole on every frame of the
  drag, so nothing is left behind. The release ends the preview, shows the panel, and places it at
  the pointer or docks it; a panel that is already an overlay child is positioned by margins.
- The preview is cut to the canvas bounds in the canvas's snapshot: the area outside them belongs to
  other widgets, whose damage the canvas does not command.
- A drag-placed panel is aligned `Start`/`Start`: margins only position a widget aligned to the
  start, and a panel left at the dock's centre/end alignment ignores them and stays put.
- The overlay's `overflow` is `visible`: a panel dragged out of a top or left dock is picked up at
  a negative offset, outside the overlay's allocation, and clipping would hide it until the pointer
  pulled it inside.
