// Imports
use crate::Image;
use crate::{Engine, WidgetFlags};
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::color;
use tracing::error;

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

    /// Update the content rendering for the current viewport.
    pub fn update_content_rendering_current_viewport(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

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
        &self,
        snapshot: &gtk4::Snapshot,
        surface_bounds: p2d::bounding_volume::Aabb,
        visible_bounds: p2d::bounding_volume::Aabb,
    ) -> anyhow::Result<()> {
        use crate::engine::visual_debug;
        use crate::engine_view;
        use crate::{drawable::DrawableOnDoc, ext::GrapheneRectExt};
        use gtk4::{graphene, prelude::*};
        use rnote_compose::ext::DAffine2Ext;

        let doc_bounds = self.document.bounds();
        let viewport = self.camera.viewport();
        let camera_transform = self.camera.transform_for_gtk_snapshot();

        snapshot.save();
        snapshot.transform(Some(&camera_transform));

        self.draw_document_shadow_to_gtk_snapshot(snapshot);

        let (non_highlighter_keys, highlighter_keys) = self
            .store
            .stroke_keys_as_rendered_intersecting_bounds_partitioned(viewport);

        // push clip
        snapshot.push_clip(&graphene::Rect::from_p2d_aabb(doc_bounds));

        self.draw_background_to_gtk_snapshot(snapshot)?;
        self.draw_format_borders_to_gtk_snapshot(snapshot)?;

        // bottom image in blend (regular content)
        for &key in non_highlighter_keys.iter() {
            self.store.draw_stroke_key_to_gtk_snapshot(snapshot, key);
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
