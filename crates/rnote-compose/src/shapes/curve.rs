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

        let at = |along: f64, across: f64| start + direction * along + normal * across;
        let handle = QUARTER_ARC_HANDLE * tip;
        let arm_handle = CURLY_ARM_HANDLE * arm;
        let apex = at(middle, depth);
        let apex_vector = Vector2::new(CURLY_APEX_ANGLE.cos(), CURLY_APEX_ANGLE.sin());

        let straight = |from: Vector2, to: Vector2| CubicBezier {
            start: from,
            cp1: from.lerp(to, 1.0 / 3.0),
            cp2: from.lerp(to, 2.0 / 3.0),
            end: to,
        };

        let top_tip = CubicBezier {
            start,
            cp1: start + normal * handle,
            cp2: at(tip - handle, tip),
            end: at(tip, tip),
        };
        let top_body = straight(at(tip, tip), at(middle - arm, tip));
        let top_arm = CubicBezier {
            start: at(middle - arm, tip),
            cp1: at(middle - arm + arm_handle, tip),
            cp2: apex
                - direction * (apex_vector.x * arm_handle)
                - normal * (apex_vector.y * arm_handle),
            end: apex,
        };
        let bottom_arm = CubicBezier {
            start: apex,
            cp1: apex + direction * (apex_vector.x * arm_handle)
                - normal * (apex_vector.y * arm_handle),
            cp2: at(middle + arm - arm_handle, tip),
            end: at(middle + arm, tip),
        };
        let bottom_body = straight(at(middle + arm, tip), at(chord - tip, tip));
        let bottom_tip = CubicBezier {
            start: at(chord - tip, tip),
            cp1: at(chord - tip + handle, tip),
            cp2: end + normal * handle,
            end,
        };

        let mut cubics = Vec::with_capacity(6);
        cubics.push(top_tip);
        if (top_body.end - top_body.start).length() > f64::EPSILON {
            cubics.push(top_body);
        }
        cubics.push(top_arm);
        cubics.push(bottom_arm);
        if (bottom_body.end - bottom_body.start).length() > f64::EPSILON {
            cubics.push(bottom_body);
        }
        cubics.push(bottom_tip);

        Some(Self { cubics })
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
        for point in sampled.iter().filter(|point| {
            (130.0..165.0).contains(&point.y) || (235.0..270.0).contains(&point.y)
        }) {
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
