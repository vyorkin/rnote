# Zen mode hides the window chrome, not the toolbars

## Status

accepted (2026-11-07)

## Context

Drawing wants the canvas and the toolbars and nothing else: the header bar (quick actions, window
title, menus) and the tab bar above the canvas take a strip of the screen they never earn while
drawing. Rnote already has a mode that hides something — focus mode — but it hides the three
dockable toolbar panels and keeps the chrome, which is the opposite of what a distraction-free
view needs. `hiding chrome` also interacts with fullscreen: a window whose header is hidden while it
has no border has no button and no area to drag, so a mistake would leave the user unable to move or
close it.

## Decision

- Zen mode is a boolean window property `zen-mode`, with a second property `zen-span` naming which
  variant the user asked for. Two window actions set them: `win.zen-mode-active-monitor` (a
  fullscreen window on the monitor it is on) and `win.zen-mode-all-monitors` (one window over the
  whole desktop area). A variant's action leaves zen when that variant is on, and switches the
  variant when the other one is on. Focus mode stays as it is.
- The two accelerators are user settings (`zen-accel-active-monitor`, default `<Ctrl><Shift>m`, and
  `zen-accel-all-monitors`, default `<Ctrl><Shift>f`), captured in the settings panel and applied to
  the app's actions at startup and on every change. The shortcuts dialog shows the current
  combinations, not the defaults. An unparsable stored value falls back to the schema default and
  then to a compiled-in one, so a settings file written for an older install cannot leave the
  shortcuts unbound.
- Entering zen hides `main_header` and `tabbar` and fullscreens the window. Hiding only the widgets
  would leave the window borderless anyway, because the header is the client-side decoration; the
  compositor's fullscreen makes that state deliberate instead of a stranded window.
- A single Wayland window cannot be fullscreened onto every output, so the spanning variant asks the
  compositor instead: it floats the window, resizes and moves it to the union of the monitor
  geometries, and drops the client-side border. Only a compositor that can carry one window across
  outputs can do this; Hyprland is such a compositor, and the dispatches use its IPC. When the
  compositor refuses the span, the variant degrades to the fullscreen of the active-monitor variant.
  Spanning also hides the desktop's own bar for as long as it lasts, and gives it back on leave, on a
  window close, or when the window is no longer spanning. While the flag stands it is also recorded
  in the state dir as this app's own, so a run that dies before its exit path does not leave the whole
  desktop without a bar: the next launch gives back a bar its marker claims. A bar the user hid
  themselves carries no marker and stays hidden.
- The toolbars are unaffected: they are overlay children of `RnOverlays`, not part of the window
  chrome. The sidebars and the overview are left hidden as before, so zen does not open panels the
  user did not ask for.
- Leaving zen restores the fullscreen state the window had when zen was entered. A window that was
  already fullscreen stays fullscreen; a windowed one comes back windowed with its chrome.
- Leaving fullscreen by any route (F11, the compositor) leaves zen too. Otherwise the window would
  keep a hidden header and no way back to its buttons. The exit clears the property before the
  fullscreen restore runs, so the restore does not undo the user's own fullscreen exit.
- Escape never leaves zen. The canvas owns Escape (ending text editing, cancelling a selection,
  returning a non-brush pen to the brush), and a window-level controller on the same key made an
  Escape meant for the canvas end zen first, which reads as a bug (2026-11-07). Zen is left with
  either zen accelerator, F11 or the compositor instead; every one of those is an explicit act, so a
  zen session is never ended by the key the user presses to get out of a half-finished stroke or text
  box.

## Consequences

- Zen mode and focus mode are orthogonal: zen + focus mode leaves the canvas alone, which is what a
  presentation or a full-screen tab should look like.
- Leaving zen restores the header and the tab bar's visibility, but the tab bar's own autohide rule
  can still keep a single-tab bar out of sight; that is its normal behavior, not a leftover of zen.
- Zen is per window: the variant is not stored in the settings and does not survive a restart. The
  accelerators are the one part of zen that is stored, and they are app-wide.
