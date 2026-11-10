// Imports
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use p2d::shape::Cuboid;
use rnote_compose::Transformable;
use rnote_compose::shaperecognition::{
    BraceStyle, BracketKind, CURLY_DEPTH_RATIO, ParenthesisStyle,
};
use rnote_compose::shapes::cubbez::cubbez_calc;
use rnote_compose::shapes::{
    CubicBezier, Curve, Ellipse, Line, QuadraticBezier, Rectangle, Shape, Shapeable,
};

/// The extent a shape keeps at least while it is adjusted, so that it can't collapse into nothing.
const MIN_EXTENT: f64 = 1.0;

/// How many points of each cubic of a brace are sampled to find its cusp.
const CURLY_DEPTH_SAMPLES: usize = 16;

/// Adjusting a recognized shape with the pen that drew it, while that pen is still down.
///
/// The pen keeps changing the shape that replaced its stroke, like when drawing one with the shaper pen:
/// a line follows the pen with its end, a rectangle with the corner closest to the pen, an ellipse with
/// its radii, an open curve with its bend and a bracket with the end the pen rested at, its other end
/// staying fixed. Any other shape is scaled between the pen and the corner of its bounds opposite to it.
///
/// The shape follows the movement of the pen since the recognition, so that it does not jump when the pen
/// does not rest exactly on the part of the shape that follows it. Only the end of a line is on the pen.
#[derive(Debug, Clone)]
pub(crate) struct ShapeAdjust {
    /// The shape as it was recognized.
    recognized: Shape,
    /// The position of the pen when the recognized shape replaced the drawn stroke.
    start_pos: Vector2,
    kind: AdjustKind,
}

#[derive(Debug, Clone)]
enum AdjustKind {
    /// The end of the line follows the pen, its start stays fixed.
    LineEnd,
    /// The corner of the rectangle closest to the pen follows it, the opposite corner stays fixed.
    ///
    /// Holds the signs of that corner in the local frame of the rectangle.
    RectangleCorner { corner_signs: Vector2 },
    /// The radii of the ellipse follow the pen, its center stays fixed.
    ///
    /// Holds the signs of the pen position in the local frame of the ellipse.
    EllipseRadii { radii_signs: Vector2 },
    /// The control point of the quadratic curve follows the pen, its endpoints stay fixed.
    QuadraticBend,
    /// Both control points of the cubic curve follow the pen, its endpoints stay fixed.
    CubicBend,
    /// The end of the bracket the pen rested at follows it, its other end stays fixed.
    BracketEnd(BracketEnd),
    /// The end of a straight-bodied bracket the pen rested at follows it, its other end stays
    /// fixed.
    ///
    /// A rounded bracket and a square one are both such a body; only the corners differ.
    BracketBodyEnd(BracketBodyEnd),
    /// The end of the curly brace the pen rested at follows it, its other end stays fixed.
    BraceEnd(BraceEnd),
    /// The shape is scaled between the pen and the corner of its bounds opposite to it.
    ScaleFromPivot {
        pivot: Vector2,
        start_extents: Vector2,
    },
}

impl ShapeAdjust {
    /// Start adjusting the given recognized shape, with the pen at `start_pos`.
    ///
    /// `bracket` tells a canonical bracket from an ordinary open curve, which look alike but are
    /// adjusted by different parts of themselves.
    pub(crate) fn new(recognized: Shape, start_pos: Vector2, bracket: Option<BracketKind>) -> Self {
        let kind = match (&recognized, bracket) {
            (Shape::CubicBezier(cubic), Some(BracketKind::Parenthesis(ParenthesisStyle::Arc))) => {
                AdjustKind::BracketEnd(BracketEnd::new(cubic, start_pos))
            }
            (Shape::Curve(curve), Some(BracketKind::Parenthesis(ParenthesisStyle::Rounded))) => {
                AdjustKind::BracketBodyEnd(BracketBodyEnd::new(
                    curve,
                    start_pos,
                    BracketBody::Rounded,
                ))
            }
            (Shape::Curve(curve), Some(BracketKind::SquareBracket)) => AdjustKind::BracketBodyEnd(
                BracketBodyEnd::new(curve, start_pos, BracketBody::Square),
            ),
            (Shape::Curve(curve), Some(BracketKind::Brace(corners))) => {
                AdjustKind::BraceEnd(BraceEnd::new(curve, start_pos, corners))
            }
            (Shape::Line(_), _) => AdjustKind::LineEnd,
            (Shape::Rectangle(rectangle), _) => AdjustKind::RectangleCorner {
                corner_signs: signs(rectangle.affine.inverse().transform_point2(start_pos)),
            },
            (Shape::Ellipse(ellipse), _) => AdjustKind::EllipseRadii {
                radii_signs: signs(ellipse.affine.inverse().transform_point2(start_pos)),
            },
            (Shape::QuadraticBezier(_), _) => AdjustKind::QuadraticBend,
            (Shape::CubicBezier(_), _) => AdjustKind::CubicBend,
            (shape, _) => {
                let bounds = shape.bounds();
                let center = bounds.center();
                // The corner of the bounds that lies opposite to the pen stays fixed.
                let pivot = Vector2::new(
                    if start_pos.x >= center.x {
                        bounds.mins.x
                    } else {
                        bounds.maxs.x
                    },
                    if start_pos.y >= center.y {
                        bounds.mins.y
                    } else {
                        bounds.maxs.y
                    },
                );

                AdjustKind::ScaleFromPivot {
                    pivot,
                    start_extents: bounds.extents(),
                }
            }
        };

        Self {
            recognized,
            start_pos,
            kind,
        }
    }

    /// The recognized shape, adjusted for the given pen position.
    pub(crate) fn adjusted(&self, pen_pos: Vector2) -> Shape {
        let offset = pen_pos - self.start_pos;

        match (&self.recognized, &self.kind) {
            // The end of a line is always on the pen
            (Shape::Line(line), AdjustKind::LineEnd) => Shape::Line(Line {
                start: line.start,
                end: pen_pos,
            }),
            (Shape::Rectangle(rectangle), AdjustKind::RectangleCorner { corner_signs }) => {
                let local_offset = rectangle.affine.inverse().transform_vector2(offset);
                let fixed_corner = -*corner_signs * rectangle.cuboid.half_extents;
                let moved_corner = *corner_signs * rectangle.cuboid.half_extents + local_offset;
                let half_extents = ((moved_corner - fixed_corner) * 0.5)
                    .abs()
                    .max(Vector2::splat(MIN_EXTENT * 0.5));
                let center = (fixed_corner + moved_corner) * 0.5;

                Shape::Rectangle(Rectangle {
                    cuboid: Cuboid::new(half_extents),
                    affine: rectangle.affine * DAffine2::from_translation(center),
                })
            }
            (Shape::Ellipse(ellipse), AdjustKind::EllipseRadii { radii_signs }) => {
                let local_offset = ellipse.affine.inverse().transform_vector2(offset);
                let radii = (ellipse.radii + *radii_signs * local_offset)
                    .abs()
                    .max(Vector2::splat(MIN_EXTENT * 0.5));

                Shape::Ellipse(Ellipse {
                    radii,
                    affine: ellipse.affine,
                })
            }
            (Shape::QuadraticBezier(quadratic), AdjustKind::QuadraticBend) => {
                Shape::QuadraticBezier(QuadraticBezier {
                    cp: quadratic.cp + offset,
                    ..*quadratic
                })
            }
            (Shape::CubicBezier(cubic), AdjustKind::BracketEnd(bracket)) => {
                Shape::CubicBezier(bracket.adjusted(cubic, offset))
            }
            (Shape::Curve(curve), AdjustKind::BracketBodyEnd(bracket)) => {
                Shape::Curve(bracket.adjusted(offset).unwrap_or_else(|| curve.clone()))
            }
            (Shape::Curve(curve), AdjustKind::BraceEnd(brace)) => {
                Shape::Curve(brace.adjusted(offset).unwrap_or_else(|| curve.clone()))
            }
            (Shape::CubicBezier(cubic), AdjustKind::CubicBend) => Shape::CubicBezier(CubicBezier {
                cp1: cubic.cp1 + offset,
                cp2: cubic.cp2 + offset,
                ..*cubic
            }),
            (
                shape,
                AdjustKind::ScaleFromPivot {
                    pivot,
                    start_extents,
                },
            ) => {
                // Moving away from the pivot grows the shape, no matter on which side of it the pen is.
                let signed_offset = Vector2::new(
                    if self.start_pos.x >= pivot.x {
                        offset.x
                    } else {
                        -offset.x
                    },
                    if self.start_pos.y >= pivot.y {
                        offset.y
                    } else {
                        -offset.y
                    },
                );
                let scale = Vector2::new(
                    axis_scale(start_extents.x, signed_offset.x),
                    axis_scale(start_extents.y, signed_offset.y),
                );

                let mut shape = shape.clone();
                shape.translate(-*pivot);
                shape.scale(scale);
                shape.translate(*pivot);
                shape
            }
            // The kind is always derived from the shape it is created with, so this can't be reached.
            (shape, _) => shape.clone(),
        }
    }
}

/// The end of a bracket that follows the pen, while the bracket keeps its shape.
///
/// The other end stays where it is. The bracket stays symmetric about the chord between the two
/// ends, with its deepest point in the middle of that chord, and keeps the bulge it was recognized
/// with relative to the chord's length. So it may tilt and change height while it is dragged, and
/// its bend does not grow under a pen that lies on one of its ends.
#[derive(Debug, Clone)]
struct BracketEnd {
    /// The end that stays where it is.
    fixed_end: Vector2,
    /// The end that follows the pen, as it was recognized.
    dragged_end: Vector2,
    /// Whether the dragged end is the `end` of the curve rather than its `start`.
    dragged_is_end: bool,
    /// The bulge of the bracket relative to its chord.
    bulge_ratio: f64,
    /// The side of the chord the bulge is on, as a unit vector.
    outward: Vector2,
}

impl BracketEnd {
    fn new(cubic: &CubicBezier, pen_pos: Vector2) -> Self {
        let middle = (cubic.start + cubic.end) * 0.5;
        let chord = (cubic.end - cubic.start).length();
        // The bracket is a circular arc, so its deepest point is halfway along it.
        let bulge = cubbez_calc(cubic.start, cubic.cp1, cubic.cp2, cubic.end, 0.5) - middle;
        let dragged_is_end = pen_pos.distance(cubic.end) <= pen_pos.distance(cubic.start);

        Self {
            fixed_end: if dragged_is_end {
                cubic.start
            } else {
                cubic.end
            },
            dragged_end: if dragged_is_end {
                cubic.end
            } else {
                cubic.start
            },
            dragged_is_end,
            bulge_ratio: if chord > f64::EPSILON {
                bulge.length() / chord
            } else {
                0.0
            },
            outward: if bulge.length() > f64::EPSILON {
                bulge.normalize()
            } else {
                Vector2::new(1.0, 0.0)
            },
        }
    }

    /// The bracket with its dragged end moved by `offset`.
    fn adjusted(&self, cubic: &CubicBezier, offset: Vector2) -> CubicBezier {
        let moved = self.dragged_end + offset;
        let chord = (moved - self.fixed_end).length();
        if chord < MIN_EXTENT {
            return *cubic;
        }

        let (start, end) = if self.dragged_is_end {
            (self.fixed_end, moved)
        } else {
            (moved, self.fixed_end)
        };

        CubicBezier::new_circular_arc(start, end, self.bulge_ratio * chord, self.outward)
            .unwrap_or(*cubic)
    }
}

/// How the body of a straight-bodied bracket is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BracketBody {
    /// A body rounded into short tips at its ends.
    Rounded,
    /// A body with a sharp corner at each end.
    Square,
}

/// The end of a straight-bodied bracket that follows the pen, while the bracket keeps its shape.
///
/// The other end stays where it is. The bracket keeps the share of its height its body is offset
/// by — the corner radius of a rounded bracket, the arm of a square one — so it stays as round or
/// as sharp as it was recognized when it is dragged, and it keeps opening to the side it did.
#[derive(Debug, Clone)]
struct BracketBodyEnd {
    /// The end that stays where it is.
    fixed_end: Vector2,
    /// The end that follows the pen, as the bracket was recognized.
    dragged_end: Vector2,
    /// Whether the dragged end is the `end` of the curve rather than its `start`.
    dragged_is_end: bool,
    /// The offset of the body from the chord, relative to the height: the corner radius of a
    /// rounded bracket, the arm of a square one.
    depth_ratio: f64,
    /// The side of the chord the body runs on, as a unit vector.
    outward: Vector2,
    /// How the body is built at its ends.
    body: BracketBody,
}

impl BracketBodyEnd {
    fn new(curve: &Curve, pen_pos: Vector2, body: BracketBody) -> Self {
        let start = curve
            .cubics
            .first()
            .map_or(Vector2::ZERO, |cubic| cubic.start);
        let end = curve.cubics.last().map_or(Vector2::ZERO, |cubic| cubic.end);
        // The first segment ends where the straight body begins, its offset from the chord.
        let corner_end = curve.cubics.first().map_or(start, |cubic| cubic.end);

        let chord_vec = end - start;
        let chord = chord_vec.length();
        let height = (end.y - start.y).abs();
        let (depth_ratio, outward) = if chord > f64::EPSILON && height > f64::EPSILON {
            let direction = chord_vec / chord;
            let normal = Vector2::new(-direction.y, direction.x);
            let offset = normal.dot(corner_end - start);
            (
                // Half the height is all the offset a bracket can have: then it is a half circle.
                (offset.abs() / height).min(0.5),
                if offset < 0.0 { -normal } else { normal },
            )
        } else {
            (0.0, Vector2::new(1.0, 0.0))
        };

        let dragged_is_end = pen_pos.distance(end) <= pen_pos.distance(start);

        Self {
            fixed_end: if dragged_is_end { start } else { end },
            dragged_end: if dragged_is_end { end } else { start },
            dragged_is_end,
            depth_ratio,
            outward,
            body,
        }
    }

    /// The bracket with its dragged end moved by `offset`.
    fn adjusted(&self, offset: Vector2) -> Option<Curve> {
        let moved = self.dragged_end + offset;
        let height = (moved.y - self.fixed_end.y).abs();
        if height < MIN_EXTENT {
            return None;
        }

        let (start, end) = if self.dragged_is_end {
            (self.fixed_end, moved)
        } else {
            (moved, self.fixed_end)
        };
        let depth = self.depth_ratio * height;

        match self.body {
            BracketBody::Rounded => Curve::new_rounded_bracket(start, end, depth, self.outward),
            BracketBody::Square => Curve::new_square_bracket(start, end, depth, self.outward),
        }
    }
}

/// A curly brace adjusted by the end of it the pen rested at.
///
/// The other end stays where it is. The brace keeps the share of its height its depth takes, so it
/// stays as slim and as pointed as it was recognized when it is dragged, and it keeps opening to
/// the side of its chord it did. It is drawn again in the representation it was recognized in.
#[derive(Debug, Clone)]
struct BraceEnd {
    /// The end that stays where it is.
    fixed_end: Vector2,
    /// The end that follows the pen, as the brace was recognized.
    dragged_end: Vector2,
    /// Whether the dragged end is the `end` of the curve rather than its `start`.
    dragged_is_end: bool,
    /// The depth of the brace, relative to its height, from its tips to its cusp.
    depth_ratio: f64,
    /// The side of the chord the cusp points to, as a unit vector.
    outward: Vector2,
    /// How the corners of the brace, its tips and its cusp, are drawn.
    corners: BraceStyle,
}

impl BraceEnd {
    fn new(curve: &Curve, pen_pos: Vector2, corners: BraceStyle) -> Self {
        let start = curve
            .cubics
            .first()
            .map_or(Vector2::ZERO, |cubic| cubic.start);
        let end = curve.cubics.last().map_or(Vector2::ZERO, |cubic| cubic.end);

        let chord_vec = end - start;
        let chord = chord_vec.length();
        let height = (end.y - start.y).abs();
        let (depth_ratio, outward) = if chord > f64::EPSILON && height > f64::EPSILON {
            let direction = chord_vec / chord;
            let normal = Vector2::new(-direction.y, direction.x);
            // The cusp is the deepest point of the brace: the arms meet at it, and nothing else
            // of the brace is further from the chord.
            let deepest =
                curve
                    .sampled(CURLY_DEPTH_SAMPLES)
                    .into_iter()
                    .fold(start, |deepest, point| {
                        if normal.dot(point - start).abs() > normal.dot(deepest - start).abs() {
                            point
                        } else {
                            deepest
                        }
                    });
            let depth = normal.dot(deepest - start);
            // A fillet turns the cusp of a rounded brace over short of the point, so its deepest
            // sample is not its depth: dividing the shortfall out keeps the brace's depth from
            // shrinking each time it is dragged.
            let depth = match corners {
                BraceStyle::Rounded => depth.abs() / (1.0 - Curve::rounded_cusp_shortfall()),
                _ => depth.abs(),
            };
            (
                (depth / height).min(0.5),
                if normal.dot(deepest - start) < 0.0 {
                    -normal
                } else {
                    normal
                },
            )
        } else {
            (CURLY_DEPTH_RATIO, Vector2::new(1.0, 0.0))
        };

        let dragged_is_end = pen_pos.distance(end) <= pen_pos.distance(start);
        let (fixed_end, dragged_end) = if dragged_is_end {
            (start, end)
        } else {
            (end, start)
        };

        Self {
            fixed_end,
            dragged_end,
            dragged_is_end,
            depth_ratio,
            outward,
            corners,
        }
    }

    /// The brace with the dragged end at the offset from where it was recognized.
    ///
    /// `None` when the height is gone.
    fn adjusted(&self, offset: Vector2) -> Option<Curve> {
        let moved = self.dragged_end + offset;
        let height = (moved.y - self.fixed_end.y).abs();
        if height < MIN_EXTENT {
            return None;
        }
        let (start, end) = if self.dragged_is_end {
            (self.fixed_end, moved)
        } else {
            (moved, self.fixed_end)
        };
        let depth = self.depth_ratio * height;

        match self.corners {
            BraceStyle::Curly => Curve::new_curly_brace(start, end, depth, self.outward),
            BraceStyle::Rounded => Curve::new_rounded_brace(start, end, depth, self.outward),
            BraceStyle::Square => Curve::new_square_brace(start, end, depth, self.outward),
        }
    }
}

/// The scale for one axis that grows the given extent by the offset, keeping the minimum extent.
fn axis_scale(start_extent: f64, signed_offset: f64) -> f64 {
    if start_extent < MIN_EXTENT {
        return 1.0;
    }

    (start_extent + signed_offset).max(MIN_EXTENT) / start_extent
}

/// The signs of the components of the given vector, with zero counting as positive.
fn signs(v: Vector2) -> Vector2 {
    Vector2::new(
        if v.x < 0.0 { -1.0 } else { 1.0 },
        if v.y < 0.0 { -1.0 } else { 1.0 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rnote_compose::shapes::Polygon;

    #[test]
    fn line_end_follows_pen() {
        let line = Line {
            start: Vector2::new(100.0, 100.0),
            end: Vector2::new(140.0, 100.0),
        };
        let adjust = ShapeAdjust::new(Shape::Line(line), Vector2::new(140.0, 100.0), None);

        match adjust.adjusted(Vector2::new(160.0, 120.0)) {
            Shape::Line(adjusted) => {
                assert_eq!(adjusted.start, line.start);
                assert_eq!(adjusted.end, Vector2::new(160.0, 120.0));
            }
            other => panic!("expected line, got {other:?}"),
        }
    }

    #[test]
    fn line_end_is_on_the_pen_even_if_it_rests_elsewhere() {
        let line = Line {
            start: Vector2::new(0.0, 0.0),
            end: Vector2::new(100.0, 0.0),
        };
        let adjust = ShapeAdjust::new(Shape::Line(line), Vector2::new(100.0, 6.0), None);

        for pen in [Vector2::new(100.0, 6.0), Vector2::new(103.0, 6.0)] {
            match adjust.adjusted(pen) {
                Shape::Line(adjusted) => {
                    assert_eq!(adjusted.start, line.start);
                    assert_eq!(adjusted.end, pen);
                }
                other => panic!("expected line, got {other:?}"),
            }
        }
    }

    #[test]
    fn rectangle_corner_follows_pen() {
        // A rectangle from (80, 90) to (120, 110), with the pen at its lower right corner.
        let rectangle = Rectangle {
            cuboid: Cuboid::new(Vector2::new(20.0, 10.0)),
            affine: DAffine2::from_translation(Vector2::new(100.0, 100.0)),
        };
        let adjust = ShapeAdjust::new(
            Shape::Rectangle(rectangle),
            Vector2::new(120.0, 110.0),
            None,
        );

        match adjust.adjusted(Vector2::new(130.0, 110.0)) {
            Shape::Rectangle(adjusted) => {
                // The upper left corner stays fixed, the lower right one moved with the pen.
                let bounds = Shape::Rectangle(adjusted).bounds();
                assert!((bounds.mins - Vector2::new(80.0, 90.0)).length() < 1e-6);
                assert!((bounds.maxs - Vector2::new(130.0, 110.0)).length() < 1e-6);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn rotated_rectangle_grows_along_its_own_axes() {
        // Rotated by 90 degrees, so moving the pen along x grows the rectangle along its local y axis.
        let rectangle = Rectangle {
            cuboid: Cuboid::new(Vector2::new(20.0, 10.0)),
            affine: DAffine2::from_translation(Vector2::new(100.0, 100.0))
                * DAffine2::from_angle(std::f64::consts::FRAC_PI_2),
        };
        let start_pos = rectangle.affine.transform_point2(Vector2::new(20.0, 10.0));
        let adjust = ShapeAdjust::new(Shape::Rectangle(rectangle), start_pos, None);

        match adjust.adjusted(start_pos + Vector2::new(0.0, 10.0)) {
            Shape::Rectangle(adjusted) => {
                assert!((adjusted.cuboid.half_extents.x - 25.0).abs() < 1e-6);
                assert!((adjusted.cuboid.half_extents.y - 10.0).abs() < 1e-6);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn ellipse_radii_follow_pen_around_its_center() {
        let ellipse = Ellipse {
            radii: Vector2::new(30.0, 20.0),
            affine: DAffine2::from_translation(Vector2::new(100.0, 100.0)),
        };
        let adjust = ShapeAdjust::new(Shape::Ellipse(ellipse), Vector2::new(130.0, 100.0), None);

        match adjust.adjusted(Vector2::new(140.0, 105.0)) {
            Shape::Ellipse(adjusted) => {
                assert!((adjusted.radii - Vector2::new(40.0, 25.0)).length() < 1e-6);
                // The center stays where it was.
                assert!((adjusted.affine.translation - Vector2::new(100.0, 100.0)).length() < 1e-6);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    #[test]
    fn quadratic_bend_follows_pen() {
        let quadratic = QuadraticBezier {
            start: Vector2::new(100.0, 200.0),
            cp: Vector2::new(150.0, 120.0),
            end: Vector2::new(200.0, 200.0),
        };
        let adjust = ShapeAdjust::new(
            Shape::QuadraticBezier(quadratic),
            Vector2::new(150.0, 120.0),
            None,
        );

        match adjust.adjusted(Vector2::new(140.0, 90.0)) {
            Shape::QuadraticBezier(adjusted) => {
                assert_eq!(adjusted.start, quadratic.start);
                assert_eq!(adjusted.end, quadratic.end);
                assert_eq!(adjusted.cp, Vector2::new(140.0, 90.0));
            }
            other => panic!("expected quadratic curve, got {other:?}"),
        }
    }

    #[test]
    fn bracket_end_follows_pen_and_keeps_the_shape() {
        // A canonical bracket from (100, 100) to (100, 200), bulging 20 to the left.
        let bracket = CubicBezier::new_circular_arc(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 200.0),
            20.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a circular arc");
        // The pen rests at the lower end.
        let adjust = ShapeAdjust::new(
            Shape::CubicBezier(bracket),
            Vector2::new(100.0, 200.0),
            Some(BracketKind::Parenthesis(ParenthesisStyle::Arc)),
        );

        // Stretching the lower end down keeps the upper end, and the bulge keeps its share of the
        // height: 20 of 100 is a fifth, so 40 of 200.
        let Shape::CubicBezier(adjusted) = adjust.adjusted(Vector2::new(100.0, 300.0)) else {
            panic!("expected a cubic");
        };
        assert_eq!(adjusted.start, bracket.start);
        assert_eq!(adjusted.end, Vector2::new(100.0, 300.0));
        let apex = cubbez_calc(
            adjusted.start,
            adjusted.cp1,
            adjusted.cp2,
            adjusted.end,
            0.5,
        );
        assert!((apex.x - 60.0).abs() < 0.01, "apex at {apex:?}");
        assert!((apex.y - 200.0).abs() < 0.01, "apex at {apex:?}");

        // Sideways, the end follows the pen and the bracket tilts around its other end, its bend
        // staying in the middle of the chord and keeping its share of the chord's length.
        let Shape::CubicBezier(adjusted) = adjust.adjusted(Vector2::new(120.0, 200.0)) else {
            panic!("expected a cubic");
        };
        let chord = adjusted.end - adjusted.start;
        let middle = (adjusted.start + adjusted.end) * 0.5;
        let apex = cubbez_calc(
            adjusted.start,
            adjusted.cp1,
            adjusted.cp2,
            adjusted.end,
            0.5,
        );
        assert_eq!(adjusted.end, Vector2::new(120.0, 200.0));
        assert!(
            (apex - middle).dot(chord).abs() < 0.01,
            "the bend is not in the middle of the chord"
        );
        assert!(
            ((apex - middle).length() - 0.2 * chord.length()).abs() < 0.01,
            "the bracket lost its bend"
        );
    }

    #[test]
    fn rounded_bracket_end_follows_pen_and_keeps_the_shape() {
        // A rounded bracket from (100, 100) to (100, 200), its body 20 to the left: the corner
        // takes a fifth of the height.
        let bracket = Curve::new_rounded_bracket(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 200.0),
            20.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a bracket");
        // The pen rests at the lower end.
        let adjust = ShapeAdjust::new(
            Shape::Curve(bracket),
            Vector2::new(100.0, 200.0),
            Some(BracketKind::Parenthesis(ParenthesisStyle::Rounded)),
        );

        // Stretching the lower end down keeps the upper end, and the corners keep their share of
        // the height: 20 of 100 is a fifth, so 40 of 200. The body stays straight.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 300.0)) else {
            panic!("expected a curve");
        };
        assert_eq!(
            adjusted.cubics.first().unwrap().start,
            Vector2::new(100.0, 100.0)
        );
        assert_eq!(
            adjusted.cubics.last().unwrap().end,
            Vector2::new(100.0, 300.0)
        );
        let sampled = adjusted.sampled(64);
        for point in sampled
            .iter()
            .filter(|point| (140.0..260.0).contains(&point.y))
        {
            assert!((point.x - 60.0).abs() < 0.5, "body at {point:?}");
        }

        // The end dragged the other way keeps the bracket on its side of the tips.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 100.0)) else {
            panic!("expected a curve");
        };
        let deepest = adjusted
            .sampled(64)
            .iter()
            .fold(f64::MAX, |x, point| x.min(point.x));
        assert!(deepest < 100.0, "the bracket lost its side");
    }

    #[test]
    fn square_bracket_end_follows_pen_and_keeps_the_shape() {
        // A square bracket from (100, 100) to (100, 200), its body 20 to the left: the arms take a
        // fifth of the height.
        let bracket = Curve::new_square_bracket(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 200.0),
            20.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a bracket");
        // The pen rests at the lower end.
        let adjust = ShapeAdjust::new(
            Shape::Curve(bracket),
            Vector2::new(100.0, 200.0),
            Some(BracketKind::SquareBracket),
        );

        // Stretching the lower end down keeps the upper end, and the arms keep their share of the
        // height: 20 of 100 is a fifth, so 40 of 200. The body stays straight and the corners sharp.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 300.0)) else {
            panic!("expected a curve");
        };
        assert_eq!(
            adjusted.cubics.first().unwrap().start,
            Vector2::new(100.0, 100.0)
        );
        assert_eq!(
            adjusted.cubics.last().unwrap().end,
            Vector2::new(100.0, 300.0)
        );
        let sampled = adjusted.sampled(64);
        for point in sampled
            .iter()
            .filter(|point| (110.0..290.0).contains(&point.y))
        {
            assert!((point.x - 60.0).abs() < 1e-6, "body at {point:?}");
        }
        let body = adjusted.cubics[1];
        let arriving = adjusted.cubics[0].end - adjusted.cubics[0].cp2;
        let leaving = body.cp1 - body.start;
        let turn = arriving.normalize().angle_to(leaving.normalize()).abs();
        assert!(
            (turn - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
            "the corner turns {turn} rad"
        );

        // The end dragged the other way keeps the bracket on its side of the tips.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 100.0)) else {
            panic!("expected a curve");
        };
        let deepest = adjusted
            .sampled(64)
            .iter()
            .fold(f64::MAX, |x, point| x.min(point.x));
        assert!(deepest < 100.0, "the bracket lost its side");
    }

    #[test]
    fn curly_brace_end_follows_pen_and_keeps_its_side() {
        // A brace from (100, 100) to (100, 300), its cusp to the right.
        let brace = Curve::new_curly_brace(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 300.0),
            36.0,
            Vector2::new(1.0, 0.0),
        )
        .expect("a curly brace");
        // The pen rests at the lower end.
        let adjust = ShapeAdjust::new(
            Shape::Curve(brace),
            Vector2::new(100.0, 300.0),
            Some(BracketKind::Brace(BraceStyle::Curly)),
        );

        // Stretching the lower end down keeps the upper end, and the cusp keeps its share of the
        // height: 36 of 200 is 18%, so 72 of 400.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 500.0)) else {
            panic!("expected a curve");
        };
        assert_eq!(
            adjusted.cubics.first().unwrap().start,
            Vector2::new(100.0, 100.0)
        );
        assert_eq!(
            adjusted.cubics.last().unwrap().end,
            Vector2::new(100.0, 500.0)
        );
        let deepest = adjusted
            .sampled(32)
            .iter()
            .fold(f64::MIN, |x, point| x.max(point.x));
        assert!((deepest - 172.0).abs() < 0.5, "the cusp at {deepest}");

        // Dragging the end across keeps the brace opening to the side it did.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(20.0, 300.0)) else {
            panic!("expected a curve");
        };
        let deepest = adjusted
            .sampled(32)
            .iter()
            .fold(f64::MIN, |x, point| x.max(point.x));
        assert!(deepest > 100.0, "the brace lost its side: {deepest}");
    }

    #[test]
    fn rounded_brace_end_dragged_keeps_its_depth() {
        // A rounded brace from (100, 100) to (100, 300), its cusp to the right, its depth a fifth
        // of the height.
        let brace = Curve::new_rounded_brace(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 300.0),
            40.0,
            Vector2::new(1.0, 0.0),
        )
        .expect("a rounded brace");
        // The cusp fills its corner, so the deepest point stays this far short of the tab's point.
        let shortfall = Curve::rounded_cusp_shortfall();
        let adjust = ShapeAdjust::new(
            Shape::Curve(brace),
            Vector2::new(100.0, 300.0),
            Some(BracketKind::Brace(BraceStyle::Rounded)),
        );

        // The depth is what the brace was drawn with, not what its deepest point reads, so
        // stretching the end down keeps the depth it has: a fifth of 200 is 40, of 400 is 80.
        let Shape::Curve(adjusted) = adjust.adjusted(Vector2::new(100.0, 500.0)) else {
            panic!("expected a curve");
        };
        assert_eq!(
            adjusted.cubics.first().unwrap().start,
            Vector2::new(100.0, 100.0)
        );
        assert_eq!(
            adjusted.cubics.last().unwrap().end,
            Vector2::new(100.0, 500.0)
        );
        let deepest = adjusted
            .sampled(32)
            .iter()
            .fold(f64::MIN, |x, point| x.max(point.x));
        assert!(
            (deepest - (100.0 + 80.0 * (1.0 - shortfall))).abs() < 0.5,
            "the cusp at {deepest}"
        );

        // Dragging it again reads the same depth back, so the cusp is not drawn shallower on each
        // drag.
        let adjust = ShapeAdjust::new(
            Shape::Curve(adjusted),
            Vector2::new(100.0, 500.0),
            Some(BracketKind::Brace(BraceStyle::Rounded)),
        );
        let Shape::Curve(again) = adjust.adjusted(Vector2::new(100.0, 700.0)) else {
            panic!("expected a curve");
        };
        let deepest = again
            .sampled(32)
            .iter()
            .fold(f64::MIN, |x, point| x.max(point.x));
        assert!(
            (deepest - (100.0 + 120.0 * (1.0 - shortfall))).abs() < 0.5,
            "the cusp at {deepest}"
        );
    }

    #[test]
    fn cubic_bends_follow_pen() {
        let cubic = CubicBezier {
            start: Vector2::new(100.0, 200.0),
            cp1: Vector2::new(130.0, 110.0),
            cp2: Vector2::new(170.0, 290.0),
            end: Vector2::new(200.0, 200.0),
        };
        let adjust = ShapeAdjust::new(Shape::CubicBezier(cubic), Vector2::new(150.0, 200.0), None);

        match adjust.adjusted(Vector2::new(150.0, 220.0)) {
            Shape::CubicBezier(adjusted) => {
                assert_eq!(adjusted.start, cubic.start);
                assert_eq!(adjusted.end, cubic.end);
                assert_eq!(adjusted.cp1, cubic.cp1 + Vector2::new(0.0, 20.0));
                assert_eq!(adjusted.cp2, cubic.cp2 + Vector2::new(0.0, 20.0));
            }
            other => panic!("expected cubic curve, got {other:?}"),
        }
    }

    #[test]
    fn polygon_scales_from_the_opposite_corner() {
        // A triangle with bounds from (100, 100) to (140, 140), with the pen at its lower right.
        let polygon = Polygon {
            start: Vector2::new(100.0, 140.0),
            path: vec![Vector2::new(140.0, 140.0), Vector2::new(120.0, 100.0)],
        };
        let adjust = ShapeAdjust::new(Shape::Polygon(polygon), Vector2::new(140.0, 140.0), None);

        // Dragging away from the upper left corner of the bounds grows the triangle.
        let adjusted = adjust.adjusted(Vector2::new(160.0, 160.0));
        let bounds = adjusted.bounds();
        assert!((bounds.mins - Vector2::new(100.0, 100.0)).length() < 1e-6);
        assert!((bounds.maxs - Vector2::new(160.0, 160.0)).length() < 1e-6);
    }
}
