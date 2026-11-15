// Imports
use super::closedcurve::outline_deviation;
use super::peaks::{angle_gap, find_cyclic_peaks, max_abs_turn, signed_turn_angles};
use super::{dist_to_segment, resample, smooth_loop};
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

/// The minimum turn (radians) at a pentagram corner.
const PENTAGRAM_MIN_TURN: f64 = 100.0 * PI / 180.0;
/// The maximum turn (radians) at a pentagram corner. A tiny loop at a tip peaks at nearly a half
/// turn, so this only rejects a corner folded back on itself.
const PENTAGRAM_MAX_TURN: f64 = PI;
/// The minimum separation of two pentagram corners, in resampled points. It is wider than the
/// outline star separation so that a small loop or a spike at a corner cannot become a second
/// corner.
const PENTAGRAM_MIN_SEPARATION: usize = STAR_N_RESAMPLE / 8;
/// How much the drawn corner radii may differ; a pentagram is often drawn by hand.
const PENTAGRAM_MAX_RADIUS_VARIATION: f64 = 0.45;
/// How much the stroke may stray from the straight chords between its corners.
const PENTAGRAM_MAX_MEAN_DEV_RATIO: f64 = 0.075;
const PENTAGRAM_MAX_DEV_RATIO: f64 = 0.20;
/// The radius of the notches of a regular five-pointed star outline, relative to the tips.
const PENTAGRAM_INNER_RATIO: f64 = 0.381_966_011_250_105;
/// A pentagram crosses itself five times; small loops at the tips add a few more crossings and a
/// loop may also add a corner of its own, so the crossings limit the false positives where the
/// corner count alone would not.
const PENTAGRAM_MIN_CROSSINGS: usize = 5;
const PENTAGRAM_MAX_CROSSINGS: usize = 10;

/// Recognizes a five-pointed star drawn as one self-crossing stroke (a pentagram).
///
/// A pentagram is five straight chords between five sharp tips, so its stroke crosses itself five
/// times; neither a simple star outline nor a pentagon crosses itself and both have more or
/// shallower corners. The snapped shape is the regular five-pointed star outline, a simple
/// polygon.
pub(super) fn recognize_pentagram(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 16 {
        return None;
    }

    let resampled = resample(points, STAR_N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 1);
    let turns = signed_turn_angles(&smoothed, 3);

    let corners = find_turn_peaks(&turns, PENTAGRAM_MIN_TURN, PENTAGRAM_MIN_SEPARATION);
    if corners.len() != 5 {
        return None;
    }
    if corners
        .iter()
        .any(|&i| !(PENTAGRAM_MIN_TURN..=PENTAGRAM_MAX_TURN).contains(&turns[i].abs()))
    {
        return None;
    }

    let crossings = self_intersections(&smoothed);
    if !(PENTAGRAM_MIN_CROSSINGS..=PENTAGRAM_MAX_CROSSINGS).contains(&crossings) {
        return None;
    }

    let vertices = corners.iter().map(|&i| resampled[i]).collect::<Vec<_>>();
    let center = vertices.iter().copied().sum::<Vector2>() / vertices.len() as f64;
    let radii = vertices
        .iter()
        .map(|p| (*p - center).length())
        .collect::<Vec<_>>();
    let tip_radius = mean(&radii);
    if tip_radius < f64::EPSILON
        || coefficient_of_variation(&radii) > PENTAGRAM_MAX_RADIUS_VARIATION
    {
        return None;
    }

    // The drawn chords must be straight: measure the stroke against the five corner-to-corner
    // segments, not against the outline of a simple pentagon.
    let mut dev_sum = 0.0;
    let mut dev_max: f64 = 0.0;
    for p in &resampled {
        let dist = (0..vertices.len())
            .map(|i| dist_to_segment(*p, vertices[i], vertices[(i + 1) % vertices.len()]))
            .fold(f64::MAX, f64::min);
        dev_sum += dist;
        dev_max = dev_max.max(dist);
    }
    let dev_mean = dev_sum / resampled.len() as f64;
    if dev_mean > PENTAGRAM_MAX_MEAN_DEV_RATIO * bounds_diag
        || dev_max > PENTAGRAM_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    // Align the regular star with the drawn tips: a five-fold symmetric set has a well-defined
    // phase, while the individual corners may be uneven.
    let (mut sin_sum, mut cos_sum) = (0.0, 0.0);
    for p in &vertices {
        let v = *p - center;
        let angle = v.y.atan2(v.x) * 5.0;
        sin_sum += angle.sin();
        cos_sum += angle.cos();
    }
    let phase = sin_sum.atan2(cos_sum) / 5.0;
    let step = TAU / 5.0;

    let mut outline = Vec::with_capacity(10);
    for k in 0..5 {
        let tip_angle = phase + k as f64 * step;
        outline.push(center + Vector2::from_angle(tip_angle) * tip_radius);
        outline.push(
            center
                + Vector2::from_angle(tip_angle + step * 0.5) * tip_radius * PENTAGRAM_INNER_RATIO,
        );
    }

    Some(Shape::Polygon(Polygon {
        start: outline[0],
        path: outline[1..].to_vec(),
    }))
}

/// Finds the strongest turns of either sign, keeping the peaks apart by at least `min_separation`
/// resampled points.
fn find_turn_peaks(turns: &[f64], threshold: f64, min_separation: usize) -> Vec<usize> {
    let n = turns.len();
    let mut candidates = (0..n)
        .filter(|&i| {
            let turn = turns[i].abs();
            turn > threshold
                && turn >= turns[(i + n - 1) % n].abs()
                && turn >= turns[(i + 1) % n].abs()
                && turn >= turns[(i + n - 2) % n].abs()
                && turn >= turns[(i + 2) % n].abs()
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|&a, &b| turns[b].abs().total_cmp(&turns[a].abs()));

    let cyclic_dist = |a: usize, b: usize| {
        let d = a.abs_diff(b);
        d.min(n - d)
    };

    let mut peaks = Vec::new();
    for i in candidates {
        if peaks.iter().all(|&p| cyclic_dist(p, i) >= min_separation) {
            peaks.push(i);
        }
    }

    peaks.sort_unstable();
    peaks
}

/// Counts the proper crossings of a closed polyline.
fn self_intersections(points: &[Vector2]) -> usize {
    let n = points.len();
    let mut crossings = 0;

    for i in 0..n {
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if segments_cross(
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            ) {
                crossings += 1;
            }
        }
    }

    crossings
}

fn segments_cross(a: Vector2, b: Vector2, c: Vector2, d: Vector2) -> bool {
    let cross = |u: Vector2, v: Vector2| u.x * v.y - u.y * v.x;
    let o1 = cross(b - a, c - a);
    let o2 = cross(b - a, d - a);
    let o3 = cross(d - c, a - c);
    let o4 = cross(d - c, b - c);

    o1 * o2 < 0.0 && o3 * o4 < 0.0
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

    fn pentagram(
        rotation: f64,
        radius_jitter: f64,
        angle_jitter: f64,
        loop_size: f64,
    ) -> Vec<Vector2> {
        let mut vertices = Vec::new();
        for k in 0..5 {
            let wobble = 1.0 + radius_jitter * ((k as f64 * 2.1).sin());
            let angle = rotation + k as f64 * TAU / 5.0 + angle_jitter * (k as f64 * 1.7).cos();
            vertices.push(Vector2::from_angle(angle) * 100.0 * wobble);
        }
        let order = [0, 2, 4, 1, 3, 0];
        let mut points = Vec::new();
        for pair in order.windows(2) {
            let a = vertices[pair[0]];
            let b = vertices[pair[1]];
            if loop_size > 0.0 && pair[0] == 0 {
                // A small loop at the corner before continuing along the chord.
                for i in 0..=16 {
                    let angle = TAU * i as f64 / 16.0;
                    points.push(a + Vector2::from_angle(angle) * loop_size);
                }
            }
            for j in 0..=24 {
                points.push(a + (b - a) * (j as f64 / 24.0));
            }
        }
        points
    }

    #[test]
    fn recognizes_a_pentagram() {
        let points = pentagram(-PI * 0.5, 0.0, 0.0, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        match recognize_pentagram(&points, (max - min).length()) {
            Some(Shape::Polygon(polygon)) => assert_eq!(polygon.path.len() + 1, 10),
            other => panic!("expected a star polygon, got {other:?}"),
        }
    }

    #[test]
    fn recognizes_a_rough_pentagram() {
        let points = pentagram(0.4, 0.18, 0.10, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_pentagram(&points, (max - min).length()).is_some());
    }

    #[test]
    fn recognizes_a_pentagram_with_a_small_corner_loop() {
        let points = pentagram(0.25, 0.12, 0.08, 4.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_pentagram(&points, (max - min).length()).is_some());
    }

    #[test]
    fn rejects_a_pentagon_as_a_pentagram() {
        let mut points = Vec::new();
        for k in 0..5 {
            let a = Vector2::from_angle(k as f64 * TAU / 5.0) * 100.0;
            let b = Vector2::from_angle((k + 1) as f64 * TAU / 5.0) * 100.0;
            for j in 0..=24 {
                points.push(a + (b - a) * (j as f64 / 24.0));
            }
        }
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_pentagram(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_star_outline_as_a_pentagram() {
        let points = star_outline(5, 0.40, 0.0, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_pentagram(&points, (max - min).length()).is_none());
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
