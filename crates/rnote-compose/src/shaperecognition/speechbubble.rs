// Imports
use super::closedcurve::{closed_curve_with_sharp, outline_deviation, sample_curve};
use super::{resample, smooth_loop};
use crate::shapes::Shape;
use p2d::math::Vector2;
use std::f64::consts::PI;

const BUBBLE_N_RESAMPLE: usize = 128;
/// The allowed ratio between the long and the short side of the body.
const BUBBLE_MIN_ASPECT: f64 = 1.15;
const BUBBLE_MAX_ASPECT: f64 = 4.0;
/// The corner radius of the rounded body, as a share of its shorter side.
const BUBBLE_CORNER_RADIUS_RATIO: f64 = 0.18;
/// The distance from the body that is still considered the outline, as a share of the shorter side.
const BUBBLE_OUTLINE_TOLERANCE_RATIO: f64 = 0.07;
/// The share of points that has to lie on the rounded body.
const BUBBLE_MIN_ON_BODY_RATIO: f64 = 0.72;
/// The minimum protrusion of the tail beyond the body, as a share of the shorter side.
const BUBBLE_MIN_TAIL_RATIO: f64 = 0.10;
/// The maximum protrusion of the tail beyond the body, as a share of the longer side.
const BUBBLE_MAX_TAIL_RATIO: f64 = 0.55;
/// The allowed share of the outline the tail may occupy.
const BUBBLE_MAX_TAIL_SHARE: f64 = 0.30;
/// The fit tolerance of the snapped bubble, relative to the bounds diagonal.
const BUBBLE_MAX_MEAN_DEV_RATIO: f64 = 0.080;
const BUBBLE_MAX_DEV_RATIO: f64 = 0.20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// Recognizes a speech bubble drawn as one closed stroke.
///
/// The body has to fit a rounded rectangle; a single contiguous bulge beyond it is the tail.
/// The snapped bubble is built from that body and tail, so the tail stays where it was drawn.
pub(super) fn recognize_speech_bubble(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    if points.len() < 24 {
        return None;
    }

    let resampled = resample(points, BUBBLE_N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 1);

    let center = smoothed.iter().copied().sum::<Vector2>() / smoothed.len() as f64;
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in &smoothed {
        let v = *p - center;
        xx += v.x * v.x;
        xy += v.x * v.y;
        yy += v.y * v.y;
    }
    let mut angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    if yy > xx {
        angle += PI * 0.5;
    }
    // A tail pulls the covariance off the body axes, so the angle is refined to the smallest
    // bounding box around that estimate.
    let mut best_area = f64::INFINITY;
    for step in -12..=12 {
        let candidate = angle + step as f64 * (0.5 * PI / 180.0);
        let axis = Vector2::from_angle(candidate);
        let perp = axis.rotate(Vector2::from_angle(PI * 0.5));
        let (min, max) = super::points_bounds(
            &smoothed
                .iter()
                .map(|p| Vector2::new(p.dot(axis), p.dot(perp)))
                .collect::<Vec<_>>(),
        );
        let area = (max.x - min.x) * (max.y - min.y);
        if area < best_area {
            best_area = area;
            angle = candidate;
        }
    }
    let axis = Vector2::from_angle(angle);
    let perp = axis.rotate(Vector2::from_angle(PI * 0.5));

    let local = smoothed
        .iter()
        .map(|p| Vector2::new((*p - center).dot(axis), (*p - center).dot(perp)))
        .collect::<Vec<_>>();
    // The bounds include the tail, so the body is measured by the outermost dense band of
    // points on each side: a rounded box has a long flat side there, a tail only a thin tip.
    let (min_x, max_x) = robust_extent(local.iter().map(|p| p.x));
    let (min_y, max_y) = robust_extent(local.iter().map(|p| p.y));
    let half_w = (max_x - min_x) * 0.5;
    let half_h = (max_y - min_y) * 0.5;
    let short = half_w.min(half_h);
    let long = half_w.max(half_h);
    if short < f64::EPSILON {
        return None;
    }
    let aspect = long / short;
    if !(BUBBLE_MIN_ASPECT..=BUBBLE_MAX_ASPECT).contains(&aspect) {
        return None;
    }
    let center_local = Vector2::new((max_x + min_x) * 0.5, (max_y + min_y) * 0.5);
    let local = local.iter().map(|p| *p - center_local).collect::<Vec<_>>();
    let radius = (BUBBLE_CORNER_RADIUS_RATIO * short)
        .clamp(0.06 * short, 0.35 * short)
        .min(half_w * 0.9)
        .min(half_h * 0.9);

    let sdf = local
        .iter()
        .map(|p| rounded_box_sdf(*p, Vector2::new(half_w, half_h), radius))
        .collect::<Vec<_>>();
    let tolerance = BUBBLE_OUTLINE_TOLERANCE_RATIO * short;
    let on_body = sdf.iter().filter(|d| d.abs() <= tolerance).count();
    if on_body < (BUBBLE_MIN_ON_BODY_RATIO * sdf.len() as f64) as usize {
        return None;
    }

    let tail_threshold = BUBBLE_MIN_TAIL_RATIO * short;
    let tail_indices = sdf
        .iter()
        .enumerate()
        .filter(|(_, d)| **d > tail_threshold)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let groups = contiguous_groups(&tail_indices, sdf.len());
    if groups.len() != 1 {
        return None;
    }
    let group = &groups[0];
    if group.len() < 3 || group.len() > (BUBBLE_MAX_TAIL_SHARE * sdf.len() as f64) as usize {
        return None;
    }
    let (tip_index, protrusion) = group
        .iter()
        .map(|&i| (i, sdf[i]))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if protrusion < BUBBLE_MIN_TAIL_RATIO * short || protrusion > BUBBLE_MAX_TAIL_RATIO * long {
        return None;
    }

    let side = nearest_side(
        group.iter().map(|&i| local[i]).sum::<Vector2>() / group.len() as f64,
        half_w,
        half_h,
    );
    let along = |p: Vector2| match side {
        Side::Top | Side::Bottom => p.x,
        Side::Left | Side::Right => p.y,
    };
    // The contour points just outside the tail mark where it meets the body: that is the base.
    let len = local.len();
    let base_before = local[(group[0] + len - 1) % len];
    let base_after = local[(group[group.len() - 1] + 1) % len];
    let (side_start, side_end) = match side {
        Side::Top | Side::Bottom => (-half_w + radius, half_w - radius),
        Side::Left | Side::Right => (-half_h + radius, half_h - radius),
    };
    let t0 = along(base_before)
        .min(along(base_after))
        .clamp(side_start + 0.02 * short, side_end - 0.02 * short);
    let t1 = along(base_before)
        .max(along(base_after))
        .clamp(side_start + 0.02 * short, side_end - 0.02 * short);
    if t1 - t0 < 0.08 * short {
        return None;
    }
    let tip_along = match side {
        Side::Top | Side::Bottom => local[tip_index].x,
        Side::Left | Side::Right => local[tip_index].y,
    }
    .clamp(t0, t1);

    let world = |p: Vector2| center + axis * p.x + perp * p.y;
    let (outline, sharp) = bubble_outline(BubbleOutline {
        half_w,
        half_h,
        radius,
        side,
        t0,
        t1,
        tip_along,
        protrusion,
    });
    let outline = outline.iter().map(|p| world(*p)).collect::<Vec<_>>();
    let curve = closed_curve_with_sharp(&outline, &sharp);
    let samples = sample_curve(&curve, 16);
    let (mean_dev, max_dev) = outline_deviation(&resampled, &samples);
    if mean_dev > BUBBLE_MAX_MEAN_DEV_RATIO * bounds_diag
        || max_dev > BUBBLE_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    Some(Shape::Curve(curve))
}

/// The extent of the densest part of a projection: the outermost bin that still holds at least
/// a few percent of the points. Sparse points beyond it are a tail, not the body.
fn robust_extent(values: impl Iterator<Item = f64>) -> (f64, f64) {
    const BINS: usize = 64;
    const MIN_SHARE: f64 = 0.04;
    let values = values.collect::<Vec<_>>();
    let (min, max) = values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| {
            (min.min(*v), max.max(*v))
        });
    let span = max - min;
    if span <= f64::EPSILON {
        return (min, max);
    }
    let mut counts = [0usize; BINS];
    for v in values.iter() {
        let bin = (((*v - min) / span * BINS as f64) as usize).min(BINS - 1);
        counts[bin] += 1;
    }
    let threshold = ((values.len() as f64 * MIN_SHARE) as usize).max(2);
    let first = counts
        .iter()
        .position(|&count| count >= threshold)
        .unwrap_or(0);
    let last = counts
        .iter()
        .rposition(|&count| count >= threshold)
        .unwrap_or(BINS - 1);
    let bin = span / BINS as f64;
    (min + first as f64 * bin, min + (last + 1) as f64 * bin)
}

fn rounded_box_sdf(p: Vector2, half: Vector2, radius: f64) -> f64 {
    let q = Vector2::new(p.x.abs(), p.y.abs()) - half + Vector2::splat(radius);
    let outside = Vector2::new(q.x.max(0.0), q.y.max(0.0)).length();
    let inside = q.x.max(q.y).min(0.0);
    outside + inside - radius
}

fn nearest_side(p: Vector2, half_w: f64, half_h: f64) -> Side {
    let distances = [
        (Side::Top, (p.y + half_h).abs()),
        (Side::Right, (p.x - half_w).abs()),
        (Side::Bottom, (p.y - half_h).abs()),
        (Side::Left, (p.x + half_w).abs()),
    ];
    distances
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(side, _)| side)
        .unwrap_or(Side::Bottom)
}

/// The contiguous cyclic groups of the given indices.
fn contiguous_groups(indices: &[usize], n: usize) -> Vec<Vec<usize>> {
    if indices.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current = vec![indices[0]];
    for &index in &indices[1..] {
        if index == *current.last().unwrap() + 1 {
            current.push(index);
        } else {
            groups.push(current);
            current = vec![index];
        }
    }
    groups.push(current);

    if groups.len() > 1
        && groups.first().unwrap()[0] == 0
        && *groups.last().unwrap().last().unwrap() == n - 1
    {
        let last = groups.pop().unwrap();
        let mut merged = last;
        merged.extend(groups.remove(0));
        groups.insert(0, merged);
    }
    groups
}

/// The rounded body and the tail inserted into one of its sides.
struct BubbleOutline {
    half_w: f64,
    half_h: f64,
    radius: f64,
    side: Side,
    t0: f64,
    t1: f64,
    tip_along: f64,
    protrusion: f64,
}

/// Build the rounded-rectangle outline with the tail inserted on the given side.
fn bubble_outline(bubble: BubbleOutline) -> (Vec<Vector2>, Vec<usize>) {
    let BubbleOutline {
        half_w,
        half_h,
        radius,
        side,
        t0,
        t1,
        tip_along,
        protrusion,
    } = bubble;
    let mut points = Vec::new();
    let mut sharp = Vec::new();
    let arc = |center: Vector2, from: f64, to: f64| {
        (0..=6)
            .map(|i| {
                let angle = from + (to - from) * i as f64 / 6.0;
                center + Vector2::from_angle(angle) * radius
            })
            .collect::<Vec<_>>()
    };

    // Top side, left to right.
    let tail = (side == Side::Top).then(|| {
        (
            Vector2::new(t0, -half_h),
            Vector2::new(t1, -half_h),
            Vector2::new(tip_along, -half_h - protrusion),
        )
    });
    push_side(
        &mut points,
        &mut sharp,
        Vector2::new(-half_w + radius, -half_h),
        Vector2::new(half_w - radius, -half_h),
        tail,
    );
    // Top-right corner.
    points.extend(arc(
        Vector2::new(half_w - radius, -half_h + radius),
        -PI * 0.5,
        0.0,
    ));
    // Right side, top to bottom.
    let tail = (side == Side::Right).then(|| {
        (
            Vector2::new(half_w, t0),
            Vector2::new(half_w, t1),
            Vector2::new(half_w + protrusion, tip_along),
        )
    });
    push_side(
        &mut points,
        &mut sharp,
        Vector2::new(half_w, -half_h + radius),
        Vector2::new(half_w, half_h - radius),
        tail,
    );
    // Bottom-right corner.
    points.extend(arc(
        Vector2::new(half_w - radius, half_h - radius),
        0.0,
        PI * 0.5,
    ));
    // Bottom side, right to left.
    let tail = (side == Side::Bottom).then(|| {
        (
            Vector2::new(t1, half_h),
            Vector2::new(t0, half_h),
            Vector2::new(tip_along, half_h + protrusion),
        )
    });
    push_side(
        &mut points,
        &mut sharp,
        Vector2::new(half_w - radius, half_h),
        Vector2::new(-half_w + radius, half_h),
        tail,
    );
    // Bottom-left corner.
    points.extend(arc(
        Vector2::new(-half_w + radius, half_h - radius),
        PI * 0.5,
        PI,
    ));
    // Left side, bottom to top.
    let tail = (side == Side::Left).then(|| {
        (
            Vector2::new(-half_w, t1),
            Vector2::new(-half_w, t0),
            Vector2::new(-half_w - protrusion, tip_along),
        )
    });
    push_side(
        &mut points,
        &mut sharp,
        Vector2::new(-half_w, half_h - radius),
        Vector2::new(-half_w, -half_h + radius),
        tail,
    );
    // Top-left corner.
    points.extend(arc(
        Vector2::new(-half_w + radius, -half_h + radius),
        PI,
        PI * 1.5,
    ));

    dedupe(points, sharp)
}

fn push_side(
    points: &mut Vec<Vector2>,
    sharp: &mut Vec<usize>,
    start: Vector2,
    end: Vector2,
    tail: Option<(Vector2, Vector2, Vector2)>,
) {
    points.push(start);
    let Some((base0, base1, tip)) = tail else {
        points.push(end);
        return;
    };
    let direction = end - start;
    let length_sq = direction.length_squared();
    if length_sq < f64::EPSILON {
        return;
    }
    let param = |p: Vector2| ((p - start).dot(direction) / length_sq).clamp(0.0, 1.0);
    let (t0, t1) = (
        param(base0).min(param(base1)),
        param(base0).max(param(base1)),
    );
    let p0 = start + direction * t0;
    let p1 = start + direction * t1;
    let edge = |points: &mut Vec<Vector2>, from: Vector2, to: Vector2, sharp: &mut Vec<usize>| {
        for k in 1..=4 {
            points.push(from + (to - from) * (k as f64 / 4.0));
        }
        sharp.push(points.len() - 1);
    };
    edge(points, start, p0, sharp);
    edge(points, p0, tip, sharp);
    edge(points, tip, p1, sharp);
    edge(points, p1, end, sharp);
}

/// Remove consecutive duplicate points, keeping the sharp indices pointing at the kept points.
fn dedupe(points: Vec<Vector2>, sharp: Vec<usize>) -> (Vec<Vector2>, Vec<usize>) {
    let mut out = Vec::with_capacity(points.len());
    let mut sharp_out = Vec::new();
    let mut last_sharp = None;
    for (i, p) in points.into_iter().enumerate() {
        if sharp.contains(&i) {
            last_sharp = Some(out.len());
        }
        if out
            .last()
            .is_none_or(|q: &Vector2| (*q - p).length() > 1e-9)
        {
            out.push(p);
        }
    }
    if let Some(index) = last_sharp
        && index < out.len()
    {
        sharp_out.push(index);
    }
    (out, sharp_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    /// A rounded rectangle body with a tail hanging from one side.
    fn bubble(
        half_w: f64,
        half_h: f64,
        radius: f64,
        side: Side,
        tail_center: f64,
        tail_width: f64,
        tail_length: f64,
    ) -> Vec<Vector2> {
        let (outline, _) = bubble_outline(BubbleOutline {
            half_w,
            half_h,
            radius,
            side,
            t0: tail_center - tail_width * 0.5,
            t1: tail_center + tail_width * 0.5,
            tip_along: tail_center,
            protrusion: tail_length,
        });
        let curve = closed_curve_with_sharp(&outline, &[]);
        sample_curve(&curve, 12)
    }

    #[test]
    fn recognizes_a_bubble_with_a_bottom_tail() {
        let points = bubble(100.0, 60.0, 14.0, Side::Bottom, -30.0, 30.0, 30.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(
            recognize_speech_bubble(&points, (max - min).length()).is_some(),
            "bottom tail"
        );
    }

    #[test]
    fn recognizes_a_bubble_with_a_right_tail() {
        let points = bubble(100.0, 60.0, 14.0, Side::Right, 10.0, 26.0, 26.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(
            recognize_speech_bubble(&points, (max - min).length()).is_some(),
            "right tail"
        );
    }

    #[test]
    fn rejects_a_rounded_rectangle_without_a_tail() {
        let points = bubble(100.0, 60.0, 14.0, Side::Bottom, -30.0, 0.0, 0.0);
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_speech_bubble(&points, (max - min).length()).is_none());
    }

    #[test]
    fn rejects_a_cloud() {
        let points = (0..128)
            .map(|i| {
                let angle = i as f64 / 128.0 * TAU;
                let radius = 100.0 * (1.0 + 0.15 * (5.0 * angle).cos());
                Vector2::from_angle(angle) * radius
            })
            .collect::<Vec<_>>();
        let (min, max) = super::super::points_bounds(&points);
        assert!(recognize_speech_bubble(&points, (max - min).length()).is_none());
    }
}
