// Imports
use super::peaks::open_turn_angles;
use super::{dist_to_segment, resample};
use crate::shapes::{Polyline, Shape};
use p2d::math::Vector2;
use std::f64::consts::PI;

const POLYLINE_N_RESAMPLE: usize = 96;
/// The minimum turn (radians) at a corner of an open polyline.
const POLYLINE_MIN_TURN: f64 = 25.0 * PI / 180.0;
/// A corner this close to an end of the stroke is an end hook, not a vertex.
const POLYLINE_END_MARGIN: usize = 5;
/// The minimum separation of two corners, in resampled points.
const POLYLINE_MIN_SEPARATION: usize = 8;
/// The maximum number of corners of an open polyline.
const POLYLINE_MAX_CORNERS: usize = 6;
/// The minimum length of each segment, as a share of the whole stroke.
const POLYLINE_MIN_SEGMENT_SHARE: f64 = 0.06;
/// The fit tolerance of the polyline, relative to the bounds diagonal.
const POLYLINE_MAX_MEAN_DEV_RATIO: f64 = 0.045;
const POLYLINE_MAX_DEV_RATIO: f64 = 0.10;

/// Recognizes an open stroke with straight segments and sharp corners, like a checkmark, a
/// lightning bolt or a zigzag.
///
/// A smooth curve has no corner this sharp, and a line has no corner at all; both are recognized
/// before this and never reach it.
pub(super) fn recognize_polyline(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 8 {
        return None;
    }

    let resampled = resample(points, POLYLINE_N_RESAMPLE, false);
    let smoothed = smooth_open(&resampled, 1);
    let turns = open_turn_angles(&smoothed, 3);
    let magnitudes = turns.iter().map(|t| t.abs()).collect::<Vec<_>>();

    let mut candidates = (POLYLINE_END_MARGIN..POLYLINE_N_RESAMPLE - POLYLINE_END_MARGIN)
        .filter(|&i| {
            let value = magnitudes[i];
            value > POLYLINE_MIN_TURN
                && value >= magnitudes[i - 1]
                && value >= magnitudes[i + 1]
                && value >= magnitudes[i.saturating_sub(2)]
                && value >= magnitudes[(i + 2).min(magnitudes.len() - 1)]
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|&a, &b| magnitudes[b].total_cmp(&magnitudes[a]));

    let cyclic_linear_dist = |a: usize, b: usize| a.abs_diff(b);
    let mut corners: Vec<usize> = Vec::new();
    for i in candidates {
        if corners
            .iter()
            .all(|&c| cyclic_linear_dist(c, i) >= POLYLINE_MIN_SEPARATION)
        {
            corners.push(i);
        }
    }
    corners.sort_unstable();

    if corners.is_empty() || corners.len() > POLYLINE_MAX_CORNERS {
        return None;
    }
    if corners.iter().any(|&i| magnitudes[i] > 155.0 * PI / 180.0) {
        return None;
    }

    let mut vertices = Vec::with_capacity(corners.len() + 2);
    vertices.push(resampled[0]);
    vertices.extend(corners.iter().map(|&i| resampled[i]));
    vertices.push(*resampled.last().unwrap());

    let total_len = super::polyline_len(&vertices);
    if total_len < f64::EPSILON {
        return None;
    }
    for pair in vertices.windows(2) {
        if (pair[1] - pair[0]).length() < POLYLINE_MIN_SEGMENT_SHARE * total_len {
            return None;
        }
    }

    let mut sum = 0.0;
    let mut max: f64 = 0.0;
    for p in &resampled {
        let mut best = f64::INFINITY;
        for pair in vertices.windows(2) {
            best = best.min(dist_to_segment(*p, pair[0], pair[1]));
        }
        sum += best;
        max = max.max(best);
    }
    let mean_dev = sum / resampled.len() as f64;
    if mean_dev > POLYLINE_MAX_MEAN_DEV_RATIO * bounds_diag
        || max > POLYLINE_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    Some(Shape::Polyline(Polyline {
        start: vertices[0],
        path: vertices[1..].to_vec(),
    }))
}

fn smooth_open(points: &[Vector2], iterations: usize) -> Vec<Vector2> {
    let n = points.len();
    let mut smoothed = points.to_vec();
    for _ in 0..iterations {
        let prev = smoothed.clone();
        for i in 1..n.saturating_sub(1) {
            smoothed[i] = (prev[i - 1] + prev[i] * 2.0 + prev[i + 1]) * 0.25;
        }
    }
    smoothed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampled(vertices: &[Vector2], per_segment: usize) -> Vec<Vector2> {
        let mut points = Vec::new();
        for pair in vertices.windows(2) {
            for j in 0..per_segment {
                points.push(pair[0] + (pair[1] - pair[0]) * (j as f64 / per_segment as f64));
            }
        }
        points.push(*vertices.last().unwrap());
        points
    }

    #[test]
    fn recognizes_a_checkmark() {
        let points = sampled(
            &[
                Vector2::new(0.0, 40.0),
                Vector2::new(30.0, 70.0),
                Vector2::new(90.0, 0.0),
            ],
            30,
        );
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_polyline(&points, (max - min).length()).is_some());
    }

    #[test]
    fn recognizes_a_zigzag() {
        let points = sampled(
            &[
                Vector2::new(0.0, 0.0),
                Vector2::new(30.0, 60.0),
                Vector2::new(60.0, 0.0),
                Vector2::new(90.0, 60.0),
            ],
            25,
        );
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_polyline(&points, (max - min).length()).is_some());
    }

    #[test]
    fn rejects_a_smooth_arc() {
        let points = (0..96)
            .map(|i| {
                let angle = PI * i as f64 / 95.0;
                Vector2::new(100.0 * angle.cos(), 100.0 * angle.sin())
            })
            .collect::<Vec<_>>();
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_polyline(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_straight_line() {
        let points = (0..96)
            .map(|i| Vector2::new(i as f64, 0.0))
            .collect::<Vec<_>>();
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_polyline(&points, (max - min).length()).is_none());
    }
}
