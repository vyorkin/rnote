// Imports
use super::{ModifyState, SelectionMode, Typewriter, TypewriterState};
use crate::engine::EngineViewMut;
use crate::pens::PenBehaviour;
use crate::strokes::{Stroke, TextStroke};
use crate::{DrawableOnDoc, StrokeStore, WidgetFlags};
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::penevent::{KeyboardKey, ModifierKey, PenProgress};
use rnote_compose::penpath::Element;
use rnote_compose::shapes::Shapeable;
use std::collections::HashSet;
use std::time::Instant;
use unicode_segmentation::GraphemeCursor;

impl Typewriter {
    pub(super) fn handle_pen_event_down(
        &mut self,
        element: Element,
        _modifier_keys: HashSet<ModifierKey>,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let typewriter_bounds = self.bounds_on_doc(&engine_view.as_im());
        let text_width = engine_view
            .config
            .pens_config
            .typewriter_config
            .text_width();
        self.pos = Some(element.pos);

        let event_result = match &mut self.state {
            TypewriterState::Idle | TypewriterState::Start { .. } => {
                let mut refresh_state = false;
                let mut new_state = TypewriterState::Start(
                    engine_view
                        .document
                        .snap_position(element.pos, engine_view.config),
                );

                if let Some(&stroke_key) = engine_view
                    .store
                    .stroke_hitboxes_contain_coord(engine_view.camera.viewport(), element.pos)
                    .last()
                {
                    // When clicked on a textstroke, we start modifying it
                    if let Some(Stroke::TextStroke(textstroke)) =
                        engine_view.store.get_stroke_ref(stroke_key)
                    {
                        let cursor = if let Ok(new_cursor) =
                            // get the cursor for the current position
                            textstroke.get_cursor_for_global_coord(element.pos)
                        {
                            new_cursor
                        } else {
                            GraphemeCursor::new(0, textstroke.text.len(), true)
                        };

                        engine_view.store.update_chrono_to_last(stroke_key);

                        new_state = TypewriterState::Modifying {
                            modify_state: ModifyState::Idle,
                            stroke_key,
                            cursor,
                            pen_down: true,
                        };
                        refresh_state = true;
                    }
                }

                self.state = new_state;
                self.reset_blink();

                // after setting new state
                if refresh_state {
                    // Update typewriter state for the current textstroke,
                    // and flag to update the UI
                    widget_flags |= self.update_state(engine_view);
                    widget_flags.refresh_ui = true;
                }

                // possibly nudge camera
                widget_flags |= engine_view
                    .camera
                    .nudge_w_pos(element.pos, engine_view.document);
                widget_flags |= engine_view
                    .document
                    .expand_autoexpand(engine_view.camera, engine_view.store);
                engine_view.store.regenerate_rendering_in_viewport_threaded(
                    engine_view.tasks_tx.clone(),
                    false,
                    engine_view.camera.viewport(),
                    engine_view.camera.image_scale(),
                );

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down,
            } => {
                match modify_state {
                    ModifyState::Idle => {
                        let mut progress = PenProgress::InProgress;

                        if let Some(typewriter_bounds) = typewriter_bounds
                            && let Some(Stroke::TextStroke(textstroke)) =
                                engine_view.store.get_stroke_ref(*stroke_key)
                        {
                            if Self::translate_node_bounds(typewriter_bounds, engine_view.camera)
                                .contains_local_point(element.pos)
                            {
                                // switch to translating state
                                self.state = TypewriterState::Modifying {
                                    modify_state: ModifyState::Translating {
                                        current_pos: element.pos,
                                    },
                                    stroke_key: *stroke_key,
                                    cursor: cursor.clone(),
                                    pen_down: true,
                                };
                            } else if Self::adjust_text_width_node_bounds(
                                Self::text_rect_bounds(text_width, textstroke).mins,
                                text_width,
                                engine_view.camera,
                            )
                            .contains_local_point(element.pos)
                            {
                                // switch to adjust text width
                                self.state = TypewriterState::Modifying {
                                    modify_state: ModifyState::AdjustTextWidth {
                                        start_text_width: text_width,
                                        start_pos: element.pos,
                                        current_pos: element.pos,
                                    },
                                    stroke_key: *stroke_key,
                                    cursor: cursor.clone(),
                                    pen_down: true,
                                };
                            // This is intentionally **not** the textstroke hitboxes
                            } else if typewriter_bounds.contains_local_point(element.pos)
                                && let Some(Stroke::TextStroke(textstroke)) =
                                    engine_view.store.get_stroke_ref(*stroke_key)
                            {
                                if let Ok(new_cursor) =
                                    textstroke.get_cursor_for_global_coord(element.pos)
                                {
                                    if new_cursor.cur_cursor() != cursor.cur_cursor() && *pen_down {
                                        // switch to selecting state
                                        self.state = TypewriterState::Modifying {
                                            modify_state: ModifyState::Selecting {
                                                selection_cursor: cursor.clone(),
                                                mode: SelectionMode::Caret,
                                                finished: false,
                                            },
                                            stroke_key: *stroke_key,
                                            cursor: cursor.clone(),
                                            pen_down: true,
                                        };
                                    } else {
                                        *cursor = new_cursor;
                                        *pen_down = true;
                                        self.reset_blink();
                                    }
                                }
                            } else {
                                // If we click outside, reset to idle
                                self.state = TypewriterState::Idle;
                                progress = PenProgress::Finished;
                            }
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress,
                        }
                    }
                    ModifyState::Selecting {
                        selection_cursor,
                        mode,
                        finished,
                    } => {
                        let mut progress = PenProgress::InProgress;

                        if let Some(typewriter_bounds) = typewriter_bounds {
                            // Clicking on the translate node
                            if Self::translate_node_bounds(typewriter_bounds, engine_view.camera)
                                .contains_local_point(element.pos)
                            {
                                self.state = TypewriterState::Modifying {
                                    modify_state: ModifyState::Translating {
                                        current_pos: element.pos,
                                    },
                                    stroke_key: *stroke_key,
                                    cursor: cursor.clone(),
                                    pen_down: true,
                                };
                            } else if typewriter_bounds.contains_local_point(element.pos) {
                                if let Some(Stroke::TextStroke(textstroke)) =
                                    engine_view.store.get_stroke_ref(*stroke_key)
                                {
                                    if *finished {
                                        if let Ok(new_cursor) =
                                            textstroke.get_cursor_for_global_coord(element.pos)
                                        {
                                            // If selecting is finished, return to modifying with the current pen position as cursor
                                            self.state = TypewriterState::Modifying {
                                                modify_state: ModifyState::Idle,
                                                stroke_key: *stroke_key,
                                                cursor: new_cursor,
                                                pen_down: true,
                                            };
                                            self.reset_blink();
                                        }
                                    } else {
                                        // Updating the cursor for the clicked position
                                        if let Ok(new_cursor) =
                                            textstroke.get_cursor_for_global_coord(element.pos)
                                        {
                                            let previous_cursor_position = cursor.cur_cursor();
                                            *cursor = new_cursor;

                                            match mode {
                                                SelectionMode::Word(start, end) => {
                                                    let mouse_position = cursor.cur_cursor();

                                                    if mouse_position <= *start {
                                                        selection_cursor.set_cursor(*end);
                                                        textstroke
                                                            .move_cursor_word_boundary_back(cursor);
                                                    } else if mouse_position >= *end {
                                                        selection_cursor.set_cursor(*start);
                                                        textstroke
                                                            .move_cursor_word_boundary_forward(
                                                                cursor,
                                                            );
                                                    } else {
                                                        selection_cursor.set_cursor(*start);
                                                        cursor.set_cursor(*end);
                                                    }
                                                }
                                                SelectionMode::Line(start, end) => {
                                                    let mouse_position = cursor.cur_cursor();

                                                    if mouse_position < *start {
                                                        selection_cursor.set_cursor(*end);
                                                        textstroke.move_cursor_line_start(cursor);
                                                    } else if mouse_position > *end {
                                                        selection_cursor.set_cursor(*start);
                                                        textstroke.move_cursor_line_end(cursor);
                                                    } else {
                                                        selection_cursor.set_cursor(*start);
                                                        cursor.set_cursor(*end);
                                                    }
                                                }
                                                SelectionMode::Caret => {}
                                            }

                                            if previous_cursor_position != cursor.cur_cursor() {
                                                self.reset_blink();
                                            }
                                        }
                                    }
                                }
                            } else {
                                // If we click outside, reset to idle
                                self.state = TypewriterState::Idle;
                                progress = PenProgress::Finished;
                            }
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress,
                        }
                    }
                    ModifyState::Translating { current_pos, .. } => {
                        if let Some(textstroke_bounds) = engine_view
                            .store
                            .get_stroke_ref(*stroke_key)
                            .map(|s| s.bounds())
                        {
                            let snap_corner_pos = textstroke_bounds.mins;
                            let offset = engine_view.document.snap_position(
                                snap_corner_pos + (element.pos - *current_pos),
                                engine_view.config,
                            ) - snap_corner_pos;

                            // The text follows the pen as a vector preview: its images would
                            // otherwise be re-uploaded as textures on every pointer sample.
                            // They are regenerated once, when the drag ends.
                            engine_view.store.set_previewing(&[*stroke_key], true);

                            if offset.length()
                                > Self::TRANSLATE_OFFSET_THRESHOLD / engine_view.camera.total_zoom()
                            {
                                // move text
                                engine_view.store.translate_strokes(&[*stroke_key], offset);
                                *current_pos += offset;

                                widget_flags.store_modified = true;
                            }

                            // possibly nudge camera
                            widget_flags |= engine_view
                                .camera
                                .nudge_w_pos(element.pos, engine_view.document);
                            widget_flags |= engine_view
                                .document
                                .expand_autoexpand(engine_view.camera, engine_view.store);

                            // The preview is drawn from the stroke geometry on every redraw, so
                            // the tile cache only has to be regenerated when the view changed.
                            if widget_flags.view_modified | widget_flags.resize {
                                engine_view.store.regenerate_rendering_in_viewport_threaded(
                                    engine_view.tasks_tx.clone(),
                                    false,
                                    engine_view.camera.viewport(),
                                    engine_view.camera.image_scale(),
                                );
                            }
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        }
                    }
                    ModifyState::AdjustTextWidth {
                        start_text_width,
                        start_pos,
                        current_pos,
                    } => {
                        let x_offset = element.pos[0] - current_pos[0];

                        if let Some(Stroke::TextStroke(textstroke)) =
                            engine_view.store.get_stroke_mut(*stroke_key)
                            && x_offset.abs()
                                > Self::ADJ_TEXT_WIDTH_THRESHOLD / engine_view.camera.total_zoom()
                        {
                            let new_text_width =
                                *start_text_width + (element.pos[0] - start_pos[0]);
                            engine_view
                                .config
                                .pens_config
                                .typewriter_config
                                .set_text_width(new_text_width);
                            textstroke.text_style.set_max_width(Some(new_text_width));
                            engine_view.store.regenerate_rendering_for_stroke(
                                *stroke_key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );

                            *current_pos = element.pos;

                            widget_flags.store_modified = true;
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        }
                    }
                }
            }
        };

        (event_result, widget_flags)
    }

    pub(super) fn handle_pen_event_up(
        &mut self,
        element: Element,
        _modifier_keys: HashSet<ModifierKey>,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        self.pos = Some(element.pos);

        let event_result = match &mut self.state {
            TypewriterState::Idle => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            TypewriterState::Start(_) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down,
                ..
            } => {
                *pen_down = false;

                match modify_state {
                    ModifyState::Idle => {}
                    ModifyState::Selecting { finished, .. } => {
                        // finished when drag ended
                        *finished = true;
                    }
                    ModifyState::Translating { .. } => {
                        engine_view.store.set_previewing(&[*stroke_key], false);
                        engine_view
                            .store
                            .update_geometry_for_strokes(&[*stroke_key]);
                        engine_view.store.regenerate_rendering_for_stroke(
                            *stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);

                        self.state = TypewriterState::Modifying {
                            modify_state: ModifyState::Idle,
                            stroke_key: *stroke_key,
                            cursor: cursor.clone(),
                            pen_down: false,
                        };

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;
                    }
                    ModifyState::AdjustTextWidth { .. } => {
                        engine_view
                            .store
                            .update_geometry_for_strokes(&[*stroke_key]);
                        engine_view.store.regenerate_rendering_for_stroke(
                            *stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);

                        self.state = TypewriterState::Modifying {
                            modify_state: ModifyState::Idle,
                            stroke_key: *stroke_key,
                            cursor: cursor.clone(),
                            pen_down: false,
                        };

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;
                    }
                }

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
        };

        (event_result, widget_flags)
    }

    pub(super) fn handle_pen_event_proximity(
        &mut self,
        element: Element,
        _modifier_keys: HashSet<ModifierKey>,
        _now: Instant,
        _engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let widget_flags = WidgetFlags::default();
        self.pos = Some(element.pos);

        let event_result = match &mut self.state {
            TypewriterState::Idle => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            TypewriterState::Start(_) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
            TypewriterState::Modifying {
                modify_state,
                pen_down,
                ..
            } => {
                if !matches!(modify_state, ModifyState::Selecting { .. }) {
                    // do nothing if the state is selected
                    // This prevents text from becoming deselected when hovering the pen
                    // see issue #1222
                    *modify_state = ModifyState::Idle;
                }
                *pen_down = false;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
        };

        (event_result, widget_flags)
    }

    pub(super) fn handle_pen_event_keypressed(
        &mut self,
        keyboard_key: KeyboardKey,
        modifier_keys: HashSet<ModifierKey>,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        self.pos = None;

        // `Escape` leaves the text input session, the way it leaves editing in any text field.
        // The text stroke is written key by key, so there is nothing left to commit - dropping to
        // idle is the whole of it. Without this the canvas keeps the bare tool keys gated out
        // (`is_handling_text_input`) and no key gets the keyboard back from the text box.
        if keyboard_key == KeyboardKey::Escape && !matches!(self.state, TypewriterState::Idle) {
            self.state = TypewriterState::Idle;

            return (
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                },
                widget_flags,
            );
        }

        // `Ctrl` and `Alt` chords are shortcuts before they are text: paste, copy, undo, bold and
        // the rest are window accelerators, and they only run if this handler lets the key pass.
        // The canvas key controller sits in the capture phase, so inserting the bare letter and
        // returning `Stop` also swallowed the chord - dictating with voxtype, which pastes with
        // `Ctrl`+`V`, left a lone "v" in the text box instead of the transcribed sentence.
        // `Ctrl`+`A` is the one chord the typewriter handles itself, by selecting the whole text.
        if let KeyboardKey::Unicode(keychar) = keyboard_key
            && (modifier_keys.contains(&ModifierKey::KeyboardCtrl)
                || modifier_keys.contains(&ModifierKey::KeyboardAlt))
            && !(keychar == 'a' && matches!(self.state, TypewriterState::Modifying { .. }))
        {
            let progress = match self.state {
                TypewriterState::Idle => PenProgress::Idle,
                TypewriterState::Start(_) | TypewriterState::Modifying { .. } => {
                    PenProgress::InProgress
                }
            };
            return (
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress,
                },
                widget_flags,
            );
        }

        let text_width = engine_view
            .config
            .pens_config
            .typewriter_config
            .text_width();
        let mut text_style = engine_view
            .config
            .pens_config
            .typewriter_config
            .text_style
            .clone();

        let event_result = match &mut self.state {
            TypewriterState::Idle => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            TypewriterState::Start(pos) => {
                #[cfg(feature = "ui")]
                super::play_sound(Some(keyboard_key), engine_view.audioplayer);

                match keyboard_key {
                    KeyboardKey::Unicode(keychar) => {
                        text_style.ranged_text_attributes.clear();
                        text_style.set_max_width(Some(text_width));
                        let textstroke = TextStroke::new(String::from(keychar), *pos, text_style);
                        let mut cursor = GraphemeCursor::new(0, textstroke.text.len(), true);

                        textstroke.move_cursor_forward(&mut cursor);
                        let stroke_key = engine_view
                            .store
                            .insert_stroke(Stroke::TextStroke(textstroke), None);
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);
                        engine_view.store.regenerate_rendering_for_stroke(
                            stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );

                        self.state = TypewriterState::Modifying {
                            modify_state: ModifyState::Idle,
                            stroke_key,
                            cursor,
                            pen_down: false,
                        };

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        }
                    }
                    _ => EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::InProgress,
                    },
                }
            }
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down,
            } => {
                match modify_state {
                    ModifyState::Idle => {
                        #[cfg(feature = "ui")]
                        super::play_sound(Some(keyboard_key), engine_view.audioplayer);

                        if let Some(Stroke::TextStroke(textstroke)) =
                            engine_view.store.get_stroke_mut(*stroke_key)
                        {
                            let mut update_stroke =
                                |store: &mut StrokeStore, keychar_is_whitespace: bool| {
                                    store.update_geometry_for_stroke(*stroke_key);
                                    store.regenerate_rendering_for_stroke(
                                        *stroke_key,
                                        engine_view.camera.viewport(),
                                        engine_view.camera.image_scale(),
                                    );
                                    widget_flags |= engine_view
                                        .document
                                        .resize_autoexpand(store, engine_view.camera);
                                    if keychar_is_whitespace {
                                        widget_flags |= store.record(Instant::now());
                                    } else {
                                        widget_flags |=
                                            store.update_latest_history_entry(Instant::now());
                                    }

                                    widget_flags.store_modified = true;
                                };

                            *pen_down = false;

                            // Handling keyboard input
                            match keyboard_key {
                                KeyboardKey::Unicode(keychar) => {
                                    if keychar == 'a'
                                        && modifier_keys.contains(&ModifierKey::KeyboardCtrl)
                                    {
                                        cursor.set_cursor(textstroke.text.len());
                                        // Select entire text
                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: GraphemeCursor::new(
                                                0,
                                                textstroke.text.len(),
                                                true,
                                            ),
                                            mode: SelectionMode::Caret,
                                            finished: true,
                                        };
                                    } else {
                                        let insert_pos = cursor.cur_cursor();
                                        let typed_text = keychar.to_string();
                                        textstroke
                                            .insert_text_after_cursor(typed_text.as_str(), cursor);
                                        textstroke.color_text_in_range(
                                            insert_pos..insert_pos + typed_text.len(),
                                            text_style.color,
                                        );
                                        update_stroke(engine_view.store, keychar.is_whitespace());
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::BackSpace => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                        textstroke.remove_word_before_cursor(cursor);
                                    } else {
                                        textstroke.remove_grapheme_before_cursor(cursor);
                                    }
                                    update_stroke(engine_view.store, false);

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::HorizontalTab => {
                                    let insert_pos = cursor.cur_cursor();
                                    textstroke.insert_text_after_cursor("\t", cursor);
                                    textstroke.color_text_in_range(
                                        insert_pos..insert_pos + "\t".len(),
                                        text_style.color,
                                    );
                                    update_stroke(engine_view.store, false);

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::CarriageReturn | KeyboardKey::Linefeed => {
                                    let insert_pos = cursor.cur_cursor();
                                    textstroke.insert_text_after_cursor("\n", cursor);
                                    textstroke.color_text_in_range(
                                        insert_pos..insert_pos + "\n".len(),
                                        text_style.color,
                                    );
                                    update_stroke(engine_view.store, true);

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::Delete => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                        textstroke.remove_word_after_cursor(cursor);
                                    } else {
                                        textstroke.remove_grapheme_after_cursor(cursor);
                                    }
                                    update_stroke(engine_view.store, false);

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavLeft => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_back(cursor);
                                        } else {
                                            textstroke.move_cursor_back(cursor);
                                        }

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        }
                                    } else {
                                        #[allow(clippy::collapsible_else_if)]
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_back(cursor);
                                        } else {
                                            textstroke.move_cursor_back(cursor);
                                        }
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavRight => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_forward(cursor);
                                        } else {
                                            textstroke.move_cursor_forward(cursor);
                                        }

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        };
                                    } else {
                                        #[allow(clippy::collapsible_else_if)]
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_forward(cursor);
                                        } else {
                                            textstroke.move_cursor_forward(cursor);
                                        }
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavUp => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        textstroke.move_cursor_line_up(cursor);

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        };
                                    } else {
                                        textstroke.move_cursor_line_up(cursor);
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavDown => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        textstroke.move_cursor_line_down(cursor);

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        };
                                    } else {
                                        textstroke.move_cursor_line_down(cursor);
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::Home => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_text_start(cursor);
                                        } else {
                                            textstroke.move_cursor_line_start(cursor);
                                        }

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        };
                                    } else {
                                        #[allow(clippy::collapsible_else_if)]
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_text_start(cursor);
                                        } else {
                                            textstroke.move_cursor_line_start(cursor);
                                        }
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::End => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        let old_cursor = cursor.clone();
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_text_end(cursor);
                                        } else {
                                            textstroke.move_cursor_line_end(cursor);
                                        }

                                        *modify_state = ModifyState::Selecting {
                                            selection_cursor: old_cursor,
                                            mode: SelectionMode::Caret,
                                            finished: false,
                                        };
                                    } else {
                                        #[allow(clippy::collapsible_else_if)]
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_text_end(cursor);
                                        } else {
                                            textstroke.move_cursor_line_end(cursor);
                                        }
                                    }

                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                _ => EventResult {
                                    handled: false,
                                    propagate: EventPropagation::Proceed,
                                    progress: PenProgress::InProgress,
                                },
                            }
                        } else {
                            EventResult {
                                handled: false,
                                propagate: EventPropagation::Proceed,
                                progress: PenProgress::InProgress,
                            }
                        }
                    }
                    ModifyState::Selecting {
                        selection_cursor,
                        finished,
                        ..
                    } => {
                        #[cfg(feature = "ui")]
                        super::play_sound(Some(keyboard_key), engine_view.audioplayer);

                        if let Some(Stroke::TextStroke(textstroke)) =
                            engine_view.store.get_stroke_mut(*stroke_key)
                        {
                            let mut update_stroke = |store: &mut StrokeStore| {
                                store.update_geometry_for_stroke(*stroke_key);
                                store.regenerate_rendering_for_stroke(
                                    *stroke_key,
                                    engine_view.camera.viewport(),
                                    engine_view.camera.image_scale(),
                                );
                                widget_flags |= engine_view
                                    .document
                                    .resize_autoexpand(store, engine_view.camera)
                                    | store.record(Instant::now());
                                widget_flags.store_modified = true;
                            };
                            let mut quit_selecting = false;

                            // Handle keyboard keys
                            let event_result = match keyboard_key {
                                KeyboardKey::Unicode(keychar) => {
                                    if keychar == 'a'
                                        && modifier_keys.contains(&ModifierKey::KeyboardCtrl)
                                    {
                                        textstroke
                                            .update_selection_entire_text(cursor, selection_cursor);
                                        *finished = true;
                                    } else {
                                        textstroke.replace_text_between_selection_cursors(
                                            cursor,
                                            selection_cursor,
                                            String::from(keychar).as_str(),
                                        );
                                        update_stroke(engine_view.store);
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavLeft => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_back(cursor);
                                        } else {
                                            textstroke.move_cursor_back(cursor);
                                        }
                                    } else {
                                        cursor.set_cursor(
                                            cursor.cur_cursor().min(selection_cursor.cur_cursor()),
                                        );
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavRight => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                            textstroke.move_cursor_word_forward(cursor);
                                        } else {
                                            textstroke.move_cursor_forward(cursor);
                                        }
                                    } else {
                                        cursor.set_cursor(
                                            cursor.cur_cursor().max(selection_cursor.cur_cursor()),
                                        );
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavUp => {
                                    textstroke.move_cursor_line_up(cursor);
                                    if !modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::NavDown => {
                                    textstroke.move_cursor_line_down(cursor);
                                    if !modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::Home => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                        textstroke.move_cursor_text_start(cursor);
                                    } else {
                                        textstroke.move_cursor_line_start(cursor);
                                    }
                                    if !modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::End => {
                                    if modifier_keys.contains(&ModifierKey::KeyboardCtrl) {
                                        textstroke.move_cursor_text_end(cursor);
                                    } else {
                                        textstroke.move_cursor_line_end(cursor);
                                    }
                                    if !modifier_keys.contains(&ModifierKey::KeyboardShift) {
                                        quit_selecting = true;
                                    }
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::CarriageReturn | KeyboardKey::Linefeed => {
                                    textstroke.replace_text_between_selection_cursors(
                                        cursor,
                                        selection_cursor,
                                        "\n",
                                    );
                                    update_stroke(engine_view.store);
                                    quit_selecting = true;
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::BackSpace | KeyboardKey::Delete => {
                                    textstroke.replace_text_between_selection_cursors(
                                        cursor,
                                        selection_cursor,
                                        "",
                                    );
                                    update_stroke(engine_view.store);
                                    quit_selecting = true;
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::HorizontalTab => {
                                    textstroke.replace_text_between_selection_cursors(
                                        cursor,
                                        selection_cursor,
                                        "\t",
                                    );
                                    update_stroke(engine_view.store);
                                    quit_selecting = true;
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                                KeyboardKey::CtrlLeft
                                | KeyboardKey::CtrlRight
                                | KeyboardKey::ShiftLeft
                                | KeyboardKey::ShiftRight => EventResult {
                                    handled: false,
                                    propagate: EventPropagation::Proceed,
                                    progress: PenProgress::InProgress,
                                },
                                _ => {
                                    quit_selecting = true;
                                    EventResult {
                                        handled: true,
                                        propagate: EventPropagation::Stop,
                                        progress: PenProgress::InProgress,
                                    }
                                }
                            };

                            if quit_selecting {
                                self.state = TypewriterState::Modifying {
                                    modify_state: ModifyState::Idle,
                                    stroke_key: *stroke_key,
                                    cursor: cursor.clone(),
                                    pen_down: false,
                                };
                            }

                            event_result
                        } else {
                            EventResult {
                                handled: false,
                                propagate: EventPropagation::Proceed,
                                progress: PenProgress::InProgress,
                            }
                        }
                    }
                    _ => EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::InProgress,
                    },
                }
            }
        };

        self.reset_blink();

        (event_result, widget_flags)
    }

    pub(super) fn handle_pen_event_text(
        &mut self,
        text: String,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let text_width = engine_view
            .config
            .pens_config
            .typewriter_config
            .text_width();
        let mut text_style = engine_view
            .config
            .pens_config
            .typewriter_config
            .text_style
            .clone();

        self.pos = None;
        self.reset_blink();

        let event_result = match &mut self.state {
            TypewriterState::Idle => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            TypewriterState::Start(pos) => {
                #[cfg(feature = "ui")]
                super::play_sound(None, engine_view.audioplayer);

                text_style.ranged_text_attributes.clear();
                text_style.set_max_width(Some(text_width));
                let text_len = text.len();
                let textstroke = TextStroke::new(text, *pos, text_style);
                let cursor = GraphemeCursor::new(text_len, text_len, true);

                let stroke_key = engine_view
                    .store
                    .insert_stroke(Stroke::TextStroke(textstroke), None);
                engine_view.store.regenerate_rendering_for_stroke(
                    stroke_key,
                    engine_view.camera.viewport(),
                    engine_view.camera.image_scale(),
                );

                self.state = TypewriterState::Modifying {
                    modify_state: ModifyState::Idle,
                    stroke_key,
                    cursor,
                    pen_down: false,
                };

                widget_flags |= engine_view.store.record(Instant::now());
                widget_flags.resize = true;
                widget_flags.store_modified = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down,
            } => {
                match modify_state {
                    ModifyState::Idle => {
                        #[cfg(feature = "ui")]
                        super::play_sound(None, engine_view.audioplayer);

                        if let Some(Stroke::TextStroke(textstroke)) =
                            engine_view.store.get_stroke_mut(*stroke_key)
                        {
                            let insert_pos = cursor.cur_cursor();
                            textstroke.insert_text_after_cursor(&text, cursor);
                            textstroke.color_text_in_range(
                                insert_pos..insert_pos + text.len(),
                                text_style.color,
                            );
                            engine_view.store.update_geometry_for_stroke(*stroke_key);
                            engine_view.store.regenerate_rendering_for_stroke(
                                *stroke_key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                            widget_flags |= engine_view
                                .document
                                .resize_autoexpand(engine_view.store, engine_view.camera);

                            *pen_down = false;

                            // only record new history entry if the text contains ascii-whitespace,
                            // else only update history
                            if text.contains(char::is_whitespace) {
                                widget_flags |= engine_view.store.record(Instant::now());
                            } else {
                                widget_flags |= engine_view
                                    .store
                                    .update_latest_history_entry(Instant::now());
                            }

                            widget_flags.store_modified = true;
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        }
                    }
                    ModifyState::Selecting {
                        selection_cursor,
                        finished,
                        ..
                    } => {
                        #[cfg(feature = "ui")]
                        super::play_sound(None, engine_view.audioplayer);

                        if let Some(Stroke::TextStroke(textstroke)) =
                            engine_view.store.get_stroke_mut(*stroke_key)
                        {
                            let insert_pos = cursor.cur_cursor().min(selection_cursor.cur_cursor());
                            textstroke.replace_text_between_selection_cursors(
                                cursor,
                                selection_cursor,
                                text.as_str(),
                            );
                            textstroke.color_text_in_range(
                                insert_pos..insert_pos + text.len(),
                                text_style.color,
                            );
                            engine_view.store.update_geometry_for_stroke(*stroke_key);
                            engine_view.store.regenerate_rendering_for_stroke(
                                *stroke_key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                            widget_flags |= engine_view
                                .document
                                .resize_autoexpand(engine_view.store, engine_view.camera);

                            *finished = true;

                            // only record new history entry if the text contains ascii-whitespace,
                            // else only update history
                            if text.contains(char::is_whitespace) {
                                widget_flags |= engine_view.store.record(Instant::now());
                            } else {
                                widget_flags |= engine_view
                                    .store
                                    .update_latest_history_entry(Instant::now());
                            }
                            widget_flags.store_modified = true;
                        }

                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        }
                    }
                    _ => EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::InProgress,
                    },
                }
            }
        };

        (event_result, widget_flags)
    }

    pub(super) fn handle_pen_event_cancel(
        &mut self,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let widget_flags = WidgetFlags::default();
        self.pos = None;

        let event_result = match &mut self.state {
            TypewriterState::Idle => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            TypewriterState::Modifying { stroke_key, .. } => {
                // a translate drag left mid-way must not keep its text in vector preview
                engine_view.store.set_previewing(&[*stroke_key], false);
                self.state = TypewriterState::Idle;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            _ => {
                self.state = TypewriterState::Idle;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
        };

        (event_result, widget_flags)
    }

    pub fn select_closest_word(&mut self, engine_view: &mut EngineViewMut) {
        match &mut self.state {
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down: _,
            } => {
                if let Some(Stroke::TextStroke(textstroke)) =
                    engine_view.store.get_stroke_mut(*stroke_key)
                {
                    textstroke.move_cursor_word_boundary_forward(cursor);

                    let mut selection_cursor = cursor.clone();
                    textstroke.move_cursor_word_boundary_back(&mut selection_cursor);

                    *modify_state = ModifyState::Selecting {
                        mode: SelectionMode::Word(
                            selection_cursor.cur_cursor(),
                            cursor.cur_cursor(),
                        ),
                        selection_cursor,
                        finished: false,
                    };
                }
            }
            _ => {}
        }
    }

    pub fn select_closest_line(&mut self, engine_view: &mut EngineViewMut) {
        match &mut self.state {
            TypewriterState::Modifying {
                modify_state,
                stroke_key,
                cursor,
                pen_down: _,
            } => {
                if let Some(Stroke::TextStroke(textstroke)) =
                    engine_view.store.get_stroke_mut(*stroke_key)
                {
                    textstroke.move_cursor_line_end(cursor);

                    let mut selection_cursor = cursor.clone();
                    textstroke.move_cursor_line_start(&mut selection_cursor);

                    *modify_state = ModifyState::Selecting {
                        mode: SelectionMode::Line(
                            selection_cursor.cur_cursor(),
                            cursor.cur_cursor(),
                        ),
                        selection_cursor,
                        finished: false,
                    };
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use crate::pens::PenStyle;
    use p2d::math::Vector2;
    use rnote_compose::penevent::PenEvent;

    /// Places a new text box by clicking the canvas with the typewriter tool.
    fn start_textbox(engine: &mut Engine, now: Instant) {
        let _ = engine.change_pen_style(PenStyle::Typewriter);
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(100.0, 100.0), Element::PRESSURE_DEFAULT),
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );
    }

    fn press(
        engine: &mut Engine,
        keychar: char,
        modifier_keys: &[ModifierKey],
        now: Instant,
    ) -> EventPropagation {
        let (propagation, _) = engine.handle_pen_event(
            PenEvent::KeyPressed {
                keyboard_key: KeyboardKey::Unicode(keychar),
                modifier_keys: modifier_keys.iter().copied().collect(),
            },
            None,
            now,
        );
        propagation
    }

    fn texts(engine: &Engine) -> Vec<String> {
        engine
            .store
            .stroke_keys_unordered()
            .into_iter()
            .filter_map(|key| match engine.store.get_stroke_ref(key) {
                Some(Stroke::TextStroke(textstroke)) => Some(textstroke.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ctrl_v_leaves_a_new_textbox_to_the_paste_accelerator() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);

        assert_eq!(
            press(&mut engine, 'v', &[ModifierKey::KeyboardCtrl], now),
            EventPropagation::Proceed
        );
        assert!(
            texts(&engine).is_empty(),
            "Ctrl+V must not start a text stroke with 'v'"
        );

        // The same key without the modifier is text, so the box is not simply refusing input.
        assert_eq!(press(&mut engine, 'v', &[], now), EventPropagation::Stop);
        assert_eq!(texts(&engine), vec!["v".to_string()]);
    }

    #[test]
    fn ctrl_v_leaves_an_open_textbox_to_the_paste_accelerator() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'h', &[], now);
        press(&mut engine, 'i', &[], now);

        assert_eq!(
            press(&mut engine, 'v', &[ModifierKey::KeyboardCtrl], now),
            EventPropagation::Proceed
        );
        assert_eq!(texts(&engine), vec!["hi".to_string()]);
    }

    #[test]
    fn ctrl_alt_chords_are_not_text_either() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);

        assert_eq!(
            press(
                &mut engine,
                '1',
                &[ModifierKey::KeyboardCtrl, ModifierKey::KeyboardAlt],
                now,
            ),
            EventPropagation::Proceed
        );
        assert!(texts(&engine).is_empty());
    }

    #[test]
    fn alt_digit_leaves_the_textbox_to_the_pen_accelerator() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'h', &[], now);

        // `Alt`+digit is the pen-style accelerator, and it must reach the window from inside the
        // box instead of being typed as text.
        assert_eq!(
            press(&mut engine, '4', &[ModifierKey::KeyboardAlt], now),
            EventPropagation::Proceed
        );
        assert_eq!(texts(&engine), vec!["h".to_string()]);
    }

    #[test]
    fn cancel_ends_the_open_textbox() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'h', &[], now);

        // The pen-style action ends the session with `PenEvent::Cancel` before it switches.
        let (_, widget_flags) = engine.handle_pen_event(PenEvent::Cancel, None, now);

        assert!(!engine.penholder.is_handling_text_input());
        assert_eq!(widget_flags.enable_text_input, Some(false));
        assert_eq!(texts(&engine), vec!["h".to_string()]);
    }

    #[test]
    fn escape_leaves_an_open_textbox_to_the_tool_keys() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'h', &[], now);
        press(&mut engine, 'i', &[], now);
        assert!(engine.penholder.is_handling_text_input());

        let (propagation, _) = engine.handle_pen_event(
            PenEvent::KeyPressed {
                keyboard_key: KeyboardKey::Escape,
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );

        assert_eq!(propagation, EventPropagation::Stop);
        assert!(
            !engine.penholder.is_handling_text_input(),
            "Escape must drop the text session to idle"
        );
        assert_eq!(texts(&engine), vec!["hi".to_string()]);
    }

    #[test]
    fn escape_leaves_an_empty_textbox_without_a_stroke() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        assert!(engine.penholder.is_handling_text_input());

        let (propagation, _) = engine.handle_pen_event(
            PenEvent::KeyPressed {
                keyboard_key: KeyboardKey::Escape,
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );

        assert_eq!(propagation, EventPropagation::Stop);
        assert!(!engine.penholder.is_handling_text_input());
        assert!(texts(&engine).is_empty());
    }

    /// The color chosen in the toolbar is the ink typed into a text box: opening a box must not
    /// replace it with the boxes color, and the typed characters take the chosen color instead of
    /// recoloring the existing text.
    #[test]
    fn typed_text_takes_the_chosen_color() {
        use rnote_compose::Color;

        let white = Color::new(1.0, 1.0, 1.0, 1.0);
        let magenta = Color::new(1.0, 0.0, 0.72, 1.0);

        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'h', &[], now);

        // The box was written in white, then the session was left.
        engine.with_view_mut(|view| {
            let keys = view.store.stroke_keys_unordered();
            for key in keys {
                if let Some(Stroke::TextStroke(textstroke)) = view.store.get_stroke_mut(key) {
                    textstroke.text_style.color = white;
                }
            }
        });
        let _ = engine.handle_pen_event(PenEvent::Cancel, None, now);

        // The color is picked in the toolbar with nothing being edited.
        engine.with_view_mut(|view| view.config.pens_config.set_all_stroke_colors(magenta));

        // Clicking the existing box opens it for editing.
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(100.0, 100.0), Element::PRESSURE_DEFAULT),
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );
        assert!(
            engine.penholder.is_handling_text_input(),
            "the click must open the text box"
        );
        let ink =
            engine.with_view_mut(|view| view.config.pens_config.typewriter_config.text_style.color);
        assert_eq!(ink, magenta, "opening a box must keep the chosen color");

        press(&mut engine, 'i', &[], now);
        press(&mut engine, 'j', &[], now);

        let (text, box_color, colors, attrs) = engine.with_view_mut(|view| {
            let textstroke = view
                .store
                .stroke_keys_unordered()
                .into_iter()
                .find_map(|key| match view.store.get_stroke_ref(key) {
                    Some(Stroke::TextStroke(textstroke)) => Some(textstroke),
                    _ => None,
                })
                .unwrap();
            let colors = textstroke
                .text
                .char_indices()
                .map(|(index, _)| textstroke.text_color_at(index))
                .collect::<Vec<_>>();
            (
                textstroke.text.clone(),
                textstroke.text_style.color,
                colors,
                textstroke.text_style.ranged_text_attributes.len(),
            )
        });
        assert_eq!(
            text.chars().count(),
            3,
            "both keys are typed into the box, got {text:?}"
        );
        assert_eq!(box_color, white, "existing text keeps the boxes color");
        for (index, ch) in text.char_indices() {
            if ch == 'h' {
                assert_eq!(
                    colors[index], white,
                    "the 'h' written before keeps its color"
                );
            } else {
                assert_eq!(
                    colors[index], magenta,
                    "typed {ch:?} takes the chosen color, text {text:?}"
                );
            }
        }
        assert_eq!(
            attrs, 1,
            "typing keeps a single color range instead of one per character"
        );
    }

    #[test]
    fn ctrl_a_still_selects_the_whole_text() {
        let mut engine = Engine::default();
        let now = Instant::now();
        start_textbox(&mut engine, now);
        press(&mut engine, 'a', &[], now);
        press(&mut engine, 'b', &[], now);

        assert_eq!(
            press(&mut engine, 'a', &[ModifierKey::KeyboardCtrl], now),
            EventPropagation::Stop
        );
        assert_eq!(texts(&engine), vec!["ab".to_string()]);
    }
}
