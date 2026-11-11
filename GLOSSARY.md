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
A pen gesture that erases the ink and the typed text under a dense back-and-forth stroke.
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
A bracket whose ends lie on one vertical line, in one of its shapes: an arc bracket, whose deepest
point is at the middle of its height and whose bend is a circular arc; an elliptical bracket, one
smooth half-ellipse bow; a rounded bracket; or a square bracket.
_Avoid_: neat bracket, clean bracket

**Elliptical bracket**:
A canonical bracket drawn as half of an ellipse whose chord is the diameter between its ends: no
straight body and no corner, its depth a share of its height, not the drawn bulge.
_Avoid_: smooth bracket, oval bracket, curved bracket

**Rounded bracket**:
A canonical bracket whose body is straight at one corner radius from the vertical of its ends, turned
out into a short tip at each end. Its radius is a share of its height, not the drawn bulge.
_Avoid_: straight bracket, stadium bracket

**Square bracket**:
A canonical bracket whose body is straight at a fixed depth from the vertical of its ends, turned out
into an arm at a right angle at each end. A stroke drawn with a sharp corner at each end is one
whatever the parenthesis shape is set to.
_Avoid_: box bracket

**Curly brace**:
A bracket drawn as one stroke with a cusp in the middle of its height, `{` or `}`.
_Avoid_: brace (when the curly one is meant), squiggle

**Cusp**:
The sharp turn at the middle of a brace's height. The side it points to is the glyph: left is `{`,
right is `}`.
_Avoid_: point, notch

**Bracket snap**:
Draw-and-Hold replacing an upright parenthesis stroke — its chord within `BRACKET_MAX_TILT` of the
vertical — with a canonical bracket of the same height, in the bracket shape the gesture settings
name.
_Avoid_: bracket completion, paren auto-shape

**Curve**:
An open pen stroke that bends without a corner, as opposed to a line or a closed shape.
_Avoid_: arc (when any bend is meant), polyline

**Bend**:
A curve's single bend, the one it makes when it stays on one side of its chord; a curve that
crosses its chord has two.
_Avoid_: hump, bow

**Curve snap**:
Draw-and-Hold replacing a curve stroke with a smooth curve at the same ends: a stroke with one
bend becomes one quadratic, a stroke with more keeps a chain of cubic segments through the same
path.
_Avoid_: curve fit, spline snap

**Ellipse fit**:
Draw-and-Hold replacing a closed stroke with a circle or an ellipse fitted to the points the pen
drew, kept at the center and orientation it drew them.
_Avoid_: ideal circle, shape completion

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

## Views

**Zen mode**:
A window mode that hides the window chrome — header bar and tab bar — and keeps the canvas and the
toolbars.
_Avoid_: distraction-free mode, immersive mode, fullscreen

**Zen variant**:
The area a zen window covers: the monitor it is on, or the whole desktop area.
_Avoid_: zen scope, zen span
