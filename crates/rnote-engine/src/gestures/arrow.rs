//! Recognizing an arrow drawn in two strokes: the shaft and the V-shaped head.
//!
//! A shaft that another stroke doubles, as the two bars of the "⇒" sign double each other, is no
//! arrow: pairing one of the bars with the V-shaped head would leave the other bar behind.

use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use rnote_compose::penpath::PenPath;
use rnote_compose::shapes::Arrow;

/// The shortest shaft (in surface coordinates) an arrow needs.
const MIN_SHAFT_SURFACE: f64 = 30.0;
/// How far the shaft may bow away from the straight line between its ends, relative to its length.
const SHAFT_MAX_DEVIATION_RATIO: f64 = 0.10;
/// The shortest barb relative to the shaft: shorter barbs are noise next to the shaft.
const MIN_BARB_RATIO: f64 = 0.08;
/// The longest barb relative to the shaft: longer strokes are more likely another drawing.
const MAX_BARB_RATIO: f64 = 0.55;
/// How much longer one barb may be than the other.
const MAX_BARB_LENGTH_DIFF: f64 = 2.2;
/// The angle (radians) a barbs makes with the other at the tip.
const MIN_HEAD_ANGLE: f64 = 20.0 * std::f64::consts::PI / 180.0;
const MAX_HEAD_ANGLE: f64 = 120.0 * std::f64::consts::PI / 180.0;
/// How far (in surface coordinates) the head may sit from the shaft end.
const HEAD_END_TOLERANCE_SURFACE: f64 = 25.0;
/// The deviation (in surface coordinates) the head may have from its two straight segments.
const HEAD_STRAIGHTNESS_TOLERANCE_SURFACE: f64 = 6.0;
/// The angle a stroke has to keep to the shaft to run alongside it.
const TWIN_MAX_ANGLE: f64 = 10.0 * std::f64::consts::PI / 180.0;
/// How far (in surface coordinates) a stroke doubling the shaft may run from it.
const TWIN_MAX_OFFSET_SURFACE: f64 = 40.0;
/// How far a stroke doubling the shaft may run from it, relative to the shaft's length.
const TWIN_MAX_OFFSET_RATIO: f64 = 0.20;
/// How much of the shaft's length a doubling stroke has to run alongside it.
const TWIN_MIN_OVERLAP_RATIO: f64 = 0.5;
/// How much shorter or longer a doubling stroke may be than the shaft.
const TWIN_MAX_LENGTH_DIFF_RATIO: f64 = 0.5;

/// Recognize a straight finished stroke as the shaft of an arrow.
///
/// The shaft is fitted here instead of being sent through the shape recognizer: a shaft drawn
/// with a slight bow is a parenthesis to that recognizer, which then hides the line from the arrow
/// candidate. Only the straightness of the stroke between its own ends matters for a shaft.
pub(crate) fn line_candidate(path: &PenPath, zoom: f64) -> Option<Arrow> {
    let points = flatten(path);
    let &start = points.first()?;
    let &tip = points.last()?;
    let shaft = Arrow { start, tip };
    let shaft_len = (tip - start).length();
    if shaft_len * zoom < MIN_SHAFT_SURFACE {
        return None;
    }

    let max_dev = points
        .iter()
        .map(|p| dist_to_segment(*p, start, tip))
        .fold(0.0, f64::max);
    (max_dev <= SHAFT_MAX_DEVIATION_RATIO * shaft_len).then_some(shaft)
}

/// Recognize the second stroke as the head of an arrow whose shaft is `shaft`.
///
/// The head may sit at either end of the shaft; the returned arrow points from the far end of
/// the shaft through the head.
pub(crate) fn recognize_head(shaft: &Arrow, path: &PenPath, zoom: f64) -> Option<Arrow> {
    recognize_head_at(shaft.start, shaft.tip, path, zoom)
        .or_else(|| recognize_head_at(shaft.tip, shaft.start, path, zoom))
}

/// The largest distance from the shaft's line a stroke doubling it may lie at.
fn twin_max_offset(shaft_len: f64, zoom: f64) -> f64 {
    (TWIN_MAX_OFFSET_SURFACE / zoom).max(shaft_len * TWIN_MAX_OFFSET_RATIO)
}

/// The area around `shaft` that a stroke doubling it can lie in.
pub(crate) fn twin_search_bounds(shaft: &Arrow, zoom: f64) -> Aabb {
    let margin = twin_max_offset((shaft.tip - shaft.start).length(), zoom);
    Aabb::new(
        Vector2::new(
            shaft.start.x.min(shaft.tip.x),
            shaft.start.y.min(shaft.tip.y),
        ) - Vector2::splat(margin),
        Vector2::new(
            shaft.start.x.max(shaft.tip.x),
            shaft.start.y.max(shaft.tip.y),
        ) + Vector2::splat(margin),
    )
}

/// Whether one of `others` doubles the shaft, running alongside it.
///
/// The bars of a "⇒" sign double each other, so a pair of one bar and the V-shaped head looks like
/// an arrow to [line_candidate] and [recognize_head], while the other bar is left behind. Such a
/// pair is no arrow.
pub(crate) fn shaft_is_doubled<'a>(
    shaft: &Arrow,
    others: impl IntoIterator<Item = &'a PenPath>,
    zoom: f64,
) -> bool {
    let axis = shaft.tip - shaft.start;
    let shaft_len = axis.length();
    if shaft_len <= f64::EPSILON {
        return false;
    }
    let along = axis / shaft_len;
    let max_offset = twin_max_offset(shaft_len, zoom);
    let max_length_diff = shaft_len * TWIN_MAX_LENGTH_DIFF_RATIO;
    let min_parallel_cos = TWIN_MAX_ANGLE.cos();
    let min_overlap = shaft_len * TWIN_MIN_OVERLAP_RATIO;

    others.into_iter().any(|other| {
        let points = flatten(other);
        let (Some(&other_start), Some(&other_tip)) = (points.first(), points.last()) else {
            return false;
        };
        let other_axis = other_tip - other_start;
        let other_len = other_axis.length();
        if other_len <= f64::EPSILON
            || (other_len - shaft_len).abs() > max_length_diff
            || (other_axis / other_len).dot(along).abs() < min_parallel_cos
        {
            return false;
        }
        // Both ends have to run alongside the shaft, not merely pass it by: a stroke drawn across
        // the shaft is no doubling of it.
        let offset = |p: Vector2| {
            let rel = p - shaft.start;
            (rel - along * rel.dot(along)).length()
        };
        if offset(other_start) > max_offset || offset(other_tip) > max_offset {
            return false;
        }
        let from = (other_start - shaft.start).dot(along);
        let to = (other_tip - shaft.start).dot(along);
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        hi.min(shaft_len) - lo.max(0.0) >= min_overlap
    })
}

/// Recognize the head at `tip`; `tail` is the other end of the shaft.
fn recognize_head_at(tail: Vector2, tip: Vector2, path: &PenPath, zoom: f64) -> Option<Arrow> {
    let points = flatten(path);
    if points.len() < 3 {
        return None;
    }

    let barbs = [points[0], *points.last()?];
    let vertex = points.iter().copied().max_by(|a, b| {
        dist_to_segment(*a, barbs[0], barbs[1]).total_cmp(&dist_to_segment(*b, barbs[0], barbs[1]))
    })?;
    let shaft_len = (tip - tail).length();
    if shaft_len <= f64::EPSILON {
        return None;
    }

    // The head has to sit at the tested end of the shaft.
    let vertex_to_tip = (vertex - tip).length();
    if vertex_to_tip > (vertex - tail).length() {
        return None;
    }
    if vertex_to_tip * zoom > (shaft_len * 0.20).max(HEAD_END_TOLERANCE_SURFACE) {
        return None;
    }

    // Both segments of the head have to be straight: a scribble or a curve is no head.
    let tolerance = (HEAD_STRAIGHTNESS_TOLERANCE_SURFACE / zoom).max(0.06 * shaft_len);
    if points.iter().any(|p| {
        dist_to_segment(*p, barbs[0], vertex) > tolerance
            && dist_to_segment(*p, vertex, barbs[1]) > tolerance
    }) {
        return None;
    }

    let barb_lens = [(barbs[0] - vertex).length(), (barbs[1] - vertex).length()];
    let barb_min = barb_lens[0].min(barb_lens[1]);
    let barb_max = barb_lens[0].max(barb_lens[1]);
    if barb_min * zoom < MIN_SHAFT_SURFACE * 0.2
        || barb_min < shaft_len * MIN_BARB_RATIO
        || barb_max > shaft_len * MAX_BARB_RATIO
        || barb_max > barb_min * MAX_BARB_LENGTH_DIFF
    {
        return None;
    }

    let axis = tip - tail;
    let a = barbs[0] - vertex;
    let b = barbs[1] - vertex;
    let cross_a = axis.perp_dot(a);
    let cross_b = axis.perp_dot(b);
    // The barbs point back along the shaft, one on each side of it.
    if a.dot(axis) >= 0.0 || b.dot(axis) >= 0.0 || cross_a * cross_b >= 0.0 {
        return None;
    }

    let cos = (a.dot(b) / (a.length() * b.length())).clamp(-1.0, 1.0);
    let angle = cos.acos();
    if !(MIN_HEAD_ANGLE..=MAX_HEAD_ANGLE).contains(&angle) {
        return None;
    }

    Some(Arrow {
        start: tail,
        tip: vertex,
    })
}

fn flatten(path: &PenPath) -> Vec<Vector2> {
    use kurbo::PathEl;

    let mut points = Vec::new();
    for el in path.to_kurbo_flattened(0.25).elements() {
        match el {
            PathEl::MoveTo(p) => points.push(Vector2::new(p.x, p.y)),
            PathEl::LineTo(p) => points.push(Vector2::new(p.x, p.y)),
            PathEl::QuadTo(p, q) => {
                points.push(Vector2::new(p.x, p.y));
                points.push(Vector2::new(q.x, q.y));
            }
            PathEl::CurveTo(p, q, r) => {
                points.push(Vector2::new(p.x, p.y));
                points.push(Vector2::new(q.x, q.y));
                points.push(Vector2::new(r.x, r.y));
            }
            PathEl::ClosePath => {}
        }
    }
    points
}

fn dist_to_segment(p: Vector2, a: Vector2, b: Vector2) -> f64 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= f64::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rnote_compose::penpath::{Element, PenPath, Segment};

    fn path_from_points(points: &[Vector2]) -> PenPath {
        let start = Element::new(points[0], 0.5);
        PenPath::new_w_segments(
            start,
            points[1..].iter().map(|p| Segment::LineTo {
                end: Element::new(*p, 0.5),
            }),
        )
    }

    fn straight_line(from: Vector2, to: Vector2) -> PenPath {
        let points = (0..=16)
            .map(|i| from + (to - from) * (i as f64 / 16.0))
            .collect::<Vec<_>>();
        path_from_points(&points)
    }

    #[test]
    fn a_straight_stroke_is_a_shaft() {
        let path = straight_line(Vector2::new(0.0, 0.0), Vector2::new(100.0, 0.0));
        let shaft = line_candidate(&path, 1.0).expect("a straight line is a shaft");
        assert!((shaft.start - Vector2::new(0.0, 0.0)).length() < 1.0);
        assert!((shaft.tip - Vector2::new(100.0, 0.0)).length() < 1.0);
    }

    #[test]
    fn a_slightly_bowed_stroke_is_a_shaft() {
        let points = (0..=32)
            .map(|i| {
                let t = i as f64 / 32.0;
                let bow = (t * std::f64::consts::PI).sin() * 6.0;
                Vector2::new(t * 120.0, bow)
            })
            .collect::<Vec<_>>();
        let path = path_from_points(&points);
        let shaft = line_candidate(&path, 1.0).expect("a slightly bowed line is still a shaft");
        assert!((shaft.start - points[0]).length() < 1.0);
        assert!((shaft.tip - *points.last().unwrap()).length() < 1.0);
    }

    #[test]
    fn a_short_stroke_is_no_shaft() {
        let path = straight_line(Vector2::new(0.0, 0.0), Vector2::new(20.0, 0.0));
        assert!(line_candidate(&path, 1.0).is_none());
    }

    #[test]
    fn a_v_at_the_shaft_end_is_a_head() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let head = path_from_points(&[
            Vector2::new(80.0, -20.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(80.0, 20.0),
        ]);

        let arrow = recognize_head(&shaft, &head, 1.0).expect("a V at the end is a head");
        assert!((arrow.tip - Vector2::new(100.0, 0.0)).length() < 1.0);
        assert!((arrow.start - shaft.start).length() < 1.0);
    }

    #[test]
    fn a_v_at_the_shaft_start_is_a_head_pointing_back() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let head = path_from_points(&[
            Vector2::new(20.0, -20.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(20.0, 20.0),
        ]);

        let arrow = recognize_head(&shaft, &head, 1.0)
            .expect("a V at the start is a head for a shaft drawn from its head");
        assert!((arrow.tip - Vector2::new(0.0, 0.0)).length() < 1.0);
        assert!((arrow.start - Vector2::new(100.0, 0.0)).length() < 1.0);
    }

    #[test]
    fn a_single_barb_is_no_head() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let head = path_from_points(&[Vector2::new(100.0, 0.0), Vector2::new(80.0, -20.0)]);

        assert!(recognize_head(&shaft, &head, 1.0).is_none());
    }

    #[test]
    fn a_bar_beside_the_shaft_doubles_it() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let upper_bar = straight_line(Vector2::new(0.0, -20.0), Vector2::new(100.0, -20.0));

        assert!(shaft_is_doubled(&shaft, [&upper_bar], 1.0));
    }

    #[test]
    fn a_single_arrow_has_no_twin() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };

        assert!(!shaft_is_doubled(&shaft, std::iter::empty(), 1.0));
    }

    #[test]
    fn a_stroke_across_or_slanted_to_the_shaft_is_no_twin() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let across = straight_line(Vector2::new(50.0, -60.0), Vector2::new(50.0, 60.0));
        let slanted = straight_line(Vector2::new(80.0, -60.0), Vector2::new(20.0, 60.0));

        assert!(!shaft_is_doubled(&shaft, [&across], 1.0));
        assert!(!shaft_is_doubled(&shaft, [&slanted], 1.0));
    }

    #[test]
    fn a_parallel_stroke_that_is_far_off_or_short_is_no_twin() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let far = straight_line(Vector2::new(0.0, -60.0), Vector2::new(100.0, -60.0));
        let short = straight_line(Vector2::new(0.0, -20.0), Vector2::new(30.0, -20.0));

        assert!(!shaft_is_doubled(&shaft, [&far], 1.0));
        assert!(!shaft_is_doubled(&shaft, [&short], 1.0));
    }

    #[test]
    fn a_barbs_that_are_too_long_are_no_head() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let head = path_from_points(&[
            Vector2::new(20.0, -80.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(20.0, 80.0),
        ]);

        assert!(recognize_head(&shaft, &head, 1.0).is_none());
    }
}
