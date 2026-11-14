// Imports
use crate::Image;
use crate::engine::{EngineTask, EngineTaskSender};
use crate::store::render_comp::{
    CONTENT_TILE_MIN_STROKES, CONTENT_TILE_TARGET_CELL_EXTENT, VIEWPORT_EXTENTS_MARGIN_FACTOR,
    gen_content_tile_image_from_strokes, image_scale_is_reusable,
};
use crate::strokes::Stroke;
use crate::{Engine, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::color;
use rnote_compose::ext::AabbExt;
use std::sync::Arc;
use tracing::error;

/// The content tile cache: the rasterized stroke content covering the extended viewport.
#[derive(Debug)]
pub(crate) struct ContentTileCache {
    /// Document-space cells, in draw order.
    pub(crate) tiles: Vec<ContentTile>,
    /// Document-space region the cells cover (the extended viewport at build time).
    pub(crate) coverage: Aabb,
    /// Image scale the tiles were rasterized at.
    pub(crate) image_scale: f64,
    /// `StrokeStore::content_revision()` at build time.
    pub(crate) revision: u64,
    /// Cells of the rebuild whose rasterization failed.
    ///
    /// Staging only, counted so that a cell whose tile never arrives cannot leave the rebuild
    /// waiting forever: the grid would then never swap in and the view would stay stale.
    pub(crate) failed_cells: usize,
}

/// One rasterized document-space cell of the content tile cache.
#[derive(Debug)]
pub(crate) struct ContentTile {
    /// The document-space cell the image covers.
    bounds: Aabb,
    /// The rasterized stroke content of the cell.
    ///
    /// Nothing reads it back: the pixels are drawn by `rendernode`, which holds them. Tests read
    /// it to check a tile really holds content, so it is kept.
    #[allow(dead_code)]
    image: Image,
    /// The node drawing the image, built once per tile.
    #[cfg(feature = "ui")]
    rendernode: Option<gtk4::gsk::RenderNode>,
}

/// The content tile rebuild whose tasks are in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContentTileRequest {
    pub(crate) coverage: Aabb,
    pub(crate) image_scale: f64,
    pub(crate) revision: u64,
    /// The number of tiles the rebuild rasterizes, i.e. the tiles of a complete grid.
    pub(crate) expected_cells: usize,
}

impl ContentTileCache {
    pub(crate) fn new(coverage: Aabb, image_scale: f64, revision: u64) -> Self {
        Self {
            tiles: vec![],
            coverage,
            image_scale,
            revision,
            failed_cells: 0,
        }
    }

    /// Whether the tiles were rasterized for the given content revision.
    ///
    /// Only the revision, and only to decide whether a rebuild is needed: a cache built for an
    /// older revision stays drawable until its replacement lands, it is just not served as the
    /// up-to-date grid.
    pub(crate) fn is_for_revision(&self, revision: u64) -> bool {
        self.revision == revision
    }

    /// Add one rasterized cell, replacing a tile of the same cell if there is one.
    pub(crate) fn push_tile(&mut self, bounds: Aabb, image: Image) -> anyhow::Result<()> {
        #[cfg(feature = "ui")]
        let rendernode = Some(image.to_rendernode()?);

        self.tiles.retain(|tile| tile.bounds != bounds);
        self.tiles.push(ContentTile {
            bounds,
            image,
            #[cfg(feature = "ui")]
            rendernode,
        });
        // cells are drawn in row-major order, which keeps overlapping content in its stroke order
        self.tiles.sort_by(|a, b| {
            a.bounds.mins[1]
                .total_cmp(&b.bounds.mins[1])
                .then(a.bounds.mins[0].total_cmp(&b.bounds.mins[0]))
        });

        Ok(())
    }

    #[cfg(feature = "ui")]
    pub(crate) fn append_to_gtk_snapshot(&self, snapshot: &gtk4::Snapshot) {
        use gtk4::prelude::*;

        for tile in self.tiles.iter() {
            if let Some(rendernode) = tile.rendernode.as_ref() {
                snapshot.append_node(rendernode);
            }
        }
    }
}

/// The document-space cells a content tile grid cuts `coverage` into, aligned to multiples of
/// `cell_extent` so a cell keeps its position when the view moves.
fn content_tile_cells(coverage: Aabb, cell_extent: f64) -> Vec<Aabb> {
    let min_col = (coverage.mins[0] / cell_extent).floor();
    let max_col = (coverage.maxs[0] / cell_extent).ceil();
    let min_row = (coverage.mins[1] / cell_extent).floor();
    let max_row = (coverage.maxs[1] / cell_extent).ceil();

    let mut cells = Vec::new();
    let mut row = min_row;
    while row < max_row {
        let mut col = min_col;
        while col < max_col {
            cells.push(Aabb::new(
                Vector2::new(col * cell_extent, row * cell_extent),
                Vector2::new((col + 1.0) * cell_extent, (row + 1.0) * cell_extent),
            ));
            col += 1.0;
        }
        row += 1.0;
    }
    cells
}

/// Rasterize one cell and send the tile back to the engine.
///
/// Shared by the worker-thread spawn and the synchronous path the tests take.
fn gen_content_tile_task(
    tasks_tx: EngineTaskSender,
    strokes: Vec<Arc<Stroke>>,
    cell_bounds: Aabb,
    image_scale: f64,
    revision: u64,
    coverage: Aabb,
) {
    match gen_content_tile_image_from_strokes(&strokes, cell_bounds, image_scale) {
        Ok(image) => tasks_tx.send(EngineTask::UpdateContentTileWithImage {
            cell_bounds,
            image,
            image_scale,
            revision,
            coverage,
        }),
        Err(e) => {
            error!("Generating a content tile image failed, cell {cell_bounds:?}, Err: {e:?}");
            tasks_tx.send(EngineTask::ContentTileFailed {
                image_scale,
                revision,
                coverage,
            });
        }
    }
}

impl Engine {
    /// Update the background rendering for the current viewport.
    ///
    /// If the background pattern or zoom has changed, the background pattern needs to be regenerated first.
    pub fn update_background_rendering_current_viewport(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        #[cfg(feature = "ui")]
        // The background is one tile repeated over the viewport when drawing (see
        // `draw_background_to_gtk_snapshot`), so a new node is only needed when the tile image
        // itself was generated again. Rebuilding it here once per viewport update meant a
        // texture-node per 128 document units, thousands of them in a zoomed out view.
        #[cfg(feature = "ui")]
        self.update_origin_indicator_rendernode();

        widget_flags.redraw = true;
        widget_flags
    }

    /// Build the render node of the origin indicator, which is drawn at a fixed surface size and
    /// therefore follows the zoom.
    #[cfg(feature = "ui")]
    fn update_origin_indicator_rendernode(&mut self) {
        use crate::ext::GrapheneRectExt;
        use gtk4::{graphene, gsk, prelude::*};

        let total_zoom = self.camera.total_zoom();

        self.origin_indicator_rendernode = self.origin_indicator_texture.as_ref().map(|texture| {
            gsk::TextureNode::new(
                texture,
                &graphene::Rect::from_p2d_aabb(
                    origin_indicator_bounds().scaled(Vector2::splat(1.0 / total_zoom)),
                ),
            )
            .upcast()
        });
    }

    /// Build the textures and render nodes of the background tile and the origin indicator.
    ///
    /// Called when their images were generated again.
    #[cfg(feature = "ui")]
    fn rebuild_background_rendering(&mut self) {
        use crate::ext::GrapheneRectExt;
        use gtk4::{graphene, gsk, prelude::*};

        self.background_tile_bounds =
            Aabb::new(Vector2::ZERO, self.document.config.background.tile_size());

        self.background_tile_texture = match self
            .background_tile_image
            .as_ref()
            .map(|image| image.to_memtexture())
        {
            None => None,
            Some(Ok(texture)) => Some(texture),
            Some(Err(e)) => {
                error!("Failed to generate memory-texture of background tile image, Err: {e:?}");
                None
            }
        };

        self.background_tile_rendernode = self.background_tile_texture.as_ref().map(|texture| {
            gsk::TextureNode::new(
                texture,
                &graphene::Rect::from_p2d_aabb(self.background_tile_bounds),
            )
            .upcast()
        });

        self.origin_indicator_texture = match self
            .origin_indicator_image
            .as_ref()
            .map(|image| image.to_memtexture())
        {
            None => None,
            Some(Ok(texture)) => Some(texture),
            Some(Err(e)) => {
                error!("Failed to generate memory-texture of origin indicator image, Err: {e:?}");
                None
            }
        };

        self.update_origin_indicator_rendernode();
    }

    /// Drop the content tiles, the rebuild in flight and its staged tiles.
    ///
    /// Used when the tiles no longer describe anything drawable: the rendering is cleared or
    /// another document is loaded.
    pub(crate) fn clear_content_tiles(&mut self) {
        self.content_tile_cache = None;
        self.content_tile_request = None;
        self.content_tile_staging = None;
        self.content_tiles_served = false;
    }

    /// Update the content tile cache for the current viewport.
    ///
    /// A dense view is drawn from rasterized content tiles instead of a render node per stroke
    /// (see [Engine::draw_to_gtk_snapshot]). This starts the rebuild when the cache no longer
    /// serves the view; the tiles arrive through [Engine::handle_engine_task].
    ///
    /// Rebuilds when the committed content changed, the image scale is away from the cached one
    /// or the cached coverage no longer contains the extended viewport. Otherwise the cache is
    /// kept.
    pub fn update_content_tile_cache_current_viewport(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        // An in-progress stroke is a preview, the committed content is unchanged. Rebuilding on
        // each preview segment would churn the cache for no gain.
        if self.penholder.pen_state_down() || self.camera.temporary_zoom() != 1.0 {
            return widget_flags;
        }

        let viewport = self.camera.viewport();
        let coverage = viewport.extend_by(viewport.extents() * VIEWPORT_EXTENTS_MARGIN_FACTOR);
        let image_scale = self.camera.image_scale();

        // The cache serves this view. This is checked before the visible stroke count: while it
        // serves, a pan or a small zoom step must not sort the visible stroke keys just to count.
        if self.content_tile_cache.as_ref().is_some_and(|cache| {
            cache.is_for_revision(self.store.content_revision())
                && image_scale_is_reusable(cache.image_scale, image_scale)
                && cache.coverage.contains(&coverage)
        }) {
            self.content_tile_request = None;
            self.content_tile_staging = None;
            return widget_flags;
        }

        let visible_non_highlighter_strokes =
            self.store.visible_non_highlighter_stroke_count(viewport);

        if visible_non_highlighter_strokes <= CONTENT_TILE_MIN_STROKES {
            if self.content_tile_cache.is_some() || self.content_tile_request.is_some() {
                self.content_tile_cache = None;
                self.content_tile_request = None;
                self.content_tile_staging = None;
                widget_flags.redraw = true;
            }
            return widget_flags;
        }

        let cell_extent = CONTENT_TILE_TARGET_CELL_EXTENT / image_scale;
        let mut cells_to_rasterize = Vec::new();

        for cell_bounds in content_tile_cells(coverage, cell_extent) {
            let strokes = self.store.content_tile_strokes(cell_bounds);
            if strokes.is_empty() {
                // nothing to rasterize, the cell stays empty
                continue;
            }
            cells_to_rasterize.push((cell_bounds, strokes));
        }

        let request = ContentTileRequest {
            coverage,
            image_scale,
            revision: self.store.content_revision(),
            expected_cells: cells_to_rasterize.len(),
        };
        if self.content_tile_request == Some(request) {
            // already being rasterized
            return widget_flags;
        }
        self.content_tile_request = Some(request);
        self.content_tile_staging = None;

        for (cell_bounds, strokes) in cells_to_rasterize {
            let tasks_tx = self.tasks_tx.clone();
            #[cfg(test)]
            gen_content_tile_task(
                tasks_tx,
                strokes,
                cell_bounds,
                image_scale,
                request.revision,
                coverage,
            );
            #[cfg(not(test))]
            rayon::spawn(move || {
                gen_content_tile_task(
                    tasks_tx,
                    strokes,
                    cell_bounds,
                    image_scale,
                    request.revision,
                    coverage,
                )
            });
        }

        widget_flags.redraw = true;

        widget_flags
    }

    /// Whether a tile or failure arriving for these rebuild parameters answers the request.
    pub(crate) fn content_tile_task_belongs_to_request(
        &self,
        image_scale: f64,
        revision: u64,
        coverage: Aabb,
    ) -> bool {
        self.content_tile_request.as_ref().is_some_and(|request| {
            request.revision == revision
                && request.image_scale == image_scale
                && request.coverage == coverage
        })
    }

    /// The staging cache of the rebuild in flight, started when its first task arrives.
    ///
    /// Only call this for a task that answers the request: a stale task must not start a grid.
    pub(crate) fn content_tile_staging_for_request(
        &mut self,
        image_scale: f64,
        revision: u64,
        coverage: Aabb,
    ) -> Option<&mut ContentTileCache> {
        let staging_is_for_request = self.content_tile_staging.as_ref().is_some_and(|staging| {
            staging.revision == revision
                && staging.image_scale == image_scale
                && staging.coverage == coverage
        });
        if !staging_is_for_request {
            self.content_tile_staging =
                Some(ContentTileCache::new(coverage, image_scale, revision));
        }
        self.content_tile_staging.as_mut()
    }

    /// Swap the staged grid in once every cell of the request has arrived or failed.
    ///
    /// Returns whether it swapped, i.e. whether the drawn content changed.
    pub(crate) fn swap_content_tile_staging_if_complete(&mut self) -> bool {
        let expected_cells = self
            .content_tile_request
            .as_ref()
            .map(|request| request.expected_cells)
            .unwrap_or_default();
        let complete = self
            .content_tile_staging
            .as_ref()
            .is_some_and(|staging| staging.tiles.len() + staging.failed_cells >= expected_cells);
        if !complete {
            return false;
        }

        match self.content_tile_staging.take() {
            // every cell failed: a grid without a single tile would blank the view, so what is
            // drawn stays and the request is dropped for the next update to retry
            Some(staging) if staging.tiles.is_empty() => {
                self.content_tile_request = None;
                false
            }
            Some(staging) => {
                self.content_tile_cache = Some(staging);
                true
            }
            None => false,
        }
    }

    /// Rasterize the highlighter strokes of the current viewport again.
    ///
    /// The highlighter layer is blended over the content tiles as a whole, so it stays per-stroke
    /// renderings: they are refreshed once a new grid is drawn, since the images of the previous
    /// highlighter content are not regenerated with the tiles.
    pub(crate) fn regenerate_highlighter_layer(&mut self) {
        self.store
            .regenerate_highlighter_rendering_in_viewport_threaded(
                self.engine_tasks_tx(),
                self.camera.viewport(),
                self.camera.image_scale(),
            );
    }

    /// Whether the drawn content is the content tile cache.
    ///
    /// The per-stroke renderings are left out while this is true: they are neither drawn nor
    /// regenerated for a view that draws tiles. A cache of older content counts: while its
    /// replacement is rasterized it stays drawable, because falling back to per-stroke images
    /// for that window would rasterize every visible stroke - the work the tiles exist to avoid.
    fn content_tiles_serve_view(&self) -> bool {
        self.content_tile_cache.is_some()
    }

    /// Update the content rendering for the current viewport.
    pub fn update_content_rendering_current_viewport(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        widget_flags |= self.update_content_tile_cache_current_viewport();

        let tiles_serve_view = self.content_tiles_serve_view();
        if tiles_serve_view {
            if !self.content_tiles_served {
                self.content_tiles_served = true;
                // no regular per-stroke image is generated, uploaded or kept for a view that
                // draws tiles
                self.store.clear_rendering();
            }
            // highlighter strokes are blended over the regular layer as a whole, so they stay
            // per-stroke renderings in the multiply layer
            self.regenerate_highlighter_layer();
            widget_flags.redraw = true;
            return widget_flags;
        }
        self.content_tiles_served = false;

        // The tiles for this view are being rasterized, or a zoom gesture is in progress on a
        // view dense enough to be tiled once it ends. The images at hand stay drawn scaled until
        // then: rasterizing every visible stroke, per wheel tick or once for a view that is about
        // to draw tiles, is the work the tiles exist to avoid.
        if self.content_tile_request.is_some()
            || (self.camera.temporary_zoom() != 1.0
                && self
                    .store
                    .visible_non_highlighter_stroke_count(self.camera.viewport())
                    > CONTENT_TILE_MIN_STROKES)
        {
            if self.content_tile_request.is_some() {
                // the highlighter layer is per-stroke also while the first grid is rasterized,
                // and nothing regenerates it once the grid swaps in: without this, the multiply
                // layer would blend the placeholder rects of unrendered highlighter strokes
                self.regenerate_highlighter_layer();
            }
            widget_flags.redraw = true;
            return widget_flags;
        }

        // While a zoom gesture is in progress, the zoom is temporary and the content is
        // rasterized at the scale it is shown at. That scale differs from the one of the existing
        // renderings, which are not stale for it: they are shown scaled, like the rest.
        if self.camera.temporary_zoom() != 1.0 {
            self.store
                .regenerate_rendering_in_viewport_zoom_gesture_threaded(
                    self.engine_tasks_tx(),
                    self.camera.viewport(),
                    self.camera.total_image_scale(),
                );
        } else {
            self.store.regenerate_rendering_in_viewport_threaded(
                self.engine_tasks_tx(),
                false,
                self.camera.viewport(),
                self.camera.image_scale(),
            );
        }

        widget_flags.redraw = true;
        widget_flags
    }

    /// Update the content and background rendering for the current viewport.
    ///
    /// If the background pattern or zoom has changed, the background pattern needs to be regenerated first.
    pub fn update_rendering_current_viewport(&mut self) -> WidgetFlags {
        self.update_background_rendering_current_viewport()
            | self.update_content_rendering_current_viewport()
    }

    /// Clear the rendering of the entire engine (e.g. when it becomes off-screen).
    pub fn clear_rendering(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        self.store.clear_rendering();
        self.clear_content_tiles();
        self.background_tile_image.take();
        self.origin_indicator_image.take();
        #[cfg(feature = "ui")]
        {
            self.background_tile_texture.take();
            self.background_tile_rendernode.take();
            self.origin_indicator_texture.take();
            self.origin_indicator_rendernode.take();
        }
        widget_flags.redraw = true;
        widget_flags
    }

    /// Regenerate the background tile image, origin indicator and updates the background rendering.
    pub fn background_rendering_regenerate(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        let image_scale = self.camera.image_scale();
        let scale_factor = self.camera.scale_factor();

        match self.document.config.background.gen_tile_image(image_scale) {
            Ok(image) => {
                self.background_tile_image = Some(image);
            }
            Err(e) => {
                error!("Regenerating background tile image failed, Err: {e:?}");
                return widget_flags;
            }
        }

        match gen_origin_indicator_image(scale_factor) {
            Ok(image) => {
                self.origin_indicator_image = Some(image);
            }
            Err(e) => {
                error!("Regenerating origin indicator image failed, Err: {e:?}");
                widget_flags.redraw = true;
                return widget_flags;
            }
        }

        #[cfg(feature = "ui")]
        self.rebuild_background_rendering();

        widget_flags |= self.update_background_rendering_current_viewport();
        widget_flags.redraw = true;
        widget_flags
    }

    /// Draws the entire engine (doc, pens, strokes, selection, ..) to a GTK snapshot.
    #[cfg(feature = "ui")]
    /// `surface_bounds` are the bounds of the canvas itself, `visible_bounds` the area that is
    /// visible on screen, both in surface coordinates. In the bounded layouts the canvas is only as
    /// large as the document, so the visible area extends past it once the document is zoomed out
    /// far enough to fit on screen.
    pub fn draw_to_gtk_snapshot(
        &mut self,
        snapshot: &gtk4::Snapshot,
        surface_bounds: p2d::bounding_volume::Aabb,
        visible_bounds: p2d::bounding_volume::Aabb,
    ) -> anyhow::Result<()> {
        use crate::engine::visual_debug;
        use crate::engine_view;
        use crate::{drawable::DrawableOnDoc, ext::GrapheneRectExt};
        use gtk4::{graphene, prelude::*};
        use rnote_compose::ext::DAffine2Ext;

        // A commit - a stroke, a trashed stroke - changes the content without moving the camera,
        // and no viewport update follows it. Start the replacement grid from the frame that first
        // draws a grid the content has moved on from, so the committed content cannot stay hidden
        // until the next pan or zoom.
        let revision = self.store.content_revision();
        let grid_is_stale = self
            .content_tile_cache
            .as_ref()
            .is_some_and(|cache| !cache.is_for_revision(revision));
        let rebuild_is_for_current_content = self
            .content_tile_request
            .as_ref()
            .is_some_and(|request| request.revision == revision);
        if grid_is_stale && !rebuild_is_for_current_content {
            let _ = self.update_content_tile_cache_current_viewport();
        }

        let doc_bounds = self.document.bounds();
        let viewport = self.camera.viewport();
        let camera_transform = self.camera.transform_for_gtk_snapshot();

        snapshot.save();
        snapshot.transform(Some(&camera_transform));

        self.draw_document_shadow_to_gtk_snapshot(snapshot);

        let tiles = self.content_tile_cache.as_ref();

        // The highlighter layer is drawn per stroke, also when the regular content comes from
        // content tiles. Only its keys are fetched then: the regular content is one node per cell,
        // so sorting and partitioning the visible stroke keys would be wasted work every frame.
        let (non_highlighter_keys, highlighter_keys) = if tiles.is_some() {
            (
                Vec::new(),
                self.store
                    .highlighter_keys_as_rendered_intersecting_bounds(viewport),
            )
        } else {
            self.store
                .stroke_keys_as_rendered_intersecting_bounds_partitioned(viewport)
        };

        // push clip
        snapshot.push_clip(&graphene::Rect::from_p2d_aabb(doc_bounds));

        self.draw_background_to_gtk_snapshot(snapshot)?;
        self.draw_format_borders_to_gtk_snapshot(snapshot)?;

        // bottom image in blend (regular content)
        match tiles {
            Some(cache) => cache.append_to_gtk_snapshot(snapshot),
            None => {
                for &key in non_highlighter_keys.iter() {
                    self.store.draw_stroke_key_to_gtk_snapshot(snapshot, key);
                }
            }
        }

        // top image in blend (highlighter layer). The whole layer is blended once,
        // so overlapping highlighter strokes don't accumulate.
        if !highlighter_keys.is_empty() {
            snapshot.push_blend(gtk4::gsk::BlendMode::Multiply);

            for &key in highlighter_keys.iter() {
                self.store.draw_stroke_key_to_gtk_snapshot(snapshot, key);
            }

            snapshot.pop();
        }

        // pop clip
        snapshot.pop();

        self.draw_origin_indicator_to_gtk_snapshot(snapshot)?;

        snapshot.restore();

        self.store
            .draw_previews_to_gtk_snapshot(snapshot, &engine_view!(self))?;

        /*
           let cairo_cx = snapshot.append_cairo(&graphene::Rect::from_p2d_aabb(surface_bounds));
               let mut piet_cx = piet_cairo::CairoRenderContext::new(&cairo_cx);
               piet_cx.transform(self.camera.transform().to_kurbo());
               self.store.draw_strokes_immediate(
                   &mut piet_cx,
                   doc_bounds,
                   viewport,
                   self.camera.image_scale(),
               );
        */
        // The ruler is drawn here instead of with the pens, because the pens are clamped to the
        // camera viewport, which in the bounded layouts is only as large as the document - the
        // ruler would be cut off outside the page.
        //
        // What it is drawn on covers the whole screen, and is painted and uploaded with every frame,
        // so it is left out altogether while the ruler is hidden: while writing, a frame is drawn
        // for every movement of the pen.
        if self
            .config
            .read()
            .pens_config
            .brush_config
            .ruler_config
            .visible
        {
            let total_zoom = self.camera.total_zoom();
            let camera_offset = self.camera.offset();
            // The zoom is positive, so the corners stay ordered.
            let visible_doc = Aabb::new(
                (visible_bounds.mins + camera_offset) / total_zoom,
                (visible_bounds.maxs + camera_offset) / total_zoom,
            );

            snapshot.save();
            let cairo_cx = snapshot.append_cairo(&graphene::Rect::from_p2d_aabb(visible_bounds));
            let mut piet_cx = piet_cairo::CairoRenderContext::new(&cairo_cx);
            piet_cx.transform(self.camera.transform().to_kurbo());
            crate::pens::ruler::draw_ruler_on_doc(
                &mut piet_cx,
                &self.config.read().pens_config.brush_config.ruler_config,
                visible_doc,
                crate::pens::pensconfig::rulerconfig::RulerView::from_camera(&self.camera),
                &self.document.config.background.color,
            )?;
            snapshot.restore();
        }

        self.penholder
            .draw_on_doc_to_gtk_snapshot(snapshot, &engine_view!(self))?;

        if self.config.read().visual_debug {
            snapshot.save();
            snapshot.transform(Some(&camera_transform));
            visual_debug::draw_stroke_debug_to_gtk_snapshot(snapshot, self, surface_bounds)?;
            snapshot.restore();

            visual_debug::draw_statistics_to_gtk_snapshot(snapshot, self, surface_bounds)?;
        }

        Ok(())
    }

    #[cfg(feature = "ui")]
    fn draw_document_shadow_to_gtk_snapshot(&self, snapshot: &gtk4::Snapshot) {
        use crate::Document;
        use crate::ext::{GdkRGBAExt, GrapheneRectExt};
        use gtk4::{gdk, graphene, gsk, prelude::*};

        let shadow_width = Document::SHADOW_WIDTH;
        let shadow_offset = Document::SHADOW_OFFSET;
        let doc_bounds = self.document.bounds();

        let corner_radius =
            graphene::Size::new(shadow_width as f32 * 0.25, shadow_width as f32 * 0.25);

        let rounded_rect = gsk::RoundedRect::new(
            graphene::Rect::from_p2d_aabb(doc_bounds),
            corner_radius,
            corner_radius,
            corner_radius,
            corner_radius,
        );

        snapshot.append_outset_shadow(
            &rounded_rect,
            &gdk::RGBA::from_compose_color(Document::SHADOW_COLOR),
            shadow_offset[0] as f32,
            shadow_offset[1] as f32,
            0.0,
            (shadow_width) as f32,
        );
    }

    #[cfg(feature = "ui")]
    fn draw_background_to_gtk_snapshot(&self, snapshot: &gtk4::Snapshot) -> anyhow::Result<()> {
        use crate::ext::{GdkRGBAExt, GrapheneRectExt};
        use gtk4::{gdk, graphene, gsk, prelude::*};
        use p2d::bounding_volume::BoundingVolume;

        let doc_bounds = self.document.bounds();

        snapshot.push_clip(&graphene::Rect::from_p2d_aabb(doc_bounds));

        // Fill with background color just in case there is any space left between the tiles
        snapshot.append_node(
            gsk::ColorNode::new(
                &gdk::RGBA::from_compose_color(self.document.config.background.color),
                //&gdk::RGBA::RED,
                &graphene::Rect::from_p2d_aabb(doc_bounds),
            )
            .upcast(),
        );

        // The tile is repeated over the visible part of the document instead of being placed once
        // per tile: a zoomed out view spans thousands of tiles, and every one of them was a node
        // of its own.
        if let (Some(tile_rendernode), Some(repeat_bounds)) = (
            self.background_tile_rendernode.as_ref(),
            doc_bounds.intersection(&self.camera.viewport()),
        ) {
            // the bounds are loosened by a fraction of a document unit, so that the last repeated
            // tile is not cut off by rounding at the edge of the viewport
            snapshot.push_repeat(
                &graphene::Rect::from_p2d_aabb(
                    repeat_bounds.loosened(1.0 / self.camera.total_zoom()),
                ),
                Some(&graphene::Rect::from_p2d_aabb(self.background_tile_bounds)),
            );
            snapshot.append_node(tile_rendernode);
            snapshot.pop();
        }

        snapshot.pop();
        Ok(())
    }

    #[cfg(feature = "ui")]
    fn draw_format_borders_to_gtk_snapshot(&self, snapshot: &gtk4::Snapshot) -> anyhow::Result<()> {
        use crate::ext::{GdkRGBAExt, GrapheneRectExt};
        use gtk4::{gdk, graphene, gsk, prelude::*};
        use p2d::bounding_volume::BoundingVolume;
        use rnote_compose::SplitOrder;
        use rnote_compose::ext::AabbExt;

        if self.document.config.format.show_borders {
            let total_zoom = self.camera.total_zoom();
            let border_width = 1.0 / total_zoom;
            let viewport = self.camera.viewport();
            let doc_bounds = self.document.bounds();

            snapshot.push_clip(&graphene::Rect::from_p2d_aabb(doc_bounds.loosened(2.0)));

            for page_bounds in doc_bounds.split_extended_origin_aligned(
                self.document.config.format.size(),
                SplitOrder::default(),
            ) {
                if !page_bounds.intersects(&viewport) {
                    continue;
                }

                let rounded_rect = gsk::RoundedRect::new(
                    graphene::Rect::from_p2d_aabb(page_bounds),
                    graphene::Size::zero(),
                    graphene::Size::zero(),
                    graphene::Size::zero(),
                    graphene::Size::zero(),
                );

                snapshot.append_border(
                    &rounded_rect,
                    &[
                        border_width as f32,
                        border_width as f32,
                        border_width as f32,
                        border_width as f32,
                    ],
                    &[
                        gdk::RGBA::from_compose_color(self.document.config.format.border_color),
                        gdk::RGBA::from_compose_color(self.document.config.format.border_color),
                        gdk::RGBA::from_compose_color(self.document.config.format.border_color),
                        gdk::RGBA::from_compose_color(self.document.config.format.border_color),
                    ],
                )
            }

            snapshot.pop();
        }

        Ok(())
    }

    /// Draw the document origin indicator cross.
    #[cfg(feature = "ui")]
    fn draw_origin_indicator_to_gtk_snapshot(
        &self,
        snapshot: &gtk4::Snapshot,
    ) -> anyhow::Result<()> {
        use gtk4::prelude::*;

        if self.document.config.format.show_origin_indicator
            && let Some(r) = &self.origin_indicator_rendernode
        {
            snapshot.append_node(r);
        }

        Ok(())
    }
}

/// Origin indicator bounds in document coordinate space.
fn origin_indicator_bounds() -> Aabb {
    const SIZE: Vector2 = Vector2::splat(17.);
    Aabb::from_half_extents(Vector2::ZERO, SIZE * 0.5)
}

fn gen_origin_indicator_image(scale_factor: f64) -> anyhow::Result<Image> {
    const PATH_COLOR: piet::Color = color::GNOME_GREENS[4];
    const PATH_WIDTH: f64 = 1.5;
    let bounds = origin_indicator_bounds();

    Image::gen_with_piet(
        |piet_cx| {
            let path = kurbo::BezPath::from_iter([
                kurbo::PathEl::MoveTo(kurbo::Point::new(
                    bounds.mins[0] + PATH_WIDTH * 0.5,
                    bounds.mins[1] + PATH_WIDTH * 0.5,
                )),
                kurbo::PathEl::LineTo(kurbo::Point::new(
                    bounds.maxs[0] - PATH_WIDTH * 0.5,
                    bounds.maxs[1] - PATH_WIDTH * 0.5,
                )),
                kurbo::PathEl::MoveTo(kurbo::Point::new(
                    bounds.mins[0] + PATH_WIDTH * 0.5,
                    bounds.maxs[1] - PATH_WIDTH * 0.5,
                )),
                kurbo::PathEl::LineTo(kurbo::Point::new(
                    bounds.maxs[0] - PATH_WIDTH * 0.5,
                    bounds.mins[1] + PATH_WIDTH * 0.5,
                )),
            ]);
            piet_cx.stroke_styled(
                path,
                &PATH_COLOR,
                PATH_WIDTH,
                &piet::StrokeStyle::default().line_cap(piet::LineCap::Round),
            );
            Ok(())
        },
        bounds,
        scale_factor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Camera;
    use crate::engine::EngineTaskReceiver;
    use crate::store::chrono_comp::StrokeLayer;
    use crate::store::render_comp::RenderCompState;
    use crate::strokes::BrushStroke;
    use rnote_compose::PenPath;
    use rnote_compose::penpath::Element;
    use rnote_compose::style::Style;

    /// The document-space region a content tile cache is built for.
    fn extended_viewport(engine: &Engine) -> Aabb {
        let viewport = engine.camera.viewport();
        viewport.extend_by(viewport.extents() * VIEWPORT_EXTENTS_MARGIN_FACTOR)
    }

    /// The grid a cache for the current view is cut into.
    fn expected_cells(engine: &Engine) -> Vec<Aabb> {
        content_tile_cells(
            extended_viewport(engine),
            CONTENT_TILE_TARGET_CELL_EXTENT / engine.camera.image_scale(),
        )
    }

    fn insert_stroke_at(engine: &mut Engine, pos: Vector2) {
        let key = engine.store.insert_stroke(
            Stroke::BrushStroke(BrushStroke::from_penpath(
                PenPath::try_from_elements([
                    Element::new(pos, Element::PRESSURE_DEFAULT),
                    Element::new(pos + Vector2::new(10., 10.), Element::PRESSURE_DEFAULT),
                ])
                .unwrap(),
                Style::default(),
            )),
            None,
        );
        engine.store.update_geometry_for_stroke(key);
    }

    /// An engine with a far-out view and more than `CONTENT_TILE_MIN_STROKES` visible strokes,
    /// covering every cell of the grid.
    fn dense_engine() -> (Engine, EngineTaskReceiver) {
        let mut engine = Engine::default();
        engine.camera = Camera::default().with_zoom(0.2);

        // fill every cell of the grid, dense enough that the views of the zoom steps below stay
        // above the tiling threshold
        let coverage = extended_viewport(&engine);
        let mut x = coverage.mins[0];
        while x <= coverage.maxs[0] {
            let mut y = coverage.mins[1];
            while y <= coverage.maxs[1] {
                insert_stroke_at(&mut engine, Vector2::new(x, y));
                y += 100.;
            }
            x += 100.;
        }

        let rx = engine
            .take_engine_tasks_rx()
            .expect("the engine holds its task receiver");
        (engine, rx)
    }

    fn take_tasks(rx: &mut EngineTaskReceiver) -> Vec<EngineTask> {
        let mut tasks = vec![];
        while let Some(task) = rx.try_recv() {
            tasks.push(task);
        }
        tasks
    }

    /// Handle every queued content tile task, returning how many there were.
    fn handle_tile_tasks(engine: &mut Engine, rx: &mut EngineTaskReceiver) -> usize {
        let tasks = take_tasks(rx);
        for task in tasks.iter().cloned() {
            let _ = engine.handle_engine_task(task);
        }
        tasks.len()
    }

    #[test]
    fn dense_far_out_view_is_tiled() {
        let (mut engine, mut rx) = dense_engine();
        let cells = expected_cells(&engine);

        let _ = engine.update_content_tile_cache_current_viewport();
        let tasks = take_tasks(&mut rx);
        assert_eq!(tasks.len(), cells.len(), "one task per non-empty cell");
        for task in tasks {
            let _ = engine.handle_engine_task(task);
        }

        let cache = engine
            .content_tile_cache
            .as_ref()
            .expect("the rasterized tiles arrived");
        assert!(cache.coverage.contains(&extended_viewport(&engine)));
        assert_eq!(cache.tiles.len(), cells.len());
        assert!(cache.image_scale > 0.0);
        assert_eq!(cache.revision, engine.store.content_revision());
        assert!(
            cache
                .tiles
                .iter()
                .all(|tile| tile.image.pixel_width > 0 && tile.image.pixel_height > 0),
            "every tile holds the rasterized cell content"
        );
    }

    #[test]
    fn sparse_documents_are_not_tiled() {
        let mut engine = Engine::default();
        let mut rx = engine
            .take_engine_tasks_rx()
            .expect("the engine holds its task receiver");

        for i in 0..10 {
            insert_stroke_at(&mut engine, Vector2::new(100. + i as f64 * 10., 100.));
        }

        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(take_tasks(&mut rx).is_empty(), "no cell is rasterized");
        assert!(engine.content_tile_cache.is_none());
        assert!(engine.content_tile_request.is_none());
    }

    #[test]
    fn content_tiles_are_reused_across_a_small_zoom_step() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);

        let (tile_count, cached_scale) = {
            let cache = engine.content_tile_cache.as_ref().unwrap();
            (cache.tiles.len(), cache.image_scale)
        };

        // one scroll step is 10%, inside the reuse tolerance, and the new viewport is still
        // contained in the cached coverage
        engine.camera = engine.camera.clone().with_zoom(0.22);
        let _ = engine.update_content_tile_cache_current_viewport();

        assert!(
            take_tasks(&mut rx).is_empty(),
            "the cache is kept, so no tile is rasterized again"
        );
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.tiles.len(), tile_count);
        assert_eq!(cache.image_scale, cached_scale);
    }

    #[test]
    fn content_tiles_are_rebuilt_past_the_reuse_tolerance() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let old_scale = engine.content_tile_cache.as_ref().unwrap().image_scale;

        engine.camera = engine.camera.clone().with_zoom(0.4);
        let _ = engine.update_content_tile_cache_current_viewport();
        let rebuilt = handle_tile_tasks(&mut engine, &mut rx);

        assert!(
            rebuilt > 0,
            "a zoom step past the tolerance rasterizes again"
        );
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_ne!(cache.image_scale, old_scale);
        assert!(image_scale_is_reusable(
            cache.image_scale,
            engine.camera.image_scale()
        ));
        assert_eq!(
            cache.tiles.len(),
            rebuilt,
            "the old tiles are replaced by the new ones"
        );
    }

    #[test]
    fn a_rebuild_keeps_the_stale_cache_drawable_until_complete() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let old_tiles = engine.content_tile_cache.as_ref().unwrap().tiles.len();
        let old_scale = engine.content_tile_cache.as_ref().unwrap().image_scale;

        engine.camera = engine.camera.clone().with_zoom(0.4);
        let _ = engine.update_content_tile_cache_current_viewport();
        let tasks = take_tasks(&mut rx);
        assert!(!tasks.is_empty(), "the rebuild is in flight");
        assert!(
            engine.content_tiles_serve_view(),
            "the stale grid still serves the view"
        );

        // one tile arriving must not replace the whole grid with a partial one
        let _ = engine.handle_engine_task(tasks[0].clone());
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.tiles.len(), old_tiles, "the stale grid is whole");
        assert_eq!(cache.image_scale, old_scale);

        for task in tasks.into_iter().skip(1) {
            let _ = engine.handle_engine_task(task);
        }
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.image_scale, engine.camera.image_scale());
        assert_ne!(cache.tiles.len(), 0, "the rebuilt grid is drawn");
    }

    #[test]
    fn a_content_change_rebuilds_the_tiles() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let revision = engine.store.content_revision();

        insert_stroke_at(&mut engine, Vector2::new(200., 200.));
        assert_ne!(engine.store.content_revision(), revision);
        assert!(
            engine.content_tiles_serve_view(),
            "the previous tiles stay drawable while the new grid is rasterized"
        );

        let _ = engine.update_content_tile_cache_current_viewport();
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_ne!(
            cache.revision,
            engine.store.content_revision(),
            "the tiles of the old content stay until the replacement lands"
        );
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.revision, engine.store.content_revision());
    }

    #[test]
    fn the_first_grid_is_rasterized_without_a_per_stroke_wave() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_rendering_current_viewport();
        let tasks = take_tasks(&mut rx);
        let tile_tasks = tasks
            .iter()
            .filter(|task| matches!(task, EngineTask::UpdateContentTileWithImage { .. }))
            .count();
        assert!(tile_tasks > 0, "the grid is rasterized");
        assert_eq!(
            tile_tasks,
            tasks.len(),
            "no visible stroke is rasterized per stroke while the grid is built"
        );
    }

    #[test]
    fn a_zoom_gesture_on_a_dense_view_does_not_rasterize_per_stroke() {
        let (mut engine, mut rx) = dense_engine();
        let backlog = take_tasks(&mut rx);
        for task in backlog {
            let _ = engine.handle_engine_task(task);
        }
        let _ = engine.camera.zoom_temporarily_to(1.2);

        let _ = engine.update_content_rendering_current_viewport();
        let tasks = take_tasks(&mut rx);
        assert!(
            tasks.is_empty(),
            "no visible stroke is rasterized while the zoom is temporary"
        );
    }

    #[test]
    fn an_edit_on_a_tiled_view_rebuilds_the_grid_without_a_per_stroke_wave() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let _ = engine.update_content_rendering_current_viewport();

        insert_stroke_at(&mut engine, Vector2::new(200., 200.));
        let _ = engine.update_content_rendering_current_viewport();
        let tasks = take_tasks(&mut rx);
        let tile_tasks = tasks
            .iter()
            .filter(|task| matches!(task, EngineTask::UpdateContentTileWithImage { .. }))
            .count();
        assert!(tile_tasks > 0, "the new grid is rasterized");
        assert_eq!(
            tile_tasks,
            tasks.len(),
            "no visible stroke is rasterized per stroke for the tiles view"
        );
    }

    #[test]
    fn only_highlighter_renderings_are_kept_while_tiles_serve() {
        let (mut engine, mut rx) = dense_engine();

        let highlighter_key = engine.store.insert_stroke(
            Stroke::BrushStroke(BrushStroke::from_penpath(
                PenPath::try_from_elements([
                    Element::new(Vector2::new(100., 100.), Element::PRESSURE_DEFAULT),
                    Element::new(Vector2::new(300., 200.), Element::PRESSURE_DEFAULT),
                ])
                .unwrap(),
                Style::default(),
            )),
            Some(StrokeLayer::Highlighter),
        );
        engine.store.update_geometry_for_stroke(highlighter_key);
        let regular_key = engine
            .store
            .keys_sorted_chrono()
            .into_iter()
            .find(|&key| key != highlighter_key)
            .expect("the dense engine holds regular strokes");

        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);

        let _ = engine.update_content_rendering_current_viewport();

        assert_eq!(
            engine.store.render_comp_state(highlighter_key),
            Some(RenderCompState::BusyRenderingInTask),
            "the highlighter stroke is still rasterized per stroke"
        );
        assert_eq!(
            engine.store.render_comp_state(regular_key),
            Some(RenderCompState::Dirty),
            "the regular layer is drawn from the tiles"
        );
    }

    #[test]
    fn appearance_changes_rebuild_the_tiles() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        assert!(engine.content_tile_cache.is_some());
        let revision = engine.store.content_revision();

        let key = engine.store.keys_sorted_chrono()[0];
        let _ = engine
            .store
            .change_stroke_colors(&[key], color::Color::BLACK);
        assert!(engine.store.content_revision() > revision);

        let _ = engine.update_content_tile_cache_current_viewport();
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_ne!(cache.revision, engine.store.content_revision());
        assert!(handle_tile_tasks(&mut engine, &mut rx) > 0);
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.revision, engine.store.content_revision());

        // geometry changes are content changes too
        let revision = engine.store.content_revision();
        engine
            .store
            .rotate_strokes(&[key], 0.1, Vector2::new(10.0, 10.0));
        assert!(engine.store.content_revision() > revision);
    }

    #[test]
    fn empty_cells_spawn_no_task() {
        let mut engine = Engine::default();
        engine.camera = Camera::default().with_zoom(0.2);
        let cells = expected_cells(&engine);

        // above the tiling threshold, but all in one cell
        for i in 0..300 {
            insert_stroke_at(&mut engine, Vector2::new(100. + i as f64 * 2.0, 100.));
        }
        let mut rx = engine
            .take_engine_tasks_rx()
            .expect("the engine holds its task receiver");

        let _ = engine.update_content_tile_cache_current_viewport();
        let tasks = take_tasks(&mut rx);
        assert_eq!(tasks.len(), 1, "only the cell holding the strokes");
        assert!(cells.len() > tasks.len());
    }

    /// The frame that first draws a grid the content has moved on from starts its replacement: a
    /// commit moves no camera, so no viewport update follows it.
    #[cfg(feature = "ui")]
    #[gtk4::test]
    fn the_frame_after_a_commit_starts_the_replacement_grid() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        let _ = handle_tile_tasks(&mut engine, &mut rx);
        assert!(engine.content_tiles_serve_view());

        // a stroke is committed, the camera does not move
        insert_stroke_at(&mut engine, Vector2::new(1000., 1000.));
        let revision = engine.store.content_revision();
        assert!(
            !engine
                .content_tile_cache
                .as_ref()
                .unwrap()
                .is_for_revision(revision)
        );
        assert!(take_tasks(&mut rx).is_empty());

        // drawing the stale grid starts its replacement
        let bounds = Aabb::new(Vector2::ZERO, Vector2::new(800., 600.));
        let snapshot = gtk4::Snapshot::new();
        engine
            .draw_to_gtk_snapshot(&snapshot, bounds, bounds)
            .unwrap();

        let request = engine
            .content_tile_request
            .expect("the replacement is requested");
        assert_eq!(request.revision, revision);

        // the committed content is drawn once the grid lands
        let tasks = take_tasks(&mut rx);
        assert!(!tasks.is_empty(), "the cells are rasterized");
        for task in tasks {
            let _ = engine.handle_engine_task(task);
        }
        let cache = engine.content_tile_cache.as_ref().unwrap();
        assert_eq!(cache.revision, revision);
    }

    #[test]
    fn clearing_the_document_drops_the_tiles() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        let _ = handle_tile_tasks(&mut engine, &mut rx);
        assert!(engine.content_tile_cache.is_some());

        // the cleared document must not keep drawing the previous document's tiles
        let _ = engine.clear();
        assert!(engine.content_tile_cache.is_none());
        assert!(engine.content_tile_request.is_none());
        assert!(!engine.content_tiles_served);
    }

    #[test]
    fn failed_cells_complete_the_rebuild() {
        let (mut engine, mut rx) = dense_engine();
        let _ = engine.update_content_tile_cache_current_viewport();
        let request = engine
            .content_tile_request
            .expect("the rebuild is requested");

        // the tile tasks are dropped and every cell reports a failure instead
        let _ = take_tasks(&mut rx);
        for _ in 0..request.expected_cells {
            let _ = engine.handle_engine_task(EngineTask::ContentTileFailed {
                image_scale: request.image_scale,
                revision: request.revision,
                coverage: request.coverage,
            });
        }

        // a grid without a single tile would blank the view, so no cache is swapped in and the
        // rebuild is not left waiting for tiles that never arrive
        assert!(engine.content_tile_cache.is_none());
        assert!(engine.content_tile_request.is_none());
    }
}
