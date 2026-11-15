// Imports
use super::dist_to_segment;
use crate::shapes::{CubicBezier, Curve};
use p2d::math::Vector2;
use std::f64::consts::TAU;

/// Build a smooth closed curve through the given outline points.
///
/// The points are the corners of a coarse outline; the curve is a Catmull-Rom spline through
/// them, closed back to the first point. Used by the shapes that have no canonical parametrization
/// of their own (a snapped cloud or speech bubble keeps the lobes and the tail that were drawn).
pub(super) fn closed_curve_through(points: &[Vector2]) -> Curve {
    closed_curve_with_sharp(points, &[])
}

/// Like [`closed_curve_through`], with sharp corners kept at the given indices.
///
/// At a sharp point the tangents of the two adjacent segments point straight at it instead of
/// along the spline, so a tail tip stays pointed.
pub(super) fn closed_curve_with_sharp(points: &[Vector2], sharp: &[usize]) -> Curve {
    let n = points.len();
    if n < 3 {
        return Curve::default();
    }

    let is_sharp = |i: usize| sharp.contains(&(i % n));
    let mut cubics = Vec::with_capacity(n);

    for i in 0..n {
        let p_prev = points[(i + n - 1) % n];
        let p = points[i];
        let p_next = points[(i + 1) % n];
        let p_next2 = points[(i + 2) % n];

        let cp1 = if is_sharp(i) {
            p
        } else {
            p + (p_next - p_prev) / 6.0
        };
        let cp2 = if is_sharp((i + 1) % n) {
            p_next
        } else {
            p_next - (p_next2 - p) / 6.0
        };

        cubics.push(CubicBezier {
            start: p,
            cp1,
            cp2,
            end: p_next,
        });
    }

    Curve { cubics }
}

/// Sample the cubics of a curve into a polyline, `per_cubic` points per segment.
pub(super) fn sample_curve(curve: &Curve, per_cubic: usize) -> Vec<Vector2> {
    let mut points = Vec::with_capacity(curve.cubics.len() * per_cubic + 1);

    for cubic in &curve.cubics {
        for j in 0..per_cubic {
            let t = j as f64 / per_cubic as f64;
            points.push(cubic_point(cubic.start, cubic.cp1, cubic.cp2, cubic.end, t));
        }
    }
    if let Some(last) = curve.cubics.last() {
        points.push(last.end);
    }

    points
}

/// The mean and maximum distance of the points from a closed outline.
///
/// The outline is treated as closed: the last segment returns to the first point.
pub(super) fn outline_deviation(points: &[Vector2], outline: &[Vector2]) -> (f64, f64) {
    if points.is_empty() || outline.len() < 2 {
        return (f64::INFINITY, f64::INFINITY);
    }

    let mut sum = 0.0;
    let mut max: f64 = 0.0;
    for p in points {
        let mut best = f64::INFINITY;
        for pair in outline.windows(2) {
            best = best.min(dist_to_segment(*p, pair[0], pair[1]));
        }
        best = best.min(dist_to_segment(
            *p,
            *outline.last().unwrap(),
            *outline.first().unwrap(),
        ));
        sum += best;
        max = max.max(best);
    }

    (sum / points.len() as f64, max)
}

/// The point on a cubic bezier at the given parameter.
fn cubic_point(start: Vector2, cp1: Vector2, cp2: Vector2, end: Vector2, t: f64) -> Vector2 {
    let mt = 1.0 - t;
    start * (mt * mt * mt)
        + cp1 * (3.0 * mt * mt * t)
        + cp2 * (3.0 * mt * t * t)
        + end * (t * t * t)
}

/// Sample a closed shape given as a radius function of the angle around `center`.
pub(super) fn sample_polar(
    center: Vector2,
    samples: usize,
    radius: impl Fn(f64) -> f64,
) -> Vec<Vector2> {
    (0..samples)
        .map(|i| {
            let angle = i as f64 / samples as f64 * TAU;
            center + Vector2::from_angle(angle) * radius(angle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_curve_through_a_square_stays_near_it() {
        let square = [
            Vector2::new(0.0, 0.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(100.0, 100.0),
            Vector2::new(0.0, 100.0),
        ];
        let curve = closed_curve_through(&square);
        assert_eq!(curve.cubics.len(), 4);
        let samples = sample_curve(&curve, 16);
        let (mean, max) = outline_deviation(&square, &samples);
        assert!(mean < 10.0, "mean {mean}");
        assert!(max < 20.0, "max {max}");
    }

    #[test]
    fn a_sharp_point_keeps_its_corner() {
        let points = [
            Vector2::new(0.0, 0.0),
            Vector2::new(50.0, 50.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(50.0, 100.0),
        ];
        let curve = closed_curve_with_sharp(&points, &[1]);
        let tip = curve.cubics[0].end;
        assert!((tip - points[1]).length() < f64::EPSILON);
        assert!((curve.cubics[0].cp2 - points[1]).length() < f64::EPSILON);
        assert!((curve.cubics[1].cp1 - points[1]).length() < f64::EPSILON);
    }
}
