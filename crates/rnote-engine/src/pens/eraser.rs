// Imports
use super::PenBehaviour;
use super::PenStyle;
use super::pensconfig::eraserconfig::EraserStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::{DrawableOnDoc, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use piet::RenderContext;
use rnote_compose::color;
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::ext::AabbExt;
use rnote_compose::penevent::{PenEvent, PenProgress};
use rnote_compose::penpath::Element;
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum EraserState {
    Up,
    Proximity(Element),
    Down(Element),
}

#[derive(Clone, Debug)]
pub struct Eraser {
    pub(crate) state: EraserState,
    /// Coordinate system axes that had ticks or numbers erased during the current eraser stroke,
    /// which are not trashed as a whole until the eraser is lifted.
    spared_axes: HashSet<StrokeKey>,
}

impl Default for Eraser {
    fn default() -> Self {
        Self {
            state: EraserState::Up,
            spared_axes: HashSet::new(),
        }
    }
}

impl PenBehaviour for Eraser {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn style(&self) -> PenStyle {
        PenStyle::Eraser
    }

    fn update_state(&mut self, _engine_view: &mut EngineViewMut) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn handle_event(
        &mut self,
        event: PenEvent,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        let event_result = match (&mut self.state, event) {
            (EraserState::Up | EraserState::Proximity { .. }, PenEvent::Down { element, .. }) => {
                self.spared_axes.clear();
                widget_flags |= erase(element, &mut self.spared_axes, engine_view);
                self.state = EraserState::Down(element);
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (EraserState::Up | EraserState::Down { .. }, PenEvent::Proximity { element, .. }) => {
                self.state = EraserState::Proximity(element);
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress: PenProgress::Idle,
                }
            }
            (
                EraserState::Up,
                PenEvent::KeyPressed { .. } | PenEvent::Up { .. } | PenEvent::Cancel,
            ) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (EraserState::Down(current_element), PenEvent::Down { element, .. }) => {
                widget_flags |= erase(element, &mut self.spared_axes, engine_view);
                *current_element = element;
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (EraserState::Down { .. }, PenEvent::Up { element, .. }) => {
                widget_flags |= erase(element, &mut self.spared_axes, engine_view)
                    | engine_view.store.record(Instant::now());
                self.state = EraserState::Up;
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (EraserState::Down(_), PenEvent::KeyPressed { .. }) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
            (EraserState::Proximity(_), PenEvent::Up { .. }) => {
                self.state = EraserState::Up;
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress: PenProgress::Idle,
                }
            }
            (EraserState::Proximity(current_element), PenEvent::Proximity { element, .. }) => {
                *current_element = element;
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress: PenProgress::Idle,
                }
            }
            (EraserState::Proximity { .. } | EraserState::Down { .. }, PenEvent::Cancel) => {
                self.state = EraserState::Up;
                widget_flags |= engine_view.store.record(Instant::now());
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (EraserState::Proximity(_), PenEvent::KeyPressed { .. }) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (EraserState::Up, PenEvent::Text { .. }) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (EraserState::Proximity(_), PenEvent::Text { .. }) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (EraserState::Down(_), PenEvent::Text { .. }) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
        };

        (event_result, widget_flags)
    }
}

impl DrawableOnDoc for Eraser {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        match &self.state {
            EraserState::Up => None,
            EraserState::Proximity(current_element) | EraserState::Down(current_element) => Some(
                engine_view
                    .config
                    .pens_config
                    .eraser_config
                    .eraser_bounds(*current_element),
            ),
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        const OUTLINE_COLOR: piet::Color = color::GNOME_REDS[2].with_a8(240);
        const FILL_COLOR: piet::Color = color::GNOME_REDS[0].with_a8(160);
        const PROXIMITY_FILL_COLOR: piet::Color = color::GNOME_REDS[0].with_a8(51);
        let outline_width = 2.0 / engine_view.camera.total_zoom();

        match &self.state {
            EraserState::Up => {}
            EraserState::Proximity(current_element) => {
                let bounds = engine_view
                    .config
                    .pens_config
                    .eraser_config
                    .eraser_bounds(*current_element);

                let fill_rect = bounds.to_kurbo_rect();
                let outline_rect = bounds.tightened(outline_width * 0.5).to_kurbo_rect();

                cx.fill(fill_rect, &PROXIMITY_FILL_COLOR);
                cx.stroke(outline_rect, &OUTLINE_COLOR, outline_width);
            }
            EraserState::Down(current_element) => {
                let bounds = engine_view
                    .config
                    .pens_config
                    .eraser_config
                    .eraser_bounds(*current_element);

                let fill_rect = bounds.to_kurbo_rect();
                let outline_rect = bounds.tightened(outline_width * 0.5).to_kurbo_rect();

                cx.fill(fill_rect, &FILL_COLOR);
                cx.stroke(outline_rect, &OUTLINE_COLOR, outline_width);
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

fn erase(
    element: Element,
    spared_axes: &mut HashSet<StrokeKey>,
    engine_view: &mut EngineViewMut,
) -> WidgetFlags {
    // the widget_flags.store_modified flag is set in the `.trash_..()` methods
    let mut widget_flags = WidgetFlags::default();

    match &engine_view.config.pens_config.eraser_config.style {
        EraserStyle::TrashCollidingStrokes => {
            let (modified_strokes, wf) = engine_view.store.trash_colliding_strokes(
                engine_view
                    .config
                    .pens_config
                    .eraser_config
                    .eraser_bounds(element),
                engine_view.camera.viewport(),
                spared_axes,
            );
            widget_flags |= wf;

            // Rasterizing on the event thread stalled every pointer sample of an erase; the
            // images arrive a frame later instead.
            engine_view.store.regenerate_rendering_for_strokes_threaded(
                engine_view.tasks_tx.clone(),
                &modified_strokes,
                engine_view.camera.viewport(),
                engine_view.camera.image_scale(),
            );
        }
        EraserStyle::SplitCollidingStrokes => {
            let (modified_strokes, wf) = engine_view.store.split_colliding_strokes(
                engine_view
                    .config
                    .pens_config
                    .eraser_config
                    .eraser_bounds(element),
                engine_view.camera.viewport(),
                spared_axes,
            );
            widget_flags |= wf;

            engine_view.store.regenerate_rendering_for_strokes_threaded(
                engine_view.tasks_tx.clone(),
                &modified_strokes,
                engine_view.camera.viewport(),
                engine_view.camera.image_scale(),
            );
        }
    }

    widget_flags
}
