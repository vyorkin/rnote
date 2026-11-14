// Imports
use super::chrono_comp::StrokeLayer;
use super::{Stroke, StrokeKey, StrokeStore};
use crate::Drawable;
use crate::Image;
use crate::engine::{EngineTask, EngineTaskSender};
use crate::strokes::Content;
use crate::strokes::content::GeneratedContentImages;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use rnote_compose::ext::AabbExt;
use rnote_compose::shapes::Shapeable;
use tracing::error;

/// The tolerance where check between scale-factors are considered "equal".
pub(crate) const RENDER_IMAGE_SCALE_TOLERANCE: f64 = 0.01;
/// The factor for which the rendering for the current viewport is extended by.
/// For example:: 1.0 means the viewport is extended by its own extents on all sides.
///
/// Used when checking rendering for new zooms or a moved viewport.
/// There is a trade off: a larger value will consume more memory, a smaller value will mean more stuttering on zooms and when moving the view.
pub const VIEWPORT_EXTENTS_MARGIN_FACTOR: f64 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderCompState {
    Complete,
    ForViewport(Aabb),
    BusyRenderingInTask,
    Dirty,
}

impl Default for RenderCompState {
    fn default() -> Self {
        Self::Dirty
    }
}

#[derive(Debug, Clone)]
pub struct RenderComponent {
    pub(super) state: RenderCompState,
    pub(super) images: Vec<Image>,
    #[cfg(feature = "ui")]
    pub(super) rendernodes: Vec<gtk4::gsk::RenderNode>,
    /// The textures of `images`, entry for entry.
    ///
    /// Kept so transforming a rendering only makes new render nodes around them instead of
    /// creating the textures again.
    #[cfg(feature = "ui")]
    pub(super) image_textures: Vec<crate::image::ImageTexture>,
}

impl Default for RenderComponent {
    fn default() -> Self {
        Self {
            state: RenderCompState::default(),
            images: vec![],
            #[cfg(feature = "ui")]
            rendernodes: vec![],
            #[cfg(feature = "ui")]
            image_textures: vec![],
        }
    }
}

#[cfg(feature = "ui")]
impl RenderComponent {
    /// Create the textures for the current images and the render nodes drawing them.
    fn rebuild_texture_nodes(&mut self) -> Result<(), anyhow::Error> {
        self.image_textures = Image::images_to_textures(&self.images)?;
        self.update_rendernode_transforms();
        Ok(())
    }

    /// Replace the images together with the textures and render nodes drawing them.
    fn replace_images(&mut self, images: Vec<Image>) -> Result<(), anyhow::Error> {
        self.images = images;
        if let Err(e) = self.rebuild_texture_nodes() {
            // the textures of the previous images no longer match the current ones
            self.clear_rendernodes();
            return Err(e);
        }
        Ok(())
    }

    /// Append the images together with the textures and render nodes drawing them.
    fn append_images(&mut self, mut images: Vec<Image>) -> Result<(), anyhow::Error> {
        let textures = Image::images_to_textures(&images)?;
        self.rendernodes.extend(
            textures
                .iter()
                .zip(images.iter())
                .map(|(texture, image)| texture.to_rendernode(&image.rectangle.affine)),
        );
        self.image_textures.extend(textures);
        self.images.append(&mut images);

        Ok(())
    }

    /// Rebuild the render nodes around the existing textures, at the current image affines.
    ///
    /// The textures hold their pixels and their local bounds, so this only recreates the cheap
    /// transform nodes.
    pub(super) fn update_rendernode_transforms(&mut self) {
        debug_assert_eq!(
            self.image_textures.len(),
            self.images.len(),
            "every image has a texture"
        );
        self.rendernodes = self
            .image_textures
            .iter()
            .zip(self.images.iter())
            .map(|(texture, image)| texture.to_rendernode(&image.rectangle.affine))
            .collect();
    }

    /// Drop the textures and render nodes but keep the images.
    fn clear_rendernodes(&mut self) {
        self.rendernodes = vec![];
        self.image_textures = vec![];
    }
}

impl StrokeStore {
    /// Rebuild the slotmap with empty render components with the keys returned from the stroke components.
    pub(crate) fn rebuild_render_components_slotmap(&mut self) {
        self.render_components = slotmap::SecondaryMap::new();
        self.stroke_components.keys().for_each(|key| {
            self.render_components
                .insert(key, RenderComponent::default());
        });
    }

    /// Rebuild the render components slotmap while retaining the components for all currently stored strokes
    pub(crate) fn rebuild_retain_valid_keys_render_components(&mut self) {
        self.render_components
            .retain(|k, _| self.stroke_components.contains_key(k));
        self.stroke_components.keys().for_each(|k| {
            if !self.render_components.contains_key(k) {
                self.render_components.insert(k, RenderComponent::default());
            }
        });

        let valid = |key: &StrokeKey| self.stroke_components.contains_key(*key);
        self.preview_keys.retain(valid);
        self.preview_until_rendered.retain(valid);
    }

    /// Ability if rendering is supported.
    #[allow(unused)]
    pub(crate) fn can_render(&self, key: StrokeKey) -> bool {
        self.render_components.get(key).is_some()
    }

    pub(crate) fn render_comp_state(&self, key: StrokeKey) -> Option<RenderCompState> {
        self.render_components
            .get(key)
            .map(|render_comp| render_comp.state)
    }

    pub(crate) fn set_rendering_dirty(&mut self, key: StrokeKey) {
        if let Some(render_comp) = self.render_components.get_mut(key) {
            render_comp.state = RenderCompState::Dirty;
        }
    }

    pub(crate) fn set_rendering_dirty_for_strokes(&mut self, keys: &[StrokeKey]) {
        keys.iter().for_each(|&key| self.set_rendering_dirty(key));
    }

    #[allow(unused)]
    pub(crate) fn holds_images(&self, key: StrokeKey) -> bool {
        self.render_components
            .get(key)
            .map(|s| !s.images.is_empty())
            .unwrap_or(false)
    }

    pub(crate) fn regenerate_rendering_for_stroke(
        &mut self,
        key: StrokeKey,
        viewport: Aabb,
        image_scale: f64,
    ) {
        let Some(stroke) = self.stroke_components.get(key).cloned() else {
            return;
        };

        if self.render_comp_state(key) == Some(RenderCompState::BusyRenderingInTask) {
            return;
        }

        let viewport_extended =
            viewport.extend_by(viewport.extents() * VIEWPORT_EXTENTS_MARGIN_FACTOR);

        match stroke.gen_images(viewport_extended, image_scale) {
            Ok(images) => self.replace_rendering_with_images(key, images),
            Err(e) => {
                self.set_rendering_dirty(key);
                error!("Generating images for stroke with key {key:?} failed, Err: {e:?}");
            }
        }
    }

    pub(crate) fn regenerate_rendering_for_strokes(
        &mut self,
        keys: &[StrokeKey],
        viewport: Aabb,
        image_scale: f64,
    ) {
        for &key in keys {
            self.regenerate_rendering_for_stroke(key, viewport, image_scale);
        }
    }

    pub(crate) fn regenerate_rendering_for_stroke_threaded(
        &mut self,
        tasks_tx: EngineTaskSender,
        key: StrokeKey,
        viewport: Aabb,
        image_scale: f64,
    ) {
        if let Some(stroke) = self.stroke_components.get(key)
            && let Some(render_comp) = self.render_components.get_mut(key)
        {
            if render_comp.state == RenderCompState::BusyRenderingInTask {
                return;
            }

            let stroke = stroke.clone();
            let viewport_extended =
                viewport.extend_by(viewport.extents() * VIEWPORT_EXTENTS_MARGIN_FACTOR);

            // indicates that a task is now started rendering the stroke
            render_comp.state = RenderCompState::BusyRenderingInTask;

            // Spawn a new thread for image rendering
            rayon::spawn(
                move || match stroke.gen_images(viewport_extended, image_scale) {
                    Ok(images) => {
                        tasks_tx.send(EngineTask::UpdateStrokeWithImages {
                            key,
                            images,
                            image_scale,
                        });
                    }
                    Err(e) => {
                        error!(
                            "Generating images of stroke failed while regenerating stroke rendering, stroke key {key:?} , Err: {e:?}"
                        );
                    }
                },
            );
        }
    }

    pub(crate) fn regenerate_rendering_for_strokes_threaded(
        &mut self,
        tasks_tx: EngineTaskSender,
        keys: &[StrokeKey],
        viewport: Aabb,
        image_scale: f64,
    ) {
        for &key in keys {
            self.regenerate_rendering_for_stroke_threaded(
                tasks_tx.clone(),
                key,
                viewport,
                image_scale,
            );
        }
    }

    /// Regenerate the rendering of all keys for the given viewport that need to be rerendered.
    pub(crate) fn regenerate_rendering_in_viewport_threaded(
        &mut self,
        tasks_tx: EngineTaskSender,
        force_regenerate: bool,
        viewport: Aabb,
        image_scale: f64,
    ) {
        let keys = self.render_components.keys().collect::<Vec<StrokeKey>>();

        for key in keys {
            if let Some(stroke) = self.stroke_components.get(key)
                && let Some(render_comp) = self.render_components.get_mut(key)
            {
                let tasks_tx = tasks_tx.clone();
                let stroke_bounds = stroke.bounds();
                let viewport_extended =
                    viewport.extend_by(viewport.extents() * VIEWPORT_EXTENTS_MARGIN_FACTOR);

                // skip and clear image buffer if stroke is not in viewport
                if !viewport_extended.intersects(&stroke_bounds) {
                    #[cfg(feature = "ui")]
                    {
                        render_comp.clear_rendernodes();
                    }
                    render_comp.images = vec![];
                    render_comp.state = RenderCompState::Dirty;
                    continue;
                }

                // only check if rerendering is not forced
                if !force_regenerate {
                    match render_comp.state {
                        RenderCompState::Complete | RenderCompState::BusyRenderingInTask => {
                            continue;
                        }
                        RenderCompState::ForViewport(old_viewport) => {
                            /// This factor is applied on top of the viewport extents margin factor,
                            /// so that rerendering is started a bit earlier to reaching
                            /// the edges of the viewport of the current rendered images.
                            const VIEWPORT_EXTENTS_MARGIN_RERENDER_THRESHOLD: f64 = 0.7;

                            if old_viewport.contains(
                                &(viewport.extend_by(
                                    viewport.extents()
                                        * VIEWPORT_EXTENTS_MARGIN_FACTOR
                                        * VIEWPORT_EXTENTS_MARGIN_RERENDER_THRESHOLD,
                                )),
                            ) {
                                continue;
                            }
                        }
                        RenderCompState::Dirty => {}
                    }
                }

                // indicates that a task has now started to render the stroke
                render_comp.state = RenderCompState::BusyRenderingInTask;
                let stroke = stroke.clone();

                // Spawn a new thread for image rendering
                rayon::spawn(
                    move || match stroke.gen_images(viewport_extended, image_scale) {
                        Ok(images) => {
                            tasks_tx.send(EngineTask::UpdateStrokeWithImages {
                                key,
                                images,
                                image_scale,
                            });
                        }
                        Err(e) => {
                            error!(
                                "Generating stroke images failed stroke while regenerating rendering in viewport `{viewport:?}`, stroke key: {key:?}, Err: {e:?}"
                            );
                        }
                    },
                );
            }
        }
    }

    /// Clear all rendering for all strokes.
    pub(crate) fn clear_rendering(&mut self) {
        for (_key, render_comp) in self.render_components.iter_mut() {
            #[cfg(feature = "ui")]
            {
                render_comp.clear_rendernodes();
            }
            render_comp.images = vec![];
            render_comp.state = RenderCompState::Dirty;
        }
    }

    /// Generate images and appends them to the render component for the last segments of brushstrokes.
    ///
    /// For other strokes the rendering is regenerated completely.
    pub(crate) fn append_rendering_last_segments(
        &mut self,
        tasks_tx: EngineTaskSender,
        key: StrokeKey,
        n_last_segments: usize,
        viewport: Aabb,
        image_scale: f64,
    ) {
        if let Some(stroke) = self.stroke_components.get(key)
            && let Some(render_comp) = self.render_components.get_mut(key)
        {
            match stroke.as_ref() {
                Stroke::BrushStroke(brushstroke) => {
                    match brushstroke.gen_image_for_last_segments(n_last_segments, image_scale) {
                        Ok(Some(image)) => {
                            #[cfg(feature = "ui")]
                            {
                                if let Err(e) = render_comp.append_images(vec![image]) {
                                    render_comp.state = RenderCompState::Dirty;
                                    error!(
                                        "Failed to generate rendernodes while appending last segments rendering, Err: {e:?}"
                                    );
                                }
                            }
                            #[cfg(not(feature = "ui"))]
                            {
                                render_comp.images.push(image);
                            }
                        }

                        Ok(None) => {}
                        Err(e) => {
                            render_comp.state = RenderCompState::Dirty;
                            error!(
                                "Failed to generate image while appending last segments rendering, Err: {e:?}"
                            );
                        }
                    }
                }
                // regenerate everything for strokes that don't support generating svgs for the last added elements
                Stroke::ShapeStroke(_)
                | Stroke::TextStroke(_)
                | Stroke::VectorImage(_)
                | Stroke::BitmapImage(_) => {
                    self.regenerate_rendering_for_stroke_threaded(
                        tasks_tx,
                        key,
                        viewport,
                        image_scale,
                    );
                }
            }
        }
    }

    /// Replace the entire current rendering with the given new images.
    ///
    /// Also updates the render component state.
    pub(crate) fn replace_rendering_with_images(
        &mut self,
        key: StrokeKey,
        images: GeneratedContentImages,
    ) {
        let mut replaced = false;
        if let Some(render_comp) = self.render_components.get_mut(key) {
            match images {
                GeneratedContentImages::Partial { images, viewport } => {
                    #[cfg(feature = "ui")]
                    match render_comp.replace_images(images) {
                        Ok(()) => {
                            render_comp.state = RenderCompState::ForViewport(viewport);
                            replaced = true;
                        }
                        Err(e) => {
                            error!(
                                "Generating rendernodes failed while replacing rendering with partial images, Err: {e:?}"
                            );
                            render_comp.state = RenderCompState::Dirty;
                        }
                    }
                    #[cfg(not(feature = "ui"))]
                    {
                        render_comp.images = images;
                        render_comp.state = RenderCompState::ForViewport(viewport);
                        replaced = true;
                    }
                }
                GeneratedContentImages::Full(images) => {
                    #[cfg(feature = "ui")]
                    match render_comp.replace_images(images) {
                        Ok(()) => {
                            render_comp.state = RenderCompState::Complete;
                            replaced = true;
                        }
                        Err(e) => {
                            error!(
                                "Generating rendernodes failed while replacing rendering with full images, Err: {e:?}"
                            );
                            render_comp.state = RenderCompState::Dirty;
                        }
                    }
                    #[cfg(not(feature = "ui"))]
                    {
                        render_comp.images = images;
                        render_comp.state = RenderCompState::Complete;
                        replaced = true;
                    }
                }
            }
        }

        // a stroke that was transformed stops being drawn from its geometry once its images show
        // the new transform
        if replaced {
            self.end_preview_if_rendered(key);
        }
    }

    /// Appends the images to the render component of the stroke.
    ///
    /// Not modifying the render component state, that is the responsibility of the caller.
    pub(crate) fn append_rendering_images(
        &mut self,
        key: StrokeKey,
        images: GeneratedContentImages,
    ) {
        if let Some(render_comp) = self.render_components.get_mut(key) {
            match images {
                GeneratedContentImages::Partial {
                    images,
                    viewport: _,
                }
                | GeneratedContentImages::Full(images) => {
                    #[cfg(feature = "ui")]
                    if let Err(e) = render_comp.append_images(images) {
                        error!(
                            "Generating rendernodes failed while appending rendering full images, Err: {e:?}"
                        );
                        render_comp.state = RenderCompState::Dirty;
                    }
                    #[cfg(not(feature = "ui"))]
                    {
                        render_comp.images.extend(images);
                    }
                }
            }
        }
    }

    /// Draw the geometry of the given strokes as a vector preview and drop their images until the
    /// transform ends.
    ///
    /// While a stroke is dragged, every step would otherwise need a full rasterization of its
    /// images. The geometry is drawn directly instead, which follows the pen exactly, and the
    /// images are regenerated once when the drag ends (their state is marked dirty here, so the
    /// next regeneration in viewport picks them up).
    pub(crate) fn set_previewing(&mut self, keys: &[StrokeKey], previewing: bool) {
        for &key in keys {
            if previewing {
                self.preview_keys.insert(key);
                self.preview_until_rendered.remove(&key);
            } else if self.preview_keys.contains(&key) {
                // The stroke keeps being drawn from its geometry until its regenerated images
                // arrive, otherwise the stale images of the previous position show for a frame.
                self.preview_until_rendered.insert(key);
                if let Some(render_comp) = self.render_components.get_mut(key) {
                    render_comp.state = RenderCompState::Dirty;
                }
            }
        }
    }

    /// End all vector previews, e.g. when the pen that started one is left mid-drag.
    ///
    /// The images of the affected strokes are marked dirty and the preview is kept until they are
    /// regenerated, so no stale image is drawn in between.
    pub(crate) fn end_all_previews(&mut self) {
        for &key in self.preview_keys.iter() {
            self.preview_until_rendered.insert(key);
            if let Some(render_comp) = self.render_components.get_mut(key) {
                render_comp.state = RenderCompState::Dirty;
            }
        }
    }

    /// The images of the stroke are current again, stop drawing it as a preview.
    fn end_preview_if_rendered(&mut self, key: StrokeKey) {
        if self.preview_until_rendered.remove(&key) {
            self.preview_keys.remove(&key);
        }
    }

    #[cfg(test)]
    pub(crate) fn debug_is_previewing(&self, key: StrokeKey) -> bool {
        self.preview_keys.contains(&key)
    }

    #[cfg(test)]
    pub(crate) fn debug_image_pixels(&self, key: StrokeKey) -> Option<(u32, u32)> {
        self.render_components
            .get(key)
            .and_then(|rc| rc.images.first().map(|i| (i.pixel_width, i.pixel_height)))
    }

    #[cfg(all(test, feature = "ui"))]
    pub(crate) fn debug_image_textures(
        &self,
        key: StrokeKey,
    ) -> Vec<crate::image::ImageTexture> {
        self.render_components
            .get(key)
            .map(|rc| rc.image_textures.clone())
            .unwrap_or_default()
    }

    /// Draws the geometry of the strokes being dragged, in surface coordinates.
    ///
    /// The snapshot must be untransformed: a cairo node draws its content in the coordinate space
    /// of the snapshot it is appended to, while gsk places the node with the transform in effect,
    /// so inside the camera transform the geometry is displaced and clipped away.
    ///
    /// The images of a dragged stroke are not regenerated while it is dragged (see
    /// [Self::set_previewing]), this is what keeps the stroke, and e.g. an inserted image, on
    /// screen during the drag.
    #[cfg(feature = "ui")]
    pub(crate) fn draw_previews_to_gtk_snapshot(
        &self,
        snapshot: &gtk4::Snapshot,
        engine_view: &crate::engine::EngineView,
    ) -> anyhow::Result<()> {
        use crate::drawable::DrawableOnDoc;
        use gtk4::prelude::*;

        if self.preview_keys.is_empty() {
            return Ok(());
        }

        let mut keys = self.preview_keys.iter().copied().collect::<Vec<StrokeKey>>();
        self.sort_keys_chrono(&mut keys);

        let highlighter_keys = keys
            .iter()
            .copied()
            .filter(|&key| self.stroke_layer(key) == StrokeLayer::Highlighter)
            .collect::<Vec<StrokeKey>>();

        if !highlighter_keys.is_empty() {
            // The preview stands in for the stroke in the highlighter layer, which is blended once.
            snapshot.push_blend(gtk4::gsk::BlendMode::Multiply);
        }

        // the regular layer first, then the highlighter layer, as in the render pass of the
        // strokes themselves
        for key in keys
            .iter()
            .copied()
            .filter(|key| !highlighter_keys.contains(key))
            .chain(highlighter_keys.iter().copied())
        {
            if let Some(stroke) = self.stroke_components.get(key) {
                stroke.draw_on_doc_to_gtk_snapshot(snapshot, engine_view)?;
            }
        }

        if !highlighter_keys.is_empty() {
            snapshot.pop();
        }

        Ok(())
    }

    #[cfg(feature = "ui")]
    pub(crate) fn draw_stroke_key_to_gtk_snapshot(
        &self,
        snapshot: &gtk4::Snapshot,
        key: StrokeKey,
    ) {
        use crate::ext::{GdkRGBAExt, GrapheneRectExt};
        use gtk4::{gdk, graphene, prelude::*};
        use rnote_compose::color;

        if let Some(stroke) = self.stroke_components.get(key)
            && let Some(render_comp) = self.render_components.get(key)
        {
            // A stroke being dragged is drawn from its geometry by
            // [Self::draw_previews_to_gtk_snapshot], its own images are stale.
            if self.preview_keys.contains(&key) {
                return;
            }

            // if the stroke currently does not have a rendering and is will create one,
            // draw a placeholder filled rect
            if render_comp.rendernodes.is_empty()
                && matches!(
                    render_comp.state,
                    RenderCompState::Dirty | RenderCompState::BusyRenderingInTask
                )
            {
                snapshot.append_color(
                    &gdk::RGBA::from_piet_color(color::GNOME_BRIGHTS[1].with_alpha(0.13)),
                    &graphene::Rect::from_p2d_aabb(stroke.bounds()),
                );
            }

            for rendernode in render_comp.rendernodes.iter() {
                snapshot.append_node(rendernode);
            }
        }
    }

    /// Draw the strokes for the given keys on the [piet::RenderContext].
    ///
    /// This always draws all strokes for the given keys, even trashed ones.
    #[allow(unused)]
    pub(crate) fn draw_keys_immediate(
        &self,
        keys: &[StrokeKey],
        piet_cx: &mut impl piet::RenderContext,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        for &key in keys {
            if let Some(stroke) = self.stroke_components.get(key) {
                stroke.draw(piet_cx, image_scale)?;
            }
        }

        Ok(())
    }

    /// Draw all strokes intersecting the viewport on the [piet::RenderContext].
    ///
    /// Immediate without any cached images.
    #[allow(unused)]
    pub(crate) fn draw_strokes_immediate(
        &self,
        piet_cx: &mut impl piet::RenderContext,
        _doc_bounds: Aabb,
        viewport: Aabb,
        image_scale: f64,
    ) {
        for key in self.stroke_keys_as_rendered_intersecting_bounds(viewport) {
            if let Some(stroke) = self.stroke_components.get(key)
                && let Err(e) = stroke.draw(piet_cx, image_scale)
            {
                error!("Drawing stroke immediate on piet RenderContext failed , Err: {e:?}");
            }
        }
    }

    /// Draw bounds, positions, etc. for all strokes for visual debugging purposes.
    #[cfg(feature = "ui")]
    pub(crate) fn draw_debug_to_gtk_snapshot(
        &self,
        snapshot: &gtk4::Snapshot,
        engine: &crate::Engine,
        _surface_bounds: Aabb,
    ) -> anyhow::Result<()> {
        use crate::engine::visual_debug;
        use gtk4::prelude::*;

        let border_widths = 1.0 / engine.camera.total_zoom();

        for key in self.keys_sorted_chrono() {
            if let Some(stroke) = self.stroke_components.get(key)
                && let Some(trash_comp) = self.trash_components.get(key)
            {
                // Push opacity for strokes which are normally hidden
                if trash_comp.trashed {
                    snapshot.push_opacity(0.2);
                }

                if let Some(render_comp) = self.render_components.get(key) {
                    match render_comp.state {
                        RenderCompState::Dirty => {
                            visual_debug::draw_fill_to_gtk_snapshot(
                                snapshot,
                                stroke.bounds(),
                                visual_debug::COLOR_STROKE_RENDERING_DIRTY,
                            );
                        }
                        RenderCompState::BusyRenderingInTask => {
                            visual_debug::draw_fill_to_gtk_snapshot(
                                snapshot,
                                stroke.bounds(),
                                visual_debug::COLOR_STROKE_RENDERING_BUSY,
                            );
                        }
                        _ => {}
                    }
                    render_comp.images.iter().for_each(|image| {
                        visual_debug::draw_bounds_to_gtk_snapshot(
                            // a little tightened not to overlap with other bounds
                            image.rectangle.bounds().tightened(2.0 * border_widths),
                            visual_debug::COLOR_IMAGE_BOUNDS,
                            snapshot,
                            border_widths,
                        )
                    });
                }

                for &hitbox_elem in stroke.hitboxes().iter() {
                    visual_debug::draw_bounds_to_gtk_snapshot(
                        hitbox_elem,
                        visual_debug::COLOR_STROKE_HITBOX,
                        snapshot,
                        border_widths,
                    );
                }

                visual_debug::draw_bounds_to_gtk_snapshot(
                    stroke.bounds(),
                    visual_debug::COLOR_STROKE_BOUNDS,
                    snapshot,
                    border_widths,
                );

                match stroke.as_ref() {
                    // Draw positions for brushstrokes
                    Stroke::BrushStroke(brushstroke) => {
                        for element in brushstroke.path.clone().into_elements().iter() {
                            visual_debug::draw_pos_to_gtk_snapshot(
                                snapshot,
                                element.pos,
                                visual_debug::COLOR_POS,
                                border_widths * 4.0,
                            )
                        }
                    }
                    _ => {}
                }

                // Pop Blur and opacity for hidden strokes
                if let Some(trash_comp) = self.trash_components.get(key)
                    && trash_comp.trashed
                {
                    snapshot.pop();
                }
            }
        }

        Ok(())
    }
}
