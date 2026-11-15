//! Recognizing a closed loop drawn around strokes.

use p2d::math::Vector2;
use rnote_compose::penpath::PenPath;
use rnote_compose::shaperecognition;
use rnote_compose::shapes::Ellipse;

/// Recognize a closed loop as the circle lasso.
///
/// The loop has to be closed and fit an ellipse; it need not be as smooth as a drawn ellipse,
/// since a lasso is drawn quickly. A line or a polygon is not a lasso.
pub(crate) fn recognize(path: &PenPath, zoom: f64) -> Option<Ellipse> {
    let ellipse = shaperecognition::recognize_lasso_ellipse(path, zoom)?;
    (ellipse.radii.x > f64::EPSILON && ellipse.radii.y > f64::EPSILON).then_some(ellipse)
}

/// Whether the point lies inside the ellipse, including its outline.
pub(crate) fn contains_point(ellipse: &Ellipse, point: Vector2) -> bool {
    let local = ellipse.affine.inverse().transform_point2(point);
    (local.x / ellipse.radii.x).powi(2) + (local.y / ellipse.radii.y).powi(2) <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2d::glamx::DAffine2;
    use rnote_compose::penpath::{Element, PenPath, Segment};

    #[test]
    fn recognizes_a_closed_loop() {
        let center = Vector2::new(100.0, 100.0);
        let radius = 50.0;
        let start = Element::new(center + Vector2::new(radius, 0.0), 0.5);
        let segments = (1..=40).map(|i| {
            let angle = i as f64 / 40.0 * std::f64::consts::TAU;
            Segment::LineTo {
                end: Element::new(
                    center + Vector2::new(radius * angle.cos(), radius * angle.sin()),
                    0.5,
                ),
            }
        });
        let path = PenPath::new_w_segments(start, segments);

        let lasso = recognize(&path, 1.0).expect("a circle is a lasso");
        assert!(contains_point(&lasso, center));
        assert!(contains_point(&lasso, center + Vector2::new(49.0, 0.0)));
        assert!(!contains_point(&lasso, center + Vector2::new(51.0, 0.0)));
    }

    #[test]
    fn recognizes_a_rough_loop() {
        // A lasso is drawn quickly and has corners; it is still a lasso.
        let center = Vector2::new(100.0, 100.0);
        let points = (0..=96)
            .map(|i| {
                let angle = i as f64 / 96.0 * std::f64::consts::TAU;
                let radius = if i % 10 < 5 { 50.0 } else { 43.0 };
                center + Vector2::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect::<Vec<_>>();
        let start = Element::new(points[0], 0.5);
        let segments = points[1..].iter().map(|&point| Segment::LineTo {
            end: Element::new(point, 0.5),
        });
        let path = PenPath::new_w_segments(start, segments);

        assert!(recognize(&path, 1.0).is_some());
    }

    #[test]
    fn does_not_recognize_an_open_line() {
        let start = Element::new(Vector2::new(0.0, 0.0), 0.5);
        let path = PenPath::new_w_segments(
            start,
            [Segment::LineTo {
                end: Element::new(Vector2::new(100.0, 0.0), 0.5),
            }],
        );

        assert!(recognize(&path, 1.0).is_none());
    }

    #[test]
    fn contains_points_inside_the_ellipse_only() {
        let ellipse = Ellipse {
            radii: Vector2::new(10.0, 5.0),
            affine: DAffine2::from_translation(Vector2::new(100.0, 100.0)),
        };

        assert!(contains_point(&ellipse, Vector2::new(100.0, 100.0)));
        assert!(contains_point(&ellipse, Vector2::new(109.0, 100.0)));
        assert!(!contains_point(&ellipse, Vector2::new(111.0, 100.0)));
        assert!(!contains_point(&ellipse, Vector2::new(100.0, 106.0)));
    }
}
