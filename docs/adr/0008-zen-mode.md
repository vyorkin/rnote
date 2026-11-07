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

- Zen mode is a boolean window property `zen-mode`, toggled by the `win.zen-mode` property action,
  bound to `<Ctrl><Shift>f`, and listed in the shortcuts dialog. Focus mode stays as it is.
- Entering zen hides `main_header` and `tabbar` and fullscreens the window. Hiding only the widgets
  would leave the window borderless anyway, because the header is the client-side decoration; the
  compositor's fullscreen makes that state deliberate instead of a stranded window.
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
  `<Ctrl><Shift>f`, F11 or the compositor instead; every one of those is an explicit act, so a zen
  session is never ended by the key the user presses to get out of a half-finished stroke or text
  box.

## Consequences

- Zen mode and focus mode are orthogonal: zen + focus mode leaves the canvas alone, which is what a
  presentation or a full-screen tab should look like.
- Leaving zen restores the header and the tab bar's visibility, but the tab bar's own autohide rule
  can still keep a single-tab bar out of sight; that is its normal behavior, not a leftover of zen.
- Zen is per window; it is not stored in the settings and does not survive a restart.
