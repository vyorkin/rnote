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
}
