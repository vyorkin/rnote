# rnote fork

Vocabulary for the features this fork adds: gestures, pens and toolbars.

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

**Parenthesis**:
A round bracket glyph, `(` or `)`, written as a single stroke.
_Avoid_: bracket (when the round one is meant), paren

**Bulge**:
The depth of a bracket's bend, the largest distance between its stroke and the chord joining its ends.
_Avoid_: curve depth, sagitta

**Canonical bracket**:
A bracket whose ends lie on one vertical line, whose deepest point is at the middle of its height, and whose bend is a circular arc.
_Avoid_: neat bracket, clean bracket

**Bracket snap**:
Draw-and-Hold replacing a parenthesis stroke with a canonical bracket of the same height and bulge.
_Avoid_: bracket completion, paren auto-shape

**Curve**:
An open pen stroke that bends without a corner, as opposed to a line or a closed shape.
_Avoid_: arc (when any bend is meant), polyline

**Curve snap**:
Draw-and-Hold replacing a curve stroke with a smooth curve through the same path.
_Avoid_: curve fit, spline snap

## Pens

**Fountain Pen**:
A brush style whose stroke is thinner the faster the pen moves and thicker the harder it presses, with tapered ends.
_Avoid_: calligraphy pen, ink pen

## Toolbars

**Floating toolbar**:
A toolbar that lies over the canvas and takes no layout space.
_Avoid_: overlay toolbar, popover

**Docked toolbar**:
A toolbar that sits at a window edge and takes layout space, shrinking the canvas.
_Avoid_: pinned toolbar, fixed toolbar

**Dock zone**:
The band along a window edge, as wide as `DOCK_SNAP_DISTANCE`, inside which a released toolbar docks.
_Avoid_: drop zone, snap area

**Toolbar dock**:
The window edge a docked toolbar is attached to: top, bottom, left or right.
_Avoid_: toolbar position, toolbar side

**Toolbar layout**:
The docks and floating positions of all toolbar panels of one window.
_Avoid_: toolbar state, panel arrangement

**Stored position**:
A floating toolbar's place, kept as fractions of its travel range so that a window resize keeps
its relative place instead of a pixel coordinate.
_Avoid_: saved coordinates, pixel position
