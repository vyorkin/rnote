//! Recognizing an arrow drawn in two strokes: the shaft and the V-shaped head.

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

/// Recognize the two finished strokes of an arrow, in either drawing order.
pub(crate) fn recognize_pair(first: &PenPath, second: &PenPath, zoom: f64) -> Option<Arrow> {
    if let Some(line) = line_candidate(first, zoom) {
        if let Some(arrow) = recognize_head(&line, second, zoom) {
            return Some(arrow);
        }
    }
    let line = line_candidate(second, zoom)?;
    recognize_head(&line, first, zoom)
}

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
