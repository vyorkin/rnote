# Gestures are a layer above pens

## Status

proposed

## Context

rnote currently maps raw pointer input straight to a pen state machine. GoodNotes-style gestures such as Scribble-to-Erase, Circle-to-Lasso, and Draw-and-Hold need to inspect a whole stroke, sometimes across strokes, and then act on the document. Putting each gesture inside `Brush` would make every new gesture a change to the brush state machine and would leave touch gestures with no shared home.

## Decision

Add a gesture layer between input and pens. Pure recognizers live in `rnote-compose`; the engine owns a `GestureManager`, a `GestureRecognizer` trait, gesture actions, configuration, and one transaction per gesture; the UI owns a small multi-touch adapter because `PenEvent` has no multi-touch identity.

A `GesturePen` decorator wraps brush-like pens and gives the manager stroke lifecycle events without duplicating the pen event stream. Eraser, selector, typewriter, and tool pens are not wrapped.

## Consequences

- A new pen gesture is a new recognizer plus an action, not a new branch in `Brush`.
- A gesture spans several engine events, so the manager must buffer recognition state and must record exactly one history entry when the gesture resolves.
- Touch gestures cannot be recognized in the engine from `PenEvent` alone; they need the UI adapter and must not steal pan, zoom, or the existing two-finger long-press shortcut.
- The layer adds indirection on the hot writing path; recognizers must be cheap or run only at stroke end and during a hold.
