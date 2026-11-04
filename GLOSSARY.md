# rnote gestures

Vocabulary for the gesture layer in this fork. It covers input patterns that act on the canvas; it is not a glossary for rnote as a whole.

## Gestures

**Gesture**:
A recognized input pattern that performs a canvas action without changing the active tool.
_Avoid_: shortcut, tool

**Pen gesture**:
A gesture made with a single pen or stylus stroke.
_Avoid_: stylus trick

**Touch gesture**:
A gesture made with one or more fingers, independent of the pen.
_Avoid_: multitouch shortcut

**Hardware gesture**:
A gesture reported by the stylus hardware itself, such as a barrel button or a squeeze.
_Avoid_: device trick

**Scribble-to-Erase**:
A pen gesture that erases the ink under a dense back-and-forth stroke.
_Avoid_: Scratch-to-Erase, scribble erase

**Circle-to-Lasso**:
A pen gesture that turns a closed circle drawn around content into a selection of that content.
_Avoid_: circle select, lasso circle

**Draw-and-Hold**:
A pen gesture that replaces a rough stroke with a fitted shape after the pen is held still at the end of the stroke.
_Avoid_: shape completion, auto-shape gesture

**AutoShape**:
A mode of the Shape tool that fits a shape to a rough stroke without a hold.
_Avoid_: Draw-and-Hold

## Recognition

**Gesture stroke**:
The stroke the user draws to perform a gesture. It is not necessarily the content the gesture acts on.
_Avoid_: gesture line, scratch

**Target stroke**:
A stroke that a gesture erases, selects, or otherwise acts on.
_Avoid_: victim stroke, affected stroke

**Candidate stroke**:
A completed stroke kept briefly because a later input may confirm a two-phase gesture.
_Avoid_: pending gesture, waiting stroke

**Gesture recognizer**:
The component that decides whether an input sequence is a particular gesture.
_Avoid_: detector, classifier

**Gesture action**:
The canvas operation produced by a recognized gesture.
_Avoid_: gesture command, effect

**Gesture binding**:
The mapping from a gesture to an action and its options.
_Avoid_: gesture setting, shortcut binding

**Gesture transaction**:
One undoable unit containing a gesture stroke and every change the gesture caused.
_Avoid_: gesture undo, batch

**Hold**:
A pen that stays down and nearly still at one position for a configured duration.
_Avoid_: pause, dwell

**Precision-first**:
A recognition policy that prefers missing a gesture over firing one during ordinary writing.
_Avoid_: conservative detection
