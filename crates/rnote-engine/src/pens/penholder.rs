// Imports
use super::penmode::PenModeState;
use super::shortcuts::ShortcutMode;
use super::{
    Brush, Eraser, Pen, PenBehaviour, PenMode, PenStyle, Selector, Shaper, Tools, Typewriter,
};
use crate::DrawableOnDoc;
use crate::camera::NudgeDirection;
use crate::engine::{EngineView, EngineViewMut};
use crate::pens::shortcuts::ShortcutAction;
use crate::widgetflags::WidgetFlags;
use futures::channel::oneshot;
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::eventresult::EventPropagation;
use rnote_compose::penevent::{KeyboardKey, ModifierKey, PenEvent, PenProgress, ShortcutKey};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename = "backlog_policy")]
pub enum BacklogPolicy {
    #[serde(rename = "no_limit")]
    NoLimit,
    #[serde(rename = "limit")]
    Limit(Duration),
    #[serde(rename = "disable")]
    Disable,
}

/// The Penholder holds the pens and related state and handles pen events.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename = "penholder")]
pub struct PenHolder {
    #[serde(rename = "pen_mode_state")]
    pen_mode_state: PenModeState,
    /// The policy for the retrieval of input event backlogs.
    #[serde(rename = "backlog_policy")]
    backlog_policy: BacklogPolicy,
    #[serde(skip)]
    current_pen: Pen,
    #[serde(skip)]
    progress: PenProgress,
    #[serde(skip)]
    toggle_pen_style: Option<PenStyle>,
    #[serde(skip)]
    prev_shortcut_key: Option<ShortcutKey>,
    /// Whether the eraser's square is left out when erasing with a pen.
    #[serde(skip)]
    hide_eraser_square: bool,
    /// Whether the last pen event came from a pen, rather than a mouse or touch.
    #[serde(skip)]
    pen_input: bool,
    /// The style a held key temporarily switches the pen to, if any.
    #[serde(skip)]
    hold_style: Option<PenStyle>,
    /// A held-key style change that arrived while a stroke was in progress.
    /// It is applied when the stroke finishes, so the stroke is not cancelled.
    #[serde(skip)]
    pending_hold_style: Option<Option<PenStyle>>,
    /// Whether the pen is currently down. The selector reports `InProgress` with a
    /// selection active, so its progress alone does not tell a stroke from a gesture
    /// that has already ended.
    #[serde(skip)]
    pen_state_down: bool,
}

impl Default for PenHolder {
    fn default() -> Self {
        Self {
            pen_mode_state: PenModeState::default(),
            backlog_policy: BacklogPolicy::NoLimit,

            current_pen: Pen::default(),
            progress: PenProgress::Idle,
            toggle_pen_style: None,
            prev_shortcut_key: None,
            hide_eraser_square: false,
            pen_input: false,
            hold_style: None,
            pending_hold_style: None,
            pen_state_down: false,
        }
    }
}

impl PenHolder {
    /// Get the current pen mode state.
    pub fn pen_mode_state(&self) -> PenModeState {
        self.pen_mode_state.clone()
    }

    /// Replace the pen mode state.
    pub fn set_pen_mode_state(&mut self, pen_mode_state: PenModeState) {
        self.pen_mode_state = pen_mode_state;
    }

    pub fn backlog_policy(&self) -> BacklogPolicy {
        self.backlog_policy
    }

    /// Get the style without the temporary override.
    pub fn current_pen_style(&self, engine_view: &EngineView) -> PenStyle {
        self.pen_mode_state.style(&engine_view.config.pens_config)
    }

    /// Get the current style, or the override if it is set.
    pub fn current_pen_style_w_override(&self, engine_view: &EngineView) -> PenStyle {
        // A held key takes precedence, so releasing it reveals the override that
        // was set by a stylus shortcut (if any).
        self.hold_style.unwrap_or_else(|| {
            self.pen_mode_state
                .current_style_w_override(&engine_view.config.pens_config)
        })
    }

    /// The style a held key currently switches the pen to, if any.
    pub fn hold_style(&self) -> Option<PenStyle> {
        self.hold_style
    }

    /// The current pen progress.
    pub fn current_pen_progress(&self) -> PenProgress {
        self.progress
    }

    /// Whether the currently selected pen is handling text input.
    pub fn is_handling_text_input(&self) -> bool {
        match &self.current_pen {
            Pen::Typewriter(t) => t.is_handling_text_input(),
            _ => false,
        }
    }

    pub fn current_pen_ref(&mut self) -> &Pen {
        &self.current_pen
    }

    pub fn current_pen(&self) -> &Pen {
        &self.current_pen
    }

    pub fn current_pen_mut(&mut self) -> &mut Pen {
        &mut self.current_pen
    }

    /// Change the pen style.
    pub fn change_style(
        &mut self,
        new_style: PenStyle,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let widget_flags = self.change_style_int(new_style, engine_view);
        // When the style is changed externally, the toggle mode / internal states are reset
        self.toggle_pen_style = None;
        self.prev_shortcut_key = None;

        widget_flags
    }

    /// Change the style override.
    pub fn change_style_override(
        &mut self,
        new_style_override: Option<PenStyle>,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if self.pen_mode_state.style_override() != new_style_override {
            // A held-key style can hide the override, so only a change of the
            // effective style cancels the selection and reinstalls the pen.
            let style_before = self.current_pen_style_w_override(&engine_view.as_im());
            self.pen_mode_state.set_style_override(new_style_override);
            let style_after = self.current_pen_style_w_override(&engine_view.as_im());

            if style_before != style_after {
                // Deselecting when changing the style override
                let all_strokes = engine_view.store.selection_keys_as_rendered();
                engine_view.store.set_selected_keys(&all_strokes, false);

                widget_flags |= self.reinstall_pen_current_style(engine_view);
            }
            widget_flags.refresh_ui = true;
        }

        widget_flags
    }

    /// Temporarily change the pen style while a key is held (e.g. Shift for the lasso).
    ///
    /// A change that arrives while the pen is down waits for the stroke to finish,
    /// so the stroke is neither cancelled nor drawn with the wrong pen.
    pub fn change_hold_style(
        &mut self,
        new_hold_style: Option<PenStyle>,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        if self.pen_state_down {
            self.pending_hold_style = Some(new_hold_style);
            return WidgetFlags::default();
        }

        self.apply_hold_style(new_hold_style, engine_view)
    }

    /// Drop the held-key style immediately, even in the middle of a stroke.
    pub fn reset_hold_style(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        self.pending_hold_style = None;
        self.apply_hold_style(None, engine_view)
    }

    /// Clear every temporary pen style and switch to `pen_style` in one step.
    pub fn force_pen_style(
        &mut self,
        pen_style: PenStyle,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        self.pending_hold_style = None;

        let style_before = self.current_pen_style_w_override(&engine_view.as_im());
        self.hold_style = None;
        self.pen_mode_state.remove_all_overrides();
        self.pen_mode_state
            .set_style(&mut engine_view.config.pens_config, pen_style);
        let style_after = self.current_pen_style_w_override(&engine_view.as_im());

        if style_before != style_after {
            let all_strokes = engine_view.store.selection_keys_as_rendered();
            engine_view.store.set_selected_keys(&all_strokes, false);

            widget_flags |= self.reinstall_pen_current_style(engine_view);
            widget_flags.refresh_ui = true;
        }

        // When the style is changed externally, the toggle mode / internal states are reset
        self.toggle_pen_style = None;
        self.prev_shortcut_key = None;

        widget_flags
    }

    fn apply_hold_style(
        &mut self,
        new_hold_style: Option<PenStyle>,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if self.hold_style == new_hold_style {
            return widget_flags;
        }

        let style_before = self.current_pen_style_w_override(&engine_view.as_im());
        self.hold_style = new_hold_style;
        let style_after = self.current_pen_style_w_override(&engine_view.as_im());

        // A hold that leaves the effective style alone must not cancel an active
        // selection: holding Shift while the selector is already current is a no-op.
        if style_before != style_after {
            let all_strokes = engine_view.store.selection_keys_as_rendered();
            engine_view.store.set_selected_keys(&all_strokes, false);

            widget_flags |= self.reinstall_pen_current_style(engine_view);
            widget_flags.refresh_ui = true;
        }

        widget_flags
    }

    /// Change the pen mode (pen, eraser, etc.).
    ///
    /// Relevant for stylus input.
    pub fn change_pen_mode(
        &mut self,
        new_pen_mode: PenMode,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if self.pen_mode_state.pen_mode() != new_pen_mode {
            self.pen_mode_state.set_pen_mode(new_pen_mode);
            widget_flags |= self.reinstall_pen_current_style(engine_view);
            widget_flags.refresh_ui = true;
        }

        widget_flags
    }

    pub fn current_pen_update_state(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        self.current_pen.update_state(engine_view)
    }

    /// Reinstall the pen for the current style.
    pub fn reinstall_pen_current_style(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        // first cancel the current pen
        let (_, mut widget_flags) =
            self.current_pen
                .handle_event(PenEvent::Cancel, Instant::now(), engine_view);

        // then reinstall a new pen instance
        let mut new_pen = new_pen(self.current_pen_style_w_override(&engine_view.as_im()));
        widget_flags |= new_pen.init(&engine_view.as_im()) | new_pen.update_state(engine_view);
        self.current_pen = new_pen;
        widget_flags |= self.handle_changed_pen_style(engine_view);
        self.progress = PenProgress::Idle;

        // Report the current text input state.
        widget_flags.enable_text_input = Some(self.is_handling_text_input());

        widget_flags
    }

    pub fn deinit_current_pen(&mut self) -> WidgetFlags {
        self.current_pen_mut().deinit()
    }

    /// Handle a pen event.
    pub fn handle_pen_event(
        &mut self,
        event: PenEvent,
        pen_mode: Option<PenMode>,
        now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventPropagation, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        self.pen_input = pen_mode.is_some();
        self.pen_state_down = matches!(event, PenEvent::Down { .. });
        if let Some(pen_mode) = pen_mode {
            widget_flags |= self.change_pen_mode(pen_mode, engine_view);
        }

        // Handle the event with the current pen
        let (mut event_result, wf) = self
            .current_pen
            .handle_event(event.clone(), now, engine_view);
        widget_flags |= wf | self.handle_pen_progress(event_result.progress, engine_view);

        if !event_result.handled {
            let (propagate, wf) = self.handle_pen_event_global(event, now, engine_view);
            event_result.propagate |= propagate;
            widget_flags |= wf;
        }

        // A gesture can ask for a different pen, e.g. the circle lasso hands the selection to
        // the selector. Do it after the pen handling the event has finished.
        if let Some(new_style) = widget_flags.new_pen_style.take() {
            // A held key already puts the requested style in effect, and switching the base
            // style too would keep it after the key is released.
            let is_held = self.hold_style.is_some()
                && self.current_pen_style_w_override(&engine_view.as_im()) == new_style;

            if !is_held {
                // Changing the style deselects, but a gesture asks for the selector because it has
                // just made a selection: that selection has to survive, so it can be moved as a whole.
                let selection = engine_view.store.selection_keys_as_rendered();
                widget_flags |= self.change_style(new_style, engine_view);
                if !selection.is_empty() {
                    engine_view.store.set_selected_keys(&selection, true);
                }
            }
        }

        // Report the current text input state.
        widget_flags.enable_text_input = Some(self.is_handling_text_input());

        // Always redraw after handling a pen event.
        //
        // This is also needed because pens might have claimed/requested an animation frame.
        widget_flags.redraw = true;

        (event_result.propagate, widget_flags)
    }

    /// Sets whether the eraser's square is left out, while it hovers and while it erases, when
    /// erasing with a pen. A mouse keeps it, as it is all that shows where the eraser is.
    pub fn set_hide_eraser_square(&mut self, hide: bool) {
        self.hide_eraser_square = hide;
    }

    /// Handle a requested animation frame.
    ///
    /// Can request another frame using `EngineViewMut#animation.claim_frame()`.
    pub fn handle_animation_frame(&mut self, engine_view: &mut EngineViewMut) {
        self.current_pen.handle_animation_frame(engine_view);
    }

    /// Insert text using the current pen, if possible.
    pub fn insert_text(
        &mut self,
        text: String,
        preferred_pos: Option<Vector2>,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if let Pen::Typewriter(t) = &mut self.current_pen {
            widget_flags |= t.insert_text(text, preferred_pos, engine_view);
        }

        // Report the current text input state.
        widget_flags.enable_text_input = Some(self.is_handling_text_input());

        widget_flags
    }

    /// Handle a pressed shortcut key.
    pub fn handle_pressed_shortcut_key(
        &mut self,
        shortcut_key: ShortcutKey,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventPropagation, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let mut propagate = EventPropagation::Proceed;

        if let Some(action) = engine_view
            .config
            .pens_config
            .get_shortcut_action(shortcut_key)
        {
            match action {
                ShortcutAction::ChangePenStyle { style, mode } => match mode {
                    ShortcutMode::Temporary => {
                        widget_flags |= self.change_style_override(Some(style), engine_view);
                    }
                    ShortcutMode::Permanent => {
                        self.toggle_pen_style = None;
                        widget_flags |= self.change_style_int(style, engine_view);
                    }
                    ShortcutMode::Toggle => {
                        if let Some(toggle_pen_style) = self.toggle_pen_style {
                            // if the previous key was different, but also in toggle mode,
                            // we switch to the new style instead of toggling back
                            if self
                                .prev_shortcut_key
                                .map(|k| k != shortcut_key)
                                .unwrap_or(true)
                            {
                                widget_flags |= self.change_style_int(style, engine_view);
                            } else {
                                self.toggle_pen_style = None;
                                widget_flags |=
                                    self.change_style_int(toggle_pen_style, engine_view);
                            }
                        } else {
                            self.toggle_pen_style =
                                Some(self.current_pen_style(&engine_view.as_im()));
                            widget_flags |= self.change_style_int(style, engine_view);
                        }
                    }
                    ShortcutMode::Disabled => {}
                },
            }

            propagate = EventPropagation::Stop;
        }

        self.prev_shortcut_key = Some(shortcut_key);
        widget_flags.redraw = true;

        (propagate, widget_flags)
    }

    /// Fetch clipboard content from the current pen.
    #[allow(clippy::type_complexity)]
    pub fn fetch_clipboard_content(
        &self,
        engine_view: &EngineView,
    ) -> oneshot::Receiver<anyhow::Result<(Vec<(Vec<u8>, String)>, WidgetFlags)>> {
        self.current_pen.fetch_clipboard_content(engine_view)
    }

    /// Cut clipboard content from the current pen.
    #[allow(clippy::type_complexity)]
    pub fn cut_clipboard_content(
        &mut self,
        engine_view: &mut EngineViewMut,
    ) -> oneshot::Receiver<anyhow::Result<(Vec<(Vec<u8>, String)>, WidgetFlags)>> {
        self.current_pen.cut_clipboard_content(engine_view)
    }

    /// Internal method for changing the pen style, without some of the side effects that are happening in the public
    /// method.
    fn change_style_int(
        &mut self,
        new_style: PenStyle,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if self.pen_mode_state.style(&engine_view.config.pens_config) != new_style {
            // Deselecting when changing the style
            let all_strokes = engine_view.store.selection_keys_as_rendered();
            engine_view.store.set_selected_keys(&all_strokes, false);

            self.pen_mode_state
                .set_style(&mut engine_view.config.pens_config, new_style);
            widget_flags |= self.reinstall_pen_current_style(engine_view);
            widget_flags.refresh_ui = true;
        }

        widget_flags
    }

    /// Handles the pen event in the global scope if the current pen has not handled it.
    ///
    /// Used to implement things like nudging the view, react to pressed buttons that weren't handled by th pen, ..
    fn handle_pen_event_global(
        &mut self,
        event: PenEvent,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventPropagation, WidgetFlags) {
        const MOVE_VIEW_FACTOR: f64 = 0.33;
        let mut widget_flags = WidgetFlags::default();

        let propagate = match event {
            PenEvent::Down { .. }
            | PenEvent::Up { .. }
            | PenEvent::Proximity { .. }
            | PenEvent::Text { .. }
            | PenEvent::Cancel => EventPropagation::Proceed,
            PenEvent::KeyPressed {
                keyboard_key,
                modifier_keys,
            } => match keyboard_key {
                KeyboardKey::NavUp => {
                    let nudge_amount = if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                        engine_view.camera.size()[1]
                    } else {
                        engine_view.camera.size()[1] * MOVE_VIEW_FACTOR
                    };
                    widget_flags |= engine_view.camera.nudge_by(
                        nudge_amount,
                        NudgeDirection::North,
                        engine_view.document,
                    );
                    engine_view.store.regenerate_rendering_in_viewport_threaded(
                        engine_view.tasks_tx.clone(),
                        false,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    EventPropagation::Stop
                }
                KeyboardKey::NavDown => {
                    let nudge_amount = if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                        engine_view.camera.size()[1]
                    } else {
                        engine_view.camera.size()[1] * MOVE_VIEW_FACTOR
                    };
                    widget_flags |= engine_view.camera.nudge_by(
                        nudge_amount,
                        NudgeDirection::South,
                        engine_view.document,
                    );
                    engine_view.store.regenerate_rendering_in_viewport_threaded(
                        engine_view.tasks_tx.clone(),
                        false,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    EventPropagation::Stop
                }
                KeyboardKey::NavLeft => {
                    let nudge_amount = if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                        engine_view.camera.size()[0]
                    } else {
                        engine_view.camera.size()[0] * MOVE_VIEW_FACTOR
                    };
                    widget_flags |= engine_view.camera.nudge_by(
                        nudge_amount,
                        NudgeDirection::West,
                        engine_view.document,
                    );
                    engine_view.store.regenerate_rendering_in_viewport_threaded(
                        engine_view.tasks_tx.clone(),
                        false,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    EventPropagation::Stop
                }
                KeyboardKey::NavRight => {
                    let nudge_amount = if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                        engine_view.camera.size()[0]
                    } else {
                        engine_view.camera.size()[0] * MOVE_VIEW_FACTOR
                    };
                    widget_flags |= engine_view.camera.nudge_by(
                        nudge_amount,
                        NudgeDirection::East,
                        engine_view.document,
                    );
                    engine_view.store.regenerate_rendering_in_viewport_threaded(
                        engine_view.tasks_tx.clone(),
                        false,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    EventPropagation::Stop
                }
                _ => EventPropagation::Proceed,
            },
        };

        (propagate, widget_flags)
    }

    fn handle_pen_progress(
        &mut self,
        progress: PenProgress,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        match progress {
            PenProgress::Idle => {}
            PenProgress::InProgress => {}
            PenProgress::Finished => {
                // take the style override when pen is finished
                let mut style_changed = self.pen_mode_state.take_style_override().is_some();

                // A held-key change that was deferred until the stroke ended.
                if let Some(pending) = self.pending_hold_style.take() {
                    style_changed |= self.hold_style != pending;
                    self.hold_style = pending;
                }

                if style_changed {
                    widget_flags.refresh_ui = true;
                    widget_flags |= self.reinstall_pen_current_style(engine_view);
                }
            }
        }

        self.progress = progress;

        widget_flags
    }

    fn handle_changed_pen_style(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        let current_style = self.current_pen_style_w_override(&engine_view.as_im());

        self.backlog_policy = match current_style {
            PenStyle::Brush => BacklogPolicy::Limit(Duration::from_millis(4)),
            PenStyle::Shaper => BacklogPolicy::Limit(Duration::from_millis(8)),
            PenStyle::Typewriter => BacklogPolicy::Limit(Duration::from_millis(33)),
            PenStyle::Eraser => BacklogPolicy::Limit(Duration::from_millis(33)),
            PenStyle::Selector => BacklogPolicy::Limit(Duration::from_millis(33)),
            PenStyle::Tools => BacklogPolicy::Disable,
        };

        // Report the current text input state.
        widget_flags.enable_text_input = Some(self.is_handling_text_input());
        widget_flags.redraw = true;

        widget_flags
    }
}

impl DrawableOnDoc for PenHolder {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        self.current_pen.bounds_on_doc(engine_view)
    }
    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        if self.hide_eraser_square && self.pen_input && matches!(self.current_pen, Pen::Eraser(_)) {
            return Ok(());
        }

        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        self.current_pen.draw_on_doc(cx, engine_view)?;

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

fn new_pen(pen_style: PenStyle) -> Pen {
    match pen_style {
        PenStyle::Brush => Pen::Brush(Brush::default()),
        PenStyle::Shaper => Pen::Shaper(Shaper::default()),
        PenStyle::Typewriter => Pen::Typewriter(Typewriter::default()),
        PenStyle::Eraser => Pen::Eraser(Eraser::default()),
        PenStyle::Selector => Pen::Selector(Selector::default()),
        PenStyle::Tools => Pen::Tools(Tools::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use crate::pens::pensconfig::selectorconfig::SelectorStyle;
    use rnote_compose::penpath::Element;
    use std::collections::HashSet;

    /// Draw a stroke through the points with the current pen.
    fn draw(engine: &mut Engine, points: &[(f64, f64)], now: Instant) {
        let element =
            |(x, y): (f64, f64)| Element::new(Vector2::new(x, y), Element::PRESSURE_DEFAULT);
        let (first, rest) = points.split_first().expect("two points");
        let (last, middle) = rest.split_last().expect("two points");

        let mut events = vec![PenEvent::Down {
            element: element(*first),
            modifier_keys: HashSet::new(),
        }];
        events.extend(middle.iter().map(|point| PenEvent::Down {
            element: element(*point),
            modifier_keys: HashSet::new(),
        }));
        events.push(PenEvent::Up {
            element: element(*last),
            modifier_keys: HashSet::new(),
        });

        // The path builder measures the time between points, so the events must not share a time.
        for (i, event) in events.into_iter().enumerate() {
            let _ =
                engine.handle_pen_event(event, None, now + Duration::from_millis(16 * i as u64));
        }
    }

    #[test]
    fn a_circle_lasso_keeps_the_selection_it_made() {
        let mut engine = Engine::default();
        let _ = engine.change_pen_style(PenStyle::Brush);
        let now = Instant::now();

        // A stroke the loop goes around.
        draw(&mut engine, &[(100.0, 100.0), (140.0, 100.0)], now);

        let circle: Vec<(f64, f64)> = (0..=32)
            .map(|i| {
                let angle = i as f64 / 32.0 * std::f64::consts::TAU;
                (120.0 + 60.0 * angle.cos(), 100.0 + 60.0 * angle.sin())
            })
            .collect();
        draw(&mut engine, &circle, now);

        // Handing over to the selector deselects, unless the selection the lasso just made survives.
        assert_eq!(
            engine.current_pen_style_w_override(),
            PenStyle::Selector,
            "the lasso hands the pen to the selector"
        );
        assert_eq!(
            engine.store.selection_keys_as_rendered().len(),
            1,
            "the loop selects the stroke it went around"
        );
    }

    /// A loop around the stroke drawn by `draw(.., &[(100.0, 100.0), (140.0, 100.0)], ..)`.
    fn lasso_circle() -> Vec<(f64, f64)> {
        (0..=32)
            .map(|i| {
                let angle = i as f64 / 32.0 * std::f64::consts::TAU;
                (120.0 + 60.0 * angle.cos(), 100.0 + 60.0 * angle.sin())
            })
            .collect()
    }

    #[test]
    fn a_hold_style_pressed_mid_stroke_waits_for_the_stroke_to_end() {
        let mut engine = Engine::default();
        let _ = engine.change_pen_style(PenStyle::Brush);
        let now = Instant::now();
        let element = Element::new(Vector2::new(100.0, 100.0), Element::PRESSURE_DEFAULT);

        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element,
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );
        let _ = engine.change_pen_style_hold(Some(PenStyle::Selector));

        assert_eq!(
            engine.current_pen_style_w_override(),
            PenStyle::Brush,
            "a stroke in progress is not cancelled by the held key"
        );
        assert_eq!(
            engine.penholder.hold_style(),
            None,
            "the held-key style is pending until the stroke ends"
        );

        let _ = engine.handle_pen_event(
            PenEvent::Up {
                element,
                modifier_keys: HashSet::new(),
            },
            None,
            now + Duration::from_millis(16),
        );

        assert_eq!(
            engine.current_pen_style_w_override(),
            PenStyle::Selector,
            "the held-key style applies when the stroke ends"
        );
    }

    #[test]
    fn a_hold_that_does_not_change_the_style_keeps_the_selection() {
        let mut engine = Engine::default();
        let _ = engine.change_pen_style(PenStyle::Brush);
        let now = Instant::now();

        // A stroke, then a loop around it: the loop hands the pen to the selector.
        draw(&mut engine, &[(100.0, 100.0), (140.0, 100.0)], now);
        draw(&mut engine, &lasso_circle(), now);
        assert_eq!(engine.current_pen_style_w_override(), PenStyle::Selector);
        assert_eq!(engine.store.selection_keys_as_rendered().len(), 1);

        // Holding Shift while the selector is already current must not drop the selection.
        let _ = engine.change_pen_style_hold(Some(PenStyle::Selector));
        assert_eq!(engine.store.selection_keys_as_rendered().len(), 1);
        let _ = engine.change_pen_style_hold(None);
        assert_eq!(engine.store.selection_keys_as_rendered().len(), 1);
        assert_eq!(engine.current_pen_style_w_override(), PenStyle::Selector);
    }

    #[test]
    fn releasing_the_hold_returns_to_the_previous_style() {
        let mut engine = Engine::default();
        let _ = engine.change_pen_style(PenStyle::Brush);
        let now = Instant::now();

        draw(&mut engine, &[(100.0, 100.0), (140.0, 100.0)], now);
        let _ = engine.change_pen_style_hold(Some(PenStyle::Selector));
        assert_eq!(engine.current_pen_style_w_override(), PenStyle::Selector);

        // The selector set to the lasso: a loop around the stroke selects it.
        engine
            .engine_config()
            .write()
            .pens_config
            .selector_config
            .style = SelectorStyle::Polygon;
        draw(&mut engine, &lasso_circle(), now);
        assert_eq!(engine.store.selection_keys_as_rendered().len(), 1);

        let _ = engine.change_pen_style_hold(None);
        assert_eq!(
            engine.current_pen_style_w_override(),
            PenStyle::Brush,
            "the previous style comes back"
        );
        assert!(
            engine.store.selection_keys_as_rendered().is_empty(),
            "the selection goes away with the temporary style"
        );
    }

    #[test]
    fn force_pen_style_drops_the_hold_and_the_override() {
        let mut engine = Engine::default();
        let _ = engine.change_pen_style(PenStyle::Selector);
        let _ = engine.change_pen_style_override(Some(PenStyle::Tools));
        let _ = engine.change_pen_style_hold(Some(PenStyle::Shaper));
        assert_eq!(engine.current_pen_style_w_override(), PenStyle::Shaper);

        let _ = engine.force_pen_style(PenStyle::Brush);
        assert_eq!(engine.current_pen_style_w_override(), PenStyle::Brush);
        assert_eq!(engine.penholder.hold_style(), None);

        let _ = engine.change_pen_style_hold(None);
        assert_eq!(
            engine.current_pen_style_w_override(),
            PenStyle::Brush,
            "no temporary override is left behind"
        );
    }
}
