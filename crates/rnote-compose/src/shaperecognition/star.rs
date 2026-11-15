// Imports
use super::closedcurve::outline_deviation;
use super::peaks::{angle_gap, find_cyclic_peaks, max_abs_turn, signed_turn_angles};
use super::{resample, smooth_loop};
use crate::shapes::{Polygon, Shape};
use p2d::math::Vector2;
use std::f64::consts::{PI, TAU};

/// The number of points the loop is resampled to for the corner peaks.
const STAR_N_RESAMPLE: usize = 192;
/// The minimum separation of two corners, in resampled points.
const STAR_MIN_SEPARATION: usize = 6;
/// The minimum turn (radians) at a star tip.
const STAR_TIP_MIN_TURN: f64 = 70.0 * PI / 180.0;
/// The minimum turn (radians) at a star notch, measured as a negative turn.
const STAR_NOTCH_MIN_TURN: f64 = 30.0 * PI / 180.0;
/// The allowed number of star points. A six-pointed star is the widest common one a single
/// outline stroke has; more points are a flower or a scribble.
const STAR_MIN_POINTS: usize = 5;
const STAR_MAX_POINTS: usize = 6;
/// How much the radii of the tips (and of the notches) may differ.
const STAR_MAX_RADIUS_VARIATION: f64 = 0.30;
/// The allowed ratio between the notch and the tip radius.
const STAR_MIN_INNER_RATIO: f64 = 0.20;
const STAR_MAX_INNER_RATIO: f64 = 0.80;
/// How much an angular step may differ from the even one, as a fraction of the step.
const STAR_MAX_ANGLE_DEVIATION: f64 = 0.35;
/// The fit tolerance of the snapped star, relative to the bounds diagonal.
const STAR_MAX_MEAN_DEV_RATIO: f64 = 0.055;
const STAR_MAX_DEV_RATIO: f64 = 0.14;

/// Recognizes a five- or six-pointed star outline drawn as one stroke.
///
/// The stroke alternates sharp tips and inward notches: the tips turn by more than the notches,
/// they are evenly spaced, and the notch radius is a fixed fraction of the tip radius. A
/// pentagram (a self-crossing star drawn over itself) has no inward notches and is recognized as
/// a polygon instead.
pub(super) fn recognize_star(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 16 {
        return None;
    }

    let resampled = resample(points, STAR_N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 1);
    let turns = signed_turn_angles(&smoothed, 3);

    let mut positive = find_cyclic_peaks(&turns, 1.0, STAR_TIP_MIN_TURN, STAR_MIN_SEPARATION);
    let mut negative = find_cyclic_peaks(&turns, -1.0, STAR_NOTCH_MIN_TURN, STAR_MIN_SEPARATION);

    // Which sign is a tip depends on the direction the loop was drawn: the tips turn harder than
    // the notches, so the stronger set is the tips.
    let mean_abs = |indices: &[usize]| {
        if indices.is_empty() {
            0.0
        } else {
            indices.iter().map(|&i| turns[i].abs()).sum::<f64>() / indices.len() as f64
        }
    };
    if mean_abs(&negative) > mean_abs(&positive) {
        std::mem::swap(&mut positive, &mut negative);
    }
    let (tips, notches) = (positive, negative);

    if tips.len() != notches.len() || !(STAR_MIN_POINTS..=STAR_MAX_POINTS).contains(&tips.len()) {
        return None;
    }
    let point_count = tips.len();
    let step = TAU / point_count as f64;

    // Tips and notches must alternate around the loop.
    let mut merged = tips
        .iter()
        .map(|&i| (i, true))
        .chain(notches.iter().map(|&i| (i, false)))
        .collect::<Vec<_>>();
    merged.sort_unstable_by_key(|(i, _)| *i);
    if merged.len() != 2 * point_count || merged.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return None;
    }

    let center = resampled.iter().copied().sum::<Vector2>() / resampled.len() as f64;
    let tip_points = tips.iter().map(|&i| resampled[i]).collect::<Vec<_>>();
    let notch_points = notches.iter().map(|&i| resampled[i]).collect::<Vec<_>>();
    let tip_radii = tip_points
        .iter()
        .map(|p| (*p - center).length())
        .collect::<Vec<_>>();
    let notch_radii = notch_points
        .iter()
        .map(|p| (*p - center).length())
        .collect::<Vec<_>>();
    let tip_radius = mean(&tip_radii);
    let notch_radius = mean(&notch_radii);
    if tip_radius < f64::EPSILON {
        return None;
    }
    let inner_ratio = notch_radius / tip_radius;
    if !(STAR_MIN_INNER_RATIO..=STAR_MAX_INNER_RATIO).contains(&inner_ratio) {
        return None;
    }
    if coefficient_of_variation(&tip_radii) > STAR_MAX_RADIUS_VARIATION
        || coefficient_of_variation(&notch_radii) > STAR_MAX_RADIUS_VARIATION
    {
        return None;
    }

    let tip_angles = tip_points
        .iter()
        .map(|p| {
            let v = *p - center;
            v.y.atan2(v.x)
        })
        .collect::<Vec<_>>();
    let notch_angles = notch_points
        .iter()
        .map(|p| {
            let v = *p - center;
            v.y.atan2(v.x)
        })
        .collect::<Vec<_>>();
    if !evenly_spaced(&tip_angles, step) || !evenly_spaced(&notch_angles, step) {
        return None;
    }
    // The notches sit halfway between the tips. The tips are evenly spaced around the full
    // circle, so their circular mean is undefined (the vectors cancel out); one of them gives
    // the phase just as well.
    let phase = tip_angles[0];
    let mut sorted_tips = tip_angles.clone();
    sorted_tips.sort_by(f64::total_cmp);
    let mut sorted_notches = notch_angles.clone();
    sorted_notches.sort_by(f64::total_cmp);
    let tip_to_notch = angle_gap(sorted_tips[0], sorted_notches[0]).rem_euclid(step);
    if (tip_to_notch - step * 0.5).abs() > step * STAR_MAX_ANGLE_DEVIATION {
        return None;
    }

    let tip_turn = mean_abs(&tips);
    let notch_turn = mean_abs(&notches);
    if tip_turn < 1.25 * notch_turn || max_abs_turn(&turns) > 175.0 * PI / 180.0 {
        return None;
    }

    // Snap to a regular star with the drawn orientation, tip radius and notch depth.
    let mut vertices = Vec::with_capacity(2 * point_count);
    for k in 0..point_count {
        let tip_angle = phase + k as f64 * step;
        let notch_angle = tip_angle + step * 0.5;
        vertices.push(center + Vector2::from_angle(tip_angle) * tip_radius);
        vertices.push(center + Vector2::from_angle(notch_angle) * notch_radius);
    }

    let mut outline = vertices.clone();
    outline.push(vertices[0]);
    let (mean_dev, max_dev) = outline_deviation(&resampled, &outline);
    if mean_dev > STAR_MAX_MEAN_DEV_RATIO * bounds_diag
        || max_dev > STAR_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    Some(Shape::Polygon(Polygon {
        start: vertices[0],
        path: vertices[1..].to_vec(),
    }))
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn coefficient_of_variation(values: &[f64]) -> f64 {
    let m = mean(values);
    if m < f64::EPSILON {
        return f64::INFINITY;
    }
    let variance = values.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / values.len() as f64;
    variance.sqrt() / m
}

fn evenly_spaced(angles: &[f64], step: f64) -> bool {
    if angles.len() < 2 {
        return false;
    }
    let mut sorted = angles.to_vec();
    sorted.sort_by(f64::total_cmp);
    (0..sorted.len()).all(|i| {
        let next = if i + 1 < sorted.len() {
            sorted[i + 1]
        } else {
            sorted[0] + TAU
        };
        let gap = next - sorted[i];
        (gap - step).abs() <= step * STAR_MAX_ANGLE_DEVIATION
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star_outline(
        point_count: usize,
        inner_ratio: f64,
        rotation: f64,
        jitter: f64,
    ) -> Vec<Vector2> {
        let mut vertices = Vec::new();
        let step = TAU / point_count as f64;
        for k in 0..2 * point_count {
            let angle = rotation + k as f64 * step * 0.5;
            let radius = if k % 2 == 0 {
                100.0
            } else {
                100.0 * inner_ratio
            };
            let wobble = 1.0 + jitter * ((k as f64 * 2.3).sin());
            vertices.push(Vector2::from_angle(angle) * radius * wobble);
        }
        let mut points = Vec::new();
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            for j in 0..10 {
                points.push(a + (b - a) * (j as f64 / 10.0));
            }
        }
        points.push(vertices[0]);
        points
    }

    fn assert_star(points: &[Vector2], expected_points: usize) {
        let (min, max) = super::super::points_bounds(points);
        let shape = recognize_star(points, (max - min).length());
        match shape {
            Some(Shape::Polygon(polygon)) => {
                assert_eq!(polygon.path.len() + 1, 2 * expected_points);
            }
            other => panic!("expected a star polygon, got {other:?}"),
        }
    }

    #[test]
    fn recognizes_a_five_pointed_star() {
        assert_star(&star_outline(5, 0.40, 0.0, 0.0), 5);
    }

    #[test]
    fn recognizes_a_rotated_six_pointed_star() {
        assert_star(&star_outline(6, 0.45, 0.7, 0.01), 6);
    }

    #[test]
    fn rejects_a_pentagram() {
        let mut vertices = Vec::new();
        for k in 0..5 {
            let angle = -PI * 0.5 + k as f64 * TAU * 2.0 / 5.0;
            vertices.push(Vector2::from_angle(angle) * 100.0);
        }
        let mut points = Vec::new();
        for i in 0..5 {
            let a = vertices[i];
            let b = vertices[(i + 1) % 5];
            for j in 0..20 {
                points.push(a + (b - a) * (j as f64 / 20.0));
            }
        }
        points.push(vertices[0]);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_star(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_smooth_polygon() {
        let mut points = Vec::new();
        for k in 0..6 {
            let angle = k as f64 * TAU / 6.0;
            points.push(Vector2::from_angle(angle) * 100.0);
        }
        points.push(points[0]);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_star(&points, (max - min).length()).is_none());
    }
}
