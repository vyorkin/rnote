// Imports
use p2d::math::Vector2;
use std::f64::consts::TAU;

/// The signed turn angles at each point of a closed loop.
///
/// Positive and negative turns alternate between convex and concave corners; which sign is which
/// depends on the direction the loop was drawn, so the recognizers compare magnitudes instead of
/// fixing a direction.
pub(super) fn signed_turn_angles(points: &[Vector2], window: usize) -> Vec<f64> {
    let n = points.len();

    (0..n)
        .map(|i| {
            let prev = points[(i + n - window) % n];
            let next = points[(i + window) % n];
            let v_in = points[i] - prev;
            let v_out = next - points[i];

            if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
                0.0
            } else {
                v_in.angle_to(v_out)
            }
        })
        .collect()
}

/// The signed turn angles at each point of an open polyline, ends excluded.
pub(super) fn open_turn_angles(points: &[Vector2], window: usize) -> Vec<f64> {
    let n = points.len();
    let mut turns = vec![0.0; n];

    for i in window..n.saturating_sub(window) {
        let v_in = points[i] - points[i - window];
        let v_out = points[i + window] - points[i];

        if v_in.length() > f64::EPSILON && v_out.length() > f64::EPSILON {
            turns[i] = v_in.angle_to(v_out);
        }
    }

    turns
}

/// Find the cyclic peaks of the given sign in a periodic sequence.
///
/// A peak is a local extremum of `sign * values` above `threshold`. Peaks closer than
/// `min_separation` (cyclically) are merged, the strongest one kept. Returns indices in
/// ascending order.
pub(super) fn find_cyclic_peaks(
    values: &[f64],
    sign: f64,
    threshold: f64,
    min_separation: usize,
) -> Vec<usize> {
    let n = values.len();
    if n < 3 {
        return Vec::new();
    }

    let score = |i: usize| sign * values[i];
    let mut candidates = (0..n)
        .filter(|&i| {
            let s = score(i);
            s > threshold
                && s >= score((i + n - 1) % n)
                && s >= score((i + 1) % n)
                && s >= score((i + n - 2) % n)
                && s >= score((i + 2) % n)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|&a, &b| score(b).total_cmp(&score(a)));

    let cyclic_dist = |a: usize, b: usize| {
        let d = a.abs_diff(b);
        d.min(n - d)
    };

    let mut peaks: Vec<usize> = Vec::new();
    for i in candidates {
        if peaks.iter().all(|&p| cyclic_dist(p, i) >= min_separation) {
            peaks.push(i);
        }
    }

    peaks.sort_unstable();
    peaks
}

/// The positive angular distance between two angles, in `[0, TAU)`.
pub(super) fn angle_gap(a: f64, b: f64) -> f64 {
    let gap = (b - a).rem_euclid(TAU);
    gap.min(TAU - gap)
}

/// The maximum absolute turn angle in the sequence.
pub(super) fn max_abs_turn(turns: &[f64]) -> f64 {
    turns.iter().fold(0.0, |max, turn| max.max(turn.abs()))
}
