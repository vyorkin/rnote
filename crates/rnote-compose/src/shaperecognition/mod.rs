// Imports
use crate::penpath::PenPath;
use crate::shapes::quadbez::quadbez_calc;
use crate::shapes::{
    CubicBezier, Curve, Ellipse, Line, Polygon, QuadraticBezier, Rectangle, Shape,
};
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use p2d::shape::Cuboid;
use serde::{Deserialize, Serialize};

/// The number of points the stroke is resampled to before analyzing it.
const N_RESAMPLE: usize = 96;
/// The minimum number of flattened input points needed to attempt a recognition.
const MIN_INPUT_POINTS: usize = 8;
/// The radius (in surface coordinates) the pen may wobble around while being held still.
///
/// The brush uses it to detect the pen being held still at the end of a stroke, which triggers the recognition,
/// and the recognition uses it to discard the jitter of the resting pen.
pub const HOLD_RADIUS_SURFACE: f64 = 6.0;
/// The minimum extent (in surface coordinates) of the stroke bounds on its larger axis needed to attempt a recognition.
const MIN_EXTENT_SURFACE: f64 = 10.0;
/// The maximum gap between the endpoints relative to the perimeter for the stroke to be considered closed.
const CLOSED_GAP_PERIMETER_RATIO: f64 = 0.15;
/// The length (in surface coordinates) up to which a stroke is judged as strictly as a short line:
/// strokes this short are as often parts of letters.
const LINE_SHORT_SURFACE: f64 = 60.0;
/// The length (in surface coordinates) from which a stroke is judged as forgivingly as a long line:
/// the longer a line, the harder it is to draw straight, and the less it can be anything else.
const LINE_LONG_SURFACE: f64 = 200.0;
/// The maximum deviation of the points from the line relative to its length, for a short line.
const LINE_SHORT_MAX_DEV_RATIO: f64 = 0.055;
/// The maximum deviation of the points from the line relative to its length, for a long line.
const LINE_LONG_MAX_DEV_RATIO: f64 = 0.08;
/// How much longer than the distance between its ends a short line can be, relative to that distance.
const LINE_SHORT_MAX_LEN_EXCESS_RATIO: f64 = 0.10;
/// How much longer than the distance between its ends a long line can be, relative to that distance.
const LINE_LONG_MAX_LEN_EXCESS_RATIO: f64 = 0.18;
/// How much of a long line is left out at each end, relative to its length: pens land and lift with
/// a little hook, which says nothing about whether the stroke is straight.
const LINE_LONG_HOOK_RATIO: f64 = 0.06;
/// The distance (in surface coordinates) between the points the length of a long line is measured
/// along, so that the tremor of the hand on a slowly drawn line does not add up to any length.
const LINE_LENGTH_SPACING_SURFACE: f64 = 6.0;
/// The deviation from the line (in surface coordinates) that is tolerated regardless of the length.
///
/// The tolerances relative to the length alone are too strict for short lines like fraction bars,
/// where the jitter of the hand is large compared to their length.
const LINE_MIN_DEV_TOLERANCE_SURFACE: f64 = 2.5;
/// The excess of the stroke length over the distance between its ends (in surface coordinates)
/// that is tolerated for a line regardless of its length.
const LINE_MIN_LEN_EXCESS_TOLERANCE_SURFACE: f64 = 4.0;
/// How many times as long a stroke no larger than [HOLD_SHORT_SURFACE] has to be held still
/// for it to be recognized, since it is as likely a part of a letter written slowly.
const HOLD_SHORT_FACTOR: f64 = 3.0;
/// The size (in surface coordinates) up to which a stroke has to be held the longest.
const HOLD_SHORT_SURFACE: f64 = 40.0;
/// The size (in surface coordinates) from which a stroke only has to be held for the set duration.
const HOLD_LONG_SURFACE: f64 = 120.0;
/// The window (in resampled points) used when measuring the turn angle at a point.
const TURN_WINDOW: usize = 3;
/// The minimum fraction of the total turning that must be concentrated in high-turn points
/// for the stroke to be treated as a polygon rather than a smooth curve.
const CORNERNESS_POLYGON_THRESHOLD: f64 = 0.5;
/// The minimum turn angle (radians) for a point to be a corner candidate.
const CORNER_MIN_TURN: f64 = 25.0 * std::f64::consts::PI / 180.0;
/// The minimum separation between two corners, in resampled points.
const CORNER_MIN_SEPARATION: usize = N_RESAMPLE / 12;
/// The maximum mean distance of the stroke points to the polygon outline,
/// relative to the bounds diagonal, for a polygon to be accepted.
const POLYGON_MAX_MEAN_DEV_RATIO: f64 = 0.025;
/// The maximum distance of a single stroke point to the polygon outline,
/// relative to the bounds diagonal, for a polygon to be accepted.
const POLYGON_MAX_DEV_RATIO: f64 = 0.08;
/// The maximum mean of the normalized ellipse fit residuals for an ellipse to be accepted.
const ELLIPSE_MAX_MEAN_RESIDUAL: f64 = 0.10;
/// The maximum single normalized ellipse fit residual for an ellipse to be accepted.
const ELLIPSE_MAX_RESIDUAL: f64 = 0.25;
/// The minimum ratio between the minor and major ellipse radii for the ellipse to be snapped to a circle.
const CIRCLE_RADII_RATIO: f64 = 0.75;
/// The maximum deviation (radians) of a corner angle from a right angle for a quadrilateral to become a rectangle.
const RECT_MAX_ANGLE_DEV: f64 = 30.0 * std::f64::consts::PI / 180.0;
/// The maximum rotation angle (radians) below which a recognized rectangle is snapped to be axis-aligned.
const RECT_AXIS_SNAP_ANGLE: f64 = 5.0 * std::f64::consts::PI / 180.0;
/// The maximum number of polygon corners that are recognized.
const POLYGON_MAX_CORNERS: usize = 6;
/// The maximum distance of the stroke points from an open curve, relative to its chord length,
/// for the curve to be accepted.
///
/// The curve is built through the stroke, so this only grows when the stroke wiggles between the
/// points it was simplified to: a curve is a cleaning of the stroke, not a different shape.
const CURVE_MAX_DEV_RATIO: f64 = 0.06;
/// The distance from an open curve (in surface coordinates) that is tolerated regardless of its length,
/// because the jitter of the hand is large compared to a short stroke.
const CURVE_MIN_TOLERANCE_SURFACE: f64 = 2.0;
/// The maximum turn angle (radians) at a point of the stroke for it to be treated as a smooth curve.
///
/// A sharper turn is a corner of a handwriting stroke, not a bend of a curve.
const CURVE_MAX_TURN: f64 = 30.0 * std::f64::consts::PI / 180.0;
/// The distance (in surface coordinates) the stroke is simplified at, to the points the curve is
/// built through.
const CURVE_KNOT_TOLERANCE_SURFACE: f64 = 1.5;
/// The largest number of points an open curve is built through.
const CURVE_MAX_KNOTS: usize = 24;
/// The maximum distance of the stroke points from a single quadratic bend, relative to its chord
/// length, for the stroke to be snapped to it.
///
/// Tighter than [`CURVE_MAX_DEV_RATIO`]: a bend is a template the stroke is snapped to, so a stroke
/// that only roughly follows one is left as its own curve rather than pulled onto it.
const BEND_MAX_DEV_RATIO: f64 = 0.035;
/// The distance from a single quadratic bend (in surface coordinates) that is tolerated regardless
/// of its size.
const BEND_MIN_TOLERANCE_SURFACE: f64 = 2.0;
/// The distance from the chord (in surface coordinates) within which a point of the stroke does not
/// say which side of the chord the bend is on.
///
/// Without it the tremor of a nearly straight stroke flips the side of single points, and a bend
/// would be refused to the stroke that comes closest to a line.
const BEND_SIDE_TOLERANCE_SURFACE: f64 = 3.0;
/// How many times the tolerance is raised when the stroke wiggles too much for [`CURVE_MAX_KNOTS`].
const CURVE_KNOT_PASSES: usize = 8;
/// The samples per cubic segment that the recognized curve is measured from the stroke at.
const CURVE_SAMPLES_PER_CUBIC: usize = 8;
/// The maximum tilt (radians) from the vertical for a stroke to be recognized as a parenthesis.
///
/// A bracket drawn by hand leans: the chord between its ends is the hand's idea of a vertical, and
/// the hooks where the pen lands and lifts pull its ends sideways. Measured that way, brackets are
/// drawn ten to twenty-five degrees off the vertical. The canonical bracket is straightened onto the
/// vertical whatever the drawing was, so a stroke this far off is still snapped into one; an arc
/// further off is closer to a chevron or a smile than to a bracket.
const BRACKET_MAX_TILT: f64 = 35.0 * std::f64::consts::PI / 180.0;
/// The minimum height (in surface coordinates) of a parenthesis.
///
/// Shorter strokes are as often parts of a letter, which only wait longer to be held.
const BRACKET_MIN_HEIGHT_SURFACE: f64 = 30.0;
/// The maximum bulge of a parenthesis, relative to its chord.
///
/// A hand-drawn parenthesis is often a deep arc, close to a half circle. A stroke that wraps
/// further than this folds back towards itself, which is a hook or a letter.
const BRACKET_MAX_BULGE_RATIO: f64 = 0.6;

/// The share of the stroke at each end that is the pen landing and lifting, not the bracket.
///
/// The hooks there turn sharper than the rest of the stroke and carry the pen's tremor, so the bend
/// is judged over the body between them. They still keep their place in the shape that is drawn.
const BRACKET_HOOK_RATIO: f64 = 0.08;

/// How far from the middle of the stroke its deepest point may sit, as a share of its length.
///
/// A bracket bends most in the middle. A stroke whose bulge sits near one end is a hook, or a
/// letter.
const BRACKET_MAX_APEX_OFFSET: f64 = 0.2;

/// How much of the bulge a point may fall short of and still count as one of the deepest.
///
/// A straight body is as deep all the way along it, so the middle of that band decides where the
/// stroke bends most, not the one point of it that happens to be first.
const BRACKET_DEPTH_PLATEAU_SHARE: f64 = 0.05;

/// The corner radius of a rounded bracket, relative to its height: the default.
pub const BRACKET_CORNER_RADIUS_RATIO_DEFAULT: f64 = 0.12;
/// The smallest corner radius of a rounded bracket, relative to its height.
pub const BRACKET_CORNER_RADIUS_RATIO_MIN: f64 = 0.02;
/// The largest corner radius of a rounded bracket, relative to its height.
///
/// A corner of half the height leaves no body at all: the bracket is then a half circle.
pub const BRACKET_CORNER_RADIUS_RATIO_MAX: f64 = 0.4;

/// The minimum depth (in surface coordinates) of a stroke that is a bracket rather than a line.
///
/// A rounded bracket is drawn nearly straight, so the depth the line recognizer would accept says
/// nothing about it. What sets it apart is the corners at its ends, and this keeps the tremor of a
/// line from being read as a corner.
const BRACKET_MIN_DEPTH_SURFACE: f64 = 4.0;

/// The share of the body at each end of a square bracket that carries a corner.
const BRACKET_CORNER_SHARE: f64 = 0.25;

/// The turn (radians) a corner of a square bracket has to turn by over that share of the body.
const BRACKET_CORNER_MIN_TURN: f64 = 60.0 * std::f64::consts::PI / 180.0;

/// The turn (radians) a single point of a square bracket has to carry.
///
/// A square bracket is drawn with the pen turning at the corner itself: one point of the stroke
/// turns by the whole right angle. A bracket drawn with rounded ends turns a little at every point
/// of a long arc instead, so no one point of it turns this far.
const SQUARE_CORNER_MIN_TURN: f64 = 30.0 * std::f64::consts::PI / 180.0;

/// The share of the height the middle of a square bracket may deviate from straight by.
const BRACKET_STRAIGHT_MAX_DEV_RATIO: f64 = 0.02;

/// The distance (in surface coordinates) from straight that is tolerated for the middle of a
/// square bracket, whatever its height.
const BRACKET_STRAIGHT_TOLERANCE_SURFACE: f64 = 2.5;

/// The depth of a curly brace, relative to its height, from the tips to its cusp.
///
/// Canonical, as the corner radius of a rounded bracket is: a brace groups a block of content, and
/// its size is set by the height it covers, not by how deeply the hand happened to bow it.
pub const CURLY_DEPTH_RATIO: f64 = 0.18;

/// The depth of a square bracket, relative to its height, from the tips to the body.
///
/// Canonical, as the corner radius of a rounded bracket is: a bracket groups a block of content,
/// and the length of its arms is set by the height it covers, not by how deeply the hand happened
/// to bow it.
pub const SQUARE_DEPTH_RATIO: f64 = 0.15;

/// The share of the stroke at each end that may be the pen landing and lifting, not the brace.
///
/// The tips of a hand-drawn brace curl a long way, and the turn where they meet the body is as
/// sharp as the cusp: it is left out of both the cusp and the middle it is judged at.
const CURLY_HOOK_RATIO: f64 = 0.15;

/// The share of the stroke on each side of a point whose turn decides whether the point is a cusp.
const CURLY_CUSP_WINDOW: f64 = 0.06;

/// The turn (radians) a curly brace's cusp has to make over that window.
///
/// A parenthesis is a smooth bend and turns no more than [`CURVE_MAX_TURN`] at any point, while a
/// hand rarely puts a turn this sharp anywhere but at the point of a brace. A brace drawn with a
/// soft point still turns some fifty degrees there.
const CURLY_CUSP_MIN_TURN: f64 = 45.0 * std::f64::consts::PI / 180.0;

/// How far up or down the height a curly brace's cusp may sit, as a share from either end.
///
/// A brace's point is at the middle of its height, not at the middle of its length: the two tips
/// of a hand-drawn brace are rarely as long as each other.
const CURLY_CUSP_EDGE_SHARE: f64 = 0.3;

/// The deepest a curly brace's cusp may be drawn, relative to its chord.
///
/// A brace written as an S bulges a good deal further than the canonical one; a stroke that wraps
/// further than this bends back towards itself, which is a hook.
const CURLY_MAX_DEPTH_RATIO: f64 = 0.6;

/// The turn (radians) at each tip of a curly brace.
///
/// The tips curl back over the chord. A stroke that runs straight to its end is a `V`, whose point
/// sits where a brace's cusp does.
const CURLY_TIP_MIN_TURN: f64 = 30.0 * std::f64::consts::PI / 180.0;

/// How a stroke recognized as a round parenthesis is drawn.
///
/// The gesture always gives a bracket; the style picks which representation of it it gets, the way
/// the parenthesis is set: round, round with a straight body, or square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParenthesisStyle {
    /// A circular arc between the ends, as deep as the stroke was drawn.
    #[serde(rename = "arc")]
    Arc,
    /// A straight body rounded into short tips at its ends, its depth a fixed share of its height.
    #[serde(rename = "rounded")]
    Rounded,
    /// A square bracket: a straight body with a right angle at each end, its depth the same fixed
    /// share of its height.
    #[serde(rename = "square")]
    Square,
}

/// How a stroke recognized as a curly brace is drawn.
///
/// The gesture always gives a curly brace, its cusp marking it; the style picks how the corners of
/// the brace, its tips and its cusp, are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BraceStyle {
    /// The brace of type: the tips curl and the arms meet in a point at the cusp.
    #[serde(rename = "curly")]
    Curly,
    /// The same brace with its cusp turned over a fillet, so it has no sharp point anywhere.
    #[serde(rename = "rounded")]
    Rounded,
    /// The same brace with a right angle at each tip and a right-angled tab at its cusp.
    #[serde(rename = "square")]
    Square,
}

/// How a recognized bracket is drawn and adjusted with the pen that drew it.
///
/// Which glyph a stroke is snapped to is decided by the stroke, never by the settings: a smooth
/// bend is a parenthesis, a stroke that turns sharply at a point in the middle of its height is a
/// curly brace, and a straight body between two corners is a square bracket. The settings only pick
/// the representation of the glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracketKind {
    /// A parenthesis, in the representation its style asks for.
    Parenthesis(ParenthesisStyle),
    /// A curly brace, in the representation its style asks for.
    Brace(BraceStyle),
    /// A square bracket.
    SquareBracket,
}

impl From<ParenthesisStyle> for BracketKind {
    fn from(style: ParenthesisStyle) -> Self {
        Self::Parenthesis(style)
    }
}

impl From<BraceStyle> for BracketKind {
    fn from(style: BraceStyle) -> Self {
        Self::Brace(style)
    }
}

/// How a bracket is shaped and adjusted.
#[derive(Debug, Clone, Copy)]
pub struct BracketOptions {
    /// How a stroke recognized as a round parenthesis is drawn.
    pub style: ParenthesisStyle,
    /// How a stroke recognized as a curly brace is drawn.
    pub brace: BraceStyle,
    /// The corner radius of a rounded parenthesis, relative to its height.
    pub corner_radius_ratio: f64,
}

impl Default for BracketOptions {
    fn default() -> Self {
        Self {
            style: ParenthesisStyle::Arc,
            brace: BraceStyle::Curly,
            corner_radius_ratio: BRACKET_CORNER_RADIUS_RATIO_DEFAULT,
        }
    }
}

/// The options of the shape recognition.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecognitionOptions {
    /// Whether open strokes are recognized as smooth curves.
    pub curves: bool,
    /// How a bracket is snapped to a canonical one.
    pub bracket: BracketOptions,
}

/// A recognized shape and how the shape is a bracket, if it is one.
#[derive(Debug, Clone)]
pub struct RecognizedShape {
    /// The shape the stroke is replaced with.
    pub shape: Shape,
    /// The kind of the canonical bracket, which is adjusted by its ends and not by its bend.
    pub bracket: Option<BracketKind>,
}

impl RecognizedShape {
    fn plain(shape: Shape) -> Self {
        Self {
            shape,
            bracket: None,
        }
    }

    fn bracket(shape: Shape, kind: BracketKind) -> Self {
        Self {
            shape,
            bracket: Some(kind),
        }
    }
}

/// Attempt to recognize a geometric shape from a drawn pen path.
///
/// Returns the recognized shape when the path resembles one closely enough, `None` otherwise.
///
/// Recognized are: lines, parentheses and curly braces, open curves as a chain of cubic segments,
/// ellipses and circles, rectangles (possibly rotated) and closed polygons with up to 6 corners
/// (triangles, quadrilaterals, ..).
///
/// `zoom` is the zoom of the view the path was drawn in. It converts the tolerances for the jitter of the hand,
/// which are defined in surface coordinates, to document coordinates.
pub fn recognize_shape(path: &PenPath, zoom: f64) -> Option<Shape> {
    recognize_shape_with_kind(path, zoom, true).map(|recognized| recognized.shape)
}

/// Like [`recognize_shape`], and tells a canonical bracket from an ordinary open curve.
///
/// `curves` enables the recognition of open curves, which are drawn as a chain of cubic segments.
/// With it off, a stroke that is only a curve is not recognized at all.
pub fn recognize_shape_with_kind(
    path: &PenPath,
    zoom: f64,
    curves: bool,
) -> Option<RecognizedShape> {
    recognize_shape_with_options(
        path,
        zoom,
        &RecognitionOptions {
            curves,
            ..RecognitionOptions::default()
        },
    )
}

/// Like [`recognize_shape_with_kind`], with the options that shape the bracket and the curves.
pub fn recognize_shape_with_options(
    path: &PenPath,
    zoom: f64,
    options: &RecognitionOptions,
) -> Option<RecognizedShape> {
    let input_points = flattened_points(path);
    if input_points.len() < MIN_INPUT_POINTS {
        return None;
    }
    let input_points = collapse_resting_tail(input_points, HOLD_RADIUS_SURFACE / zoom);

    let (min, max) = points_bounds(&input_points);
    let extents = max - min;
    if extents.x.max(extents.y) < MIN_EXTENT_SURFACE / zoom {
        return None;
    }
    let bounds_diag = extents.length();

    let perimeter = polyline_len(&input_points);
    if perimeter < f64::EPSILON {
        return None;
    }

    let gap = (*input_points.last().unwrap() - *input_points.first().unwrap()).length();
    let closed = gap < CLOSED_GAP_PERIMETER_RATIO * perimeter;

    if closed {
        recognize_closed(&input_points, bounds_diag).map(RecognizedShape::plain)
    } else {
        // The glyph is decided before the representation, and a stroke with a cusp is a brace
        // whatever else it looks like: a cornered bracket has a straight middle, and the cusp of a
        // brace is the one sharp turn in the middle of a stroke.
        if let Some((shape, kind)) = recognize_curly_brace(&input_points, zoom, &options.bracket) {
            return Some(RecognizedShape::bracket(shape, kind));
        }

        if let Some((shape, kind)) = recognize_square_bracket(&input_points, zoom) {
            return Some(RecognizedShape::bracket(shape, kind));
        }

        // A parenthesis is judged before a line, so that a shallow one stays a bracket. An arc
        // bracket gate rejects every stroke a line is: its bulge has to be larger than the
        // deviation a line of that length may have.
        if let Some((shape, kind)) = recognize_bracket(&input_points, zoom, &options.bracket) {
            return Some(RecognizedShape::bracket(shape, kind));
        }

        recognize_line(&input_points, perimeter, zoom)
            .map(RecognizedShape::plain)
            .or_else(|| {
                options
                    .curves
                    .then(|| recognize_curve(&input_points, zoom))
                    .flatten()
                    .map(RecognizedShape::plain)
            })
    }
}

/// How many times the set duration the pen has to be held still on the given stroke for it to be
/// recognized.
///
/// A slowly written letter pauses for about as long as a line is held, and its strokes look like
/// short lines. Long strokes are hardly ever letters, so only the short ones have to wait longer.
pub fn hold_factor(path: &PenPath, zoom: f64) -> f64 {
    let points = flattened_points(path);
    if points.is_empty() {
        return HOLD_SHORT_FACTOR;
    }
    let (min, max) = points_bounds(&points);
    let extents = max - min;
    let size = extents.x.max(extents.y) * zoom;
    let long =
        ((size - HOLD_SHORT_SURFACE) / (HOLD_LONG_SURFACE - HOLD_SHORT_SURFACE)).clamp(0.0, 1.0);

    HOLD_SHORT_FACTOR + (1.0 - HOLD_SHORT_FACTOR) * long
}

/// The mean width of the stroke as it was drawn, for the width of the shape that replaces it.
///
/// `width_for_pressure` is the width a stroke has at a given pen pressure, as its style draws it.
/// The width is averaged, and not the pressure, since the two only agree for a curve that is a
/// straight line: for any other, a stroke drawn with the mean pressure is not as wide as the
/// stroke was on average.
///
/// Weighted by length, so that what counts is how wide the stroke was along the way and not how
/// slowly it moved: a slow stretch reports many more events than a quick one covering the same
/// distance. The resting tail at the end of the stroke, where the pen is held still to trigger the
/// recognition, is left out for the same reason - it reports for as long as the hold lasts.
///
/// `None` for a path without elements.
pub fn mean_width(
    path: &PenPath,
    zoom: f64,
    width_for_pressure: impl Fn(f64) -> f64,
) -> Option<f64> {
    let mut elements = vec![path.start];
    elements.extend(path.segments.iter().map(|segment| segment.end()));
    let last_pos = elements.last()?.pos;
    let radius = HOLD_RADIUS_SURFACE / zoom;

    // Keep the element that moves into the hold, and nothing that came to rest after it. A stroke
    // that never leaves the hold radius has no tail to speak of, so all of it is used.
    let kept = elements
        .iter()
        .rposition(|element| (element.pos - last_pos).length() > radius)
        .map_or(elements.len(), |i| (i + 2).min(elements.len()));
    let elements = &elements[..kept];

    // A stroke is drawn with the width changing steadily between two elements, so the mean along
    // it is that of the widths at both ends.
    let (mut weighted, mut length) = (0.0, 0.0);
    for pair in elements.windows(2) {
        let segment = (pair[1].pos - pair[0].pos).length();
        let width =
            (width_for_pressure(pair[0].pressure) + width_for_pressure(pair[1].pressure)) * 0.5;
        weighted += segment * width;
        length += segment;
    }
    if length > f64::EPSILON {
        return Some(weighted / length);
    }

    // Nothing moved, so there is nothing to weigh by.
    Some(
        elements
            .iter()
            .map(|element| width_for_pressure(element.pressure))
            .sum::<f64>()
            / elements.len() as f64,
    )
}

/// Extract the flattened points of the pen path.
fn flattened_points(path: &PenPath) -> Vec<Vector2> {
    let mut points = Vec::new();

    for el in path.to_kurbo_flattened(0.25).elements() {
        match el {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
                points.push(Vector2::new(p.x, p.y));
            }
            _ => {}
        }
    }

    points.dedup_by(|a, b| (*a - *b).length() < f64::EPSILON);
    points
}

/// Collapse the points at the end of the stroke where the pen rested, all within `radius` of the last point,
/// into the last point.
///
/// The recognition is triggered by holding the pen still at the end of the stroke. The jitter of the resting pen
/// inflates the stroke length, and on short strokes makes up a large enough part of it to fail the recognition.
fn collapse_resting_tail(mut points: Vec<Vector2>, radius: f64) -> Vec<Vector2> {
    let Some(&last) = points.last() else {
        return points;
    };
    let rest_start = points
        .iter()
        .rposition(|p| (*p - last).length() > radius)
        .map_or(0, |i| i + 1);

    points.truncate(rest_start);
    points.push(last);
    points
}

/// The min/max corners of the bounds of the given points.
fn points_bounds(points: &[Vector2]) -> (Vector2, Vector2) {
    let mut min = points[0];
    let mut max = points[0];

    for p in points {
        min = min.min(*p);
        max = max.max(*p);
    }

    (min, max)
}

/// The total length of the polyline through the given points.
fn polyline_len(points: &[Vector2]) -> f64 {
    points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

/// Resample the polyline to `n` points with equal arc-length spacing.
///
/// When `wrap` is true the polyline is treated as a closed loop
/// (an implicit closing segment from the last to the first point is added).
fn resample(points: &[Vector2], n: usize, wrap: bool) -> Vec<Vector2> {
    let mut polyline = points.to_vec();
    if wrap {
        polyline.push(points[0]);
    }

    let total_len = polyline_len(&polyline);
    let interval = total_len / n as f64;

    let mut resampled = Vec::with_capacity(n);
    resampled.push(polyline[0]);

    let mut dist_accum = 0.0;
    let mut prev = polyline[0];
    let mut i = 1;

    while i < polyline.len() && resampled.len() < n {
        let seg_len = (polyline[i] - prev).length();

        if dist_accum + seg_len >= interval && seg_len > 0.0 {
            let t = (interval - dist_accum) / seg_len;
            let new_point = prev + (polyline[i] - prev) * t;
            resampled.push(new_point);
            prev = new_point;
            dist_accum = 0.0;
        } else {
            dist_accum += seg_len;
            prev = polyline[i];
            i += 1;
        }
    }

    while resampled.len() < n {
        resampled.push(*polyline.last().unwrap());
    }

    resampled
}

/// The deviation from a line (in document coordinates) that is tolerated for a stroke of the given
/// length, together with the distance within which the jitter of the hand is forgiven.
fn line_dev_tolerance(span: f64, zoom: f64) -> f64 {
    let long = ((span * zoom - LINE_SHORT_SURFACE) / (LINE_LONG_SURFACE - LINE_SHORT_SURFACE))
        .clamp(0.0, 1.0);
    let ratio =
        LINE_SHORT_MAX_DEV_RATIO + (LINE_LONG_MAX_DEV_RATIO - LINE_SHORT_MAX_DEV_RATIO) * long;

    (ratio * span).max(LINE_MIN_DEV_TOLERANCE_SURFACE / zoom)
}

/// An upright stroke that is some bracket, with the canonical ends it is snapped to.
struct BracketStroke {
    /// The start of the bracket, on the vertical the ends are snapped onto.
    canonical_start: Vector2,
    /// The end of the bracket, on the same vertical.
    canonical_end: Vector2,
    /// The height between the ends.
    height: f64,
    /// The length of the chord between the ends.
    chord: f64,
    /// The largest distance between the stroke and its chord.
    bulge: f64,
    /// The side the stroke bulges to, as a unit vector across the chord.
    outward: Vector2,
    /// The stroke, resampled to an even spacing.
    resampled: Vec<Vector2>,
    /// Whether the stroke bulges deepest in the middle of its height, as a bracket does: either a
    /// smooth bend drawn in one sweep, or a straight body turned out at its ends.
    apex_in_the_middle: bool,
}

impl BracketStroke {
    /// `None` when the stroke is not an upright one of the least height, when it bulges to both
    /// sides of its chord, or when it wraps further than a bracket bends.
    fn new(points: &[Vector2], zoom: f64) -> Option<Self> {
        let start = *points.first()?;
        let end = *points.last()?;
        let chord_vec = end - start;
        let chord = chord_vec.length();
        if chord < f64::EPSILON {
            return None;
        }

        // A bracket stands upright.
        let upright = (chord_vec / chord).dot(Vector2::new(0.0, 1.0)).abs();
        if upright < BRACKET_MAX_TILT.cos() {
            return None;
        }
        let height = (end.y - start.y).abs();
        if height * zoom < BRACKET_MIN_HEIGHT_SURFACE {
            return None;
        }

        let resampled = resample(points, N_RESAMPLE, false);
        let hook = (resampled.len() as f64 * BRACKET_HOOK_RATIO).round() as usize;
        if resampled.len() <= 2 * hook + 2 * TURN_WINDOW {
            return None;
        }
        let body = resampled[hook..resampled.len() - hook].to_vec();

        // One bulge only: the stroke stays on one side of its chord. Points close to the chord say
        // nothing about the side, they are the jitter of the hand near the ends.
        let side_tolerance = LINE_MIN_DEV_TOLERANCE_SURFACE / zoom;
        let normal = Vector2::new(-chord_vec.y, chord_vec.x) / chord;
        let mut bulge = 0.0_f64;
        let mut bulge_side = 0.0_f64;
        for point in body.iter() {
            let distance = normal.dot(*point - start);
            if distance.abs() <= side_tolerance {
                continue;
            }
            let side = distance.signum();
            if bulge_side != 0.0 && side != bulge_side {
                return None;
            }
            bulge_side = side;
            bulge = bulge.max(distance.abs());
        }
        if bulge_side == 0.0 || bulge > BRACKET_MAX_BULGE_RATIO * chord {
            return None;
        }

        // The stroke bends where a bracket bends: in the middle. A stroke whose deepest point sits
        // at one of its ends is a hook, or a letter.
        let plateau: Vec<usize> = (0..body.len())
            .filter(|&index| {
                normal.dot(body[index] - start).abs() >= bulge * (1.0 - BRACKET_DEPTH_PLATEAU_SHARE)
            })
            .collect();
        let apex = plateau[plateau.len() / 2];
        let apex_fraction = apex as f64 / (body.len() - 1) as f64;
        let apex_in_the_middle = (apex_fraction - 0.5).abs() <= BRACKET_MAX_APEX_OFFSET;

        // Snap the ends onto one vertical, keep their height.
        let x = (start.x + end.x) * 0.5;
        let outward_x = if bulge_side * normal.x < 0.0 {
            -1.0
        } else {
            1.0
        };

        Some(Self {
            canonical_start: Vector2::new(x, start.y),
            canonical_end: Vector2::new(x, end.y),
            height,
            chord,
            bulge,
            outward: Vector2::new(outward_x, 0.0),
            resampled,
            apex_in_the_middle,
        })
    }

    /// Whether the stroke is a straight run between a corner at each end.
    fn has_corners_at_the_ends(&self, zoom: f64) -> bool {
        has_corners_at_the_ends(&self.resampled, zoom, self.chord)
    }
}

/// Recognize an upright stroke the round bracket gesture draws and snap it to a bracket.
///
/// The stroke is drawn in one sweep, deepest in the middle of its height — a smooth bend or a
/// straight body turned out at its ends. What it becomes depends on [`BracketOptions::style`]: an
/// arc keeps the drawn bulge, the sagitta of the circular arc the bracket is snapped to; a rounded
/// parenthesis has a straight body, its depth the configured share of the height, turned out into
/// short tips at the ends; a square bracket has the same straight body, its depth a fixed share of
/// the height, turned out at right angles. A stroke drawn with a sharp corner at its ends is a
/// square bracket whichever style is set, with a gesture of its own, and never reaches this.
///
/// The bulge is the largest distance between the stroke and its chord.
fn recognize_bracket(
    points: &[Vector2],
    zoom: f64,
    options: &BracketOptions,
) -> Option<(Shape, BracketKind)> {
    let stroke = BracketStroke::new(points, zoom)?;

    // A stroke the line recognizer accepts is a line, not a shallow parenthesis. A stroke whose
    // deepest point sits at one of its ends is a hook or a letter: a parenthesis bulges deepest in
    // the middle of its height. How its ends turn out of it, smoothly or at a sharp corner, says
    // nothing here: a square bracket has been told apart from a parenthesis by then.
    if !stroke.apex_in_the_middle || stroke.bulge <= line_dev_tolerance(stroke.chord, zoom) {
        return None;
    }

    match options.style {
        ParenthesisStyle::Arc => {
            let arc = CubicBezier::new_circular_arc(
                stroke.canonical_start,
                stroke.canonical_end,
                stroke.bulge,
                stroke.outward,
            )?;
            Some((Shape::CubicBezier(arc), BracketKind::from(options.style)))
        }
        ParenthesisStyle::Rounded => {
            let radius = options.corner_radius_ratio.clamp(
                BRACKET_CORNER_RADIUS_RATIO_MIN,
                BRACKET_CORNER_RADIUS_RATIO_MAX,
            ) * stroke.height;
            let bracket = Curve::new_rounded_bracket(
                stroke.canonical_start,
                stroke.canonical_end,
                radius,
                stroke.outward,
            )?;
            Some((Shape::Curve(bracket), BracketKind::from(options.style)))
        }
        ParenthesisStyle::Square => {
            let bracket = Curve::new_square_bracket(
                stroke.canonical_start,
                stroke.canonical_end,
                SQUARE_DEPTH_RATIO * stroke.height,
                stroke.outward,
            )?;
            Some((Shape::Curve(bracket), BracketKind::SquareBracket))
        }
    }
}

/// Recognize an upright stroke drawn as a square bracket: a straight body between two corners.
///
/// The corners are the tips the bracket is drawn from: a stroke turns sharply at both of its ends,
/// the same way, and runs exactly straight between them. A smooth bend has no such corners, and a
/// stroke with a hook at one end only turns at one end.
///
/// The depth is a fixed share of the height, so the bracket is as deep as it is set in type
/// however shallow it was drawn.
fn recognize_square_bracket(points: &[Vector2], zoom: f64) -> Option<(Shape, BracketKind)> {
    let stroke = BracketStroke::new(points, zoom)?;
    if stroke.bulge * zoom < BRACKET_MIN_DEPTH_SURFACE || !stroke.has_corners_at_the_ends(zoom) {
        return None;
    }
    // The corners turn at the corner: a bracket whose ends are rounded turns gradually through an
    // arc there, and is a parenthesis drawn by the other gesture.
    if sharpest_turn(&stroke.resampled) < SQUARE_CORNER_MIN_TURN {
        return None;
    }

    let bracket = Curve::new_square_bracket(
        stroke.canonical_start,
        stroke.canonical_end,
        SQUARE_DEPTH_RATIO * stroke.height,
        stroke.outward,
    )?;

    Some((Shape::Curve(bracket), BracketKind::SquareBracket))
}

/// Recognize an open stroke drawn as a curly brace.
///
/// A brace is the one open stroke with a cusp: where a parenthesis turns smoothly all along its
/// height, a brace turns sharply at the point where its two arms meet, and that point sits in the
/// middle of the height. The side the cusp points to is the side the brace closes towards: left
/// is `{`, right is `}`.
///
/// The stroke is snapped to a brace of the height it was drawn, on the side of the cusp, in the
/// representation [`BraceStyle`] asks for: the brace of type, the same with a rounded cusp, or the
/// same with right angles.
fn recognize_curly_brace(
    points: &[Vector2],
    zoom: f64,
    options: &BracketOptions,
) -> Option<(Shape, BracketKind)> {
    let start = *points.first()?;
    let end = *points.last()?;
    let chord_vec = end - start;
    let chord = chord_vec.length();
    if chord < f64::EPSILON {
        return None;
    }

    // A brace stands upright.
    let upright = (chord_vec / chord).dot(Vector2::new(0.0, 1.0)).abs();
    if upright < BRACKET_MAX_TILT.cos() {
        return None;
    }
    let height = (end.y - start.y).abs();
    if height * zoom < BRACKET_MIN_HEIGHT_SURFACE {
        return None;
    }

    let resampled = resample(points, N_RESAMPLE, false);
    let hook = ((resampled.len() as f64 * CURLY_HOOK_RATIO).round() as usize).max(TURN_WINDOW + 1);
    if resampled.len() <= 2 * hook + 2 {
        return None;
    }
    let body = &resampled[hook..resampled.len() - hook];
    let window = ((body.len() as f64 * CURLY_CUSP_WINDOW).round() as usize).max(TURN_WINDOW);
    if body.len() <= 2 * window {
        return None;
    }

    // One tip curls back over the chord: a `V` runs straight into its point at both ends. The other
    // end of a brace is often the long tail the pen swept out and lifted from, and does not curl.
    let tip_turn = turn_through(&resampled[..hook])
        .abs()
        .max(turn_through(&resampled[resampled.len() - hook..]).abs());
    if tip_turn < CURLY_TIP_MIN_TURN {
        return None;
    }

    // The cusp is the one sharp turn of the stroke, and a brace turns at its point in the middle of
    // its height: a stroke that turns sharply at one of its ends is a hook or a letter.
    let mut cusp: Option<(usize, f64)> = None;
    for index in window..body.len() - window {
        let share = (body[index].y - start.y) / (end.y - start.y);
        if !(CURLY_CUSP_EDGE_SHARE..=1.0 - CURLY_CUSP_EDGE_SHARE).contains(&share) {
            continue;
        }
        let turn = turn_through(&body[index - window..=index + window]).abs();
        if cusp.is_none_or(|(_, sharpest)| turn > sharpest) {
            cusp = Some((index, turn));
        }
    }
    let (index, turn) = cusp?;
    if turn < CURLY_CUSP_MIN_TURN {
        return None;
    }

    // The point sticks out from the chord, and deeper than a line of that length wanders: a stroke
    // that wraps further than a brace bends back towards itself, which is a hook.
    let normal = Vector2::new(-chord_vec.y, chord_vec.x) / chord;
    let depth = normal.dot(body[index] - start);
    if depth.abs() * zoom < BRACKET_MIN_DEPTH_SURFACE {
        return None;
    }
    if depth.abs() > CURLY_MAX_DEPTH_RATIO * chord {
        return None;
    }

    // Snap: the tips onto one vertical, the glyph a canonical depth to the side the cusp points to.
    let x = (start.x + end.x) * 0.5;
    let outward = if depth < 0.0 { -normal } else { normal };
    let snapped_start = Vector2::new(x, start.y);
    let snapped_end = Vector2::new(x, end.y);

    let depth = CURLY_DEPTH_RATIO * height;
    let brace = match options.brace {
        BraceStyle::Curly => Curve::new_curly_brace(snapped_start, snapped_end, depth, outward),
        BraceStyle::Rounded => Curve::new_rounded_brace(snapped_start, snapped_end, depth, outward),
        BraceStyle::Square => Curve::new_square_brace(snapped_start, snapped_end, depth, outward),
    }?;

    Some((Shape::Curve(brace), BracketKind::from(options.brace)))
}

/// Whether the stroke is a straight run between a corner at each end, as a tall bracket is drawn.
///
/// Such a bracket is a nearly vertical body with a short turn at each end, where it comes out of
/// its tip. Its middle is exactly straight, which no smooth bend is, and both corners turn the same
/// way, which tells it from a stroke with a hook at one end only, like a letter.
fn has_corners_at_the_ends(stroke: &[Vector2], zoom: f64, chord: f64) -> bool {
    let corner = (stroke.len() as f64 * BRACKET_CORNER_SHARE).round() as usize;
    if corner < TURN_WINDOW + 1 || stroke.len() <= 2 * corner {
        return false;
    }

    let middle = &stroke[corner..stroke.len() - corner];
    let middle_start = middle[0];
    let middle_end = *middle.last().unwrap();
    let straight_tolerance =
        (BRACKET_STRAIGHT_MAX_DEV_RATIO * chord).max(BRACKET_STRAIGHT_TOLERANCE_SURFACE / zoom);
    if middle
        .iter()
        .any(|point| dist_to_segment(*point, middle_start, middle_end) > straight_tolerance)
    {
        return false;
    }

    let start_turn = turn_through(&stroke[..=corner]);
    let end_turn = turn_through(&stroke[stroke.len() - 1 - corner..]);
    start_turn.abs() >= BRACKET_CORNER_MIN_TURN
        && end_turn.abs() >= BRACKET_CORNER_MIN_TURN
        && start_turn.signum() == end_turn.signum()
}

/// The sharpest turn, in radians, the stroke makes at any one of its points.
fn sharpest_turn(points: &[Vector2]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }

    (1..points.len() - 1)
        .map(|i| turn_through(&points[i - 1..=i + 1]).abs())
        .fold(0.0, f64::max)
}

/// The signed turn, in radians, the stroke makes from its first point to its last.
fn turn_through(points: &[Vector2]) -> f64 {
    let mut total = 0.0;
    let mut previous: Option<Vector2> = None;

    for segment in points.windows(2) {
        let direction = segment[1] - segment[0];
        if direction.length() < f64::EPSILON {
            continue;
        }
        let direction = direction.normalize();
        if let Some(previous) = previous {
            total += previous.angle_to(direction);
        }
        previous = Some(direction);
    }

    total
}

/// Recognize an open stroke as a line.
///
/// Short strokes have to be nearly straight, since they could as well be parts of letters. The
/// longer a stroke, the more forgiving this gets: the hooks at its ends are left out, so that they
/// do not tilt the line it is measured from, and it may wander further from that line.
fn recognize_line(points: &[Vector2], perimeter: f64, zoom: f64) -> Option<Shape> {
    let first = *points.first().unwrap();
    let last = *points.last().unwrap();
    let long = (((last - first).length() * zoom - LINE_SHORT_SURFACE)
        / (LINE_LONG_SURFACE - LINE_SHORT_SURFACE))
        .clamp(0.0, 1.0);
    let lerp = |short: f64, long_value: f64| short + (long_value - short) * long;

    let resampled = resample(points, N_RESAMPLE, false);
    let hook = (resampled.len() as f64 * LINE_LONG_HOOK_RATIO * long).round() as usize;
    let body = &resampled[hook..resampled.len() - hook];
    let (body_first, body_last) = (*body.first().unwrap(), *body.last().unwrap());
    let span = (body_last - body_first).length();
    if span < f64::EPSILON {
        return None;
    }

    // On long strokes measured over points some distance apart, so that the tremor of the hand on a
    // slowly drawn line does not add up to any length. A stroke that turns back still does.
    let spacing = lerp(1.0, LINE_LENGTH_SPACING_SURFACE) / zoom;
    let spaced = resample(
        body,
        ((perimeter / spacing).round() as usize).clamp(2, N_RESAMPLE),
        false,
    );
    let len_excess_tolerance = (lerp(
        LINE_SHORT_MAX_LEN_EXCESS_RATIO,
        LINE_LONG_MAX_LEN_EXCESS_RATIO,
    ) * span)
        .max(LINE_MIN_LEN_EXCESS_TOLERANCE_SURFACE / zoom);
    if polyline_len(&spaced) - span > len_excess_tolerance {
        return None;
    }

    let direction = (body_last - body_first) / span;
    let max_dev = body
        .iter()
        .map(|p| direction.perp_dot(*p - body_first).abs())
        .fold(0.0, f64::max);
    if max_dev > line_dev_tolerance(span, zoom) {
        return None;
    }

    // The hooks are only left out of judging the stroke: the line starts where the pen landed and
    // ends where the pen is
    Some(Shape::Line(Line {
        start: first,
        end: last,
    }))
}

/// Recognize an open stroke as a curve, a chain of cubic segments.
///
/// The first and the last segment start and end where the stroke does, so the curve keeps where the
/// stroke started and ended. Between them the curve is built through the stroke simplified to
/// [`CURVE_MAX_KNOTS`] points, which removes the jitter of the hand and leaves the shape: the curve
/// is a cleaning of the stroke, not a template it is snapped to.
fn recognize_curve(points: &[Vector2], zoom: f64) -> Option<Shape> {
    let start = *points.first()?;
    let end = *points.last()?;
    let chord = (end - start).length();
    if chord < f64::EPSILON {
        return None;
    }

    let resampled = resample(points, N_RESAMPLE, false);
    // A corner is a handwriting stroke and not a bend of a curve.
    if max_open_turn(&resampled) > CURVE_MAX_TURN {
        return None;
    }

    // A stroke that bends once is one clean bend, not a chain of segments through it.
    if let Some(bend) = recognize_bend_curve(&resampled, start, end, zoom) {
        return Some(bend);
    }

    // The resampled stroke stops one spacing short of its end, so the curve takes the stroke's own
    // endpoints: it has to start where the pen landed and end where it was lifted.
    let mut knots = curve_knots(&resampled, zoom)?;
    let last = knots.len() - 1;
    knots[0] = start;
    knots[last] = end;
    let curve = Curve::new_through_points(&knots)?;

    // The curve passes through the knots, but between them it may leave the stroke. Measure it
    // against the stroke, so that a stroke the knots describe badly stays as it was drawn.
    let samples = curve
        .cubics
        .iter()
        .flat_map(|cubic| {
            (0..=CURVE_SAMPLES_PER_CUBIC).map(move |i| {
                cubic_point(
                    cubic.start,
                    cubic.cp1,
                    cubic.cp2,
                    cubic.end,
                    i as f64 / CURVE_SAMPLES_PER_CUBIC as f64,
                )
            })
        })
        .collect::<Vec<_>>();
    let deviation = resampled
        .iter()
        .map(|point| {
            samples
                .iter()
                .map(|sample| (*sample - *point).length())
                .fold(f64::MAX, f64::min)
        })
        .fold(0.0, f64::max);
    let tolerance = (CURVE_MAX_DEV_RATIO * chord).max(CURVE_MIN_TOLERANCE_SURFACE / zoom);
    (deviation <= tolerance).then_some(Shape::Curve(curve))
}

/// Recognize an open stroke that bends once as a single quadratic.
///
/// A stroke that bends once — a swoosh, an arch, a shallow `C` — is neither a line nor a bracket:
/// it is one smooth bend, and snapping it to a quadratic cleans it into a careful curve. The stroke
/// has to stay on one side of its chord: a stroke that crosses the line between its ends bends
/// twice, and one bend would be a different shape than the one drawn.
fn recognize_bend_curve(
    points: &[Vector2],
    start: Vector2,
    end: Vector2,
    zoom: f64,
) -> Option<Shape> {
    let chord_vec = end - start;
    let chord = chord_vec.length();
    if chord < f64::EPSILON {
        return None;
    }
    let normal = Vector2::new(-chord_vec.y, chord_vec.x) / chord;

    // The side the stroke is on. Points close to the chord say nothing about it, and a stroke that
    // is on neither side is a straight stroke, which is a line and not a bend.
    let side_tolerance = BEND_SIDE_TOLERANCE_SURFACE / zoom;
    let mut side = 0.0;
    for point in points {
        let distance = normal.dot(*point - start);
        if distance.abs() <= side_tolerance {
            continue;
        }
        if side != 0.0 && distance.signum() != side {
            return None;
        }
        side = distance.signum();
    }
    if side == 0.0 {
        return None;
    }

    // The quadratic passes through the point of the bend furthest from the chord, which is where the
    // tangent runs parallel to the chord, and that is the middle of the quadratic: the control point
    // follows from it directly, so the stroke is not distorted by a fit that could move the bend.
    let apex = points
        .iter()
        .map(|point| (normal.dot(*point - start).abs(), point))
        .fold((0.0, &points[0]), |worst, point| {
            if point.0 > worst.0 { point } else { worst }
        })
        .1;
    let bend = QuadraticBezier {
        start,
        cp: *apex * 2.0 - (start + end) * 0.5,
        end,
    };

    // How far the stroke is from the bend, measured the way the chain of segments is measured: a
    // bend the stroke only roughly follows would pull it somewhere the pen never went.
    const SAMPLES: usize = 32;
    let samples = (0..=SAMPLES)
        .map(|i| quadbez_calc(bend.start, bend.cp, bend.end, i as f64 / SAMPLES as f64))
        .collect::<Vec<_>>();
    let deviation = points
        .iter()
        .map(|point| {
            samples
                .windows(2)
                .map(|segment| dist_to_segment(*point, segment[0], segment[1]))
                .fold(f64::MAX, f64::min)
        })
        .fold(0.0, f64::max);
    let tolerance = (BEND_MAX_DEV_RATIO * chord).max(BEND_MIN_TOLERANCE_SURFACE / zoom);
    (deviation <= tolerance).then_some(Shape::QuadraticBezier(bend))
}

/// The points an open curve is built through.
///
/// The stroke is simplified until it has few enough points, raising the tolerance each pass: a
/// stroke that still wiggles at that tolerance is not a smooth curve.
fn curve_knots(points: &[Vector2], zoom: f64) -> Option<Vec<Vector2>> {
    let mut tolerance = (CURVE_KNOT_TOLERANCE_SURFACE / zoom).max(f64::EPSILON);

    for _ in 0..CURVE_KNOT_PASSES {
        let knots = simplify(points, tolerance);
        if knots.len() <= CURVE_MAX_KNOTS {
            return Some(knots);
        }
        tolerance *= 1.5;
    }

    None
}

/// Simplify a polyline with the Douglas-Peucker algorithm.
///
/// Every point left out is within `tolerance` of the line between the two points that are kept
/// around it.
fn simplify(points: &[Vector2], tolerance: f64) -> Vec<Vector2> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut ranges = vec![(0, points.len() - 1)];

    while let Some((first, last)) = ranges.pop() {
        let (mut worst, mut index) = (0.0, first);
        for i in first + 1..last {
            let distance = dist_to_segment(points[i], points[first], points[last]);
            if distance > worst {
                worst = distance;
                index = i;
            }
        }
        if worst > tolerance {
            keep[index] = true;
            ranges.push((first, index));
            ranges.push((index, last));
        }
    }

    points
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(point, _)| *point)
        .collect()
}

/// The largest turn angle along an open polyline, measured over [`TURN_WINDOW`] points.
fn max_open_turn(points: &[Vector2]) -> f64 {
    if points.len() <= 2 * TURN_WINDOW {
        return 0.0;
    }

    (TURN_WINDOW..points.len() - TURN_WINDOW)
        .map(|i| {
            let v_in = points[i] - points[i - TURN_WINDOW];
            let v_out = points[i + TURN_WINDOW] - points[i];

            if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
                0.0
            } else {
                v_in.angle_to(v_out).abs()
            }
        })
        .fold(0.0, f64::max)
}

/// The point on a cubic bezier curve at the given parameter.
fn cubic_point(start: Vector2, cp1: Vector2, cp2: Vector2, end: Vector2, t: f64) -> Vector2 {
    let mt = 1.0 - t;
    start * (mt * mt * mt)
        + cp1 * (3.0 * mt * mt * t)
        + cp2 * (3.0 * mt * t * t)
        + end * (t * t * t)
}

/// Smooth a closed resampled loop with a cyclic three-point moving average.
///
/// Suppresses input noise before measuring turn angles while mostly preserving corners.
fn smooth_loop(points: &[Vector2], iterations: usize) -> Vec<Vector2> {
    let n = points.len();
    let mut smoothed = points.to_vec();

    for _ in 0..iterations {
        let prev = smoothed.clone();
        for i in 0..n {
            smoothed[i] = (prev[(i + n - 1) % n] + prev[i] * 2.0 + prev[(i + 1) % n]) * 0.25;
        }
    }

    smoothed
}

/// Recognize a closed stroke as an ellipse, rectangle or polygon.
fn recognize_closed(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    let resampled = resample(points, N_RESAMPLE, true);
    // The ellipse is fitted to what was drawn and not to the segment that closes the loop: on a
    // loop the pen did not close, that segment is a straight chord the pen never drew, and it
    // pulls the fit off the shape the stroke actually has.
    let drawn = resample(points, N_RESAMPLE, false);
    let smoothed = smooth_loop(&resampled, 2);
    let turns = loop_turn_angles(&smoothed);

    let total_turning: f64 = turns.iter().sum();
    if total_turning < f64::EPSILON {
        return None;
    }
    let mean_turn = total_turning / turns.len() as f64;

    // The fraction of the total turning concentrated in high-turn samples.
    // Near 0 for smooth curves (ellipses), near 1 for polygons.
    let cornerness: f64 =
        turns.iter().filter(|&&t| t > 1.5 * mean_turn).sum::<f64>() / total_turning;

    if cornerness < CORNERNESS_POLYGON_THRESHOLD {
        recognize_ellipse(&drawn)
    } else {
        let corners = detect_corners(&resampled, &turns, mean_turn);

        (3..=POLYGON_MAX_CORNERS)
            .contains(&corners.len())
            .then(|| recognize_polygon(&resampled, &corners, bounds_diag))
            .flatten()
            // Flat ellipses concentrate their turning at the high-curvature ends of the
            // major axis and can end up here, so fall back to the ellipse fit.
            .or_else(|| recognize_ellipse(&drawn))
    }
}

/// The turn angles at each point of a closed resampled loop.
///
/// The turn angle at a point is the angle between the chords leading to and away from it,
/// measured over a window of [`TURN_WINDOW`] points.
fn loop_turn_angles(points: &[Vector2]) -> Vec<f64> {
    let n = points.len();

    (0..n)
        .map(|i| {
            let prev = points[(i + n - TURN_WINDOW) % n];
            let next = points[(i + TURN_WINDOW) % n];
            let v_in = points[i] - prev;
            let v_out = next - points[i];

            if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
                0.0
            } else {
                v_in.angle_to(v_out).abs()
            }
        })
        .collect()
}

/// Detect the corner indices of a closed resampled loop from its turn angles.
///
/// Corners are local turning maxima above a threshold, with clusters of
/// adjacent high-turn points merged into a single corner.
fn detect_corners(points: &[Vector2], turns: &[f64], mean_turn: f64) -> Vec<Vector2> {
    let n = turns.len();
    let threshold = (1.8 * mean_turn).max(CORNER_MIN_TURN);

    let mut candidates = (0..n).filter(|&i| turns[i] > threshold).collect::<Vec<_>>();
    // Sort by turn angle, strongest first, then greedily suppress neighbors.
    candidates.sort_by(|&a, &b| turns[b].total_cmp(&turns[a]));

    let mut corners: Vec<usize> = Vec::new();
    for i in candidates {
        let cyclic_dist = |a: usize, b: usize| {
            let d = a.abs_diff(b);
            d.min(n - d)
        };
        if corners
            .iter()
            .all(|&c| cyclic_dist(c, i) >= CORNER_MIN_SEPARATION)
        {
            corners.push(i);
        }
    }

    corners.sort_unstable();
    corners.into_iter().map(|i| points[i]).collect()
}

/// An ellipse fitted to the points of a stroke.
#[derive(Debug, Clone, Copy)]
struct EllipseFit {
    /// The center of the ellipse.
    center: Vector2,
    /// The tilt of the major axis.
    angle: f64,
    /// The semi-axes, along the major and the minor axis.
    radii: Vector2,
    /// How far the points stray from the outline, as a share of the radius at the point: the mean
    /// of the strays, and the largest of them.
    mean_residual: f64,
    max_residual: f64,
}

/// How many Gauss-Newton steps an ellipse fit is refined with.
const ELLIPSE_FIT_STEPS: usize = 32;
/// The largest change a single refinement step may make to any of its parameters, whether it is a
/// coordinate, a logarithm of a radius or an angle.
const ELLIPSE_FIT_MAX_STEP: f64 = 4.0;

/// Fit an ellipse to the points of a stroke.
///
/// The starting point is the fit by the second moments, which is exact for points spread evenly
/// around a whole ellipse. On a loop the pen did not close it is not: the missing part pulls the
/// center into the drawn arc, and the radii with it, so that a stroke drawn round reads as far
/// less round than it is. A few Gauss-Newton steps then move the center, the radii and the tilt to
/// where the points sit closest to the outline, which is where a hand-drawn circle belongs
/// whatever share of it was drawn.
fn fit_ellipse(points: &[Vector2]) -> Option<EllipseFit> {
    let n = points.len() as f64;
    if n < 3.0 {
        return None;
    }
    let center = points.iter().copied().sum::<Vector2>() / n;

    // Covariance matrix of the centered points.
    let (mut cxx, mut cxy, mut cyy) = (0.0, 0.0, 0.0);
    for p in points {
        let d = *p - center;
        cxx += d.x * d.x;
        cxy += d.x * d.y;
        cyy += d.y * d.y;
    }
    cxx /= n;
    cxy /= n;
    cyy /= n;

    // Eigenvalues/-vectors of the symmetric covariance matrix.
    let trace_half = (cxx + cyy) * 0.5;
    let det = cxx * cyy - cxy * cxy;
    let discr = (trace_half * trace_half - det).max(0.0).sqrt();
    let (lambda1, lambda2) = (trace_half + discr, trace_half - discr);
    if lambda2 <= f64::EPSILON {
        return None;
    }

    let major_axis = if cxy.abs() > f64::EPSILON {
        Vector2::new(lambda1 - cyy, cxy).normalize()
    } else if cxx >= cyy {
        Vector2::X
    } else {
        Vector2::Y
    };

    // For points distributed on an ellipse outline, the variance along an axis is ~half the
    // squared semi-axis. The radii are refined in log, so that a step can never cross zero.
    let mut params = [
        center.x,
        center.y,
        (2.0 * lambda1).sqrt().max(f64::MIN_POSITIVE).ln(),
        (2.0 * lambda2).sqrt().max(f64::MIN_POSITIVE).ln(),
        Vector2::X.angle_to(major_axis),
    ];

    let mut sse = ellipse_fit_sse(points, &params);
    for _ in 0..ELLIPSE_FIT_STEPS {
        let (a, b) = (params[2].exp(), params[3].exp());
        let (sin, cos) = params[4].sin_cos();
        let (major, minor) = (Vector2::new(cos, sin), Vector2::new(-sin, cos));
        let center = Vector2::new(params[0], params[1]);
        let (mut normal, mut gradient) = ([[0.0f64; 5]; 5], [0.0f64; 5]);

        for p in points {
            let d = *p - center;
            let u = d.dot(major) / a;
            let v = d.dot(minor) / b;
            let r = (u * u + v * v).sqrt();
            if r < f64::EPSILON {
                continue;
            }
            let residual = r - 1.0;
            let jacobian = [
                -(u * major.x + v * minor.x) / (r * a),
                -(u * major.y + v * minor.y) / (r * b),
                -u * u / r,
                -v * v / r,
                u * v * (b / a - a / b) / r,
            ];
            for i in 0..5 {
                gradient[i] += jacobian[i] * residual;
                for k in 0..5 {
                    normal[i][k] += jacobian[i] * jacobian[k];
                }
            }
        }

        // A stroke that is not a curve at all can point the fit far away in a single step, so the
        // diagonal is damped and the step is limited.
        let damping = 1e-9 * (0..5).map(|i| normal[i][i]).sum::<f64>();
        for (i, row) in normal.iter_mut().enumerate() {
            row[i] += damping;
        }
        let step = solve_linear(
            normal,
            [
                -gradient[0],
                -gradient[1],
                -gradient[2],
                -gradient[3],
                -gradient[4],
            ],
        )?;
        let largest = step.iter().fold(0.0f64, |largest, s| largest.max(s.abs()));
        let scale = if largest > ELLIPSE_FIT_MAX_STEP {
            ELLIPSE_FIT_MAX_STEP / largest
        } else {
            1.0
        };
        let mut stepped = params;
        for (param, s) in stepped.iter_mut().zip(step) {
            *param += s * scale;
        }
        // A step that does not bring the points closer to the outline is the end of the
        // refinement; keeping the fit that was there is always better than taking it.
        let stepped_sse = ellipse_fit_sse(points, &stepped);
        if stepped_sse >= sse {
            break;
        }
        params = stepped;
        let converged = largest * scale < 1e-9;
        sse = stepped_sse;
        if converged {
            break;
        }
    }

    let (mut a, mut b) = (params[2].exp(), params[3].exp());
    let mut angle = params[4];
    // Keep the semi-axis along the major direction first, as the fit starts out with it: the
    // refinement can turn the axes by a quarter turn, and an ellipse that names its minor axis as
    // the major one would read as a circle.
    if b > a {
        std::mem::swap(&mut a, &mut b);
        angle += std::f64::consts::FRAC_PI_2;
    }
    let (sin, cos) = angle.sin_cos();
    let (major, minor) = (Vector2::new(cos, sin), Vector2::new(-sin, cos));
    let center = Vector2::new(params[0], params[1]);

    let normalized_radii = points
        .iter()
        .map(|p| {
            let d = *p - center;
            let u = d.dot(major) / a;
            let v = d.dot(minor) / b;
            (u * u + v * v).sqrt()
        })
        .collect::<Vec<f64>>();

    // The residuals are measured against the radius the points actually have, so that a fit that
    // sits a little small reads as a shape that is as round as it was drawn.
    let scale = normalized_radii.iter().sum::<f64>() / n;
    if scale < f64::EPSILON {
        return None;
    }

    let mean_residual = normalized_radii
        .iter()
        .map(|r| (r / scale - 1.0).abs())
        .sum::<f64>()
        / n;
    let max_residual = normalized_radii
        .iter()
        .map(|r| (r / scale - 1.0).abs())
        .fold(0.0, f64::max);

    Some(EllipseFit {
        center,
        angle,
        radii: Vector2::new(a * scale, b * scale),
        mean_residual,
        max_residual,
    })
}

/// The sum of the squared deviations of the points from the ellipse the parameters describe.
fn ellipse_fit_sse(points: &[Vector2], params: &[f64; 5]) -> f64 {
    let (a, b) = (params[2].exp(), params[3].exp());
    let (sin, cos) = params[4].sin_cos();
    let (major, minor) = (Vector2::new(cos, sin), Vector2::new(-sin, cos));
    let center = Vector2::new(params[0], params[1]);

    points
        .iter()
        .map(|p| {
            let d = *p - center;
            let u = d.dot(major) / a;
            let v = d.dot(minor) / b;
            let r = (u * u + v * v).sqrt();
            (r - 1.0) * (r - 1.0)
        })
        .sum()
}

/// Recognize a closed smooth stroke as an ellipse (or circle).
fn recognize_ellipse(points: &[Vector2]) -> Option<Shape> {
    let fit = fit_ellipse(points)?;

    if fit.mean_residual > ELLIPSE_MAX_MEAN_RESIDUAL || fit.max_residual > ELLIPSE_MAX_RESIDUAL {
        return None;
    }

    let (radii, angle) = if fit.radii.y / fit.radii.x > CIRCLE_RADII_RATIO {
        // Close to a circle, snap to one.
        let r = (fit.radii.x + fit.radii.y) * 0.5;
        (Vector2::new(r, r), 0.0)
    } else {
        (fit.radii, fit.angle)
    };

    Some(Shape::Ellipse(Ellipse {
        radii,
        affine: DAffine2::from_angle_translation(angle, fit.center),
    }))
}

/// Solve a small linear system with partial pivoting, `None` when it is singular.
fn solve_linear(mut matrix: [[f64; 5]; 5], mut rhs: [f64; 5]) -> Option<[f64; 5]> {
    for col in 0..5 {
        let pivot =
            (col..5).max_by(|&a, &b| matrix[a][col].abs().total_cmp(&matrix[b][col].abs()))?;
        if matrix[pivot][col].abs() < f64::EPSILON {
            return None;
        }
        matrix.swap(col, pivot);
        rhs.swap(col, pivot);

        for row in (col + 1)..5 {
            let factor = matrix[row][col] / matrix[col][col];
            for k in col..5 {
                matrix[row][k] -= factor * matrix[col][k];
            }
            rhs[row] -= factor * rhs[col];
        }
    }

    let mut solution = [0.0; 5];
    for row in (0..5).rev() {
        let mut sum = rhs[row];
        for (k, s) in solution.iter().enumerate().skip(row + 1) {
            sum -= matrix[row][k] * s;
        }
        solution[row] = sum / matrix[row][row];
    }
    Some(solution)
}

/// Recognize a closed stroke with detected corners as a rectangle or polygon.
fn recognize_polygon(
    resampled: &[Vector2],
    corners: &[Vector2],
    bounds_diag: f64,
) -> Option<Shape> {
    // Verify that the stroke stays close to the polygon outline through the corners.
    let mut dev_sum = 0.0;
    let mut dev_max: f64 = 0.0;
    for p in resampled {
        let dist = (0..corners.len())
            .map(|i| dist_to_segment(*p, corners[i], corners[(i + 1) % corners.len()]))
            .fold(f64::MAX, f64::min);
        dev_sum += dist;
        dev_max = dev_max.max(dist);
    }
    let dev_mean = dev_sum / resampled.len() as f64;

    if dev_mean > POLYGON_MAX_MEAN_DEV_RATIO * bounds_diag
        || dev_max > POLYGON_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    if corners.len() == 4
        && let Some(rect) = try_rectangle(corners)
    {
        return Some(Shape::Rectangle(rect));
    }

    Some(Shape::Polygon(Polygon {
        start: corners[0],
        path: corners[1..].to_vec(),
    }))
}

/// The distance of a point to a line segment.
fn dist_to_segment(p: Vector2, a: Vector2, b: Vector2) -> f64 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < f64::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Try to build a rectangle out of four corners.
///
/// Succeeds when all corner angles are close to right angles.
/// The rectangle orientation is the circular mean of the edge directions,
/// snapped to be axis-aligned when it is close to it.
fn try_rectangle(corners: &[Vector2]) -> Option<Rectangle> {
    debug_assert_eq!(corners.len(), 4);

    // All corner angles must be close to right angles.
    for i in 0..4 {
        let prev = corners[(i + 3) % 4];
        let next = corners[(i + 1) % 4];
        let v_in = corners[i] - prev;
        let v_out = next - corners[i];

        if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
            return None;
        }

        let angle = v_in.angle_to(v_out).abs();
        if (angle - std::f64::consts::FRAC_PI_2).abs() > RECT_MAX_ANGLE_DEV {
            return None;
        }
    }

    // Circular mean of the edge directions with period pi/2.
    let (mut sin_sum, mut cos_sum) = (0.0, 0.0);
    for i in 0..4 {
        let edge = corners[(i + 1) % 4] - corners[i];
        let angle = edge.y.atan2(edge.x) * 4.0;
        sin_sum += angle.sin() * edge.length();
        cos_sum += angle.cos() * edge.length();
    }
    let mut orientation = sin_sum.atan2(cos_sum) / 4.0;
    if orientation.abs() < RECT_AXIS_SNAP_ANGLE
        || (orientation.abs() - std::f64::consts::FRAC_PI_4).abs() < RECT_AXIS_SNAP_ANGLE
    {
        orientation =
            (orientation / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    }

    // Bounds of the corners in the rotated frame determine center and extents.
    let rot_inv = DAffine2::from_angle(-orientation);
    let local = corners
        .iter()
        .map(|c| rot_inv.transform_point2(*c))
        .collect::<Vec<Vector2>>();
    let (local_min, local_max) = points_bounds(&local);

    let half_extents = (local_max - local_min) * 0.5;
    let local_center = (local_min + local_max) * 0.5;
    let center = DAffine2::from_angle(orientation).transform_point2(local_center);

    Some(Rectangle {
        cuboid: Cuboid::new(half_extents),
        affine: DAffine2::from_angle_translation(orientation, center),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::penpath::Element;

    fn pen_path_from_points(points: impl IntoIterator<Item = Vector2>) -> PenPath {
        PenPath::try_from_elements(points.into_iter().map(|p| Element::new(p, 0.5))).unwrap()
    }

    // Deterministic pseudo-random jitter to simulate hand wobble.
    fn jitter(i: usize, magnitude: f64) -> Vector2 {
        let a = ((i as f64) * 12.9898).sin() * 43758.5453;
        let b = ((i as f64) * 78.233).sin() * 24634.6345;
        Vector2::new(a.fract(), b.fract()) * magnitude
    }

    /// A path through the given positions, each with its own pressure.
    fn pressured_path(points: impl IntoIterator<Item = (Vector2, f64)>) -> PenPath {
        PenPath::try_from_elements(
            points
                .into_iter()
                .map(|(p, pressure)| Element::new(p, pressure)),
        )
        .unwrap()
    }

    #[test]
    fn mean_width_of_a_steady_stroke_is_its_width() {
        let path =
            pressured_path((0..=20).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.8)));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!((pressure - 0.8).abs() < 1e-9, "{pressure}");
    }

    #[test]
    fn mean_width_is_weighted_by_length_and_not_by_event_count() {
        // A long quick stretch at 0.8, then a short slow stretch at 0.2 that reports ten times as
        // often. Counting events would call this stroke light, but nearly all of the ink is heavy.
        let quick = (0..=4).map(|i| (Vector2::new(100.0 + 50.0 * i as f64, 100.0), 0.8));
        let slow = (1..=40).map(|i| (Vector2::new(300.0 + 0.25 * i as f64, 100.0), 0.2));
        let path = pressured_path(quick.chain(slow));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!(
            pressure > 0.7,
            "expected the long heavy stretch to win, got {pressure}"
        );
    }

    #[test]
    fn mean_width_is_the_mean_of_the_widths_and_not_the_width_of_the_mean_pressure() {
        // Half of the stroke at the lightest pressure and half at the hardest, with a width that
        // goes with the square of the pressure. Its mean pressure is 0.5, which would be a width
        // of 0.25, but the stroke was on average a good deal wider than that.
        let light = (0..=10).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.0));
        let hard = (1..=10).map(|i| (Vector2::new(150.0 + 5.0 * i as f64, 100.0), 1.0));
        let path = pressured_path(light.chain(hard));

        let width = mean_width(&path, 1.0, |pressure| pressure * pressure).unwrap();

        assert!(width > 0.4, "expected about half, got {width}");
    }

    #[test]
    fn mean_width_leaves_out_the_resting_tail() {
        // Drawn at 0.8, then held still for the recognition: a hundred events at 0.1, all within
        // a pixel of where the stroke ended. They say nothing about how hard it was drawn.
        let drawn = (0..=20).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.8));
        let rest = (0..100).map(|i| (Vector2::new(200.0, 100.0) + jitter(i, 0.5), 0.1));
        let path = pressured_path(drawn.chain(rest));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!(
            pressure > 0.75,
            "the hold dragged the pressure to {pressure}"
        );
    }

    #[test]
    fn mean_width_of_a_stroke_that_never_moved_is_still_something() {
        let path =
            pressured_path((0..10).map(|i| (Vector2::new(100.0, 100.0) + jitter(i, 0.2), 0.6)));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!((pressure - 0.6).abs() < 1e-9, "{pressure}");
    }

    #[test]
    fn recognize_straight_line() {
        let points = (0..=50)
            .map(|i| Vector2::new(20.0 + 4.0 * i as f64, 30.0 + 2.0 * i as f64) + jitter(i, 1.5));

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    /// A stroke along the given points, followed by the pen resting at the last point
    /// for a while, jittering slightly, like when it is held still to trigger the recognition.
    fn with_resting_tail(points: impl IntoIterator<Item = Vector2>) -> Vec<Vector2> {
        let mut points: Vec<Vector2> = points.into_iter().collect();
        let rest = *points.last().unwrap();
        points.extend((0..100).map(|i| rest + jitter(1000 + i, 0.1)));
        points
    }

    #[test]
    fn recognize_short_line_with_resting_pen() {
        // A fraction bar: a short horizontal line, then the pen held still.
        let points = with_resting_tail(
            (0..=15).map(|i| Vector2::new(100.0 + 2.0 * i as f64, 200.0) + jitter(i, 0.4)),
        );

        match recognize_shape(&pen_path_from_points(points), 1.0) {
            Some(Shape::Line(line)) => {
                assert!((line.end - Vector2::new(130.0, 200.0)).length() < 1.0);
            }
            other => panic!("expected line, got {other:?}"),
        }
    }

    #[test]
    fn recognize_short_line_when_zoomed_in() {
        // At 250% zoom, a line of 12 document units is 30 pixels long on screen.
        let points = with_resting_tail(
            (0..=12).map(|i| Vector2::new(100.0 + i as f64, 200.0) + jitter(i, 0.15)),
        );

        let shape = recognize_shape(&pen_path_from_points(points), 2.5);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    /// A parenthesis drawn from `start` to `end`, bulging the given distance to one side.
    fn parenthesis_points(start: Vector2, end: Vector2, bulge: f64) -> Vec<Vector2> {
        let chord = end - start;
        let normal = Vector2::new(-chord.y, chord.x) / chord.length();

        (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0;
                start + chord * t + normal * (bulge * (t * std::f64::consts::PI).sin())
            })
            .collect()
    }

    #[test]
    fn recognize_short_parenthesis() {
        // A parenthesis is short and nearly straight, but must become a bracket and not a line.
        let points = with_resting_tail((0..=20).map(|i| {
            let t = i as f64 / 20.0;
            Vector2::new(
                100.0 - 5.0 * (t * std::f64::consts::PI).sin(),
                200.0 + 40.0 * t,
            )
        }));

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .expect("a parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
            "expected a bracket, got {:?}",
            recognized.shape
        );
        match recognized.shape {
            Shape::CubicBezier(cubic) => {
                assert!((cubic.start.x - 100.0).abs() < 0.01);
                assert!((cubic.end.x - 100.0).abs() < 0.01);
                // The bulge is kept: the arc bends five pixels away from the chord.
                let apex = cubic_point(cubic.start, cubic.cp1, cubic.cp2, cubic.end, 0.5);
                assert!((apex.x - 95.0).abs() < 1.0, "apex at {apex:?}");
            }
            other => panic!("expected a cubic, got {other:?}"),
        }
    }

    #[test]
    fn recognize_upright_parenthesis_as_a_canonical_bracket() {
        let start = Vector2::new(100.0, 200.0);
        let end = Vector2::new(106.0, 260.0);
        let recognized = recognize_shape_with_kind(
            &pen_path_from_points(parenthesis_points(start, end, 12.0)),
            1.0,
            true,
        )
        .expect("a parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
            "expected a bracket, got {:?}",
            recognized.shape
        );

        let Shape::CubicBezier(cubic) = recognized.shape else {
            panic!("expected a cubic");
        };
        // The ends are on one vertical, at the mean of where they were drawn, keeping their height.
        assert!((cubic.start.x - 103.0).abs() < 0.01);
        assert!((cubic.end.x - 103.0).abs() < 0.01);
        assert!((cubic.start.y - 200.0).abs() < 0.01);
        assert!((cubic.end.y - 260.0).abs() < 0.01);
        // The deepest point is in the middle of the height.
        let apex = cubic_point(cubic.start, cubic.cp1, cubic.cp2, cubic.end, 0.5);
        assert!((apex.y - 230.0).abs() < 0.01);
        // The bulge is the one that was drawn.
        let bulge = (apex.x - 103.0).abs();
        assert!((bulge - 12.0).abs() < 0.5, "bulge {bulge}");
    }

    #[test]
    fn recognized_brackets_bulge_toward_the_drawn_arc() {
        // A deep bracket is an arc of more than ninety degrees, where the choice of the tangent for
        // the canonical arc is close to a tie. The recognized bracket still has to bulge the way the
        // stroke does: a bracket mirrored to the other side is not the shape that was drawn.
        for &bulge_ratio in &[0.2, 0.3, 0.4, 0.5, 0.55] {
            for &tilt_deg in &[-30.0_f64, -15.0, 0.0, 15.0, 30.0] {
                let start = Vector2::new(140.0, 200.0);
                let tilt = tilt_deg.to_radians();
                let chord = Vector2::new(-300.0 * tilt.sin(), 300.0 * tilt.cos());
                let end = start + chord;
                let normal = Vector2::new(-chord.y, chord.x) / chord.length();
                let points = parenthesis_points(start, end, bulge_ratio * chord.length());

                let recognized = recognize_shape(&pen_path_from_points(points), 1.0)
                    .unwrap_or_else(|| panic!("not recognized at {bulge_ratio}, {tilt_deg}"));
                let Shape::CubicBezier(arc) = recognized else {
                    panic!("expected a bracket, got {recognized:?}");
                };
                let apex = cubic_point(arc.start, arc.cp1, arc.cp2, arc.end, 0.5);
                let middle = (arc.start + arc.end) * 0.5;
                assert!(
                    normal.dot(apex - middle) > 0.0,
                    "the bracket is mirrored at bulge ratio {bulge_ratio}, tilt {tilt_deg}"
                );
            }
        }
    }

    #[test]
    fn a_straight_stroke_is_a_line_and_not_a_bracket() {
        let points =
            with_resting_tail((0..=20).map(|i| Vector2::new(100.0, 200.0 + 5.0 * i as f64)));

        let recognized =
            recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true).expect("a line");
        assert!(recognized.bracket.is_none());
        assert!(matches!(recognized.shape, Shape::Line(_)));
    }

    #[test]
    fn a_shallow_bow_on_a_line_is_still_a_line() {
        // Six percent of the height, under the deviation a line of that length may have.
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 300.0), 12.0);

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .expect("a line is recognized");
        assert!(recognized.bracket.is_none());
        assert!(
            matches!(recognized.shape, Shape::Line(_)),
            "expected a line, got {:?}",
            recognized.shape
        );
    }

    #[test]
    fn a_parenthesis_drawn_at_a_slant_is_straightened() {
        // A bracket as it is drawn by hand: the chord between its ends leans twenty-five degrees,
        // and the ends are put on one vertical, keeping the height and the bulge.
        let start = Vector2::new(100.0, 200.0);
        let end = Vector2::new(146.7, 300.0);
        let points = parenthesis_points(start, end, 30.0);

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .expect("a slanted parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
            "expected a bracket, got {:?}",
            recognized.shape
        );

        let Shape::CubicBezier(cubic) = recognized.shape else {
            panic!("expected a cubic");
        };
        let x = (start.x + end.x) * 0.5;
        assert!((cubic.start.x - x).abs() < 0.01);
        assert!((cubic.end.x - x).abs() < 0.01);
        assert!((cubic.start.y - 200.0).abs() < 0.01);
        assert!((cubic.end.y - 300.0).abs() < 0.01);
        let apex = cubic_point(cubic.start, cubic.cp1, cubic.cp2, cubic.end, 0.5);
        assert!((apex.y - 250.0).abs() < 0.01);
        let bulge = (apex.x - x).abs();
        assert!((bulge - 30.0).abs() < 0.5, "bulge {bulge}");
    }

    #[test]
    fn a_parenthesis_tilted_too_far_is_not_a_bracket() {
        // Forty degrees off the vertical: an arc that far from upright is not a bracket.
        let points =
            parenthesis_points(Vector2::new(100.0, 200.0), Vector2::new(183.9, 300.0), 12.0);

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true);
        assert!(
            recognized
                .as_ref()
                .is_none_or(|recognized| recognized.bracket.is_none())
        );
        // A bend that is not upright is still a bend, cleaned into one quadratic.
        assert!(
            matches!(recognized.map(|r| r.shape), Some(Shape::QuadraticBezier(_))),
            "a tilted bend is not cleaned into one bend"
        );
    }

    #[test]
    fn a_deep_parenthesis_is_a_bracket() {
        // A hand-drawn parenthesis is often a deep arc: forty-seven percent of its chord, as in the
        // stroke of a 200 px tall bracket that bulges 94 px.
        let points =
            parenthesis_points(Vector2::new(100.0, 200.0), Vector2::new(100.0, 340.0), 66.0);

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .expect("a deep parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
            "expected a bracket, got {:?}",
            recognized.shape
        );

        let Shape::CubicBezier(cubic) = recognized.shape else {
            panic!("expected a cubic");
        };
        let apex = cubic_point(cubic.start, cubic.cp1, cubic.cp2, cubic.end, 0.5);
        let bulge = (apex.x - 100.0).abs();
        assert!((bulge - 66.0).abs() < 1.0, "bulge {bulge}");
        assert!((apex.y - 270.0).abs() < 0.01);
    }

    #[test]
    fn a_parenthesis_with_hooked_ends_is_still_a_bracket() {
        // The pen lands with a short lead-in and lifts with a short tail, each of them turning
        // sharper than the bracket itself.
        let mut points =
            parenthesis_points(Vector2::new(100.0, 200.0), Vector2::new(100.0, 300.0), 30.0);
        let mut lead_in: Vec<Vector2> = (0..5)
            .map(|i| Vector2::new(100.0 + 7.0 - 1.4 * i as f64, 200.0))
            .collect();
        lead_in.extend(points.drain(..));
        let mut points = lead_in;
        points.extend((0..5).map(|i| Vector2::new(100.0 + 1.4 * i as f64, 300.0)));

        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .expect("a parenthesis with hooked ends is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
            "expected a bracket, got {:?}",
            recognized.shape
        );
    }

    #[test]
    fn a_deep_bend_is_not_a_bracket() {
        // Three quarters of the chord: the stroke wraps back towards itself, it is a hook.
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 200.0), 75.0);

        let bracket = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .is_some_and(|r| r.bracket.is_some());
        assert!(!bracket);
    }

    #[test]
    fn a_parenthesis_shorter_than_the_minimum_is_not_a_bracket() {
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 120.0), 5.0);

        let bracket = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .is_some_and(|r| r.bracket.is_some());
        assert!(!bracket);
    }

    #[test]
    fn an_s_curve_is_not_a_bracket() {
        let points = (0..=60).map(|i| {
            let t = i as f64 / 60.0;
            Vector2::new(
                100.0 + 20.0 * (2.0 * std::f64::consts::PI * t).sin(),
                100.0 + 200.0 * t,
            )
        });

        let bracket = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, true)
            .is_some_and(|r| r.bracket.is_some());
        assert!(!bracket);
    }

    /// The points of a quadratic bezier, for strokes a test draws along it.
    fn quadratic_points(start: Vector2, cp: Vector2, end: Vector2, n: usize) -> Vec<Vector2> {
        (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let mt = 1.0 - t;
                start * (mt * mt) + cp * (2.0 * mt * t) + end * (t * t)
            })
            .collect()
    }

    /// How far the stroke is from the curve, at its furthest.
    fn max_deviation(stroke: &[Vector2], curve: &Curve) -> f64 {
        let samples = curve
            .cubics
            .iter()
            .flat_map(|cubic| {
                (0..=16).map(move |i| {
                    cubic_point(
                        cubic.start,
                        cubic.cp1,
                        cubic.cp2,
                        cubic.end,
                        i as f64 / 16.0,
                    )
                })
            })
            .collect::<Vec<_>>();

        stroke
            .iter()
            .map(|point| {
                samples
                    .iter()
                    .map(|sample| (*sample - *point).length())
                    .fold(f64::MAX, f64::min)
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_curve_is_not_recognized_with_curves_off() {
        let drawn = quadratic_points(
            Vector2::new(100.0, 200.0),
            Vector2::new(150.0, 120.0),
            Vector2::new(200.0, 200.0),
            60,
        );

        let recognized = recognize_shape_with_kind(&pen_path_from_points(drawn), 1.0, false);

        assert!(
            recognized.is_none(),
            "got {:?}",
            recognized.map(|recognized| recognized.shape)
        );
    }

    #[test]
    fn recognize_single_bend_curve() {
        let start = Vector2::new(100.0, 200.0);
        let cp = Vector2::new(150.0, 120.0);
        let end = Vector2::new(200.0, 200.0);
        let drawn = quadratic_points(start, cp, end, 60);
        let points = drawn.iter().enumerate().map(|(i, p)| *p + jitter(i, 0.25));

        match recognize_shape(&pen_path_from_points(points), 1.0) {
            // One bend is cleaned into one quadratic, not a chain of segments: the curve is a
            // careful version of the stroke, and a bend is all the stroke has.
            Some(Shape::QuadraticBezier(bend)) => {
                assert!((bend.start - start).length() < 0.5);
                assert!((bend.end - end).length() < 0.5);
                let drawn_middle = start * 0.25 + cp * 0.5 + end * 0.25;
                let bend_middle = bend.start * 0.25 + bend.cp * 0.5 + bend.end * 0.25;
                assert!(
                    (bend_middle - drawn_middle).length() < 0.5,
                    "the bend is not the stroke's"
                );
            }
            other => panic!("expected a single bend, got {other:?}"),
        }
    }

    #[test]
    fn recognize_two_bend_curve() {
        let start = Vector2::new(100.0, 200.0);
        let cp1 = Vector2::new(130.0, 110.0);
        let cp2 = Vector2::new(170.0, 290.0);
        let end = Vector2::new(200.0, 200.0);
        let drawn = (0..=80)
            .map(|i| cubic_point(start, cp1, cp2, end, i as f64 / 80.0))
            .collect::<Vec<_>>();
        let points = drawn.iter().enumerate().map(|(i, p)| *p + jitter(i, 0.25));

        match recognize_shape(&pen_path_from_points(points), 1.0) {
            Some(Shape::Curve(curve)) => {
                let first = curve.cubics.first().unwrap();
                let last = curve.cubics.last().unwrap();
                assert!((first.start - start).length() < 0.5);
                assert!((last.end - end).length() < 0.5);
                // Two bends are two segments at the least, and the curve keeps them.
                assert!(curve.cubics.len() > 1);
                assert!(
                    max_deviation(&drawn, &curve) < 3.0,
                    "the curve leaves the stroke"
                );
            }
            other => panic!("expected a curve, got {other:?}"),
        }
    }

    /// A hand-drawn curly brace as one stroke, `left` its cusp to the left as `{`.
    ///
    /// Both are the two ways a brace is written by hand: `{` keeps its body on one side of the
    /// tips and juts out at the cusp, `}` crosses the chord with its cusp in the middle, as an S.
    fn curly_brace_points(left: bool) -> Vec<Vector2> {
        let points = if left {
            vec![
                (160.0, 100.0),
                (140.0, 112.0),
                (130.0, 130.0),
                (128.0, 160.0),
                (125.0, 205.0),
                (80.0, 250.0),
                (125.0, 295.0),
                (128.0, 340.0),
                (130.0, 370.0),
                (140.0, 388.0),
                (160.0, 400.0),
            ]
        } else {
            vec![
                (60.0, 110.0),
                (90.0, 100.0),
                (112.0, 112.0),
                (122.0, 140.0),
                (128.0, 180.0),
                (132.0, 215.0),
                (175.0, 250.0),
                (135.0, 285.0),
                (130.0, 320.0),
                (125.0, 340.0),
                (105.0, 362.0),
                (70.0, 368.0),
            ]
        };
        points
            .into_iter()
            .map(|(x, y)| Vector2::new(x, y))
            .collect()
    }

    #[test]
    fn recognize_a_left_curly_brace() {
        let points = with_resting_tail(curly_brace_points(true));
        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, false)
            .expect("a curly brace is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Curly))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        let tip = curve.cubics.first().unwrap().start;
        assert!((tip.x - 160.0).abs() < 1.0, "tips at {tip:?}");
        // The cusp juts to the left, a canonical depth away from the tips.
        let deepest = curve.sampled(16).iter().fold(f64::MAX, |x, p| x.min(p.x));
        assert!(
            (deepest - (160.0 - CURLY_DEPTH_RATIO * 300.0)).abs() < 0.5,
            "the cusp at {deepest}"
        );
    }

    #[test]
    fn recognize_a_right_curly_brace() {
        let points = with_resting_tail(curly_brace_points(false));
        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, false)
            .expect("a curly brace is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Curly))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        let deepest = curve.sampled(16).iter().fold(f64::MIN, |x, p| x.max(p.x));
        assert!(
            (deepest - (65.0 + CURLY_DEPTH_RATIO * 258.0)).abs() < 1.0,
            "the cusp at {deepest}"
        );
    }

    /// Recognize a stroke with the bracket and brace glyphs the settings ask for.
    fn recognize_with_brackets(
        points: Vec<Vector2>,
        bracket: BracketOptions,
    ) -> Option<RecognizedShape> {
        recognize_shape_with_options(
            &pen_path_from_points(points),
            1.0,
            &RecognitionOptions {
                curves: false,
                bracket,
            },
        )
    }

    /// A square bracket as it is drawn: a straight body, a corner at each end, the tips
    /// horizontal, `depth` to the left of the chord.
    fn square_bracket_points(x: f64, top: f64, bottom: f64, depth: f64) -> Vec<Vector2> {
        let mut points = Vec::new();
        for i in 0..=6 {
            points.push(Vector2::new(x - depth * i as f64 / 6.0, top));
        }
        for i in 1..=40 {
            points.push(Vector2::new(
                x - depth,
                top + (bottom - top) * i as f64 / 40.0,
            ));
        }
        for i in 1..=6 {
            points.push(Vector2::new(x - depth + depth * i as f64 / 6.0, bottom));
        }
        points
    }

    #[test]
    fn a_cornered_stroke_snaps_to_a_square_bracket() {
        // The square bracket has its own gesture: a straight body with a corner at each end.
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 400.0);
        let recognized = recognize_with_brackets(
            with_resting_tail(square_bracket_points(100.0, 100.0, 400.0, 15.0)),
            BracketOptions::default(),
        )
        .expect("a square bracket is recognized");
        assert_eq!(recognized.bracket, Some(BracketKind::SquareBracket));

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        let tip = curve.cubics.first().unwrap().start;
        assert!((tip - start).length() < 0.01, "tip at {tip:?}");
        // The body is a canonical share of the height to the side the stroke went to.
        let body = curve.cubics[1].start;
        assert!(
            (body.x - (100.0 - SQUARE_DEPTH_RATIO * (end.y - start.y))).abs() < 0.5,
            "body at {body:?}"
        );
    }

    #[test]
    fn a_smooth_parenthesis_is_not_a_square_bracket() {
        // The parenthesis gesture has no corner anywhere, so it keeps its own bracket whatever
        // the settings say of the other gestures.
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 400.0);
        let recognized = recognize_with_brackets(
            with_resting_tail(parenthesis_points(start, end, 40.0)),
            BracketOptions::default(),
        )
        .expect("a parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc))
        );
    }

    #[test]
    fn a_curly_brace_snaps_to_a_square_cornered_brace_when_the_settings_say_so() {
        let recognized = recognize_with_brackets(
            with_resting_tail(curly_brace_points(true)),
            BracketOptions {
                brace: BraceStyle::Square,
                ..BracketOptions::default()
            },
        )
        .expect("a curly brace is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Square))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        // The first segment runs out of the tip at a right angle and touches the body at a corner:
        // it is straight, its end a tip share of the depth to the side of the tip.
        let tip = curve.cubics.first().unwrap().start;
        assert!((tip.x - 160.0).abs() < 1.0, "tip at {tip:?}");
        let first = curve.cubics[0];
        let corner = first.end;
        let tip_share = 0.5 * CURLY_DEPTH_RATIO * 300.0;
        assert!((first.cp1 - tip.lerp(corner, 1.0 / 3.0)).length() < 0.01);
        assert!(
            (corner.x - (tip.x - tip_share)).abs() < 0.5,
            "corner at {corner:?}"
        );
        assert!((corner.y - tip.y).abs() < 0.5, "corner at {corner:?}");
        // The cusp is a right-angled tab, its mesa as deep as the canonical brace.
        let deepest = curve.sampled(64).iter().fold(f64::MAX, |x, p| x.min(p.x));
        assert!(
            (deepest - (160.0 - CURLY_DEPTH_RATIO * 300.0)).abs() < 0.5,
            "the cusp at {deepest}"
        );
    }

    #[test]
    fn a_curly_brace_snaps_to_a_rounded_brace_when_the_settings_say_so() {
        let recognized = recognize_with_brackets(
            with_resting_tail(curly_brace_points(true)),
            BracketOptions {
                brace: BraceStyle::Rounded,
                ..BracketOptions::default()
            },
        )
        .expect("a curly brace is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Rounded))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        let tip = curve.cubics.first().unwrap().start;
        assert!((tip.x - 160.0).abs() < 1.0, "tip at {tip:?}");
        // The tip turns over a quarter circle into the body: its end is a tip share of the depth
        // to the side of the tip and the same share down from it.
        let corner = curve.cubics[0].end;
        let tip_share = 0.5 * CURLY_DEPTH_RATIO * 300.0;
        assert!(
            (corner.x - (tip.x - tip_share)).abs() < 0.5,
            "corner at {corner:?}"
        );
        assert!(
            (corner.y - (tip.y + tip_share)).abs() < 0.5,
            "corner at {corner:?}"
        );
        // The cusp turns over a fillet: it peaks short of the canonical point, and the turn across
        // it is spread over the fillet instead of happening at one sample as a sharp point.
        let deepest = curve.sampled(64).iter().fold(f64::MAX, |x, p| x.min(p.x));
        let shortfall = Curve::rounded_cusp_shortfall() * CURLY_DEPTH_RATIO * 300.0;
        assert!(
            (deepest - (160.0 - CURLY_DEPTH_RATIO * 300.0 + shortfall)).abs() < 2.0,
            "the cusp at {deepest}"
        );
        let turn = max_open_turn(&curve.sampled(64));
        assert!(
            turn < 30.0_f64.to_radians(),
            "a rounded cusp still has a sharp point: {turn} rad"
        );
    }

    #[test]
    fn a_stroke_that_turns_sharply_at_its_end_is_not_a_curly_brace() {
        // A tall straight stroke with a hook at its foot, like a letter's: the sharp turn is where
        // the pen left, not in the middle where a brace turns.
        let points = with_resting_tail([
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 200.0),
            Vector2::new(100.0, 300.0),
            Vector2::new(100.0, 340.0),
            Vector2::new(125.0, 370.0),
        ]);
        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, false);
        assert!(
            recognized.and_then(|recognized| recognized.bracket)
                != Some(BracketKind::Brace(BraceStyle::Curly)),
            "a hooked stroke is not a curly brace"
        );
    }

    #[test]
    fn a_brace_with_one_curled_tip_and_a_long_tail_snaps() {
        // A brace as it is written by hand: a tail the pen swept out at the bottom and lifted
        // from, the cusp, and a curled tip at the top. Only the tip is a hook — the tail is nearly
        // straight, and a brace is not a `V` at both ends.
        let drawn = vec![
            (17.0, 101.0),
            (20.0, 100.0),
            (23.0, 99.0),
            (26.0, 98.0),
            (29.0, 97.0),
            (32.0, 96.0),
            (35.0, 94.0),
            (35.0, 91.0),
            (34.0, 88.0),
            (33.0, 85.0),
            (31.0, 82.0),
            (29.0, 79.0),
            (28.0, 76.0),
            (27.0, 73.0),
            (27.0, 70.0),
            (28.0, 67.0),
            (26.0, 65.0),
            (23.0, 65.0),
            (20.0, 65.0),
            (17.0, 64.0),
            (17.0, 61.0),
            (17.0, 58.0),
            (18.0, 55.0),
            (19.0, 52.0),
            (20.0, 49.0),
            (20.0, 46.0),
            (21.0, 43.0),
            (21.0, 40.0),
            (20.0, 37.0),
            (19.0, 34.0),
            (16.0, 32.0),
            (13.0, 32.0),
            (10.0, 32.0),
            (7.0, 33.0),
            (6.0, 34.0),
        ]
        .into_iter()
        .map(|(x, y)| Vector2::new(x, y));
        let path = pen_path_from_points(with_resting_tail(drawn));
        let recognized = recognize_shape_with_kind(&path, 1.0, false).expect("recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Curly))
        );
    }

    #[test]
    fn a_brace_whose_cusp_sits_above_the_middle_of_its_length_snaps() {
        // The same hand, another brace: the curled tip is short and the tail long, so the cusp sits
        // above the middle of the stroke though it is at the middle of the height.
        let drawn = vec![
            (248.0, 28.0),
            (251.0, 27.0),
            (254.0, 28.0),
            (255.0, 31.0),
            (255.0, 34.0),
            (255.0, 37.0),
            (255.0, 40.0),
            (254.0, 43.0),
            (254.0, 46.0),
            (254.0, 49.0),
            (256.0, 52.0),
            (259.0, 53.0),
            (262.0, 54.0),
            (261.0, 57.0),
            (261.0, 60.0),
            (261.0, 63.0),
            (262.0, 66.0),
            (263.0, 69.0),
            (264.0, 72.0),
            (266.0, 75.0),
            (268.0, 78.0),
            (269.0, 81.0),
            (270.0, 84.0),
            (270.0, 87.0),
            (269.0, 90.0),
            (266.0, 92.0),
            (263.0, 92.0),
            (260.0, 93.0),
            (257.0, 93.0),
            (254.0, 94.0),
            (251.0, 94.0),
            (248.0, 95.0),
            (245.0, 95.0),
            (244.0, 95.0),
        ]
        .into_iter()
        .map(|(x, y)| Vector2::new(x, y));
        let path = pen_path_from_points(with_resting_tail(drawn));
        let recognized = recognize_shape_with_kind(&path, 1.0, false).expect("recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Brace(BraceStyle::Curly))
        );
    }

    #[test]
    fn a_v_is_not_a_curly_brace() {
        // A `V`'s point sits in the middle of its height, as a brace's cusp does, but its ends run
        // straight to the point and do not curl over the chord.
        let points = with_resting_tail((0..=20).map(|i| {
            let t = i as f64 / 20.0;
            if t < 0.5 {
                Vector2::new(160.0 - 60.0 * t, 100.0 + 300.0 * t)
            } else {
                Vector2::new(130.0 + 60.0 * (t - 0.5), 100.0 + 300.0 * t)
            }
        }));
        let recognized = recognize_shape_with_kind(&pen_path_from_points(points), 1.0, false);
        assert!(
            recognized.and_then(|recognized| recognized.bracket)
                != Some(BracketKind::Brace(BraceStyle::Curly)),
            "a v is not a curly brace"
        );
    }

    #[test]
    fn an_s_curve_is_a_curve_and_not_a_bracket() {
        let drawn = (0..=60)
            .map(|i| {
                let t = i as f64 / 60.0;
                Vector2::new(
                    100.0 + 20.0 * (2.0 * std::f64::consts::PI * t).sin(),
                    100.0 + 200.0 * t,
                )
            })
            .collect::<Vec<_>>();

        let recognized = recognize_shape_with_kind(&pen_path_from_points(drawn.clone()), 1.0, true)
            .expect("a smooth stroke is recognized");
        assert!(recognized.bracket.is_none(), "an s-curve is not a bracket");
        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve, got {:?}", recognized.shape);
        };
        assert!(max_deviation(&drawn, &curve) < 3.0);
    }

    #[test]
    fn reject_sharp_corner() {
        // Two straight segments meeting at a right angle are a corner, not a bend of a curve.
        let points = (0..=40).map(|i| {
            if i < 20 {
                Vector2::new(100.0 + 5.0 * i as f64, 200.0)
            } else {
                Vector2::new(200.0, 200.0 + 5.0 * (i - 20) as f64)
            }
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(shape.is_none(), "expected no shape, got {shape:?}");
    }

    #[test]
    fn recognize_long_line_with_resting_pen() {
        let points = with_resting_tail(
            (0..=50).map(|i| Vector2::new(100.0 + 4.0 * i as f64, 200.0) + jitter(i, 0.4)),
        );

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    #[test]
    fn recognize_circle() {
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 50.0 * angle.cos(), 100.0 + 50.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                assert!((ellipse.radii.x - 50.0).abs() < 5.0);
                assert!((ellipse.radii.y - 50.0).abs() < 5.0);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    #[test]
    fn recognize_imperfect_circle_snaps_to_circle() {
        // A slightly squashed circle (radii ratio 0.8) should still snap to a circle.
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 50.0 * angle.cos(), 100.0 + 40.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                assert!(
                    (ellipse.radii.x - ellipse.radii.y).abs() < f64::EPSILON,
                    "expected circle, got radii {:?}",
                    ellipse.radii
                );
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    #[test]
    fn recognize_flat_ellipse() {
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 80.0 * angle.cos(), 100.0 + 30.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                // The fit underestimates the major radius of this jittered ellipse by about 10%.
                assert!((ellipse.radii.x - 80.0).abs() < 9.0);
                assert!((ellipse.radii.y - 30.0).abs() < 5.0);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    fn closed_polygon_points(corners: &[Vector2], per_edge: usize) -> Vec<Vector2> {
        let mut points = Vec::new();
        for i in 0..corners.len() {
            let a = corners[i];
            let b = corners[(i + 1) % corners.len()];
            for j in 0..per_edge {
                let t = j as f64 / per_edge as f64;
                points.push(a + (b - a) * t + jitter(i * per_edge + j, 1.0));
            }
        }
        points.push(corners[0]);
        points
    }

    #[test]
    fn recognize_axis_aligned_rectangle() {
        let corners = [
            Vector2::new(50.0, 50.0),
            Vector2::new(250.0, 50.0),
            Vector2::new(250.0, 150.0),
            Vector2::new(50.0, 150.0),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                assert!((rect.cuboid.half_extents.x - 100.0).abs() < 8.0);
                assert!((rect.cuboid.half_extents.y - 50.0).abs() < 8.0);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_rotated_rectangle() {
        let rot = DAffine2::from_angle_translation(0.4, Vector2::new(150.0, 150.0));
        let corners = [
            rot.transform_point2(Vector2::new(-100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, 50.0)),
            rot.transform_point2(Vector2::new(-100.0, 50.0)),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                let mut half_extents = [rect.cuboid.half_extents.x, rect.cuboid.half_extents.y];
                half_extents.sort_by(f64::total_cmp);
                assert!((half_extents[0] - 50.0).abs() < 8.0);
                assert!((half_extents[1] - 100.0).abs() < 8.0);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_slightly_rotated_rectangle_snaps_to_axis() {
        // A rectangle drawn ~4 degrees off-axis should be snapped to be axis-aligned.
        let rot = DAffine2::from_angle_translation(
            4.0 * std::f64::consts::PI / 180.0,
            Vector2::new(150.0, 150.0),
        );
        let corners = [
            rot.transform_point2(Vector2::new(-100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, 50.0)),
            rot.transform_point2(Vector2::new(-100.0, 50.0)),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                let orientation = rect.affine.to_scale_angle_translation().1;
                assert!(
                    orientation.abs() < 1e-9,
                    "expected axis-aligned rectangle, got orientation {orientation}"
                );
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_triangle() {
        let corners = [
            Vector2::new(100.0, 40.0),
            Vector2::new(180.0, 180.0),
            Vector2::new(20.0, 180.0),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 25)),
            1.0,
        );
        match shape {
            Some(Shape::Polygon(polygon)) => {
                assert_eq!(polygon.path.len(), 2);
            }
            other => panic!("expected polygon, got {other:?}"),
        }
    }

    #[test]
    fn reject_scribble() {
        // A scribble: a fine zigzag with sharp reversals, neither a line nor a curve.
        let points = (0..=60).map(|i| {
            let x = 50.0 + 4.0 * i as f64;
            let y = if i % 4 < 2 { 100.0 } else { 130.0 };
            Vector2::new(x, y)
        });

        assert!(recognize_shape(&pen_path_from_points(points), 1.0).is_none());
    }

    #[test]
    fn reject_tiny_stroke() {
        let points = (0..=20).map(|i| {
            let angle = (i as f64 / 20.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 4.0 * angle.cos(), 100.0 + 4.0 * angle.sin())
        });

        assert!(recognize_shape(&pen_path_from_points(points), 1.0).is_none());
    }

    /// A long line as a hand draws it: bowed a little, trembling, with a hook where the pen landed
    /// and one where it lifted, and then held still.
    fn crooked_line(length: f64, bow: f64, tremor: f64, hook: f64) -> Vec<Vector2> {
        let n = (length / 2.0) as usize;
        let body = (0..=n).map(|i| {
            let t = i as f64 / n as f64;
            Vector2::new(
                100.0 + length * t,
                200.0 + bow * (t * std::f64::consts::PI).sin(),
            ) + jitter(i, tremor)
        });
        let landing =
            (0..6).map(|i| Vector2::new(100.0 + i as f64, 200.0 - hook * (1.0 - i as f64 / 6.0)));
        let lifting = (0..6).map(|i| {
            Vector2::new(
                100.0 + length - i as f64,
                200.0 + hook * (1.0 - i as f64 / 6.0),
            )
        });
        with_resting_tail(
            landing
                .chain(body)
                .chain(lifting.collect::<Vec<_>>().into_iter().rev()),
        )
    }

    #[test]
    fn a_long_crooked_line_is_a_line() {
        for (length, bow, tremor, hook) in [
            (300.0, 15.0, 1.5, 8.0),
            (500.0, 30.0, 1.5, 12.0),
            (800.0, 45.0, 2.0, 12.0),
            (300.0, 0.0, 2.5, 0.0),
        ] {
            let path = pen_path_from_points(crooked_line(length, bow, tremor, hook));
            let shape = recognize_shape(&path, 1.0);
            assert!(
                matches!(shape, Some(Shape::Line(_))),
                "{length} long, bowed {bow}: {shape:?}"
            );
        }
    }

    #[test]
    fn a_long_arc_is_no_line() {
        let path = pen_path_from_points(crooked_line(300.0, 60.0, 0.5, 0.0));
        assert!(recognize_shape(&path, 1.0).is_none());
    }

    #[test]
    fn a_line_runs_from_where_the_pen_landed_to_the_pen() {
        let points = crooked_line(400.0, 10.0, 0.5, 12.0);
        let (landed, pen) = (points[0], *points.last().unwrap());
        let Some(Shape::Line(line)) = recognize_shape(&pen_path_from_points(points), 1.0) else {
            panic!("expected a line");
        };
        assert!((line.start - landed).length() < 1e-9, "{:?}", line.start);
        assert!((line.end - pen).length() < 1e-9, "{:?}", line.end);
    }

    #[test]
    fn short_strokes_have_to_be_held_longer() {
        let stroke = |length: f64| {
            pen_path_from_points(
                (0..=20).map(move |i| Vector2::new(100.0, 100.0 + length * i as f64 / 20.0)),
            )
        };
        assert_eq!(hold_factor(&stroke(25.0), 1.0), HOLD_SHORT_FACTOR);
        assert_eq!(hold_factor(&stroke(300.0), 1.0), 1.0);
        let between = hold_factor(&stroke(80.0), 1.0);
        assert!(1.0 < between && between < HOLD_SHORT_FACTOR, "{between}");
        // What counts is how large it is on screen
        assert_eq!(hold_factor(&stroke(25.0), 8.0), 1.0);
    }

    /// A circle drawn in one loop the way a hand draws one: the pen lands a little inside the rim,
    /// the hand shakes along the way, the loop ends where `covered` says, and the pen lifts
    /// turning back inside the circle.
    fn hand_drawn_circle(center: Vector2, r: f64, covered: f64, hook: f64) -> Vec<Vector2> {
        let mut points = (0..4)
            .map(|i| {
                let inward = hook * (1.0 - i as f64 / 3.0);
                center + Vector2::new(r * (1.0 - inward), 0.0) + jitter(i, 1.0)
            })
            .collect::<Vec<_>>();

        let n = 100;
        points.extend((1..=n).map(|i| {
            let angle = covered * (i as f64 / n as f64);
            center + Vector2::new(angle.cos(), angle.sin()) * r + jitter(i, 1.5)
        }));

        let last = *points.last().unwrap();
        points.extend((1..=4).map(|i| {
            let inward = hook * (i as f64 / 4.0);
            last + (center - last) * inward
        }));
        points
    }

    fn assert_circle_at(shape: Option<Shape>, center: Vector2, r: f64) {
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                assert!(
                    (ellipse.radii.x - r).abs() < 0.08 * r,
                    "radius {}, expected {r}",
                    ellipse.radii.x
                );
                assert!(
                    (ellipse.radii.y - r).abs() < 0.08 * r,
                    "radius {}, expected {r}",
                    ellipse.radii.y
                );
                let offset = ellipse.affine.translation - center;
                assert!(
                    offset.length() < 0.08 * r,
                    "center off by {offset:?}, expected {center:?}"
                );
            }
            other => panic!("expected a circle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_a_circle_the_pen_did_not_close_exactly() {
        // 340 of the 360 degrees, the pen landing and lifting 15% of the radius inside the rim.
        // The segment that closes the loop cuts through the inside of the circle, so an ellipse
        // fitted to the closed loop reads the stroke as far less round than it was drawn.
        let points = hand_drawn_circle(Vector2::new(100.0, 100.0), 50.0, 340f64.to_radians(), 0.15);
        assert_circle_at(
            recognize_shape(&pen_path_from_points(points), 1.0),
            Vector2::new(100.0, 100.0),
            50.0,
        );
    }

    #[test]
    fn recognize_a_circle_drawn_past_its_start() {
        // 380 degrees: the loop runs 20 degrees past where it began, so the stroke covers a
        // stretch of the rim twice and still is a circle.
        let points =
            hand_drawn_circle(Vector2::new(100.0, 100.0), 100.0, 380f64.to_radians(), 0.12);
        assert_circle_at(
            recognize_shape(&pen_path_from_points(points), 1.0),
            Vector2::new(100.0, 100.0),
            100.0,
        );
    }

    /// Recognize a stroke the way the brush does when the bracket shape is rounded.
    fn recognize_rounded(
        points: Vec<Vector2>,
        corner_radius_ratio: f64,
    ) -> Option<RecognizedShape> {
        recognize_with_style(points, ParenthesisStyle::Rounded, corner_radius_ratio)
    }

    /// Recognize a stroke the way the brush does, with the given parenthesis shape and the brace
    /// of type.
    fn recognize_with_style(
        points: Vec<Vector2>,
        style: ParenthesisStyle,
        corner_radius_ratio: f64,
    ) -> Option<RecognizedShape> {
        recognize_shape_with_options(
            &pen_path_from_points(points),
            1.0,
            &RecognitionOptions {
                curves: true,
                bracket: BracketOptions {
                    style,
                    brace: BraceStyle::Curly,
                    corner_radius_ratio,
                },
            },
        )
    }

    /// The points of a bracket drawn as the gesture is meant to be drawn: a straight body between
    /// a quarter circle at each end, the tips horizontal.
    fn straight_bracket_points(x: f64, top: f64, bottom: f64, corner: f64) -> Vec<Vector2> {
        let mut points = Vec::new();
        for i in 0..=12 {
            let angle =
                -std::f64::consts::FRAC_PI_2 - std::f64::consts::FRAC_PI_2 * i as f64 / 12.0;
            points.push(Vector2::new(
                x + corner + corner * angle.cos(),
                top + corner + corner * angle.sin(),
            ));
        }
        for i in 0..=40 {
            points.push(Vector2::new(
                x,
                top + corner + (bottom - top - 2.0 * corner) * i as f64 / 40.0,
            ));
        }
        for i in 0..=12 {
            let angle = std::f64::consts::PI - std::f64::consts::FRAC_PI_2 * i as f64 / 12.0;
            points.push(Vector2::new(
                x + corner + corner * angle.cos(),
                bottom - corner + corner * angle.sin(),
            ));
        }
        points
    }

    #[test]
    fn a_tall_parenthesis_becomes_a_nearly_vertical_bracket() {
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 400.0), 40.0);
        let recognized = recognize_rounded(points, 0.12).expect("a tall parenthesis is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Rounded))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve");
        };
        let sampled = curve.sampled(64);
        // The ends stay where they were drawn, the whole height.
        assert!((sampled[0] - Vector2::new(100.0, 100.0)).length() < 0.01);
        assert!((*sampled.last().unwrap() - Vector2::new(100.0, 400.0)).length() < 0.01);
        // The body is straight, one corner radius to the side of the ends: 12% of the height.
        for point in &sampled {
            if point.y > 140.0 && point.y < 360.0 {
                assert!((point.x - 64.0).abs() < 0.5, "body at {point:?}");
            }
        }
        // The corners are round: the ends reach back to the vertical of the tips.
        assert!((sampled[0].x - 100.0).abs() < 0.01);
    }

    #[test]
    fn a_smooth_parenthesis_snaps_to_a_square_bracket_with_the_square_style() {
        // The same stroke as the parenthesis above, its glyph asked for: a smooth bend, which is
        // no square bracket drawn by hand, becomes one all the same.
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 400.0), 40.0);
        let recognized = recognize_with_style(points, ParenthesisStyle::Square, 0.12)
            .expect("a tall parenthesis is recognized");
        assert_eq!(recognized.bracket, Some(BracketKind::SquareBracket));

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve");
        };
        let sampled = curve.sampled(64);
        assert!((sampled[0] - Vector2::new(100.0, 100.0)).length() < 0.01);
        assert!((*sampled.last().unwrap() - Vector2::new(100.0, 400.0)).length() < 0.01);
        // The body is straight, the square bracket's depth to the side of the ends: 15% of the
        // height, not the depth the stroke was bowed with.
        for point in &sampled {
            if point.y > 160.0 && point.y < 340.0 {
                assert!((point.x - 55.0).abs() < 0.5, "body at {point:?}");
            }
        }
    }

    #[test]
    fn a_bracket_drawn_with_a_straight_body_is_recognized() {
        // As the parenthesis gesture is meant to be drawn: nearly vertical, rounded at the ends
        // only. Its corners are turned too gently to be a square bracket's.
        let points = straight_bracket_points(100.0, 100.0, 400.0, 40.0);
        let recognized =
            recognize_rounded(points, 0.12).expect("a straight-bodied bracket is recognized");
        assert_eq!(
            recognized.bracket,
            Some(BracketKind::Parenthesis(ParenthesisStyle::Rounded))
        );

        let Shape::Curve(curve) = recognized.shape else {
            panic!("expected a curve");
        };
        let sampled = curve.sampled(64);
        assert!((sampled[0] - Vector2::new(140.0, 100.0)).length() < 0.01);
        assert!((*sampled.last().unwrap() - Vector2::new(140.0, 400.0)).length() < 0.01);
        // The body is the configured radius to the side of the tips.
        for point in &sampled {
            if point.y > 140.0 && point.y < 360.0 {
                assert!((point.x - 104.0).abs() < 0.5, "body at {point:?}");
            }
        }
    }

    #[test]
    fn a_straight_stroke_is_a_line_when_the_bracket_is_rounded() {
        let points =
            with_resting_tail((0..=20).map(|i| Vector2::new(100.0, 200.0 + 5.0 * i as f64)));
        let recognized = recognize_rounded(points, 0.12).expect("a line is recognized");
        assert!(recognized.bracket.is_none());
        assert!(matches!(recognized.shape, Shape::Line(_)));
    }

    #[test]
    fn a_shallow_bow_is_a_line_when_the_bracket_is_rounded() {
        // Under the deviation a line of that length may have, and with no corners: a line.
        let points =
            parenthesis_points(Vector2::new(100.0, 100.0), Vector2::new(100.0, 300.0), 12.0);
        let recognized = recognize_rounded(points, 0.12).expect("a line is recognized");
        assert!(recognized.bracket.is_none());
        assert!(matches!(recognized.shape, Shape::Line(_)));
    }

    #[test]
    fn an_s_curve_is_not_a_rounded_bracket() {
        let points = (0..=60)
            .map(|i| {
                let t = i as f64 / 60.0;
                Vector2::new(
                    100.0 + 20.0 * (2.0 * std::f64::consts::PI * t).sin(),
                    100.0 + 200.0 * t,
                )
            })
            .collect::<Vec<_>>();
        let recognized = recognize_rounded(points, 0.12).expect("an s-curve is a curve");
        assert!(recognized.bracket.is_none());
    }
}
