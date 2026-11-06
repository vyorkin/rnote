//! Keeps popovers inside the application window.
//!
//! GTK4 draws a `GtkPopover` in its own popup surface and leaves the placement
//! to the compositor, which constrains it to the monitor instead of the window
//! (`gtk_popover_present()` builds a `GdkPopupLayout` with `FLIP_X` / `SLIDE_Y`
//! hints and no window bounds). A popover therefore hangs over the window edge
//! whenever its anchor sits near one, and a pen tablet mapped onto the window
//! cannot reach that part of it.
//!
//! We place such a popover ourselves: pick the side of the anchor that has room
//! for it and shift the leftover overflow back with `GtkPopover::set_offset()`,
//! which offsets the anchor rect the compositor positions against. The same
//! pass caps a scrolled panel to the window height, so a tall configuration
//! popover scrolls instead of leaving the window.

use gtk4::prelude::*;
use gtk4::{Align, Orientation, Popover, PositionType, ScrolledWindow, Widget, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// The free space kept between a confined popover and the window edge.
const WINDOW_MARGIN: i32 = 8;

/// The smallest height a scrolled popover may shrink to.
const MIN_SCROLLED_HEIGHT: i32 = 160;

/// How often an open popover checks whether its anchor or the window moved.
const FOLLOW_INTERVAL: Duration = Duration::from_millis(50);

const CONFINE_KEY: &str = "rnote-popover-confine";

/// The popover's anchor rectangle in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Anchor {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Where a popover goes: the side of the anchor it opens on and the shift that
/// brings the rest of it inside the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    position: PositionType,
    offset: (i32, i32),
}

/// What the last placement was computed from, so an unchanged anchor and
/// window do not measure the popover again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Measured {
    anchor: Anchor,
    window: (i32, i32),
    size: (i32, i32),
}

#[derive(Debug)]
struct State {
    /// The side the popover opens on when it is not confined, i.e. the side it
    /// was configured with before we started moving it.
    preferred: Cell<PositionType>,
    /// The side we set last, to tell our own change from one the app made.
    applied: Cell<Option<PositionType>>,
    /// The arrow is only meaningful while the popover is next to its anchor.
    has_arrow: Cell<bool>,
    /// The scrolled child's `max-content-height` before we capped it.
    original_max_height: Cell<i32>,
    last: RefCell<Option<(Measured, Placement)>>,
    follow: RefCell<Option<glib::SourceId>>,
}

/// Confines `popover` to the window it is attached to.
///
/// Repeated calls on the same popover do nothing.
pub(crate) fn confine(popover: &Popover) {
    if unsafe { popover.data::<Rc<State>>(CONFINE_KEY) }.is_some() {
        return;
    }

    let state = Rc::new(State {
        preferred: Cell::new(popover.position()),
        applied: Cell::new(None),
        has_arrow: Cell::new(popover.has_arrow()),
        original_max_height: Cell::new(i32::MIN),
        last: RefCell::new(None),
        follow: RefCell::new(None),
    });
    unsafe { popover.set_data(CONFINE_KEY, state.clone()) };

    popover.connect_notify_local(
        Some("visible"),
        glib::clone!(move |popover, _| {
            if !popover.is_visible() {
                stop_following(&state);
                state.last.replace(None);
                return;
            }
            update(popover, &state);
            start_following(popover, &state);
        }),
    );
}

/// Confines every popover in the widget tree below `widget`.
pub(crate) fn confine_tree(widget: &impl IsA<Widget>) {
    let widget = widget.as_ref();
    if let Some(popover) = widget.downcast_ref::<Popover>() {
        confine(popover);
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        confine_tree(&next);
        child = next.next_sibling();
    }
}

fn start_following(popover: &Popover, state: &Rc<State>) {
    if state.follow.borrow().is_some() {
        return;
    }
    let source = glib::timeout_add_local(FOLLOW_INTERVAL, {
        let state = state.clone();
        let popover = popover.downgrade();
        move || {
            let open = popover.upgrade().filter(|popover| popover.is_visible());
            let Some(popover) = open else {
                // The source ends here, so drop the id a later reopen would
                // otherwise mistake for a running follow.
                state.follow.take();
                return glib::ControlFlow::Break;
            };
            update(&popover, &state);
            glib::ControlFlow::Continue
        }
    });
    state.follow.replace(Some(source));
}

fn stop_following(state: &State) {
    if let Some(source) = state.follow.take() {
        source.remove();
    }
}

/// Places an open popover again if its anchor or the window moved, and caps a
/// scrolled panel to the window.
fn update(popover: &Popover, state: &State) {
    let Some(anchor_widget) = popover.parent() else {
        return;
    };
    let Some(native) = anchor_widget.native() else {
        return;
    };
    // GTK positions a popover inside the native it belongs to: the window, or
    // a dialog presented in one.
    let Some(frame) = native.dynamic_cast_ref::<Widget>() else {
        return;
    };
    let window_size = (frame.width(), frame.height());
    if window_size.0 <= 0 || window_size.1 <= 0 {
        return;
    }
    let Some(bounds) = anchor_widget.compute_bounds(frame) else {
        return;
    };
    // A popover with a pointing rect (the coordinate-system card) positions
    // against that rect, not against its anchor's whole allocation.
    let anchor = match popover.pointing_to() {
        (true, rect) => Anchor {
            x: bounds.x() + rect.x() as f32,
            y: bounds.y() + rect.y() as f32,
            width: rect.width() as f32,
            height: rect.height() as f32,
        },
        _ => Anchor {
            x: bounds.x(),
            y: bounds.y(),
            width: bounds.width(),
            height: bounds.height(),
        },
    };

    // The app may have set a new side since the last placement (the
    // coordinate-system card reuses one popover for every axis).
    let configured = popover.position();
    if state.applied.get() != Some(configured) {
        state.preferred.set(configured);
    }

    let available_height = (window_size.1 - 2 * WINDOW_MARGIN).max(0);
    let mut size = popover_size(popover);
    if cap_scrolled(popover, state, available_height) {
        size = popover_size(popover);
    }

    let measured = Measured {
        anchor,
        window: window_size,
        size,
    };
    if let Some((last_measured, last_placement)) = *state.last.borrow()
        && last_measured == measured
    {
        apply(popover, state, last_placement);
        return;
    }

    let placement = place_in_window(
        anchor,
        size,
        window_size,
        state.preferred.get(),
        popover.valign(),
        popover.halign(),
    );
    state.last.replace(Some((measured, placement)));
    apply(popover, state, placement);
}

fn apply(popover: &Popover, state: &State, placement: Placement) {
    if popover.position() != placement.position {
        // Re-presents the popover when it is visible.
        popover.set_position(placement.position);
    }
    state.applied.set(Some(placement.position));

    if popover.offset() != placement.offset {
        popover.set_offset(placement.offset.0, placement.offset.1);
        popover.present();
    }

    // A shifted popover no longer sits next to its anchor, so the arrow would
    // point at empty canvas.
    let want_arrow = state.has_arrow.get() && placement.offset == (0, 0);
    if popover.has_arrow() != want_arrow {
        popover.set_has_arrow(want_arrow);
    }
}

/// Caps a scrolled popover's content to the window height, so the panel
/// scrolls instead of sticking out of the window.
fn cap_scrolled(popover: &Popover, state: &State, available_height: i32) -> bool {
    let Some(child) = popover.child() else {
        return false;
    };
    let Ok(scroller) = child.downcast::<ScrolledWindow>() else {
        return false;
    };
    if !scroller.propagates_natural_height() {
        return false;
    }
    // The popover adds its own padding and arrow around the panel, so the
    // panel may only be as tall as the window minus that chrome.
    let (_, panel_natural, _, _) = scroller.measure(Orientation::Vertical, -1);
    let (_, popover_natural, _, _) = popover.measure(Orientation::Vertical, -1);
    let chrome = (popover_natural - panel_natural).max(0);
    let available_height = available_height - chrome;
    if state.original_max_height.get() == i32::MIN {
        state.original_max_height.set(scroller.max_content_height());
    }
    let original = state.original_max_height.get();
    let capped = if original >= 0 {
        original.min(available_height)
    } else {
        available_height
    }
    .max(MIN_SCROLLED_HEIGHT);
    if scroller.max_content_height() == capped {
        return false;
    }
    scroller.set_max_content_height(capped);
    true
}

/// The size the popover surface will get, margins included.
fn popover_size(popover: &Popover) -> (i32, i32) {
    let (_, natural_width, _, _) = popover.measure(Orientation::Horizontal, -1);
    let width = natural_width + popover.margin_start() + popover.margin_end();
    let (_, natural_height, _, _) = popover.measure(Orientation::Vertical, width.max(-1));
    let height = natural_height + popover.margin_top() + popover.margin_bottom();
    (width.max(0), height.max(0))
}

/// Picks the side of the anchor the popover fits on, keeping `preferred` when
/// it fits and falling back to it when neither side does, then clamps whatever
/// still overflows into the window.
fn place_in_window(
    anchor: Anchor,
    size: (i32, i32),
    window: (i32, i32),
    preferred: PositionType,
    valign: Align,
    halign: Align,
) -> Placement {
    let (width, height) = size;
    let (window_width, window_height) = window;
    let margin = WINDOW_MARGIN;

    let anchor_left = anchor.x.round() as i32;
    let anchor_top = anchor.y.round() as i32;
    let anchor_right = (anchor.x + anchor.width).round() as i32;
    let anchor_bottom = (anchor.y + anchor.height).round() as i32;

    let (position, left, top) = if matches!(preferred, PositionType::Left | PositionType::Right) {
        let room_left = anchor_left - margin;
        let room_right = window_width - anchor_right - margin;
        let fits_left = width <= room_left;
        let fits_right = width <= room_right;
        let position = if preferred == PositionType::Left {
            if fits_left || !fits_right {
                PositionType::Left
            } else {
                PositionType::Right
            }
        } else if fits_right || !fits_left {
            PositionType::Right
        } else {
            PositionType::Left
        };
        let left = if position == PositionType::Left {
            anchor_left - width
        } else {
            anchor_right
        };
        let top = match valign {
            Align::Start => anchor_top,
            Align::End => anchor_bottom - height,
            _ => anchor_top + (anchor.height as i32 - height) / 2,
        };
        (position, left, top)
    } else {
        let room_top = anchor_top - margin;
        let room_bottom = window_height - anchor_bottom - margin;
        let fits_top = height <= room_top;
        let fits_bottom = height <= room_bottom;
        let position = if preferred == PositionType::Top {
            if fits_top || !fits_bottom {
                PositionType::Top
            } else {
                PositionType::Bottom
            }
        } else if fits_bottom || !fits_top {
            PositionType::Bottom
        } else {
            PositionType::Top
        };
        let top = if position == PositionType::Top {
            anchor_top - height
        } else {
            anchor_bottom
        };
        let left = match halign {
            Align::Start => anchor_left,
            Align::End => anchor_right - width,
            _ => anchor_left + (anchor.width as i32 - width) / 2,
        };
        (position, left, top)
    };

    let clamped_left = clamp(left, width, window_width, margin);
    let clamped_top = clamp(top, height, window_height, margin);

    Placement {
        position,
        offset: (clamped_left - left, clamped_top - top),
    }
}

/// Clamps `value` into `[margin, window - size - margin]`, keeping a popover
/// larger than the window flush against the start edge.
fn clamp(value: i32, size: i32, window_extent: i32, margin: i32) -> i32 {
    let max = (window_extent - size - margin).max(0);
    value.clamp(margin.min(max), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(x: f32, y: f32, width: f32, height: f32) -> Anchor {
        Anchor {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn keeps_the_configured_side_when_it_fits() {
        // A 200 wide popover to the left of an anchor at x = 400.
        let placement = place_in_window(
            anchor(400.0, 300.0, 40.0, 40.0),
            (200, 300),
            (800, 600),
            PositionType::Left,
            Align::Center,
            Align::Center,
        );
        assert_eq!(placement.position, PositionType::Left);
        assert_eq!(placement.offset, (0, 0));
    }

    #[test]
    fn flips_to_the_side_with_room() {
        // The same popover next to the left window edge: to the left it would
        // hang outside, to the right it fits.
        let placement = place_in_window(
            anchor(40.0, 300.0, 40.0, 40.0),
            (200, 300),
            (800, 600),
            PositionType::Left,
            Align::Center,
            Align::Center,
        );
        assert_eq!(placement.position, PositionType::Right);
        assert_eq!(placement.offset, (0, 0));
    }

    #[test]
    fn shifts_a_popover_that_overflows_the_bottom() {
        // A 400 high popover below an anchor in the middle of the window fits
        // on neither side, so it stays below and is shifted up to stay inside.
        let placement = place_in_window(
            anchor(400.0, 280.0, 40.0, 40.0),
            (200, 400),
            (800, 600),
            PositionType::Bottom,
            Align::Center,
            Align::Center,
        );
        assert_eq!(placement.position, PositionType::Bottom);
        // Natural top is the anchor's bottom edge (320), the window offers
        // 600 - 400 - 8.
        assert_eq!(placement.offset, (0, 192 - 320));
    }

    #[test]
    fn keeps_a_popover_wider_than_the_window_flush() {
        let placement = place_in_window(
            anchor(400.0, 100.0, 40.0, 40.0),
            (900, 100),
            (800, 600),
            PositionType::Right,
            Align::Center,
            Align::Center,
        );
        assert_eq!(placement.position, PositionType::Right);
        // Natural left is the anchor's right edge, the clamp puts it at 0.
        assert_eq!(placement.offset, (0 - 440, 0));
    }
}
