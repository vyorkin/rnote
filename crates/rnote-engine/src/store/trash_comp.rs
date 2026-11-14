// Imports
use super::chrono_comp::StrokeLayer;
use super::{StrokeKey, StrokeStore};
use crate::WidgetFlags;
use crate::strokes::{AxisErase, BrushStroke, Stroke};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use rnote_compose::PenPath;
use rnote_compose::shapes::Shapeable;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// The share of a stroke's ink a scribble has to cover for the stroke to be erased.
///
/// A scribble stroke is recognized from its own shape alone, and handwriting can pass that test:
/// a cursive "д" drawn with a bowl and a descender loop has the turns and the length of a
/// scratch-out. What it does not do is cover the stroke next to it, so erasing on the first
/// touched hitbox would take the neighbour with it. A real scratch-out runs over the ink it
/// means to erase and covers most of it.
const SCRIBBLE_COVERAGE_RATIO: f64 = 0.5;

/// The fewest ink tiles a scribble has to cover, whatever the share.
///
/// A stroke short enough to have only a tile or two is erased when the scribble covers it, not
/// when one of its tiles is grazed: half of two tiles is one tile, which a letter written tight
/// against it touches.
const SCRIBBLE_COVERAGE_MIN_TILES: usize = 2;

/// How much bigger, by the diagonal of its bounds, a stroke has to be than the scribble for the
/// scribble to erase it by crossing it instead of covering half of it.
///
/// A stroke much bigger than the scribble cannot be covered by it: scratching across the arc of an
/// enclosing ellipse or across a long line touches only a fraction of its ink, and asking for half
/// of it would ask for scratching along the whole outline. Half coverage is what keeps a scratch
/// over a word from taking the letter beside it, a guard that only means something between things
/// of comparable size. A stroke that outgrows the scribble this much is erased where the scribble
/// crosses it.
const SCRIBBLE_OUTGROW_FACTOR: f64 = 2.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, rename = "trash_component")]
pub struct TrashComponent {
    #[serde(rename = "trashed")]
    pub trashed: bool,
}

impl Default for TrashComponent {
    fn default() -> Self {
        Self { trashed: false }
    }
}

/// Systems that are related to trashing.
impl StrokeStore {
    /// Rebuild the slotmap with empty trash components with the keys returned from the stroke components.
    pub(crate) fn rebuild_trash_components_slotmap(&mut self) {
        self.trash_components = Arc::new(slotmap::SecondaryMap::new());
        self.stroke_components.keys().for_each(|key| {
            Arc::make_mut(&mut self.trash_components)
                .insert(key, Arc::new(TrashComponent::default()));
        });
    }

    /// Ability if trashing is supported.
    #[allow(unused)]
    pub(crate) fn can_trash(&self, key: StrokeKey) -> bool {
        self.trash_components.get(key).is_some()
    }

    pub(crate) fn trashed(&self, key: StrokeKey) -> Option<bool> {
        self.trash_components.get(key).map(|t| t.trashed)
    }

    pub(crate) fn set_trashed(&mut self, key: StrokeKey, trash: bool) {
        if let Some(trash_comp) = Arc::make_mut(&mut self.trash_components)
            .get_mut(key)
            .map(Arc::make_mut)
        {
            trash_comp.trashed = trash;
            self.update_chrono_to_last(key);
            self.bump_content_revision();
        }
    }

    pub(crate) fn set_trashed_keys(&mut self, keys: &[StrokeKey], trash: bool) {
        keys.iter().for_each(|&key| {
            self.set_selected(key, false);
            self.set_trashed(key, trash);
            self.update_chrono_to_last(key);
        });
    }

    pub(crate) fn trashed_keys_unordered(&self) -> Vec<StrokeKey> {
        self.stroke_components
            .keys()
            .filter(|&key| self.trashed(key).unwrap_or(false))
            .collect()
    }

    /// Removes all trashed strokes permanently from the store.
    #[allow(unused)]
    pub(crate) fn remove_trashed_strokes(&mut self) -> Vec<Stroke> {
        self.trashed_keys_unordered()
            .into_iter()
            .filter_map(|k| self.remove_stroke(k))
            .collect()
    }

    /// For a coordinate system axis with unit markings, erase the ticks and numbers for which `hit`
    /// returns true.
    ///
    /// Returns `None` for other strokes. The stroke is only modified when ticks or numbers were hit,
    /// its geometry is then updated and it needs to update its rendering.
    fn erase_axis_parts(
        &mut self,
        key: StrokeKey,
        hit: impl Fn(&Aabb) -> bool,
    ) -> Option<AxisErase> {
        let stroke = self.stroke_components.get(key)?;
        let Stroke::ShapeStroke(shapestroke) = stroke.as_ref() else {
            return None;
        };
        shapestroke.axis_ticks.as_ref()?;
        // Check first, to not copy strokes that were not hit
        if !stroke.hitboxes().iter().any(&hit) {
            return Some(AxisErase::Miss);
        }

        let stroke = Arc::make_mut(&mut self.stroke_components)
            .get_mut(key)
            .map(Arc::make_mut)?;
        let Stroke::ShapeStroke(shapestroke) = stroke else {
            return None;
        };
        let erase = shapestroke.erase_axis_parts(hit);
        if erase == Some(AxisErase::Parts) {
            self.key_tree.update_with_key(key, stroke.bounds());
            self.set_rendering_dirty(key);
            self.bump_content_revision();
        }
        erase
    }

    /// Trash strokes that collide with the given bounds.
    ///
    /// Ticks and numbers of coordinate system axes are erased individually. Axes that had parts
    /// erased are added to `spared` and are not trashed as a whole while they are in it, so wiping
    /// over a tick does not take the axis it crosses with it.
    ///
    /// Returns the keys of the modified strokes, which need to update their rendering.
    pub(crate) fn trash_colliding_strokes(
        &mut self,
        eraser_bounds: Aabb,
        viewport: Aabb,
        spared: &mut HashSet<StrokeKey>,
    ) -> (Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let mut modified_keys = vec![];

        self.stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .for_each(|key| {
                let mut trash_current_stroke = false;

                match self.erase_axis_parts(key, |hitbox| eraser_bounds.intersects(hitbox)) {
                    Some(AxisErase::Parts) => {
                        spared.insert(key);
                        modified_keys.push(key);
                        widget_flags.store_modified = true;
                        widget_flags.resize = true;
                        return;
                    }
                    Some(AxisErase::Axis) if !spared.contains(&key) => {
                        trash_current_stroke = true;
                    }
                    Some(_) => return,
                    None => {}
                }

                if let Some(stroke) = self.stroke_components.get(key)
                    && !trash_current_stroke
                {
                    match stroke.as_ref() {
                        Stroke::BrushStroke(_) | Stroke::ShapeStroke(_) => {
                            // First check if eraser even intersects stroke bounds, avoiding unnecessary work
                            if eraser_bounds.intersects(&stroke.bounds()) {
                                for hitbox in stroke.hitboxes().into_iter() {
                                    if eraser_bounds.intersects(&hitbox) {
                                        trash_current_stroke = true;

                                        break;
                                    }
                                }
                            }
                        }
                        // Ignore other strokes when trashing with the Eraser
                        Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                        }
                    }
                }

                if trash_current_stroke {
                    self.set_trashed(key, true);
                    widget_flags.store_modified = true;
                    widget_flags.resize = true;
                }
            });

        (modified_keys, widget_flags)
    }

    /// Trash the strokes that the stroke with the given key was drawn over.
    ///
    /// Used for the scribble gesture: the strokes the scribble actually went over are trashed, not
    /// everything inside its bounds, so scratching out a word between two others only takes that
    /// word. A stroke has to be covered, not merely touched: see [`SCRIBBLE_COVERAGE_RATIO`]. A
    /// stroke much bigger than the scribble is the exception — it cannot be covered, so it is
    /// erased where the scribble crosses it: see [`SCRIBBLE_OUTGROW_FACTOR`].
    /// Ticks and numbers of coordinate system axes are erased individually.
    ///
    /// Typed text is erased too, but it is one block that cannot be split, so its ink tiles are its
    /// line boxes: the same share of them has to be covered, which for a two-line block means a
    /// scratch across both lines and, for a paragraph, most of its lines.
    ///
    /// Returns how many strokes were trashed or modified, and the keys of the modified strokes,
    /// which need to update their rendering.
    pub(crate) fn trash_strokes_hit_by(
        &mut self,
        key: StrokeKey,
        viewport: Aabb,
    ) -> (usize, Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        let Some(hitting_stroke) = self.stroke_components.get(key) else {
            return (0, vec![], widget_flags);
        };
        let hitting_bounds = hitting_stroke.bounds();
        let hitting_hitboxes = hitting_stroke.hitboxes();
        let hitting_extents_len = hitting_bounds.extents().length();

        let candidate_keys = self
            .stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .filter(|k| *k != key)
            .collect::<Vec<StrokeKey>>();

        let mut modified_keys = vec![];
        let mut axis_trashed_keys = vec![];
        for &k in candidate_keys.iter() {
            match self.erase_axis_parts(k, |hitbox| {
                hitting_hitboxes
                    .iter()
                    .any(|hitting_hitbox| hitting_hitbox.intersects(hitbox))
            }) {
                Some(AxisErase::Parts) => modified_keys.push(k),
                Some(AxisErase::Axis) => axis_trashed_keys.push(k),
                _ => {}
            }
        }

        let mut hit_keys = candidate_keys
            .into_iter()
            .filter(|k| {
                // Axes with unit markings were handled above
                if let Some(Stroke::ShapeStroke(shapestroke)) =
                    self.stroke_components.get(*k).map(|s| s.as_ref())
                    && shapestroke.axis_ticks.is_some()
                {
                    return false;
                }
                let Some(stroke) = self.stroke_components.get(*k) else {
                    return false;
                };
                // Scribble-to-erase is a pen-tool gesture; it does not erase highlighter ink.
                if self
                    .chrono_components
                    .get(*k)
                    .map(|chrono_component| chrono_component.layer)
                    == Some(StrokeLayer::Highlighter)
                {
                    return false;
                }
                let is_text = match stroke.as_ref() {
                    Stroke::BrushStroke(_) | Stroke::ShapeStroke(_) => false,
                    Stroke::TextStroke(_) => true,
                    // Images are content, not ink, and the eraser ignores them too.
                    Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                        return false;
                    }
                };
                // First check the bounds, avoiding unnecessary work
                let stroke_bounds = stroke.bounds();
                if !hitting_bounds.intersects(&stroke_bounds) {
                    return false;
                }

                // Only a stroke the scribble covered is erased. A stroke it merely touches at its
                // edge is not: a letter written tight against another one grazes it, and a
                // scribble stroke misrecognized from such a letter would otherwise take the
                // neighbour down with it.
                let hitboxes = stroke.hitboxes();
                if hitboxes.is_empty() {
                    return false;
                }
                let covered = hitboxes
                    .iter()
                    .filter(|hitbox| {
                        hitting_hitboxes
                            .iter()
                            .any(|hitting_hitbox| hitting_hitbox.intersects(hitbox))
                    })
                    .count();

                // A stroke much bigger than the scribble cannot be covered by it: scratching
                // across the arc of an enclosing ellipse touches a few of its tiles and no more.
                // Half coverage guards against taking a neighbour beside the crossed-out word,
                // which only applies between things of comparable size, so for a stroke that
                // outgrows the scribble crossing it is enough. Typed text is left out: its tiles
                // are line boxes, and a paragraph must not go because one line was crossed.
                let outgrows_scribble = !is_text
                    && hitting_extents_len > f64::EPSILON
                    && stroke_bounds.extents().length()
                        >= SCRIBBLE_OUTGROW_FACTOR * hitting_extents_len;
                if outgrows_scribble {
                    return covered >= 1;
                }

                covered as f64 >= hitboxes.len() as f64 * SCRIBBLE_COVERAGE_RATIO
                    && covered >= SCRIBBLE_COVERAGE_MIN_TILES.min(hitboxes.len())
            })
            .collect::<Vec<StrokeKey>>();
        hit_keys.extend(axis_trashed_keys);

        for hit_key in hit_keys.iter() {
            self.set_trashed(*hit_key, true);
        }
        if !hit_keys.is_empty() || !modified_keys.is_empty() {
            widget_flags.store_modified = true;
            widget_flags.resize = true;
        }

        (
            hit_keys.len() + modified_keys.len(),
            modified_keys,
            widget_flags,
        )
    }

    /// Remove colliding stroke segments with the given bounds.
    /// The stroke is then split. Strokes that don't have segments are trashed completely.
    ///
    /// Returns the keys of all created or modified strokes.
    ///
    /// The returned strokes need to update their rendering.
    pub(crate) fn split_colliding_strokes(
        &mut self,
        eraser_bounds: Aabb,
        viewport: Aabb,
        spared: &mut HashSet<StrokeKey>,
    ) -> (Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let mut modified_keys = vec![];

        // Coordinate system axes have their ticks and numbers erased individually
        let mut axis_keys = HashSet::new();
        for key in self.stroke_keys_as_rendered_intersecting_bounds(viewport) {
            match self.erase_axis_parts(key, |hitbox| eraser_bounds.intersects(hitbox)) {
                Some(AxisErase::Parts) => {
                    spared.insert(key);
                    modified_keys.push(key);
                }
                Some(AxisErase::Axis) if !spared.contains(&key) => {
                    self.set_trashed(key, true);
                    modified_keys.push(key);
                }
                Some(_) => {}
                None => continue,
            }
            axis_keys.insert(key);
        }

        let new_strokes = self
            .stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .filter(|key| !axis_keys.contains(key))
            .flat_map(|key| {
                let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                    .get_mut(key)
                    .map(Arc::make_mut)
                else {
                    return vec![];
                };

                let Some(chrono_comp) = self.chrono_components.get(key) else {
                    return vec![];
                };

                let mut new_strokes = vec![];
                let mut trash_current_stroke = false;
                let stroke_bounds = stroke.bounds();

                match stroke {
                    Stroke::BrushStroke(brushstroke) => {
                        if eraser_bounds.intersects(&stroke_bounds) {
                            let mut split = Vec::new();

                            let mut hits = brushstroke
                                .path
                                .hittest(&eraser_bounds, brushstroke.style.stroke_width() * 0.5)
                                .into_iter();

                            if let Some(first_hit) = hits.next() {
                                let mut prev = first_hit;
                                for hit in hits {
                                    let split_slice = &brushstroke.path.segments[prev..hit];

                                    // skip splits that don't have at least two segments (one's end as path start, one additional)
                                    if split_slice.len() > 1 {
                                        split.push(split_slice.to_vec());
                                    }

                                    prev = hit;
                                }

                                // Catch the last
                                let last_split = &brushstroke.path.segments[prev..];
                                if last_split.len() > 1 {
                                    split.push(last_split.to_vec());
                                }

                                for next_split in split {
                                    let mut next_split_iter = next_split.into_iter();
                                    let next_start = next_split_iter.next().unwrap().end();

                                    new_strokes.push((
                                        Stroke::BrushStroke(BrushStroke::from_penpath(
                                            PenPath::new_w_segments(next_start, next_split_iter),
                                            brushstroke.style.clone(),
                                        )),
                                        chrono_comp.layer,
                                    ));
                                }

                                let first_split = &brushstroke.path.segments[..first_hit];
                                // Modify the original stroke at the end.
                                // We keep the start, so we only need at least one segment
                                if !first_split.is_empty() {
                                    brushstroke.replace_path(PenPath::new_w_segments(
                                        brushstroke.path.start,
                                        first_split.to_vec(),
                                    ));
                                } else {
                                    trash_current_stroke = true;
                                }

                                modified_keys.push(key);
                            }
                        }
                    }
                    Stroke::ShapeStroke(_) => {
                        if eraser_bounds.intersects(&stroke_bounds) {
                            for hitbox_elem in stroke.hitboxes().iter() {
                                if eraser_bounds.intersects(hitbox_elem) {
                                    trash_current_stroke = true;
                                    modified_keys.push(key);
                                }
                            }
                        }
                    }
                    // Ignore other strokes when trashing with the Eraser
                    Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {}
                }

                if trash_current_stroke {
                    self.set_trashed(key, true);
                    widget_flags.store_modified = true;
                    widget_flags.resize = true;
                }

                new_strokes
            })
            .collect::<Vec<(Stroke, StrokeLayer)>>();

        modified_keys.append(
            &mut new_strokes
                .into_iter()
                .map(|(new_stroke, layer)| self.insert_stroke(new_stroke, Some(layer)))
                .collect(),
        );

        if !modified_keys.is_empty() {
            widget_flags.store_modified = true;
            widget_flags.resize = true;
        }

        (modified_keys, widget_flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strokes::TextStroke;
    use crate::strokes::textstroke::TextStyle;
    use p2d::math::Vector2;
    use rnote_compose::Style;
    use rnote_compose::penpath::Element;
    use rnote_compose::style::smooth::SmoothOptions;

    /// Insert a horizontal stroke at the given height, reaching from `x_start` to `x_end`.
    fn insert_line(store: &mut StrokeStore, y: f64, x_start: f64, x_end: f64) -> StrokeKey {
        let path = PenPath::try_from_elements(
            [x_start, x_end]
                .into_iter()
                .map(|x| Element::new(Vector2::new(x, y), 0.5)),
        )
        .unwrap();
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

    /// Insert a stroke through the given points.
    fn insert_path(
        store: &mut StrokeStore,
        points: impl IntoIterator<Item = Vector2>,
    ) -> StrokeKey {
        let path =
            PenPath::try_from_elements(points.into_iter().map(|p| Element::new(p, 0.5))).unwrap();
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

    /// Insert a text stroke with the given text, its upper left corner at `pos`.
    fn insert_text(store: &mut StrokeStore, text: &str, pos: Vector2) -> StrokeKey {
        let mut text_style = TextStyle::default();
        text_style.font_size = 16.0;
        let key = store.insert_stroke(
            Stroke::TextStroke(TextStroke::new(text.to_string(), pos, text_style)),
            None,
        );
        store.update_geometry_for_stroke(key);

        key
    }

    /// A scratch-out: passes that each run from one corner of the given rect to the other.
    fn scratch_points(min: Vector2, max: Vector2, passes: usize) -> Vec<Vector2> {
        (0..passes)
            .flat_map(|pass| {
                (0..=20).map(move |i| {
                    let t = i as f64 / 20.0;
                    let x = if pass % 2 == 0 { t } else { 1.0 - t };
                    Vector2::new(min.x + x * (max.x - min.x), min.y + t * (max.y - min.y))
                })
            })
            .collect()
    }

    #[test]
    fn trashes_a_text_stroke_a_scratch_out_covers() {
        // Typed text is erased by the same gesture as ink, so a scratch across a block of it takes
        // the whole block: text cannot be split, unlike a brush stroke.
        let mut store = StrokeStore::default();
        let text = insert_text(
            &mut store,
            "first line\nsecond line",
            Vector2::new(100.0, 100.0),
        );
        let bounds = store.get_stroke_ref(text).unwrap().bounds();
        let scribble = insert_path(
            &mut store,
            scratch_points(
                bounds.mins - Vector2::splat(10.0),
                bounds.maxs + Vector2::splat(10.0),
                5,
            ),
        );
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 1);
        assert_eq!(store.trashed(text), Some(true));
    }

    #[test]
    fn keeps_a_text_stroke_a_scratch_out_crosses_one_line_of() {
        // The block's line boxes are its ink tiles, so a scratch that stays in the band of one line
        // of a paragraph does not cover the block.
        let mut store = StrokeStore::default();
        let text = insert_text(
            &mut store,
            "first line\nsecond line\nthird line",
            Vector2::new(100.0, 100.0),
        );
        let bounds = store.get_stroke_ref(text).unwrap().bounds();
        let line_height = bounds.extents().y / 3.0;
        let scribble = insert_path(
            &mut store,
            scratch_points(
                Vector2::new(bounds.mins.x - 10.0, bounds.mins.y + 0.25 * line_height),
                Vector2::new(bounds.maxs.x + 10.0, bounds.mins.y + 0.75 * line_height),
                5,
            ),
        );
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 0);
        assert_eq!(store.trashed(text), Some(false));
    }

    #[test]
    fn trashes_only_the_strokes_that_were_scribbled_over() {
        let mut store = StrokeStore::default();
        let scribbled_over = insert_line(&mut store, 100.0, 100.0, 200.0);
        let untouched = insert_line(&mut store, 400.0, 100.0, 200.0);
        let scribble = insert_line(&mut store, 100.0, 120.0, 180.0);
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 1);
        assert_eq!(store.trashed(scribbled_over), Some(true));
        assert_eq!(store.trashed(untouched), Some(false));
        // The scribble itself is left to the caller, which removes it.
        assert_eq!(store.trashed(scribble), Some(false));
    }

    #[test]
    fn keeps_highlighter_strokes() {
        let mut store = StrokeStore::default();
        let highlighted = insert_line(&mut store, 100.0, 100.0, 200.0);
        let scribble = insert_line(&mut store, 100.0, 120.0, 180.0);
        let components = Arc::make_mut(&mut store.chrono_components);
        Arc::make_mut(components.get_mut(highlighted).unwrap()).layer = StrokeLayer::Highlighter;
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 0);
        assert_eq!(store.trashed(highlighted), Some(false));
    }

    /// A "д" drawn like a "g": a bowl and a descender loop under it, the shape that passes the
    /// scribble thresholds on its own. Its left side runs down to `(102, 150)`, a pen width from
    /// the neighbouring stroke at `x = 100`.
    fn letter_d_points() -> Vec<Vector2> {
        let (cx, cy, r) = (127.0, 150.0, 25.0);
        let bowl = (0..=48).map(|i| {
            let angle = std::f64::consts::FRAC_PI_2 - (i as f64 / 48.0) * std::f64::consts::TAU;
            Vector2::new(cx + r * angle.cos(), cy + r * angle.sin())
        });
        let loop_tail = [
            (cx + 20.0, 172.0),
            (cx + 14.0, 178.0),
            (cx + 6.0, 174.0),
            (cx + 10.0, 166.0),
        ]
        .into_iter()
        .map(|(x, y)| Vector2::new(x, y));

        bowl.chain(loop_tail).collect()
    }

    #[test]
    fn keeps_a_stroke_a_letter_merely_touches() {
        // A letter written tight against a neighbour grazes its ink edge. It is not the
        // scratch-out gesture, and even misrecognized it must not take the neighbour with it.
        let mut store = StrokeStore::default();
        let neighbour = insert_path(
            &mut store,
            (0..=10).map(|i| Vector2::new(100.0, 120.0 + 3.0 * i as f64)),
        );
        let letter = insert_path(&mut store, letter_d_points());
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(letter, viewport);

        assert_eq!(trashed, 0);
        assert_eq!(store.trashed(neighbour), Some(false));
    }

    #[test]
    fn trashes_a_stroke_a_scratch_out_covers() {
        // A scratch-out runs over the whole stroke, not only past its edge.
        let mut store = StrokeStore::default();
        let scribbled_over = insert_path(
            &mut store,
            (0..=10).map(|i| Vector2::new(100.0, 120.0 + 3.0 * i as f64)),
        );
        let scribble = insert_path(
            &mut store,
            (0..5).flat_map(|pass| {
                (0..=10).map(move |i| {
                    let t = i as f64 / 10.0;
                    let x = if pass % 2 == 0 {
                        90.0 + t * 80.0
                    } else {
                        170.0 - t * 80.0
                    };
                    Vector2::new(x, 118.0 + 8.0 * pass as f64)
                })
            }),
        );
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 1);
        assert_eq!(store.trashed(scribbled_over), Some(true));
    }

    #[test]
    fn trashes_an_enclosing_outline_a_scratch_out_crosses() {
        // An outline much bigger than the scratch-out cannot be covered by it: a scratch across
        // the arc of an ellipse covers a few percent of its ink. Crossing it out is what deletes
        // it, or a big shape could only be deleted by scratching along its whole circumference.
        let mut store = StrokeStore::default();
        let ellipse = insert_path(
            &mut store,
            (0..=360).map(|i| {
                let a = i as f64 / 360.0 * std::f64::consts::TAU;
                Vector2::new(354.0 + 346.0 * a.cos(), 162.0 + 92.0 * a.sin())
            }),
        );
        let scribble = insert_path(
            &mut store,
            scratch_points(Vector2::new(340.0, 15.0), Vector2::new(530.0, 95.0), 6),
        );
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 1);
        assert_eq!(store.trashed(ellipse), Some(true));
    }

    #[test]
    fn keeps_a_stroke_of_its_own_size_a_scratch_out_only_grazes() {
        // The same graze as `keeps_a_stroke_a_letter_merely_touches`, with the neighbour as long
        // as the scratch-out is wide: being crossed is not enough for a stroke of its own size,
        // it still has to be covered.
        let mut store = StrokeStore::default();
        let neighbour = insert_path(
            &mut store,
            (0..=10).map(|i| Vector2::new(110.0, 60.0 + 14.0 * i as f64)),
        );
        let scribble = insert_path(
            &mut store,
            scratch_points(Vector2::new(100.0, 110.0), Vector2::new(220.0, 190.0), 6),
        );
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 0);
        assert_eq!(store.trashed(neighbour), Some(false));
    }
}
