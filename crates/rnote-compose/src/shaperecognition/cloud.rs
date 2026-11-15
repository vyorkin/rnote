// Imports
use super::closedcurve::{closed_curve_through, outline_deviation, sample_polar};
use super::peaks::{angle_gap, find_cyclic_peaks, max_abs_turn, signed_turn_angles};
use super::{resample, smooth_loop};
use crate::shapes::Shape;
use p2d::math::Vector2;
use std::f64::consts::{PI, TAU};

const CLOUD_N_RESAMPLE: usize = 128;
/// The allowed number of cloud lobes.
const CLOUD_MIN_LOBES: usize = 4;
const CLOUD_MAX_LOBES: usize = 8;
/// The minimum radial variation of a scalloped outline, as a share of the mean radius.
const CLOUD_MIN_AMPLITUDE: f64 = 0.10;
const CLOUD_MAX_AMPLITUDE: f64 = 0.65;
/// The allowed ratio between the notch and the lobe radius.
const CLOUD_MIN_INNER_RATIO: f64 = 0.55;
const CLOUD_MAX_INNER_RATIO: f64 = 0.95;
/// How much an angular step between lobes may differ from the even one, as a fraction of it.
const CLOUD_MAX_ANGLE_DEVIATION: f64 = 0.40;
/// The maximum absolute turn anywhere on a cloud: a scalloped outline turns gently, a polygon
/// or a star has sharp corners.
const CLOUD_MAX_TURN: f64 = 60.0 * PI / 180.0;
/// A run of points this long with an almost constant direction is a straight side, not a cloud.
const CLOUD_MAX_STRAIGHT_RUN_RATIO: f64 = 0.10;
const CLOUD_STRAIGHT_TURN: f64 = 3.0 * PI / 180.0;
/// The fit tolerance of the snapped cloud, relative to the bounds diagonal.
const CLOUD_MAX_MEAN_DEV_RATIO: f64 = 0.070;
const CLOUD_MAX_DEV_RATIO: f64 = 0.18;

/// Recognizes a cloud drawn as one closed stroke.
///
/// A cloud is a scalloped loop: four to eight smooth lobes around the center, with inward notches
/// between them and no straight sides. The snapped cloud keeps the number, the phase and the depth
/// of the lobes that were drawn.
pub(super) fn recognize_cloud(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 24 {
        return None;
    }

    let resampled = resample(points, CLOUD_N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 2);
    let turns = signed_turn_angles(&smoothed, 3);
    if max_abs_turn(&turns) > CLOUD_MAX_TURN || has_long_straight_run(&resampled) {
        return None;
    }

    let center = smoothed.iter().copied().sum::<Vector2>() / smoothed.len() as f64;
    let mut radii = smoothed
        .iter()
        .map(|p| (*p - center).length())
        .collect::<Vec<_>>();
    smooth_radii(&mut radii, 2);
    let mean_radius = radii.iter().sum::<f64>() / radii.len() as f64;
    if mean_radius < f64::EPSILON {
        return None;
    }
    let (min_radius, max_radius) = radii
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(min, max), r| {
            (min.min(*r), max.max(*r))
        });
    let amplitude = (max_radius - min_radius) / mean_radius;
    if !(CLOUD_MIN_AMPLITUDE..=CLOUD_MAX_AMPLITUDE).contains(&amplitude) {
        return None;
    }

    let centered = radii.iter().map(|r| r - mean_radius).collect::<Vec<_>>();
    let threshold = 0.25 * (max_radius - min_radius);
    let min_separation = CLOUD_N_RESAMPLE / (2 * CLOUD_MAX_LOBES);
    let mut lobes = find_cyclic_peaks(&centered, 1.0, threshold, min_separation);
    let mut notches = find_cyclic_peaks(&centered, -1.0, threshold, min_separation);
    if notches.len() > lobes.len() {
        std::mem::swap(&mut lobes, &mut notches);
    }
    let lobe_count = lobes.len();
    if !(CLOUD_MIN_LOBES..=CLOUD_MAX_LOBES).contains(&lobe_count) || notches.len() != lobe_count {
        return None;
    }

    let mut merged = lobes
        .iter()
        .map(|&i| (i, true))
        .chain(notches.iter().map(|&i| (i, false)))
        .collect::<Vec<_>>();
    merged.sort_unstable_by_key(|(i, _)| *i);
    if merged.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return None;
    }

    let lobe_radius = mean(&lobes.iter().map(|&i| radii[i]).collect::<Vec<_>>());
    let notch_radius = mean(&notches.iter().map(|&i| radii[i]).collect::<Vec<_>>());
    if lobe_radius < f64::EPSILON {
        return None;
    }
    let inner_ratio = notch_radius / lobe_radius;
    if !(CLOUD_MIN_INNER_RATIO..=CLOUD_MAX_INNER_RATIO).contains(&inner_ratio) {
        return None;
    }

    let lobe_angles = lobes
        .iter()
        .map(|&i| angle_of(smoothed[i] - center))
        .collect::<Vec<_>>();
    let step = TAU / lobe_count as f64;
    if !evenly_spaced(&lobe_angles, step) {
        return None;
    }
    // The lobes are evenly spaced around the full circle, so their circular mean is undefined
    // (the vectors cancel out); one of them gives the phase just as well.
    let phase = lobe_angles[0];
    let mid = (lobe_radius + notch_radius) * 0.5;
    let lobe_amplitude = lobe_radius - notch_radius;

    let samples = sample_polar(center, 8 * lobe_count, |angle| {
        mid + lobe_amplitude * 0.5 * (lobe_count as f64 * (angle - phase)).cos()
    });
    let curve = closed_curve_through(&samples);
    let outline = super::closedcurve::sample_curve(&curve, 16);
    let (mean_dev, max_dev) = outline_deviation(&resampled, &outline);
    if mean_dev > CLOUD_MAX_MEAN_DEV_RATIO * bounds_diag
        || max_dev > CLOUD_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    Some(Shape::Curve(curve))
}

fn angle_of(v: Vector2) -> f64 {
    v.y.atan2(v.x)
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn smooth_radii(radii: &mut [f64], iterations: usize) {
    let n = radii.len();
    for _ in 0..iterations {
        let prev = radii.to_vec();
        for i in 0..n {
            radii[i] = (prev[(i + n - 1) % n] + prev[i] * 2.0 + prev[(i + 1) % n]) * 0.25;
        }
    }
}

fn has_long_straight_run(points: &[Vector2]) -> bool {
    let n = points.len();
    let max_run = (n as f64 * CLOUD_MAX_STRAIGHT_RUN_RATIO).max(4.0) as usize;
    let mut run = 0;
    let mut prev_direction = 0.0;
    for i in 0..n {
        let direction = angle_of(points[(i + 1) % n] - points[i]);
        if i > 0 && angle_gap(prev_direction, direction) < CLOUD_STRAIGHT_TURN {
            run += 1;
            if run >= max_run {
                return true;
            }
        } else {
            run = 0;
        }
        prev_direction = direction;
    }
    false
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
        (gap - step).abs() <= step * CLOUD_MAX_ANGLE_DEVIATION
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalloped(lobes: usize, amplitude: f64, rotation: f64, jitter: f64) -> Vec<Vector2> {
        let samples = 256;
        (0..samples)
            .map(|i| {
                let angle = i as f64 / samples as f64 * TAU;
                let wobble = 1.0 + jitter * (i as f64 * 0.7).sin();
                let radius = 100.0
                    * (1.0 + amplitude * 0.5 * (lobes as f64 * (angle - rotation)).cos())
                    * wobble;
                Vector2::from_angle(angle) * radius
            })
            .collect()
    }

    #[test]
    fn recognizes_a_cloud() {
        let points = scalloped(5, 0.35, 0.2, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_cloud(&points, (max - min).length()).is_some());
    }

    #[test]
    fn recognizes_a_six_lobed_cloud() {
        let points = scalloped(6, 0.30, 1.1, 0.01);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_cloud(&points, (max - min).length()).is_some());
    }

    #[test]
    fn rejects_a_circle() {
        let points = scalloped(0, 0.0, 0.0, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_cloud(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_rectangle() {
        let points = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(150.0, 0.0),
            Vector2::new(150.0, 100.0),
            Vector2::new(0.0, 100.0),
            Vector2::new(0.0, 0.0),
        ];
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_cloud(&points, (max - min).length()).is_none());
    }
}
