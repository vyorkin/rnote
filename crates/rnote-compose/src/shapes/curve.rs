// Imports
use super::{CubicBezier, Shapeable};
use crate::Transformable;
use crate::ext::Vector2Ext;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename = "curve")]
/// A smooth curve, drawn as a chain of cubic bezier segments.
///
/// The segments share their endpoints, so the curve is one continuous stroke and not a set of
/// strokes that happen to touch.
pub struct Curve {
    /// The segments of the curve, from its start to its end.
    #[serde(rename = "cubics")]
    pub cubics: Vec<CubicBezier>,
}

impl Transformable for Curve {
    fn translate(&mut self, offset: Vector2) {
        for cubic in &mut self.cubics {
            cubic.translate(offset);
        }
    }

    fn rotate(&mut self, angle: f64, center: Vector2) {
        for cubic in &mut self.cubics {
            cubic.rotate(angle, center);
        }
    }

    fn scale(&mut self, scale: Vector2) {
        for cubic in &mut self.cubics {
            cubic.scale(scale);
        }
    }
}

impl Shapeable for Curve {
    fn bounds(&self) -> Aabb {
        let mut cubics = self.cubics.iter();
        let Some(first) = cubics.next() else {
            return Aabb::new(Vector2::ZERO, Vector2::ZERO);
        };

        cubics.fold(first.bounds(), |bounds, cubic| {
            bounds.merged(&cubic.bounds())
        })
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        self.cubics
            .iter()
            .flat_map(|cubic| cubic.hitboxes())
            .collect()
    }

    fn outline_path(&self) -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();

        for cubic in &self.cubics {
            path.move_to(cubic.start.to_kurbo_point());
            path.curve_to(
                cubic.cp1.to_kurbo_point(),
                cubic.cp2.to_kurbo_point(),
                cubic.end.to_kurbo_point(),
            );
        }

        path
    }
}

impl Curve {
    /// A curve through the given points, with a smooth transition at each of them.
    ///
    /// The first and the last point are the endpoints of the curve, so a curve built from the
    /// points of a stroke keeps where that stroke started and ended. `None` when there are fewer
    /// than two points, or when two of them coincide and leave a segment without direction.
    pub fn new_through_points(points: &[Vector2]) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }

        let cubics = (0..points.len() - 1)
            .map(|i| {
                CubicBezier::new_w_catmull_rom(
                    points[i.saturating_sub(1)],
                    points[i],
                    points[i + 1],
                    points[(i + 2).min(points.len() - 1)],
                )
            })
            .collect::<Option<Vec<_>>>()?;

        Some(Self { cubics })
    }

    /// Whether the curve has no segments.
    pub fn is_empty(&self) -> bool {
        self.cubics.is_empty()
    }

    /// The points along the curve, evenly spaced in the parameter of each segment.
    pub fn sampled(&self, samples_per_cubic: usize) -> Vec<Vector2> {
        let samples_per_cubic = samples_per_cubic.max(1);
        let mut points = Vec::with_capacity(self.cubics.len() * samples_per_cubic + 1);

        for cubic in &self.cubics {
            for i in 0..samples_per_cubic {
                points.push(super::cubbez::cubbez_calc(
                    cubic.start,
                    cubic.cp1,
                    cubic.cp2,
                    cubic.end,
                    i as f64 / samples_per_cubic as f64,
                ));
            }
        }
        if let Some(last) = self.cubics.last() {
            points.push(last.end);
        }

        points
    }
}
