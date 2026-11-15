// Imports
use super::closedcurve::{outline_deviation, sample_curve};
use super::peaks::{find_cyclic_peaks, signed_turn_angles};
use super::{resample, smooth_loop};
use crate::shapes::{CubicBezier, Curve, Shape};
use p2d::math::Vector2;
use std::f64::consts::PI;

const HEART_N_RESAMPLE: usize = 128;
/// The minimum turn at the bottom tip and at the top notch.
const HEART_TIP_MIN_TURN: f64 = 30.0 * PI / 180.0;
const HEART_CUSP_MIN_TURN: f64 = 25.0 * PI / 180.0;
/// The allowed ratio between the width and the distance from the notch to the tip.
const HEART_MIN_ASPECT: f64 = 0.60;
const HEART_MAX_ASPECT: f64 = 2.00;
/// The fit tolerance of the snapped heart, relative to the bounds diagonal.
const HEART_MAX_MEAN_DEV_RATIO: f64 = 0.090;
const HEART_MAX_DEV_RATIO: f64 = 0.24;

/// The canonical heart, in local coordinates: the notch at the origin, the tip at `(0, 1)`,
/// the lobes rising to `y ≈ -0.46`, the widest points at `x ≈ ±0.8`.
fn canonical_heart() -> Curve {
    let cusp = Vector2::ZERO;
    let tip = Vector2::new(0.0, 1.0);
    Curve {
        cubics: vec![
            CubicBezier {
                start: tip,
                cp1: Vector2::new(-0.25, 0.70),
                cp2: Vector2::new(-0.85, 0.35),
                end: Vector2::new(-0.80, 0.02),
            },
            CubicBezier {
                start: Vector2::new(-0.80, 0.02),
                cp1: Vector2::new(-0.78, -0.32),
                cp2: Vector2::new(-0.30, -0.46),
                end: cusp,
            },
            CubicBezier {
                start: cusp,
                cp1: Vector2::new(0.30, -0.46),
                cp2: Vector2::new(0.78, -0.32),
                end: Vector2::new(0.80, 0.02),
            },
            CubicBezier {
                start: Vector2::new(0.80, 0.02),
                cp1: Vector2::new(0.85, 0.35),
                cp2: Vector2::new(0.25, 0.70),
                end: tip,
            },
        ],
    }
}

/// Recognizes a heart drawn as one stroke.
///
/// A heart has exactly two sharp turns with opposite signs: the rounded bottom tip (convex) and
/// the notch between the two lobes (concave). The notch-to-tip axis gives the orientation, the
/// width gives the scale, and the stroke has to lie close to a canonical heart in that frame.
pub(super) fn recognize_heart(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 16 {
        return None;
    }

    let resampled = resample(points, HEART_N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 1);
    let turns = signed_turn_angles(&smoothed, 3);

    let convex = find_cyclic_peaks(&turns, 1.0, HEART_TIP_MIN_TURN, HEART_N_RESAMPLE / 16);
    let concave = find_cyclic_peaks(&turns, -1.0, HEART_CUSP_MIN_TURN, HEART_N_RESAMPLE / 16);

    // A heart has one strong convex corner, the tip; a loop at the start can add one more. A star
    // or a scribble has several, and fitting any one of them to the template would otherwise
    // accept the whole scribble.
    if convex.len() > 2 {
        return None;
    }

    // The sign of a turn depends on the direction the stroke was drawn, so the tip and the cusp
    // are not known by their sign alone. Every pair of opposite-sign corners is fitted to the
    // canonical heart and the best fit wins: a small loop at the start is then just a worse fit
    // than the true tip and notch, not a reason to reject the whole heart.
    let mut best: Option<(Shape, f64)> = None;
    let corners = convex
        .iter()
        .chain(concave.iter())
        .copied()
        .collect::<Vec<_>>();
    for &a in &corners {
        for &b in &corners {
            if a == b || turns[a] * turns[b] >= 0.0 {
                continue;
            }
            for (tip_index, cusp_index) in [(a, b), (b, a)] {
                if turns[tip_index].abs() < HEART_TIP_MIN_TURN
                    || turns[cusp_index].abs() < HEART_CUSP_MIN_TURN
                {
                    continue;
                }
                if let Some((shape, score)) =
                    heart_at(&resampled, tip_index, cusp_index, bounds_diag)
                    && best
                        .as_ref()
                        .is_none_or(|(_, best_score)| score < *best_score)
                {
                    best = Some((shape, score));
                }
            }
        }
    }
    best.map(|(shape, _)| shape)
}

/// Fit the canonical heart to the stroke with `tip_index` and `cusp_index` as its two corners.
///
/// Returns the snapped curve and its fit error relative to the bounds diagonal, or `None` when
/// the two corners do not frame a heart-shaped stroke.
fn heart_at(
    resampled: &[Vector2],
    tip_index: usize,
    cusp_index: usize,
    bounds_diag: f64,
) -> Option<(Shape, f64)> {
    // The two corners are on opposite sides of the heart.
    let n = resampled.len();
    let cyclic_dist = tip_index.abs_diff(cusp_index);
    let cyclic_dist = cyclic_dist.min(n - cyclic_dist);
    if cyclic_dist < n / 5 || cyclic_dist > n * 4 / 5 {
        return None;
    }

    let tip = resampled[tip_index];
    let cusp = resampled[cusp_index];
    let axis = tip - cusp;
    let height = axis.length();
    if height < 0.35 * bounds_diag {
        return None;
    }
    let axis = axis / height;
    let perp = axis.rotate(Vector2::from_angle(PI * 0.5));

    let local = resampled
        .iter()
        .map(|p| {
            let v = *p - cusp;
            Vector2::new(v.dot(perp), v.dot(axis))
        })
        .collect::<Vec<_>>();
    let (min, max) = super::points_bounds(&local);
    let width = max.x - min.x;
    if width < f64::EPSILON {
        return None;
    }
    let aspect = width / height;
    if !(HEART_MIN_ASPECT..=HEART_MAX_ASPECT).contains(&aspect) {
        return None;
    }
    // The lobes rise above the notch, and nothing goes far past the tip.
    if min.y > -0.05 * height || max.y > 1.15 * height {
        return None;
    }
    // The notch and the tip are centered between the lobes.
    if (max.x + min.x).abs() > 0.20 * width {
        return None;
    }

    let template = canonical_heart();
    let samples = sample_curve(&template, 20);
    let scale_x = width / 2.0 / 0.80;
    let world_template = samples
        .iter()
        .map(|p| cusp + perp * (p.x * scale_x) + axis * (p.y * height))
        .collect::<Vec<_>>();

    let (mean_dev, max_dev) = outline_deviation(resampled, &world_template);
    if mean_dev > HEART_MAX_MEAN_DEV_RATIO * bounds_diag
        || max_dev > HEART_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    let cubics = template
        .cubics
        .iter()
        .map(|cubic| CubicBezier {
            start: cusp + perp * (cubic.start.x * scale_x) + axis * (cubic.start.y * height),
            cp1: cusp + perp * (cubic.cp1.x * scale_x) + axis * (cubic.cp1.y * height),
            cp2: cusp + perp * (cubic.cp2.x * scale_x) + axis * (cubic.cp2.y * height),
            end: cusp + perp * (cubic.end.x * scale_x) + axis * (cubic.end.y * height),
        })
        .collect();

    Some((
        Shape::Curve(Curve { cubics }),
        mean_dev / bounds_diag + 0.25 * max_dev / bounds_diag,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn sampled_heart(width: f64, height: f64, rotation: f64, jitter: f64) -> Vec<Vector2> {
        let template = canonical_heart();
        let samples = sample_curve(&template, 24);
        let axis = Vector2::from_angle(rotation);
        let perp = axis.rotate(Vector2::from_angle(PI * 0.5));
        samples
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let wobble = 1.0 + jitter * (i as f64 * 1.7).sin();
                perp * (p.x * width * 0.5 * wobble) + axis * (p.y * height * wobble - height * 0.2)
            })
            .collect()
    }

    #[test]
    fn recognizes_a_heart() {
        let points = sampled_heart(100.0, 80.0, 0.0, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_heart(&points, (max - min).length()).is_some());
    }

    #[test]
    fn recognizes_a_rotated_heart() {
        let points = sampled_heart(110.0, 90.0, 0.5, 0.01);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_heart(&points, (max - min).length()).is_some());
    }

    #[test]
    fn recognizes_a_heart_with_a_loop_at_the_start() {
        let mut points = sampled_heart(100.0, 80.0, 0.0, 0.01);
        let at = points.len() / 4;
        let p = points[at];
        let loop_points = (0..8)
            .map(|k| p + Vector2::from_angle(k as f64 / 8.0 * TAU) * 3.0)
            .collect::<Vec<_>>();
        points.splice(at..at, loop_points);
        let (min, max) = super::super::points_bounds(&points);
        assert!(
            recognize_heart(&points, (max - min).length()).is_some(),
            "a small loop at the start is not a different shape"
        );
    }

    #[test]
    fn rejects_a_star_scribble() {
        let mut points = Vec::new();
        let n = 5;
        for i in 0..n * 2 {
            let angle = i as f64 / (n * 2) as f64 * TAU;
            let radius = if i % 2 == 0 { 100.0 } else { 45.0 };
            let vertex = Vector2::from_angle(angle) * radius;
            let next_angle = (i + 1) as f64 / (n * 2) as f64 * TAU;
            let next_radius = if (i + 1) % 2 == 0 { 100.0 } else { 45.0 };
            let next = Vector2::from_angle(next_angle) * next_radius;
            for step in 0..8 {
                points.push(vertex.lerp(next, step as f64 / 8.0));
            }
        }
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_heart(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_lens() {
        let points = (0..64)
            .map(|i| {
                let angle = i as f64 / 64.0 * TAU;
                Vector2::new(100.0 * angle.cos(), 40.0 * angle.sin())
            })
            .collect::<Vec<_>>();
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_heart(&points, (max - min).length()).is_none());
    }
}
