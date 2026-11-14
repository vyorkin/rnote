// Imports
use super::StrokeKey;
use super::chrono_comp::StrokeLayer;
use super::render_comp::RenderCompState;
use crate::engine::{LayeredStroke, StrokeContent};
use crate::strokes::{Content, Stroke};
use crate::{StrokeStore, WidgetFlags};
use geo::intersects::Intersects;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use rnote_compose::Color;
use rnote_compose::Transformable;
use rnote_compose::penpath::Element;
use rnote_compose::shapes::Shapeable;
use std::sync::Arc;

/// Systems that are related to the stroke components.
impl StrokeStore {
    /// Gets a immutable reference to a stroke.
    pub(crate) fn get_stroke_ref(&self, key: StrokeKey) -> Option<&Stroke> {
        self.stroke_components.get(key).map(|stroke| &**stroke)
    }

    /// Gets a mutable reference to a stroke.
    pub(crate) fn get_stroke_mut(&mut self, key: StrokeKey) -> Option<&mut Stroke> {
        Arc::make_mut(&mut self.stroke_components)
            .get_mut(key)
            .map(Arc::make_mut)
    }

    /// Gets the stroke by cloning the Arc that is wrapped around it.
    #[allow(unused)]
    pub(crate) fn get_stroke_arc(&self, key: StrokeKey) -> Option<Arc<Stroke>> {
        self.stroke_components.get(key).cloned()
    }

    /// Gets immutable references to the strokes.
    pub(crate) fn get_strokes_ref(&self, keys: &[StrokeKey]) -> Vec<&Stroke> {
        keys.iter()
            .filter_map(|&key| self.stroke_components.get(key).map(|stroke| &**stroke))
            .collect()
    }

    /// Gets the strokes by cloning the Arc's that are wrapped around them.
    #[allow(unused)]
    pub(crate) fn get_strokes_arc(&self, keys: &[StrokeKey]) -> Vec<Arc<Stroke>> {
        keys.iter()
            .filter_map(|&key| self.stroke_components.get(key).cloned())
            .collect()
    }

    /// All keys from the stroke components slotmap, unordered.
    pub(crate) fn keys_unordered(&self) -> Vec<StrokeKey> {
        self.stroke_components.keys().collect()
    }

    #[allow(unused)]
    pub(crate) fn keys_unordered_intersecting_bounds(&self, bounds: Aabb) -> Vec<StrokeKey> {
        self.key_tree.keys_intersecting_bounds(bounds)
    }

    /// All stroke keys that are not trashed, unordered.
    pub(crate) fn stroke_keys_unordered(&self) -> Vec<StrokeKey> {
        self.stroke_components
            .keys()
            .filter(|&key| !(self.trashed(key).unwrap_or(false)))
            .collect()
    }

    /// Storke keys in the order that they should be rendered.
    pub(crate) fn stroke_keys_as_rendered(&self) -> Vec<StrokeKey> {
        self.keys_sorted_chrono()
            .into_iter()
            .filter(|&key| !(self.trashed(key).unwrap_or(false)))
            .collect::<Vec<StrokeKey>>()
    }

    /// Stroke keys intersecting the given bounds, in the order that they should be rendered.
    pub(crate) fn stroke_keys_as_rendered_intersecting_bounds(
        &self,
        bounds: Aabb,
    ) -> Vec<StrokeKey> {
        self.keys_sorted_chrono_intersecting_bounds(bounds)
            .into_iter()
            .filter(|&key| !(self.trashed(key).unwrap_or(false)))
            .collect::<Vec<StrokeKey>>()
    }

    /// Stroke keys contained in the given bounds, in the order that they should be rendered.
    pub(crate) fn stroke_keys_as_rendered_in_bounds(&self, bounds: Aabb) -> Vec<StrokeKey> {
        self.keys_sorted_chrono_in_bounds(bounds)
            .into_iter()
            .filter(|&key| !(self.trashed(key).unwrap_or(false)))
            .collect::<Vec<StrokeKey>>()
    }

    /// Stroke keys intersecting the given bounds, in the order that they should be rendered.
    /// partitioned into layers that need special handling during rendering.
    ///
    /// Returns a tuple of `(regular layer keys, highlighter layer keys)`.
    #[allow(unused)]
    pub(crate) fn stroke_keys_as_rendered_intersecting_bounds_partitioned(
        &self,
        bounds: Aabb,
    ) -> (Vec<StrokeKey>, Vec<StrokeKey>) {
        self.stroke_keys_as_rendered_intersecting_bounds(bounds)
            .into_iter()
            .partition(|key| {
                self.chrono_components
                    .get(*key)
                    .map(|chrono_comp| chrono_comp.layer)
                    .unwrap_or_default()
                    != StrokeLayer::Highlighter
            })
    }

    /// The number of visible (non-trashed) non-highlighter strokes intersecting the bounds.
    ///
    /// Cheaper than [Self::stroke_keys_as_rendered_intersecting_bounds_partitioned] when only the
    /// count matters: no sort and no key list.
    pub(crate) fn visible_non_highlighter_stroke_count(&self, bounds: Aabb) -> usize {
        self.key_tree
            .keys_intersecting_bounds(bounds)
            .into_iter()
            .filter(|&key| {
                !self.trashed(key).unwrap_or(false)
                    && self.stroke_layer(key) != StrokeLayer::Highlighter
            })
            .count()
    }

    /// Highlighter stroke keys intersecting the given bounds, in the order they should be rendered.
    ///
    /// The highlighter layer stays per-stroke when the regular content is drawn from tiles, so its
    /// keys are fetched without sorting the whole visible content.
    #[cfg(feature = "ui")]
    pub(crate) fn highlighter_keys_as_rendered_intersecting_bounds(
        &self,
        bounds: Aabb,
    ) -> Vec<StrokeKey> {
        let mut keys = self
            .key_tree
            .keys_intersecting_bounds(bounds)
            .into_iter()
            .filter(|&key| {
                !self.trashed(key).unwrap_or(false)
                    && self.stroke_layer(key) == StrokeLayer::Highlighter
            })
            .collect::<Vec<StrokeKey>>();
        self.sort_keys_chrono(&mut keys);
        keys
    }

    /// Clone the strokes for the given keys.
    #[allow(unused)]
    pub(crate) fn clone_strokes(&self, keys: &[StrokeKey]) -> Vec<Stroke> {
        keys.iter()
            .filter_map(|&key| Some((**self.stroke_components.get(key)?).clone()))
            .collect::<Vec<Stroke>>()
    }

    /// Updates the stroke geometry.
    ///
    /// The stroke then needs to update its rendering.
    pub(crate) fn update_geometry_for_stroke(&mut self, key: StrokeKey) {
        if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
            .get_mut(key)
            .map(Arc::make_mut)
        {
            stroke.update_geometry();
            self.key_tree.update_with_key(key, stroke.bounds());
            self.set_rendering_dirty(key);
        }
        self.bump_content_revision();
    }

    /// Updates the strokes geometries.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn update_geometry_for_strokes(&mut self, keys: &[StrokeKey]) {
        keys.iter().for_each(|&key| {
            self.update_geometry_for_stroke(key);
        });
    }

    /// Calculate the height needed to fit all strokes.
    pub(crate) fn calc_height(&self) -> f64 {
        let strokes_iter = self
            .stroke_keys_unordered()
            .into_iter()
            .filter_map(|key| self.stroke_components.get(key));

        let strokes_min_y = strokes_iter
            .clone()
            .fold(0.0, |acc, stroke| stroke.bounds().mins[1].min(acc));
        let strokes_max_y = strokes_iter.fold(0.0, |acc, stroke| stroke.bounds().maxs[1].max(acc));

        strokes_max_y - strokes_min_y
    }

    /// Calculate the width needed to fit all strokes.
    #[allow(unused)]
    pub(crate) fn calc_width(&self) -> f64 {
        let strokes_iter = self
            .stroke_keys_unordered()
            .into_iter()
            .filter_map(|key| self.stroke_components.get(key));

        let strokes_min_x = strokes_iter
            .clone()
            .fold(0.0, |acc, stroke| stroke.bounds().mins[0].min(acc));
        let strokes_max_x = strokes_iter.fold(0.0, |acc, stroke| stroke.bounds().maxs[0].max(acc));

        strokes_max_x - strokes_min_x
    }

    /// Generate the enclosing bounds for the given keys.
    pub(crate) fn bounds_for_strokes(&self, keys: &[StrokeKey]) -> Option<Aabb> {
        let mut keys_iter = keys.iter();
        let key = keys_iter.next()?;
        let first = self.stroke_components.get(*key)?;
        let mut bounds = first.bounds();

        keys_iter
            .filter_map(|&key| self.stroke_components.get(key))
            .for_each(|stroke| {
                bounds.merge(&stroke.bounds());
            });

        Some(bounds)
    }

    /// The enclosing bounds of all rendered (not trashed) strokes.
    ///
    /// Read from the key tree instead of recomputing the composed bounds of every stroke, which
    /// costs tens of milliseconds on a document with tens of thousands of strokes.
    pub(crate) fn bounds_as_rendered(&self) -> Option<Aabb> {
        self.key_tree
            .iter()
            .filter(|(key, _)| !self.trashed(*key).unwrap_or(false))
            .map(|(_, bounds)| bounds)
            .reduce(|mut acc, bounds| {
                acc.merge(&bounds);
                acc
            })
    }

    /// Collect all stroke bounds for the given keys.
    pub(crate) fn strokes_bounds(&self, keys: &[StrokeKey]) -> Vec<Aabb> {
        keys.iter()
            .filter_map(|&key| Some(self.stroke_components.get(key)?.bounds()))
            .collect::<Vec<Aabb>>()
    }

    pub(crate) fn set_stroke_pos(&mut self, key: StrokeKey, pos: Vector2) {
        let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
            .get_mut(key)
            .map(Arc::make_mut)
        else {
            return;
        };
        stroke.set_pos(pos);
        self.bump_content_revision();
    }

    /// Translate the strokes by the offset.
    ///
    /// The strokes then need to update their geometry and rendering.
    pub(crate) fn translate_strokes(&mut self, keys: &[StrokeKey], offset: Vector2) {
        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                {
                    // translate the stroke geometry
                    stroke.translate(offset);
                    self.key_tree.update_with_key(key, stroke.bounds());
                }
            }
        });
        self.bump_content_revision();
    }

    /// Translate the stroke rendering images.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn translate_strokes_images(&mut self, keys: &[StrokeKey], offset: Vector2) {
        keys.iter().for_each(|&key| {
            if let Some(render_comp) = self.render_components.get_mut(key) {
                // while previewing, the stroke is drawn from its geometry and its images are
                // regenerated once the transform ended
                if self.preview_keys.contains(&key) {
                    render_comp.state = RenderCompState::Dirty;
                    return;
                }

                for image in render_comp.images.iter_mut() {
                    image.translate(offset);
                }

                // the pixels of the images are unchanged, only the affines they are drawn at
                #[cfg(feature = "ui")]
                render_comp.update_rendernode_transforms();
            }
        });
    }

    /// Rotate the stroke by the given angle (in radians) around the center.
    ///
    /// Strokes then need to update their rendering.
    pub(crate) fn rotate_strokes(&mut self, keys: &[StrokeKey], angle: f64, center: Vector2) {
        self.bump_content_revision();
        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                {
                    // rotate the stroke geometry
                    stroke.rotate(angle, center);
                    self.key_tree.update_with_key(key, stroke.bounds());
                }
            }
        });
    }

    /// Change the stroke and text color for the given keys.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn change_stroke_colors(&mut self, keys: &[StrokeKey], color: Color) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if keys.is_empty() {
            return widget_flags;
        }

        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                {
                    match stroke {
                        Stroke::BrushStroke(brush_stroke) => {
                            brush_stroke.style.set_stroke_color(color);
                            self.set_rendering_dirty(key);
                        }
                        Stroke::ShapeStroke(shape_stroke) => {
                            shape_stroke.style.set_stroke_color(color);
                            self.set_rendering_dirty(key);
                        }
                        Stroke::TextStroke(text_stroke) => {
                            text_stroke.text_style.color = color;
                            self.set_rendering_dirty(key);
                        }
                        _ => {}
                    }
                }
            }
        });

        widget_flags.redraw = true;
        widget_flags.store_modified = true;

        self.bump_content_revision();

        widget_flags
    }

    /// Invert the stroke, text and fill color of the given keys.
    ///
    /// Strokes then need to update their rendering.
    pub fn invert_color_brightness(&mut self, keys: &[StrokeKey]) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if keys.is_empty() {
            return widget_flags;
        }

        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                let stroke_modified = stroke.set_to_inverted_brightness_color();

                if stroke_modified {
                    self.set_rendering_dirty(key);
                }
            }
        });

        widget_flags.redraw = true;
        widget_flags.store_modified = true;

        self.bump_content_revision();

        widget_flags
    }

    /// Toggle the color inversion of the given bitmap images.
    ///
    /// If all given images are already inverted, the inversion is turned off,
    /// otherwise it is turned on. Other strokes are ignored.
    pub(crate) fn toggle_bitmap_images_invert_colors(&mut self, keys: &[StrokeKey]) -> WidgetFlags {
        let bitmap_keys = self.bitmap_image_keys(keys);
        if bitmap_keys.is_empty() {
            return WidgetFlags::default();
        }

        let turn_off = bitmap_keys.iter().all(|&key| {
            matches!(
                self.get_stroke_ref(key),
                Some(Stroke::BitmapImage(bitmap_image)) if bitmap_image.adjustments.invert_colors
            )
        });
        let invert_colors = !turn_off;

        let mut changed_keys = Vec::new();
        bitmap_keys.iter().for_each(|&key| {
            if let Some(Stroke::BitmapImage(bitmap_image)) = self.get_stroke_mut(key) {
                let mut adjustments = bitmap_image.adjustments;
                if adjustments.invert_colors != invert_colors {
                    adjustments.invert_colors = invert_colors;
                    bitmap_image.set_adjustments(adjustments);
                    changed_keys.push(key);
                }
            }
        });

        self.finish_bitmap_adjustments(&changed_keys)
    }

    /// Toggle making the background of the given bitmap images transparent.
    ///
    /// If all given images already have a transparent background, it is restored,
    /// otherwise it is removed. Images whose background can not be detected are
    /// left unchanged. Other strokes are ignored.
    pub(crate) fn toggle_bitmap_images_background(&mut self, keys: &[StrokeKey]) -> WidgetFlags {
        let bitmap_keys = self.bitmap_image_keys(keys);
        if bitmap_keys.is_empty() {
            return WidgetFlags::default();
        }

        let turn_off = bitmap_keys.iter().all(|&key| {
            matches!(
                self.get_stroke_ref(key),
                Some(Stroke::BitmapImage(bitmap_image))
                    if bitmap_image.adjustments.remove_background.is_some()
            )
        });

        let mut changed_keys = Vec::new();
        bitmap_keys.iter().for_each(|&key| {
            if let Some(Stroke::BitmapImage(bitmap_image)) = self.get_stroke_mut(key) {
                let mut adjustments = bitmap_image.adjustments;
                if turn_off {
                    if adjustments.remove_background.is_some() {
                        adjustments.remove_background = None;
                        bitmap_image.set_adjustments(adjustments);
                        changed_keys.push(key);
                    }
                } else if adjustments.remove_background.is_none()
                    && let Some(background_removal) = bitmap_image.detect_background()
                {
                    adjustments.remove_background = Some(background_removal);
                    bitmap_image.set_adjustments(adjustments);
                    changed_keys.push(key);
                }
            }
        });

        self.finish_bitmap_adjustments(&changed_keys)
    }

    fn bitmap_image_keys(&self, keys: &[StrokeKey]) -> Vec<StrokeKey> {
        keys.iter()
            .copied()
            .filter(|&key| matches!(self.get_stroke_ref(key), Some(Stroke::BitmapImage(_))))
            .collect()
    }

    fn finish_bitmap_adjustments(&mut self, changed_keys: &[StrokeKey]) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if changed_keys.is_empty() {
            return widget_flags;
        }

        self.set_rendering_dirty_for_strokes(changed_keys);

        self.bump_content_revision();

        widget_flags.redraw = true;
        widget_flags.store_modified = true;

        widget_flags
    }

    /// Change the fill color of the given keys.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn change_fill_colors(&mut self, keys: &[StrokeKey], color: Color) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if keys.is_empty() {
            return widget_flags;
        }

        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                {
                    match stroke {
                        Stroke::BrushStroke(brush_stroke) => {
                            brush_stroke.style.set_fill_color(color);
                            self.set_rendering_dirty(key);
                        }
                        Stroke::ShapeStroke(shape_stroke) => {
                            shape_stroke.style.set_fill_color(color);
                            self.set_rendering_dirty(key);
                        }
                        _ => {}
                    }
                }
            }
        });

        widget_flags.redraw = true;
        widget_flags.store_modified = true;

        self.bump_content_revision();

        widget_flags
    }

    /// Scale the strokes with the factor.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn scale_strokes(&mut self, keys: &[StrokeKey], scale: Vector2) {
        self.bump_content_revision();
        keys.iter().for_each(|&key| {
            if let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                .get_mut(key)
                .map(Arc::make_mut)
            {
                {
                    // rotate the stroke geometry
                    stroke.scale(scale);
                    self.key_tree.update_with_key(key, stroke.bounds());
                }
            }
        });
    }

    /// Scale the stroke rendering images.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn scale_strokes_images(&mut self, keys: &[StrokeKey], scale: Vector2) {
        keys.iter().for_each(|&key| {
            if let Some(render_comp) = self.render_components.get_mut(key) {
                render_comp.state = RenderCompState::Dirty;

                // while previewing, the stroke is drawn from its geometry and its images are
                // regenerated once the transform ended - recreating the textures here would cost
                // an image upload per step of the drag
                if self.preview_keys.contains(&key) {
                    return;
                }

                for image in render_comp.images.iter_mut() {
                    image.scale(scale);
                }

                // the pixels of the images are unchanged, only the affines they are drawn at
                #[cfg(feature = "ui")]
                render_comp.update_rendernode_transforms();
            }
        });
    }

    /// Scale the strokes with a pivot as the scaling origin.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn scale_strokes_with_pivot(
        &mut self,
        keys: &[StrokeKey],
        scale: Vector2,
        pivot: Vector2,
    ) {
        self.translate_strokes(keys, -pivot);
        self.scale_strokes(keys, scale);
        self.translate_strokes(keys, pivot);
    }

    /// Scale the stroke rendering images with a pivot.
    ///
    /// The strokes then need to update their rendering.
    pub(crate) fn scale_strokes_images_with_pivot(
        &mut self,
        strokes: &[StrokeKey],
        scale: Vector2,
        pivot: Vector2,
    ) {
        self.translate_strokes_images(strokes, -pivot);
        self.scale_strokes_images(strokes, scale);
        self.translate_strokes_images(strokes, pivot);
    }

    /// Return the keys for strokes whose hitboxes are inside or only partly inside the given
    /// polygon path.
    pub(crate) fn strokes_hitboxes_intersecting_path_polygon(
        &mut self,
        path: &[Element],
        viewport: Aabb,
    ) -> Vec<StrokeKey> {
        let mut bounds = viewport;
        for p in path {
            bounds.take_point(p.pos);
        }

        let path_polygon = {
            let selector_path_points = path
                .iter()
                .map(|element| geo::Coord {
                    x: element.pos[0],
                    y: element.pos[1],
                })
                .collect::<Vec<geo::Coord<f64>>>();

            geo::Polygon::new(selector_path_points.into(), vec![])
        };

        self.keys_sorted_chrono_intersecting_bounds(bounds)
            .into_iter()
            .filter_map(|key| {
                // skip if stroke is trashed
                if self.trashed(key)? {
                    return None;
                }

                let stroke = self.stroke_components.get(key)?;

                // A stroke that only reaches into the selection by a part of it is selected too.
                if stroke.hitboxes().iter().any(|hitbox| {
                    path_polygon.intersects(&crate::utils::p2d_aabb_to_geo_polygon(*hitbox))
                }) {
                    return Some(key);
                }

                None
            })
            .collect()
    }

    /// Return the keys for strokes whose hitboxes intersect in the given path.
    pub(crate) fn strokes_hitboxes_intersect_path(
        &mut self,
        path: &[Element],
        viewport: Aabb,
    ) -> Vec<StrokeKey> {
        let mut bounds = viewport;
        for p in path {
            bounds.take_point(p.pos);
        }

        let path_linestring = {
            let selector_path_points = path
                .iter()
                .map(|element| geo::Coord {
                    x: element.pos[0],
                    y: element.pos[1],
                })
                .collect::<Vec<geo::Coord<f64>>>();

            geo::LineString::new(selector_path_points)
        };

        self.keys_sorted_chrono_intersecting_bounds(bounds)
            .into_iter()
            .filter_map(|key| {
                // skip if stroke is trashed
                if self.trashed(key)? {
                    return None;
                }

                let stroke = self.stroke_components.get(key)?;
                let stroke_bounds = stroke.bounds();

                if path_linestring.intersects(&crate::utils::p2d_aabb_to_geo_polygon(stroke_bounds))
                {
                    for &hitbox_elem in stroke.hitboxes().iter() {
                        if path_linestring
                            .intersects(&crate::utils::p2d_aabb_to_geo_polygon(hitbox_elem))
                        {
                            return Some(key);
                        }
                    }
                }

                None
            })
            .collect()
    }

    /// Return the keys for strokes whose hitboxes are inside or only partly inside the given Aabb.
    pub(crate) fn strokes_hitboxes_intersecting_aabb(
        &mut self,
        aabb: Aabb,
        viewport: Aabb,
    ) -> Vec<StrokeKey> {
        self.keys_sorted_chrono_intersecting_bounds(viewport.merged(&aabb))
            .into_iter()
            .filter_map(|key| {
                // skip if stroke is trashed
                if self.trashed(key)? {
                    return None;
                }

                let stroke = self.stroke_components.get(key)?;

                // A stroke that only reaches into the selection by a part of it is selected too.
                if stroke
                    .hitboxes()
                    .iter()
                    .any(|hitbox| aabb.intersects(hitbox))
                {
                    return Some(key);
                }

                None
            })
            .collect()
    }

    /// Return the keys for strokes where the given coord is inside at least one of their hitboxes.
    pub(crate) fn stroke_hitboxes_contain_coord(
        &self,
        viewport: Aabb,
        coord: Vector2,
    ) -> Vec<StrokeKey> {
        let mut bounds = viewport;
        bounds.take_point(coord);

        self.stroke_keys_as_rendered_intersecting_bounds(bounds)
            .into_iter()
            .filter(|&key| {
                if let Some(stroke) = self.stroke_components.get(key) {
                    stroke
                        .hitboxes()
                        .into_iter()
                        .any(|hitbox| hitbox.contains_local_point(coord))
                } else {
                    false
                }
            })
            .collect()
    }

    /// Return all keys below the given `y`.
    pub(crate) fn keys_below_y(&self, y: f64) -> Vec<StrokeKey> {
        self.stroke_components
            .iter()
            .filter_map(|(key, stroke)| {
                if stroke.bounds().mins[1] > y {
                    Some(key)
                } else {
                    None
                }
            })
            .collect::<Vec<StrokeKey>>()
    }

    pub(crate) fn keys_between(
        &self,
        y_start: f64,
        y_end: f64,
        x_lims: (f64, f64),
        limit_movement_vertical_border: bool,
        limit_movement_horizontal_border: bool,
    ) -> Vec<StrokeKey> {
        self.key_tree.keys_intersecting_bounds(Aabb::new(
            Vector2::new(
                if limit_movement_vertical_border {
                    x_lims.0
                } else {
                    f64::NEG_INFINITY
                },
                y_start,
            ),
            Vector2::new(
                if limit_movement_vertical_border {
                    x_lims.1
                } else {
                    f64::INFINITY
                },
                if limit_movement_horizontal_border {
                    y_end
                } else {
                    f64::INFINITY
                },
            ),
        ))
    }

    pub(crate) fn fetch_stroke_content(&self, keys: &[StrokeKey]) -> StrokeContent {
        let strokes = keys
            .iter()
            .filter_map(|&key| {
                let stroke = self.stroke_components.get(key).cloned()?;
                let layer = self
                    .chrono_components
                    .get(key)
                    .map(|chrono_comp| chrono_comp.layer)
                    .unwrap_or_else(|| stroke.extract_default_layer());

                Some(LayeredStroke { stroke, layer })
            })
            .collect::<Vec<LayeredStroke>>();

        StrokeContent::default().with_strokes(strokes)
    }

    /// Cut the strokes for the given keys and return them as stroke content.
    pub(crate) fn cut_stroke_content(&mut self, keys: &[StrokeKey]) -> StrokeContent {
        for &key in keys {
            self.set_selected(key, false);
            self.set_trashed(key, true);
        }

        let strokes = keys
            .iter()
            .filter_map(|&key| {
                let stroke = self.stroke_components.get(key).cloned()?;
                let layer = self
                    .chrono_components
                    .get(key)
                    .map(|chrono_comp| chrono_comp.layer)
                    .unwrap_or_else(|| stroke.extract_default_layer());

                Some(LayeredStroke { stroke, layer })
            })
            .collect::<Vec<LayeredStroke>>();

        StrokeContent::default().with_strokes(strokes)
    }

    /// Paste the clipboard content as a selection.
    ///
    /// Returns the keys for the inserted strokes.
    ///
    /// The inserted strokes then need to update their geometry and rendering.
    pub(crate) fn insert_stroke_content(
        &mut self,
        clipboard_content: StrokeContent,
        ratio: f64,
        pos: Vector2,
    ) -> Vec<StrokeKey> {
        if clipboard_content.strokes.is_empty() {
            return vec![];
        }

        let clipboard_bounds = clipboard_content
            .strokes
            .iter()
            .fold(Aabb::new_invalid(), |acc, s| acc.merged(&s.stroke.bounds()));

        clipboard_content
            .strokes
            .into_iter()
            .map(|stroke_content_stroke| {
                let LayeredStroke { stroke, layer } = stroke_content_stroke;
                let offset = stroke.bounds().mins - clipboard_bounds.mins;
                let key = self.insert_stroke((*stroke).clone(), Some(layer));

                // position strokes without resizing
                self.set_stroke_pos(key, pos);
                self.translate_strokes(&[key], offset);

                // apply a rescale around a pivot
                self.scale_strokes_with_pivot(&[key], Vector2::new(ratio, ratio), pos);
                self.scale_strokes_images_with_pivot(&[key], Vector2::new(ratio, ratio), pos);

                // select keys
                self.set_selected(key, true);
                key
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageMemoryFormat;
    use crate::strokes::BrushStroke;
    use rnote_compose::PenPath;
    use rnote_compose::Style;
    use rnote_compose::ext::AabbExt;
    use rnote_compose::shapes::Rectangle;
    use rnote_compose::style::smooth::SmoothOptions;

    /// Insert a horizontal stroke at the given height, reaching from `x_start` to `x_end`.
    fn insert_line(store: &mut StrokeStore, y: f64, x_start: f64, x_end: f64) -> StrokeKey {
        insert_path(
            store,
            [[x_start, y], [x_end, y]]
                .into_iter()
                .map(|[x, y]| Element::new(Vector2::new(x, y), 0.5)),
        )
    }

    fn insert_path(store: &mut StrokeStore, elements: impl Iterator<Item = Element>) -> StrokeKey {
        let path = PenPath::try_from_elements(elements).unwrap();
        let key = store.insert_stroke(
            Stroke::BrushStroke(BrushStroke::from_penpath(
                path,
                Style::Smooth(SmoothOptions::default()),
            )),
            None,
        );
        store.update_geometry_for_stroke(key);

        key
    }

    fn viewport() -> Aabb {
        Aabb::new(Vector2::new(-1000.0, -1000.0), Vector2::new(1000.0, 1000.0))
    }

    #[test]
    fn rectangle_selects_a_stroke_that_only_partly_falls_inside() {
        let mut store = StrokeStore::default();
        let fully_inside = insert_line(&mut store, 200.0, 170.0, 230.0);
        let crossing = insert_line(&mut store, 150.0, 100.0, 400.0);
        let outside = insert_line(&mut store, 150.0, 300.0, 400.0);
        // Its bounds span the selection, but none of its ink is in it.
        let ink_outside = insert_path(
            &mut store,
            [[0.0, 0.0], [900.0, 0.0], [900.0, 900.0]]
                .into_iter()
                .map(|[x, y]| Element::new(Vector2::new(x, y), 0.5)),
        );
        let selection = Aabb::new_positive(Vector2::new(150.0, 100.0), Vector2::new(250.0, 300.0));

        let selected = store.strokes_hitboxes_intersecting_aabb(selection, viewport());

        assert_eq!(selected, vec![fully_inside, crossing]);
        assert!(!selected.contains(&outside));
        assert!(!selected.contains(&ink_outside));
    }

    #[test]
    fn polygon_lasso_selects_a_stroke_that_only_partly_falls_inside() {
        let mut store = StrokeStore::default();
        let fully_inside = insert_line(&mut store, 200.0, 170.0, 230.0);
        let crossing = insert_line(&mut store, 150.0, 100.0, 400.0);
        let outside = insert_line(&mut store, 150.0, 300.0, 400.0);
        let path = [
            [150.0, 100.0],
            [250.0, 100.0],
            [250.0, 300.0],
            [150.0, 300.0],
        ]
        .into_iter()
        .map(|[x, y]| Element::new(Vector2::new(x, y), 0.5))
        .collect::<Vec<Element>>();

        let selected = store.strokes_hitboxes_intersecting_path_polygon(&path, viewport());

        assert_eq!(selected, vec![fully_inside, crossing]);
        assert!(!selected.contains(&outside));
    }

    fn bitmap_image(pixels: &[[u8; 4]], width: u32) -> crate::strokes::BitmapImage {
        let height = pixels.len() as u32 / width;
        let data = pixels.iter().flatten().copied().collect::<Vec<u8>>();
        let bounds = Aabb::new(Vector2::ZERO, Vector2::new(width as f64, height as f64));

        crate::strokes::BitmapImage {
            image: crate::Image {
                data: glib::Bytes::from_owned(data),
                rectangle: Rectangle::from_p2d_aabb(bounds),
                pixel_width: width,
                pixel_height: height,
                memory_format: crate::image::ImageMemoryFormat::R8g8b8a8Premultiplied,
            },
            rectangle: Rectangle::from_p2d_aabb(bounds),
            ..Default::default()
        }
    }

    #[test]
    fn bitmap_image_adjustments_toggle_per_image() {
        let mut store = StrokeStore::default();
        let white_red_border = bitmap_image(
            &[
                [255, 255, 255, 255],
                [255, 255, 255, 255],
                [255, 255, 255, 255],
                [255, 255, 255, 255],
                [255, 0, 0, 255],
                [255, 255, 255, 255],
                [255, 255, 255, 255],
                [255, 255, 255, 255],
                [255, 255, 255, 255],
            ],
            3,
        );
        let key = store.insert_stroke(Stroke::BitmapImage(white_red_border), None);
        store.update_geometry_for_stroke(key);

        // Inverting on.
        let flags = store.toggle_bitmap_images_invert_colors(&[key]);
        assert!(flags.store_modified);
        let Some(Stroke::BitmapImage(image)) = store.get_stroke_ref(key) else {
            panic!("expected a bitmap image");
        };
        assert!(image.adjustments.invert_colors);
        let data = image.effective_image().data.as_ref();
        assert_eq!(&data[0..4], &[0, 0, 0, 255]);
        assert_eq!(&data[16..20], &[0, 255, 255, 255]);

        // Inverting off again restores the original pixels.
        let flags = store.toggle_bitmap_images_invert_colors(&[key]);
        assert!(flags.store_modified);
        let Some(Stroke::BitmapImage(image)) = store.get_stroke_ref(key) else {
            panic!("expected a bitmap image");
        };
        assert!(!image.adjustments.invert_colors);
        let data = image.effective_image().data.as_ref();
        assert_eq!(&data[0..4], &[255, 255, 255, 255]);
        assert_eq!(&data[16..20], &[255, 0, 0, 255]);

        // Removing the detected background.
        let flags = store.toggle_bitmap_images_background(&[key]);
        assert!(flags.store_modified);
        let Some(Stroke::BitmapImage(image)) = store.get_stroke_ref(key) else {
            panic!("expected a bitmap image");
        };
        assert!(image.adjustments.remove_background.is_some());
        let data = image.effective_image().data.as_ref();
        assert_eq!(&data[0..4], &[0, 0, 0, 0]);
        assert_eq!(&data[16..20], &[255, 0, 0, 255]);

        // Restoring the background.
        let flags = store.toggle_bitmap_images_background(&[key]);
        assert!(flags.store_modified);
        let Some(Stroke::BitmapImage(image)) = store.get_stroke_ref(key) else {
            panic!("expected a bitmap image");
        };
        assert!(image.adjustments.remove_background.is_none());
        assert_eq!(
            &image.effective_image().data.as_ref()[0..4],
            &[255, 255, 255, 255]
        );
    }

    #[test]
    fn bitmap_image_adjustments_ignore_other_strokes() {
        let mut store = StrokeStore::default();
        let line = insert_line(&mut store, 0.0, 0.0, 100.0);
        let image = bitmap_image(&[[255, 255, 255, 255]], 1);
        let image_key = store.insert_stroke(Stroke::BitmapImage(image), None);
        store.update_geometry_for_stroke(image_key);

        let flags = store.toggle_bitmap_images_invert_colors(&[line]);
        assert!(!flags.store_modified);
        assert!(!flags.redraw);

        let flags = store.toggle_bitmap_images_invert_colors(&[line, image_key]);
        assert!(flags.store_modified);
        let Some(Stroke::BitmapImage(image)) = store.get_stroke_ref(image_key) else {
            panic!("expected a bitmap image");
        };
        assert!(image.adjustments.invert_colors);
        let Some(Stroke::BrushStroke(_)) = store.get_stroke_ref(line) else {
            panic!("expected the line to stay a brush stroke");
        };
    }

    /// A translate or a scale only changes the affine a rendering is drawn at, its pixels are
    /// unchanged and must not be handed to gdk again.
    #[cfg(feature = "ui")]
    #[test]
    fn transforming_a_rendering_keeps_its_textures() {
        let mut store = StrokeStore::default();
        let key = insert_line(&mut store, 100.0, 100.0, 200.0);
        store.regenerate_rendering_for_strokes(&[key], viewport(), 1.0);

        let textures_before = store.debug_image_textures(key);
        assert!(!textures_before.is_empty(), "the stroke is rendered");

        store.translate_strokes_images(&[key], Vector2::new(50.0, 20.0));
        store.scale_strokes_images(&[key], Vector2::splat(2.0));

        let textures_after = store.debug_image_textures(key);
        assert_eq!(
            textures_before
                .iter()
                .map(|texture| texture.texture().clone())
                .collect::<Vec<_>>(),
            textures_after
                .iter()
                .map(|texture| texture.texture().clone())
                .collect::<Vec<_>>(),
            "the rendering keeps its textures while it is transformed"
        );
    }
}
