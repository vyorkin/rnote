// Imports
use super::{CubicBezier, Shapeable};
use crate::Transformable;
use crate::ext::Vector2Ext;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename = "curve")]
/// A smooth curve, drawn as a chain of cubic bezier segments.
///
/// The segments share their endpoints, so the curve is one continuous stroke and not a set of
/// strokes that happen to touch.
pub struct Curve {
    /// The segments of the curve, from its start to its end.
    #[serde(rename = "cubics")]
    pub cubics: Vec<CubicBezier>,
}

impl Transformable for Curve {
    fn translate(&mut self, offset: Vector2) {
        for cubic in &mut self.cubics {
            cubic.translate(offset);
        }
    }

    fn rotate(&mut self, angle: f64, center: Vector2) {
        for cubic in &mut self.cubics {
            cubic.rotate(angle, center);
        }
    }

    fn scale(&mut self, scale: Vector2) {
        for cubic in &mut self.cubics {
            cubic.scale(scale);
        }
    }
}

impl Shapeable for Curve {
    fn bounds(&self) -> Aabb {
        let mut cubics = self.cubics.iter();
        let Some(first) = cubics.next() else {
            return Aabb::new(Vector2::ZERO, Vector2::ZERO);
        };

        cubics.fold(first.bounds(), |bounds, cubic| {
            bounds.merged(&cubic.bounds())
        })
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        self.cubics
            .iter()
            .flat_map(|cubic| cubic.hitboxes())
            .collect()
    }

    fn outline_path(&self) -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();

        for cubic in &self.cubics {
            path.move_to(cubic.start.to_kurbo_point());
            path.curve_to(
                cubic.cp1.to_kurbo_point(),
                cubic.cp2.to_kurbo_point(),
                cubic.end.to_kurbo_point(),
            );
        }

        path
    }
}

/// The length of the handle that makes a cubic trace a quarter of a circle.
const QUARTER_ARC_HANDLE: f64 = 0.552_284_749_830_793_4;

/// The share of a curly brace's depth its tips turn over.
///
/// The body of the brace runs that far from the vertical of its tips, and the cusp takes the rest
/// of the depth.
const CURLY_TIP_SHARE: f64 = 0.5;

/// How long the arms of a curly brace are, relative to the part of its depth that is the cusp.
const CURLY_ARM_RATIO: f64 = 1.6;

/// The angle (radians) an arm of a curly brace arrives at its cusp with, to the body.
///
/// The two arms meet at twice that angle from the body, so the cusp is a point of about 70
/// degrees, as it is in type.
const CURLY_APEX_ANGLE: f64 = 55.0 * std::f64::consts::PI / 180.0;

/// The share of an arm its control points are set at, from its ends.
const CURLY_ARM_HANDLE: f64 = 0.65;

/// The radius of the fillet a rounded brace turns its cusp over with, relative to its depth.
const CURLY_CUSP_FILLET_SHARE: f64 = 0.25;

/// The half width of the tab a square-cornered brace turns out at its cusp, relative to its depth.
const SQUARE_BRACE_MESA_SHARE: f64 = 0.35;

/// A straight cubic from one point to another.
fn straight_cubic(from: Vector2, to: Vector2) -> CubicBezier {
    CubicBezier {
        start: from,
        cp1: from.lerp(to, 1.0 / 3.0),
        cp2: from.lerp(to, 2.0 / 3.0),
        end: to,
    }
}

/// The frame a brace is drawn in: its chord, and the offsets along and across it.
struct BraceFrame {
    start: Vector2,
    direction: Vector2,
    normal: Vector2,
    chord: f64,
    depth: f64,
    tip: f64,
    middle: f64,
    arm: f64,
}

impl BraceFrame {
    /// `None` when the ends coincide or the depth is too small to leave a body and arms.
    fn new(start: Vector2, end: Vector2, depth: f64, outward: Vector2) -> Option<Self> {
        let chord_vec = end - start;
        let chord = chord_vec.length();
        if chord < f64::EPSILON || depth <= f64::EPSILON {
            return None;
        }

        let direction = chord_vec / chord;
        let mut normal = Vector2::new(-direction.y, direction.x);
        if normal.dot(outward) < 0.0 {
            normal = -normal;
        }

        let depth = depth.min(chord * 0.5);
        let tip = depth * CURLY_TIP_SHARE;
        let middle = chord * 0.5;
        let arm = ((depth - tip) * CURLY_ARM_RATIO).min(middle - tip);
        if arm <= f64::EPSILON || tip <= f64::EPSILON {
            return None;
        }

        Some(Self {
            start,
            direction,
            normal,
            chord,
            depth,
            tip,
            middle,
            arm,
        })
    }

    /// The point `along` the chord and `across` it, on the side the brace opens to.
    fn at(&self, along: f64, across: f64) -> Vector2 {
        self.start + self.direction * along + self.normal * across
    }

    /// The unit vector, in `along`/`across`, an arm arrives at the cusp with.
    fn arm_vector(&self) -> Vector2 {
        Vector2::new(CURLY_APEX_ANGLE.cos(), CURLY_APEX_ANGLE.sin())
    }

    /// The tip at the start: a quarter of a circle from the chord into the body.
    fn top_tip(&self) -> CubicBezier {
        let handle = QUARTER_ARC_HANDLE * self.tip;
        CubicBezier {
            start: self.at(0.0, 0.0),
            cp1: self.at(0.0, handle),
            cp2: self.at(self.tip - handle, self.tip),
            end: self.at(self.tip, self.tip),
        }
    }

    /// The tip at the end, the mirror of the one at the start.
    fn bottom_tip(&self) -> CubicBezier {
        let handle = QUARTER_ARC_HANDLE * self.tip;
        CubicBezier {
            start: self.at(self.chord - self.tip, self.tip),
            cp1: self.at(self.chord - self.tip + handle, self.tip),
            cp2: self.at(self.chord, handle),
            end: self.at(self.chord, 0.0),
        }
    }

    /// The two straight parts of the body, between a tip and an arm.
    ///
    /// Either is left out when the arms take the whole body.
    fn bodies(&self) -> (Option<CubicBezier>, Option<CubicBezier>) {
        let straight = |from: Vector2, to: Vector2| {
            ((to - from).length() > f64::EPSILON).then(|| straight_cubic(from, to))
        };
        (
            straight(
                self.at(self.tip, self.tip),
                self.at(self.middle - self.arm, self.tip),
            ),
            straight(
                self.at(self.middle + self.arm, self.tip),
                self.at(self.chord - self.tip, self.tip),
            ),
        )
    }
}

/// The brace of the frame: its tips and two bodies around the arms that make its cusp.
fn braced(frame: &BraceFrame, arms: &[CubicBezier]) -> Curve {
    let (top_body, bottom_body) = frame.bodies();
    let mut cubics = Vec::with_capacity(arms.len() + 4);
    cubics.push(frame.top_tip());
    cubics.extend(top_body);
    cubics.extend_from_slice(arms);
    cubics.extend(bottom_body);
    cubics.push(frame.bottom_tip());

    Curve { cubics }
}

impl Curve {
    /// A bracket from `start` to `end`: a straight body rounded into short tips at both ends.
    ///
    /// The body runs parallel to the chord between the ends, `radius` away from it on the side
    /// `outward` points to, and each end turns from the body out into its tip with a quarter of a
    /// circle. The tips stay where the ends are, so the bracket is as deep as `radius`.
    ///
    /// A radius of half the chord or more leaves no body: the two quarters meet and the bracket is
    /// the half circle through both ends. `None` when the ends coincide or the radius vanishes.
    pub fn new_rounded_bracket(
        start: Vector2,
        end: Vector2,
        radius: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let chord_vec = end - start;
        let chord = chord_vec.length();
        if chord < f64::EPSILON || radius <= f64::EPSILON {
            return None;
        }

        let direction = chord_vec / chord;
        let mut normal = Vector2::new(-direction.y, direction.x);
        if normal.dot(outward) < 0.0 {
            normal = -normal;
        }

        let radius = radius.min(chord * 0.5);
        let handle = QUARTER_ARC_HANDLE * radius;
        let body_start = start + direction * radius + normal * radius;
        let body_end = end - direction * radius + normal * radius;

        let start_corner = CubicBezier {
            start,
            cp1: start + normal * handle,
            cp2: body_start - direction * handle,
            end: body_start,
        };
        let end_corner = CubicBezier {
            start: body_end,
            cp1: body_end + direction * handle,
            cp2: end + normal * handle,
            end,
        };

        let mut cubics = Vec::with_capacity(3);
        cubics.push(start_corner);
        if (body_end - body_start).length() > f64::EPSILON {
            cubics.push(CubicBezier {
                start: body_start,
                cp1: body_start.lerp(body_end, 1.0 / 3.0),
                cp2: body_start.lerp(body_end, 2.0 / 3.0),
                end: body_end,
            });
        }
        cubics.push(end_corner);

        Some(Self { cubics })
    }

    /// An elliptical bracket from `start` to `end`: one smooth bow, deepest in the middle.
    ///
    /// The bracket is half of the ellipse whose chord is the diameter between the ends and whose
    /// middle stands `depth` away from the chord on the side `outward` points to. It has no
    /// straight body and no corner: the curvature is largest at the ends, where the tips turn out
    /// of the chord, and smallest in the middle. The tips stay where the ends are, so the bracket
    /// is as deep as `depth`.
    ///
    /// Each half is a quarter of the ellipse, traced by one cubic. The depth is at most half the
    /// chord. `None` when the ends coincide or the depth vanishes.
    pub fn new_elliptical_bracket(
        start: Vector2,
        end: Vector2,
        depth: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let chord_vec = end - start;
        let chord = chord_vec.length();
        if chord < f64::EPSILON || depth <= f64::EPSILON {
            return None;
        }

        let direction = chord_vec / chord;
        let mut normal = Vector2::new(-direction.y, direction.x);
        if normal.dot(outward) < 0.0 {
            normal = -normal;
        }

        let depth = depth.min(chord * 0.5);
        let apex = (start + end) * 0.5 + normal * depth;
        // The handles of a cubic that traces a quarter of an ellipse: one along the chord and one
        // across it, each the quarter-arc share of the semi-axis it belongs to.
        let along = QUARTER_ARC_HANDLE * chord * 0.5;
        let across = QUARTER_ARC_HANDLE * depth;

        Some(Self {
            cubics: vec![
                CubicBezier {
                    start,
                    cp1: start + normal * across,
                    cp2: apex - direction * along,
                    end: apex,
                },
                CubicBezier {
                    start: apex,
                    cp1: apex + direction * along,
                    cp2: end + normal * across,
                    end,
                },
            ],
        })
    }

    /// A square bracket from `start` to `end`: a straight body with a sharp corner at each end.
    ///
    /// The body runs parallel to the chord between the ends, `depth` away from it on the side
    /// `outward` points to, and each end runs straight out to the body along the normal of the
    /// chord. So each corner is a right angle, and the bracket is as deep as `depth`.
    ///
    /// The depth is at most half the chord. `None` when the ends coincide or the depth vanishes.
    pub fn new_square_bracket(
        start: Vector2,
        end: Vector2,
        depth: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let chord_vec = end - start;
        let chord = chord_vec.length();
        if chord < f64::EPSILON || depth <= f64::EPSILON {
            return None;
        }

        let direction = chord_vec / chord;
        let mut normal = Vector2::new(-direction.y, direction.x);
        if normal.dot(outward) < 0.0 {
            normal = -normal;
        }

        let depth = depth.min(chord * 0.5);
        let straight = |from: Vector2, to: Vector2| CubicBezier {
            start: from,
            cp1: from.lerp(to, 1.0 / 3.0),
            cp2: from.lerp(to, 2.0 / 3.0),
            end: to,
        };
        let top_corner = start + normal * depth;
        let bottom_corner = end + normal * depth;

        Some(Self {
            cubics: vec![
                straight(start, top_corner),
                straight(top_corner, bottom_corner),
                straight(bottom_corner, end),
            ],
        })
    }

    /// A curly brace from `start` to `end`, its cusp `depth` away from the chord on the side
    /// `outward` points to.
    ///
    /// The ends are the tips: each turns a quarter of a circle toward the cusp and into a straight
    /// body, which runs half the depth to the side of the chord between the tips. The cusp is a
    /// point in the middle of the body, reached by two arms that leave the body along it and meet
    /// at [`CURLY_APEX_ANGLE`] to it — the shape of the brace as it is set in type.
    ///
    /// The depth is measured from the tips to the cusp and is at most half the chord, and the arms
    /// are cut short of the tips, so that the body never folds into itself.
    /// `None` when the ends coincide or the depth vanishes.
    pub fn new_curly_brace(
        start: Vector2,
        end: Vector2,
        depth: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let frame = BraceFrame::new(start, end, depth, outward)?;
        let apex = frame.at(frame.middle, frame.depth);
        let arm_vector = frame.arm_vector();
        let arm_handle = CURLY_ARM_HANDLE * frame.arm;

        let top_arm = CubicBezier {
            start: frame.at(frame.middle - frame.arm, frame.tip),
            cp1: frame.at(frame.middle - frame.arm + arm_handle, frame.tip),
            cp2: apex
                - frame.direction * (arm_vector.x * arm_handle)
                - frame.normal * (arm_vector.y * arm_handle),
            end: apex,
        };
        let bottom_arm = CubicBezier {
            start: apex,
            cp1: apex + frame.direction * (arm_vector.x * arm_handle)
                - frame.normal * (arm_vector.y * arm_handle),
            cp2: frame.at(frame.middle + frame.arm - arm_handle, frame.tip),
            end: frame.at(frame.middle + frame.arm, frame.tip),
        };

        Some(braced(&frame, &[top_arm, bottom_arm]))
    }

    /// The same brace, its cusp turned over a fillet instead of meeting in a point.
    ///
    /// The arms stop a fillet's tangent length short of the cusp and a circular arc of
    /// [`CURLY_CUSP_FILLET_SHARE`] of the depth joins them, so the brace has no sharp point
    /// anywhere: the tips are rounded as in [`Self::new_curly_brace`] and so is the cusp.
    ///
    /// `None` when the ends coincide, the depth vanishes, or the fillet would eat the arms.
    pub fn new_rounded_brace(
        start: Vector2,
        end: Vector2,
        depth: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let frame = BraceFrame::new(start, end, depth, outward)?;
        let cusp_turn = 2.0 * CURLY_APEX_ANGLE;
        let radius = CURLY_CUSP_FILLET_SHARE * frame.depth;
        // Where the fillet is tangent to an arm, measured from the cusp along that arm: the
        // tangent of half the angle the arms leave between them.
        let tangent = radius * (cusp_turn * 0.5).tan();
        if tangent >= frame.arm {
            return None;
        }

        let apex = frame.at(frame.middle, frame.depth);
        let arm_vector = frame.arm_vector();
        // The arms are the brace's own, only as long as the fillet leaves them: their handles
        // shrink with the arms they belong to.
        let arm_handle = CURLY_ARM_HANDLE * (frame.arm - tangent);
        let top_end = apex
            - frame.direction * (arm_vector.x * tangent)
            - frame.normal * (arm_vector.y * tangent);
        // The bottom arm runs out of the cusp towards the body, so its end is a step along it.
        let bottom_start = apex + frame.direction * (arm_vector.x * tangent)
            - frame.normal * (arm_vector.y * tangent);

        let top_arm = CubicBezier {
            start: frame.at(frame.middle - frame.arm, frame.tip),
            cp1: frame.at(frame.middle - frame.arm + arm_handle, frame.tip),
            cp2: top_end
                - frame.direction * (arm_vector.x * arm_handle)
                - frame.normal * (arm_vector.y * arm_handle),
            end: top_end,
        };
        let cusp = CubicBezier::new_circular_arc(
            top_end,
            bottom_start,
            radius * (1.0 - (cusp_turn * 0.5).cos()),
            frame.normal,
        )?;
        let bottom_arm = CubicBezier {
            start: bottom_start,
            cp1: bottom_start + frame.direction * (arm_vector.x * arm_handle)
                - frame.normal * (arm_vector.y * arm_handle),
            cp2: frame.at(frame.middle + frame.arm - arm_handle, frame.tip),
            end: frame.at(frame.middle + frame.arm, frame.tip),
        };

        Some(braced(&frame, &[top_arm, cusp, bottom_arm]))
    }

    /// The share of its depth a rounded brace's cusp falls short of.
    ///
    /// The fillet replaces the point of the cusp with an arc that is tangent to both arms, so the
    /// arc peaks where the point would have cut in: this is how far short of the point it stays,
    /// relative to the depth. A rounded brace measured at its deepest point divides this out to
    /// find the depth it was drawn with.
    pub fn rounded_cusp_shortfall() -> f64 {
        // Half of the angle the arms leave between them at the cusp.
        let half_interior = std::f64::consts::FRAC_PI_2 - CURLY_APEX_ANGLE;
        CURLY_CUSP_FILLET_SHARE * (1.0 / half_interior.sin() - 1.0)
    }

    /// The same brace with every corner a right angle: at its tips and at its cusp.
    ///
    /// The body runs a straight arm out of each tip, parallel to the chord, and the cusp is a tab
    /// turned out of the body at right angles, [`SQUARE_BRACE_MESA_SHARE`] of the depth wide on
    /// each side of the middle. Every segment is straight, so the brace has no rounded part.
    ///
    /// `None` when the ends coincide or the depth vanishes.
    pub fn new_square_brace(
        start: Vector2,
        end: Vector2,
        depth: f64,
        outward: Vector2,
    ) -> Option<Self> {
        let frame = BraceFrame::new(start, end, depth, outward)?;
        let mesa = (SQUARE_BRACE_MESA_SHARE * frame.depth).min(frame.middle - frame.tip);
        if mesa <= f64::EPSILON {
            return None;
        }

        let corners = [
            frame.at(0.0, 0.0),
            frame.at(0.0, frame.tip),
            frame.at(frame.tip, frame.tip),
            frame.at(frame.middle - mesa, frame.tip),
            frame.at(frame.middle - mesa, frame.depth),
            frame.at(frame.middle + mesa, frame.depth),
            frame.at(frame.middle + mesa, frame.tip),
            frame.at(frame.chord - frame.tip, frame.tip),
            frame.at(frame.chord, frame.tip),
            frame.at(frame.chord, 0.0),
        ];

        Some(Self {
            cubics: corners
                .windows(2)
                .map(|segment| straight_cubic(segment[0], segment[1]))
                .collect(),
        })
    }

    /// A curve through the given points, with a smooth transition at each of them.
    ///
    /// The first and the last point are the endpoints of the curve, so a curve built from the
    /// points of a stroke keeps where that stroke started and ended. `None` when there are fewer
    /// than two points, or when two of them coincide and leave a segment without direction.
    pub fn new_through_points(points: &[Vector2]) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }

        let cubics = (0..points.len() - 1)
            .map(|i| {
                CubicBezier::new_w_catmull_rom(
                    points[i.saturating_sub(1)],
                    points[i],
                    points[i + 1],
                    points[(i + 2).min(points.len() - 1)],
                )
            })
            .collect::<Option<Vec<_>>>()?;

        Some(Self { cubics })
    }

    /// Whether the curve has no segments.
    pub fn is_empty(&self) -> bool {
        self.cubics.is_empty()
    }

    /// The points along the curve, evenly spaced in the parameter of each segment.
    pub fn sampled(&self, samples_per_cubic: usize) -> Vec<Vector2> {
        let samples_per_cubic = samples_per_cubic.max(1);
        let mut points = Vec::with_capacity(self.cubics.len() * samples_per_cubic + 1);

        for cubic in &self.cubics {
            for i in 0..samples_per_cubic {
                points.push(super::cubbez::cubbez_calc(
                    cubic.start,
                    cubic.cp1,
                    cubic.cp2,
                    cubic.end,
                    i as f64 / samples_per_cubic as f64,
                ));
            }
        }
        if let Some(last) = self.cubics.last() {
            points.push(last.end);
        }

        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_bracket_has_a_straight_body_one_radius_to_the_side() {
        let bracket = Curve::new_rounded_bracket(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 400.0),
            36.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a bracket");

        let sampled = bracket.sampled(64);
        assert!((sampled[0] - Vector2::new(100.0, 100.0)).length() < 1e-9);
        assert!((*sampled.last().unwrap() - Vector2::new(100.0, 400.0)).length() < 1e-9);
        // The body runs straight at one radius to the side of the chord.
        for point in sampled
            .iter()
            .filter(|point| (140.0..360.0).contains(&point.y))
        {
            assert!((point.x - 64.0).abs() < 0.5, "body at {point:?}");
        }
        // The corners stay inside the radius of the chord's vertical, and reach the tips.
        let deepest = sampled.iter().fold(f64::MAX, |x, point| x.min(point.x));
        assert!((deepest - 64.0).abs() < 0.5);
    }

    #[test]
    fn rounded_bracket_without_a_body_is_a_half_circle() {
        // A radius as large as the bracket is tall leaves no straight part: a half circle.
        let bracket = Curve::new_rounded_bracket(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 200.0),
            1000.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a bracket");

        assert_eq!(bracket.cubics.len(), 2);
        let sampled = bracket.sampled(64);
        let deepest = sampled.iter().fold(f64::MAX, |x, point| x.min(point.x));
        assert!((deepest - 50.0).abs() < 0.5, "deepest at {deepest}");
        // Every point is on the half circle of radius 50 around the middle of the chord.
        for point in &sampled {
            let offset = *point - Vector2::new(100.0, 150.0);
            assert!((offset.length() - 50.0).abs() < 0.5, "point {point:?}");
        }
    }

    #[test]
    fn square_bracket_has_right_angles_and_straight_arms() {
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 400.0);
        let depth = 30.0;
        let bracket = Curve::new_square_bracket(start, end, depth, Vector2::new(-1.0, 0.0))
            .expect("a square bracket");

        let sampled = bracket.sampled(64);
        // The ends stay where they were drawn, the body runs one depth to the side of the chord.
        assert!((sampled[0] - start).length() < 1e-9);
        assert!((*sampled.last().unwrap() - end).length() < 1e-9);
        for point in sampled
            .iter()
            .filter(|point| (110.0..390.0).contains(&point.y))
        {
            assert!((point.x - 70.0).abs() < 1e-6, "body at {point:?}");
        }

        // A corner at each end is a right angle, not a rounded tip.
        for (arm, body) in [(0, 1), (2, 1)] {
            let arriving = bracket.cubics[arm].end - bracket.cubics[arm].cp2;
            let leaving = bracket.cubics[body].cp1 - bracket.cubics[body].start;
            let turn = arriving.normalize().angle_to(leaving.normalize()).abs();
            assert!(
                (turn - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
                "the corner turns {turn} rad"
            );
        }
    }

    #[test]
    fn elliptical_bracket_is_a_smooth_half_ellipse() {
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 400.0);
        let depth = 36.0;
        let bracket = Curve::new_elliptical_bracket(start, end, depth, Vector2::new(-1.0, 0.0))
            .expect("an elliptical bracket");

        // Two quarters of the ellipse, and no straight segment anywhere.
        assert_eq!(bracket.cubics.len(), 2);
        let sampled = bracket.sampled(128);
        assert!((sampled[0] - start).length() < 1e-9);
        assert!((*sampled.last().unwrap() - end).length() < 1e-9);

        // Every point is on the ellipse of half the height the long way and the depth the short
        // way, so the bracket is smoothly bowed all along and not straightened in the middle.
        for point in &sampled {
            let offset = *point - Vector2::new(100.0, 250.0);
            let on_ellipse = (offset.x / depth).powi(2) + (offset.y / 150.0).powi(2);
            assert!(
                (on_ellipse - 1.0).abs() < 1e-2,
                "point {point:?} off the ellipse"
            );
        }
        let deepest = sampled.iter().fold(f64::MAX, |x, point| x.min(point.x));
        assert!((deepest - 64.0).abs() < 0.01, "deepest at {deepest}");
        // A quarter way down the bracket it is already most of the depth out: the bow is not a
        // straight body one depth to the side, as a rounded bracket's is.
        let quarter = sampled
            .iter()
            .min_by(|a, b| (a.y - 190.0).abs().total_cmp(&(b.y - 190.0).abs()))
            .unwrap();
        assert!((quarter.x - 67.0).abs() < 0.2, "quarter at {quarter:?}");

        // The tips leave the chord at a right angle, as the corners of a rounded bracket do.
        let arriving = bracket.cubics[0].cp1 - bracket.cubics[0].start;
        assert!(arriving.normalize().dot(Vector2::new(0.0, 1.0)).abs() < 1e-9);
        let leaving = bracket.cubics[1].end - bracket.cubics[1].cp2;
        assert!(leaving.normalize().dot(Vector2::new(0.0, 1.0)).abs() < 1e-9);
    }

    #[test]
    fn curly_brace_has_a_straight_body_and_a_cusp_half_way_out() {
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 300.0);
        let depth = 36.0;
        let brace = Curve::new_curly_brace(start, end, depth, Vector2::new(1.0, 0.0))
            .expect("a curly brace");

        let sampled = brace.sampled(64);
        // The ends stay where they were drawn.
        assert!((sampled[0] - start).length() < 1e-9);
        assert!((*sampled.last().unwrap() - end).length() < 1e-9);

        // The body runs straight half the depth to the side of the tips. The arms leave it towards
        // the cusp from the middle, so only the stretch between the tips and the arms is a body.
        let tip = depth * CURLY_TIP_SHARE;
        for point in sampled
            .iter()
            .filter(|point| (130.0..165.0).contains(&point.y) || (235.0..270.0).contains(&point.y))
        {
            assert!((point.x - (100.0 + tip)).abs() < 0.5, "body at {point:?}");
        }

        // The cusp is the deepest point of the brace, in the middle of its height.
        let apex = sampled.iter().fold(sampled[0], |deepest, point| {
            if point.x > deepest.x { *point } else { deepest }
        });
        assert!((apex.x - (100.0 + depth)).abs() < 0.5, "apex at {apex:?}");
        assert!((apex.y - 200.0).abs() < 0.5, "apex at {apex:?}");
    }

    #[test]
    fn curly_brace_cusp_is_a_point_and_not_a_rounded_bend() {
        let brace = Curve::new_curly_brace(
            Vector2::new(100.0, 100.0),
            Vector2::new(100.0, 300.0),
            36.0,
            Vector2::new(1.0, 0.0),
        )
        .expect("a curly brace");

        // The two arms meet at the cusp with different tangents: a corner, not a smooth bend.
        let top_arm = &brace.cubics[2];
        let bottom_arm = &brace.cubics[3];
        let arriving = top_arm.end - top_arm.cp2;
        let leaving = bottom_arm.cp1 - bottom_arm.start;
        let turn = arriving.normalize().angle_to(leaving.normalize()).abs();
        assert!(turn > 60.0_f64.to_radians(), "the cusp turns {turn} rad");
    }

    #[test]
    fn tmp_debug_rounded() {
        let brace = Curve::new_rounded_brace(
            Vector2::new(160.0, 100.0),
            Vector2::new(160.0, 400.0),
            54.0,
            Vector2::new(-1.0, 0.0),
        )
        .expect("a rounded brace");
        for (i, c) in brace.cubics.iter().enumerate() {
            println!("{i}: {c:?}");
        }
        let sampled = brace.sampled(64);
        let deepest = sampled.iter().fold(f64::MAX, |x, p| x.min(p.x));
        println!("deepest {deepest}");
    }

    #[test]
    fn curly_brace_curls_to_the_side_it_is_asked_for() {
        let start = Vector2::new(100.0, 100.0);
        let end = Vector2::new(100.0, 300.0);
        let right = Curve::new_curly_brace(start, end, 36.0, Vector2::new(1.0, 0.0))
            .expect("a curly brace");
        let left = Curve::new_curly_brace(start, end, 36.0, Vector2::new(-1.0, 0.0))
            .expect("a curly brace");

        let right_deepest = right.sampled(16).iter().fold(f64::MIN, |x, p| x.max(p.x));
        let left_deepest = left.sampled(16).iter().fold(f64::MAX, |x, p| x.min(p.x));
        assert!((right_deepest - 136.0).abs() < 0.5, "{right_deepest}");
        assert!((left_deepest - 64.0).abs() < 0.5, "{left_deepest}");
    }
}
