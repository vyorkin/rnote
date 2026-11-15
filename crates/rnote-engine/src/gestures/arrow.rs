//! Recognizing an arrow drawn in two strokes: the shaft, then the V-shaped head.

use p2d::math::Vector2;
use rnote_compose::penpath::PenPath;
use rnote_compose::shaperecognition::{self, RecognitionOptions};
use rnote_compose::shapes::{Arrow, Line, Shape};

/// The shortest shaft (in surface coordinates) an arrow needs.
const MIN_SHAFT_SURFACE: f64 = 30.0;
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
const HEAD_END_TOLERANCE_SURFACE: f64 = 20.0;
/// The deviation (in surface coordinates) the head may have from its two straight segments.
const HEAD_STRAIGHTNESS_TOLERANCE_SURFACE: f64 = 4.0;

/// Recognize a straight finished stroke as the shaft of an arrow.
pub(crate) fn line_candidate(path: &PenPath, zoom: f64) -> Option<Arrow> {
    let Shape::Line(Line { start, end }) =
        shaperecognition::recognize_shape_with_options(path, zoom, &RecognitionOptions::default())?
            .shape
    else {
        return None;
    };

    let shaft = Arrow { start, tip: end };
    ((shaft.tip - shaft.start).length() * zoom >= MIN_SHAFT_SURFACE).then_some(shaft)
}

/// Recognize the second stroke as the head of an arrow whose shaft is `shaft`.
pub(crate) fn recognize_head(shaft: &Arrow, path: &PenPath, zoom: f64) -> Option<Arrow> {
    let points = flatten(path);
    if points.len() < 3 {
        return None;
    }

    let barbs = [points[0], *points.last()?];
    let vertex = points.iter().copied().max_by(|a, b| {
        dist_to_segment(*a, barbs[0], barbs[1]).total_cmp(&dist_to_segment(*b, barbs[0], barbs[1]))
    })?;
    let tip = shaft.tip;
    let shaft_len = (tip - shaft.start).length();
    if shaft_len <= f64::EPSILON {
        return None;
    }

    // The head is drawn at the end of the shaft, not at its start.
    let vertex_to_tip = (vertex - tip).length();
    if vertex_to_tip > (vertex - shaft.start).length() {
        return None;
    }
    if vertex_to_tip * zoom > (shaft_len * 0.15).max(HEAD_END_TOLERANCE_SURFACE) {
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

    let axis = tip - shaft.start;
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
        start: shaft.start,
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
    fn a_v_at_the_shaft_start_is_no_head() {
        let shaft = Arrow {
            start: Vector2::new(0.0, 0.0),
            tip: Vector2::new(100.0, 0.0),
        };
        let head = path_from_points(&[
            Vector2::new(20.0, -20.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(20.0, 20.0),
        ]);

        assert!(recognize_head(&shaft, &head, 1.0).is_none());
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
