// Imports
use crate::RnPensSideBar;
use crate::canvaswrapper::RnCanvasWrapper;
use crate::{RnAppWindow, RnColorPicker, RnPenPicker, dialogs};
use core::time::Duration;
use gettextrs::gettext;
use gtk4::{
    Align, Box, CompositeTemplate, Image, Overlay, ProgressBar, ScrolledWindow, Widget, gio, glib,
    glib::clone, prelude::*, subclass::prelude::*,
};
use rnote_engine::ext::GdkRGBAExt;
use rnote_engine::pens::PenStyle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use tracing::error;

/// The toolbar panels that can be floating or docked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ToolbarPanel {
    PenPicker,
    ColorPicker,
    PensSideBar,
}

impl ToolbarPanel {
    /// Every panel that can be floating or docked.
    pub(crate) const ALL: [Self; 3] = [Self::PenPicker, Self::ColorPicker, Self::PensSideBar];

    /// The name the panel is stored under in the toolbar layout.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::PenPicker => "penpicker",
            Self::ColorPicker => "colorpicker",
            Self::PensSideBar => "penssidebar",
        }
    }
}

/// The window edge a toolbar panel is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ToolbarDock {
    /// The panel floats over the canvas and takes no layout space.
    #[default]
    Floating,
    Top,
    Bottom,
    Left,
    Right,
}

impl ToolbarDock {
    /// The dock named by a stored settings string.
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Floating,
        }
    }

    /// The name the dock is stored under in the app settings.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Floating => "floating",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// How close to an edge a dragged panel must be let go to dock there, in pixels.
const DOCK_SNAP_DISTANCE: f64 = 24.0;

/// Thickness of the strip that marks the edge a release would dock the dragged panel to.
const DOCK_INDICATOR_THICKNESS: i32 = DOCK_SNAP_DISTANCE as i32;

/// How often a deferred placement is retried while a panel still has no allocation.
///
/// The panels are allocated within a frame or two of the window being mapped, so the retries
/// are a guard against a panel that never is, not a timer.
const FLOAT_APPLY_TRIES: u32 = 50;

/// Whether a release docks the dragged panel, given the preference and the Shift key.
///
/// Shift inverts the preference: with docking-on-drop off, holding Shift is the only way to dock;
/// with it on, holding Shift is the way to keep the panel floating.
fn dock_release_docks(dock_on_drop: bool, shift_held: bool) -> bool {
    dock_on_drop != shift_held
}

/// The dock a release uses, or `None` when the release keeps the panel floating.
fn dock_at_release(target: (f64, ToolbarDock), docks: bool) -> Option<ToolbarDock> {
    (docks && target.0 <= DOCK_SNAP_DISTANCE).then_some(target.1)
}

/// A pixel position as a fraction of the distance the panel can travel inside the overlay.
fn position_fraction(position: f64, range: f64) -> f64 {
    if range <= 0.0 {
        0.0
    } else {
        (position / range).clamp(0.0, 1.0)
    }
}

/// The pixel position a stored fraction stands for.
fn fraction_position(fraction: f64, range: f64) -> f64 {
    fraction.clamp(0.0, 1.0) * range.max(0.0)
}

/// Show the strip of `dock` and hide the others.
fn show_dock_indicator(indicators: &[(ToolbarDock, Box)], dock: ToolbarDock) {
    for (edge, strip) in indicators {
        strip.set_visible(*edge == dock);
    }
}

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/overlays.ui")]
    pub(crate) struct RnOverlays {
        pub(crate) progresspulses_active: Cell<usize>,
        pub(crate) progresspulse_id: RefCell<Option<glib::SourceId>>,
        /// Where each toolbar panel is docked.
        pub(crate) docks: RefCell<HashMap<ToolbarPanel, ToolbarDock>>,
        /// Whether the toolbars are locked to the top edge. Locked toolbars can't be dragged.
        pub(crate) toolbars_locked: Cell<bool>,
        /// Whether a plain drop near an edge docks a toolbar. Shift inverts it.
        pub(crate) dock_on_drop: Cell<bool>,
        /// The strips that mark the edge a release would dock the dragged panel to.
        pub(crate) dock_indicators: RefCell<Vec<(ToolbarDock, Box)>>,
        /// Where each panel was left, as fractions of its travel range inside the overlay.
        pub(crate) float_positions: RefCell<HashMap<ToolbarPanel, (f64, f64)>>,
        /// The toolbar overlay size the remembered positions were last applied at.
        pub(crate) float_apply_size: Cell<(i32, i32)>,
        /// The pending application of the remembered positions, if one is queued.
        pub(crate) float_apply_source: RefCell<Option<glib::SourceId>>,
        /// How many times the pending application was deferred for an unallocated panel.
        pub(crate) float_apply_tries: Cell<u32>,
        /// Whether the window surface's "layout" signal is connected.
        pub(crate) float_layout_connected: Cell<bool>,

        #[template_child]
        pub(crate) toolbar_overlay: TemplateChild<Overlay>,
        #[template_child]
        pub(crate) dock_top_scroller: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub(crate) dock_top: TemplateChild<Box>,
        #[template_child]
        pub(crate) dock_bottom_scroller: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub(crate) dock_bottom: TemplateChild<Box>,
        #[template_child]
        pub(crate) dock_left: TemplateChild<Box>,
        #[template_child]
        pub(crate) dock_right: TemplateChild<Box>,
        #[template_child]
        pub(crate) penpicker_box: TemplateChild<Box>,
        #[template_child]
        pub(crate) colorpicker_box: TemplateChild<Box>,
        #[template_child]
        pub(crate) penpicker_handle: TemplateChild<Image>,
        #[template_child]
        pub(crate) colorpicker_handle: TemplateChild<Image>,
        #[template_child]
        pub(crate) sidebar_handle: TemplateChild<Image>,
        #[template_child]
        pub(crate) toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub(crate) progressbar: TemplateChild<ProgressBar>,
        #[template_child]
        pub(crate) penpicker: TemplateChild<RnPenPicker>,
        #[template_child]
        pub(crate) colorpicker: TemplateChild<RnColorPicker>,
        #[template_child]
        pub(crate) tabview: TemplateChild<adw::TabView>,
        #[template_child]
        pub(crate) sidebar_box: TemplateChild<gtk4::Box>,
        #[template_child]
        pub(crate) sidebar_scroller: TemplateChild<ScrolledWindow>,
        #[template_child]
        pub(crate) penssidebar: TemplateChild<RnPensSideBar>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnOverlays {
        const NAME: &'static str = "RnOverlays";
        type Type = super::RnOverlays;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnOverlays {
        fn constructed(&self) {
            self.parent_constructed();
        }

        fn dispose(&self) {
            self.dispose_template();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RnOverlays {
        fn map(&self) {
            self.parent_map();
            self.obj().connect_float_layout();
            self.obj().queue_float_positions();
        }

        fn realize(&self) {
            self.parent_realize();
            self.obj().connect_float_layout();
        }
    }
    impl RnOverlays {
        pub(crate) fn setup_toolbar_overlay(&self) {
            // The panels are placed inside the overlay by their margins. Measuring them into the
            // overlay would make a panel's position part of the overlay's size, and the panel is
            // placed inside the size it changes.
            self.toolbar_overlay
                .set_measure_overlay(&*self.colorpicker_box, false);
            self.toolbar_overlay
                .set_measure_overlay(&*self.penpicker_box, false);
            self.toolbar_overlay
                .set_measure_overlay(&*self.sidebar_box, false);
        }
    }
}

/// Stops the buttons inside `widget` from taking the keyboard focus when they are clicked.
/// Keyboard focus can still reach them with Tab.
fn disable_focus_on_click(widget: &impl IsA<Widget>) {
    let mut child = widget.first_child();
    while let Some(child_widget) = child {
        if child_widget.is::<gtk4::Button>() {
            child_widget.set_focus_on_click(false);
        }
        disable_focus_on_click(&child_widget);
        child = child_widget.next_sibling();
    }
}

/// The default timeout for regular text toasts.
pub(crate) const TEXT_TOAST_TIMEOUT_DEFAULT: Option<Duration> = Some(Duration::from_secs(5));

glib::wrapper! {
    pub(crate) struct RnOverlays(ObjectSubclass<imp::RnOverlays>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnOverlays {
    fn default() -> Self {
        Self::new()
    }
}

impl RnOverlays {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn penpicker(&self) -> RnPenPicker {
        self.imp().penpicker.get()
    }

    pub(crate) fn colorpicker(&self) -> RnColorPicker {
        self.imp().colorpicker.get()
    }

    pub(crate) fn toast_overlay(&self) -> adw::ToastOverlay {
        self.imp().toast_overlay.get()
    }

    pub(crate) fn progressbar(&self) -> ProgressBar {
        self.imp().progressbar.get()
    }

    pub(crate) fn tabview(&self) -> adw::TabView {
        self.imp().tabview.get()
    }

    pub(crate) fn sidebar_box(&self) -> gtk4::Box {
        self.imp().sidebar_box.get()
    }

    pub(crate) fn penpicker_box(&self) -> gtk4::Box {
        self.imp().penpicker_box.get()
    }

    pub(crate) fn colorpicker_box(&self) -> gtk4::Box {
        self.imp().colorpicker_box.get()
    }

    /// Show or hide the toolbar panels.
    pub(crate) fn set_toolbars_visible(&self, visible: bool) {
        let imp = self.imp();
        imp.penpicker_box.set_visible(visible);
        imp.colorpicker_box.set_visible(visible);
        imp.sidebar_box.set_visible(visible);
    }

    pub(crate) fn sidebar_scroller(&self) -> ScrolledWindow {
        self.imp().sidebar_scroller.get()
    }

    pub(crate) fn penssidebar(&self) -> RnPensSideBar {
        self.imp().penssidebar.get()
    }

    /// Where the given toolbar panel is currently docked.
    pub(crate) fn panel_dock(&self, panel: ToolbarPanel) -> ToolbarDock {
        self.imp()
            .docks
            .borrow()
            .get(&panel)
            .copied()
            .unwrap_or_default()
    }

    /// Dock the given toolbar panel to a window edge, or float it over the canvas.
    pub(crate) fn set_panel_dock(&self, panel: ToolbarPanel, dock: ToolbarDock) {
        self.imp().docks.borrow_mut().insert(panel, dock);
        self.reparent_panel(panel, dock, None);
    }

    /// Reparent a panel now, or after the current event when it is already on screen.
    ///
    /// The drag that floats a docked panel starts inside the `drag-begin` emission of a gesture
    /// GTK is dispatching, and GTK's own state for a widget does not survive being unparented
    /// there: it aborts on the `!priv->mapped` assertion in `gtk_widget_real_unrealize`.
    fn reparent_panel(&self, panel: ToolbarPanel, dock: ToolbarDock, at: Option<(f64, f64)>) {
        if self.panel_widget(panel).is_mapped() {
            glib::source::timeout_add_local_once(
                Duration::ZERO,
                clone!(
                    #[weak(rename_to = overlays)]
                    self,
                    move || overlays.apply_panel_dock(panel, dock, at),
                ),
            );
            return;
        }
        self.apply_panel_dock(panel, dock, at);
    }

    /// Whether a plain drop near an edge docks a toolbar.
    pub(crate) fn dock_on_drop(&self) -> bool {
        self.imp().dock_on_drop.get()
    }

    /// Set whether a plain drop near an edge docks a toolbar, and update the handle tooltips.
    ///
    /// Shift inverts the preference, so the tooltip names whichever gesture is not the plain one.
    pub(crate) fn set_dock_on_drop(&self, dock_on_drop: bool) {
        self.imp().dock_on_drop.set(dock_on_drop);
        let tooltip = if dock_on_drop {
            gettext(
                "Drag to move this toolbar. Release near an edge to dock it; hold Shift to keep it floating.",
            )
        } else {
            gettext("Drag to move this toolbar. Hold Shift and release near an edge to dock it.")
        };
        for panel in [
            ToolbarPanel::PenPicker,
            ToolbarPanel::ColorPicker,
            ToolbarPanel::PensSideBar,
        ] {
            self.panel_handle(panel).set_tooltip_text(Some(&tooltip));
        }
    }

    /// Show the strip of the edge a release would dock the dragged panel to.
    fn set_dock_indicator(&self, dock: ToolbarDock) {
        show_dock_indicator(&self.imp().dock_indicators.borrow(), dock);
    }

    /// Whether the toolbars are locked to the top edge.
    pub(crate) fn toolbars_locked(&self) -> bool {
        self.imp().toolbars_locked.get()
    }

    /// Lock the toolbars to the top edge, or free them to be dragged to any edge.
    ///
    /// Locking moves every panel to the top dock and hides the drag handles. Unlocking leaves the
    /// panels where they are, so they can be dragged out again.
    pub(crate) fn set_toolbars_locked(&self, locked: bool) {
        if self.imp().toolbars_locked.replace(locked) == locked {
            return;
        }

        for panel in [
            ToolbarPanel::PenPicker,
            ToolbarPanel::ColorPicker,
            ToolbarPanel::PensSideBar,
        ] {
            self.panel_handle(panel).set_visible(!locked);
        }

        if locked {
            for panel in [
                ToolbarPanel::PenPicker,
                ToolbarPanel::ColorPicker,
                ToolbarPanel::PensSideBar,
            ] {
                self.set_panel_dock(panel, ToolbarDock::Top);
            }
        }
    }

    fn panel_widget(&self, panel: ToolbarPanel) -> Widget {
        let imp = self.imp();
        match panel {
            ToolbarPanel::PenPicker => imp.penpicker_box.clone().upcast(),
            ToolbarPanel::ColorPicker => imp.colorpicker_box.clone().upcast(),
            ToolbarPanel::PensSideBar => imp.sidebar_box.clone().upcast(),
        }
    }

    fn panel_handle(&self, panel: ToolbarPanel) -> Image {
        let imp = self.imp();
        match panel {
            ToolbarPanel::PenPicker => imp.penpicker_handle.get(),
            ToolbarPanel::ColorPicker => imp.colorpicker_handle.get(),
            ToolbarPanel::PensSideBar => imp.sidebar_handle.get(),
        }
    }

    fn dock_slot(&self, dock: ToolbarDock) -> Option<Box> {
        let imp = self.imp();
        match dock {
            ToolbarDock::Top => Some(imp.dock_top.get()),
            ToolbarDock::Bottom => Some(imp.dock_bottom.get()),
            ToolbarDock::Left => Some(imp.dock_left.get()),
            ToolbarDock::Right => Some(imp.dock_right.get()),
            ToolbarDock::Floating => None,
        }
    }

    fn update_dock_slots_visibility(&self) {
        let imp = self.imp();
        for dock in [
            ToolbarDock::Top,
            ToolbarDock::Bottom,
            ToolbarDock::Left,
            ToolbarDock::Right,
        ] {
            if let Some(slot) = self.dock_slot(dock) {
                let used = slot.first_child().is_some();
                // The horizontal slots live in a scroller, which is the widget that hides.
                match dock {
                    ToolbarDock::Top => imp.dock_top_scroller.set_visible(used),
                    ToolbarDock::Bottom => imp.dock_bottom_scroller.set_visible(used),
                    _ => slot.set_visible(used),
                }
            }
        }
    }

    /// Reparent a panel into its dock slot or the overlay and style it for that dock.
    ///
    /// `at` places a floating panel in overlay coordinates; without it the panel falls back to
    /// its default corner.
    fn apply_panel_dock(&self, panel: ToolbarPanel, dock: ToolbarDock, at: Option<(f64, f64)>) {
        let widget = self.panel_widget(panel);
        let overlay = self.imp().toolbar_overlay.get();

        // `GtkBox::append` refuses a widget that still has a parent, and the overlay's measure
        // flag can only be set while the widget is an overlay child, so detach first. Detaching a
        // mapped widget from a scrolled dock slot trips GTK's map accounting, so hide it first:
        // an unmapped widget is detached and unrealized in a clean order.
        let was_mapped = widget.is_mapped();
        if widget.parent().is_some() {
            if was_mapped {
                widget.set_visible(false);
            }
            widget.unparent();
        }

        match dock {
            ToolbarDock::Floating => {
                // A dragged panel is placed at the position the drag started from; one that is
                // only floating goes back to its default corner.
                widget.set_hexpand(false);
                widget.set_vexpand(false);

                // `add_overlay` reparents the panel out of its dock slot, so measure has to be
                // set after it is an overlay child.
                overlay.add_overlay(&widget);
                // A floating panel is never measured into the overlay: its margin would become
                // part of the overlay's own size, so moving the panel would resize the overlay
                // that the panel is placed inside of. A panel is placed by its margins and keeps
                // its natural size; letting it contribute to the measure also makes every pointer
                // motion re-measure the whole overlay, canvas included, and the panel drops out of
                // the frame.
                overlay.set_measure_overlay(&widget, false);

                // A dragged panel is placed at the position the drag started from; one that is
                // only floating goes back to its default corner.
                let (halign, valign, margin_start, margin_top, margin_end, margin_bottom) = match at
                {
                    Some((x, y)) => (
                        Align::Start,
                        Align::Start,
                        x.round() as i32,
                        y.round() as i32,
                        0,
                        0,
                    ),
                    None => match panel {
                        ToolbarPanel::PenPicker => (Align::Center, Align::End, 6, 6, 6, 18),
                        ToolbarPanel::ColorPicker => (Align::Center, Align::Start, 18, 18, 18, 6),
                        ToolbarPanel::PensSideBar => (Align::End, Align::Center, 18, 72, 18, 72),
                    },
                };

                widget.set_margin_top(margin_top);
                widget.set_margin_bottom(margin_bottom);
                widget.set_margin_start(margin_start);
                widget.set_margin_end(margin_end);
                widget.set_halign(halign);
                widget.set_valign(valign);
            }
            docked => {
                widget.set_margin_top(0);
                widget.set_margin_bottom(0);
                widget.set_margin_start(0);
                widget.set_margin_end(0);
                widget.set_halign(Align::Start);
                widget.set_valign(Align::Start);
                widget.set_hexpand(false);
                // Only the pen sidebar fills the height of a side dock; the other panels keep
                // their natural size.
                widget.set_vexpand(
                    matches!(docked, ToolbarDock::Left | ToolbarDock::Right)
                        && panel == ToolbarPanel::PensSideBar,
                );

                if let Some(slot) = self.dock_slot(docked) {
                    slot.append(&widget);
                }
            }
        }

        if was_mapped {
            widget.set_visible(true);
        }
        self.update_dock_slots_visibility();
    }

    fn position_in_overlay(&self, widget: &Widget) -> (f64, f64) {
        let Some(root) = widget
            .root()
            .and_then(|root| root.downcast::<gtk4::Window>().ok())
        else {
            return (0.0, 0.0);
        };
        let overlay = self.imp().toolbar_overlay.get();
        let Some(widget_bounds) = widget.compute_bounds(&root) else {
            return (0.0, 0.0);
        };
        let Some(overlay_bounds) = overlay.compute_bounds(&root) else {
            return (0.0, 0.0);
        };

        (
            widget_bounds.x() as f64 - overlay_bounds.x() as f64,
            widget_bounds.y() as f64 - overlay_bounds.y() as f64,
        )
    }

    /// Float a panel at the given position of the overlay, in overlay coordinates.
    ///
    /// `size` is the size the panel has when it floats; it is the one the position was clamped
    /// against, and the one the range of its travel range is computed from.
    fn float_panel_at(&self, panel: ToolbarPanel, x: f64, y: f64, size: (f64, f64)) {
        self.remember_float_position(panel, (x, y), size);
        self.imp()
            .docks
            .borrow_mut()
            .insert(panel, ToolbarDock::Floating);
        self.reparent_panel(panel, ToolbarDock::Floating, Some((x, y)));
    }

    /// Where the panel was left, as fractions of its travel range inside the overlay.
    pub(crate) fn panel_position(&self, panel: ToolbarPanel) -> Option<(f64, f64)> {
        self.imp().float_positions.borrow().get(&panel).copied()
    }

    /// Remember a panel position given as fractions of its travel range.
    ///
    /// A floating panel moves to it at once; a docked one keeps the position for when it floats.
    pub(crate) fn set_panel_position(&self, panel: ToolbarPanel, fraction: (f64, f64)) {
        self.imp().float_positions.borrow_mut().insert(
            panel,
            (fraction.0.clamp(0.0, 1.0), fraction.1.clamp(0.0, 1.0)),
        );
        self.apply_float_position(panel);
    }

    /// Remember the position of a floating panel given in overlay pixels.
    ///
    /// The pixels are not applied here: the drag that produced them moved the panel itself.
    ///
    /// `size` is the panel's size for that position, supplied by the caller instead of read off
    /// the widget: a drag hides the panel while it moves a picture of it, and a panel that is
    /// hidden, just reparented or stretched by a stale alignment reports a size it does not have
    /// on screen. A range collapsed to zero would clamp the fraction to 0 or 1 and lose the
    /// position the user chose.
    pub(crate) fn remember_float_position(
        &self,
        panel: ToolbarPanel,
        at: (f64, f64),
        size: (f64, f64),
    ) {
        let range = self.travel_range(size);
        self.imp().float_positions.borrow_mut().insert(
            panel,
            (
                position_fraction(at.0, range.0),
                position_fraction(at.1, range.1),
            ),
        );
    }

    /// Whether the default floating margins still apply to the panel.
    ///
    /// A panel the user placed keeps its own position, so the defaults must leave its margins
    /// alone.
    pub(crate) fn uses_default_margins(&self, panel: ToolbarPanel) -> bool {
        self.panel_dock(panel) == ToolbarDock::Floating && self.panel_position(panel).is_none()
    }

    /// The distance a panel of `size` can travel inside the overlay, in pixels.
    fn travel_range(&self, size: (f64, f64)) -> (f64, f64) {
        let overlay = self.imp().toolbar_overlay.get();
        (
            (f64::from(overlay.width()) - size.0).max(0.0),
            (f64::from(overlay.height()) - size.1).max(0.0),
        )
    }

    /// The size the overlay has given `panel`, once it has one.
    fn allocated_size(&self, panel: ToolbarPanel) -> Option<(f64, f64)> {
        let widget = self.panel_widget(panel);
        (widget.width() > 0 && widget.height() > 0)
            .then(|| (f64::from(widget.width()), f64::from(widget.height())))
    }

    /// Move a floating panel to its remembered position.
    ///
    /// Returns `false` while the panel has no allocation, and moves nothing. A remembered
    /// position is a fraction of the travel range, and the range only exists once the overlay
    /// has given the panel its size: measured instead of allocated, the panel is as wide as
    /// whatever content happens to be built at that moment — a color picker before its swatches
    /// exist is as wide as the overlay, a pens sidebar before its pages is shorter than it ends
    /// up — and the fraction lands the panel at the wrong edge. Placing it then would also
    /// record the window size as done, so the same size is never placed again.
    fn apply_float_position(&self, panel: ToolbarPanel) -> bool {
        if self.panel_dock(panel) != ToolbarDock::Floating {
            return true;
        }
        let Some(fraction) = self.panel_position(panel) else {
            return true;
        };
        let Some(size) = self.allocated_size(panel) else {
            return false;
        };
        let range = self.travel_range(size);
        let widget = self.panel_widget(panel);
        widget.set_halign(Align::Start);
        widget.set_valign(Align::Start);
        widget.set_margin_start(fraction_position(fraction.0, range.0) as i32);
        widget.set_margin_top(fraction_position(fraction.1, range.1) as i32);
        widget.set_margin_end(0);
        widget.set_margin_bottom(0);
        true
    }

    /// Move every floating panel that has a remembered position to it.
    ///
    /// Returns whether every panel that has a position could be placed.
    fn apply_float_positions(&self) -> bool {
        let mut all_placed = true;
        for panel in ToolbarPanel::ALL {
            all_placed &= self.apply_float_position(panel);
        }
        all_placed
    }

    /// Reapply the remembered positions whenever the window surface is laid out.
    ///
    /// A remembered position is a fraction of the travel range, so it can only be applied once
    /// the toolbar overlay is allocated, and has to be applied again when a resize changes that
    /// range. GTK4 notifies neither through the overlay's "width"/"height" properties nor
    /// through this widget: the template puts a `GtkBoxLayout` on it, and with a layout manager
    /// set GTK never calls `size_allocate` on the widget itself. The surface's "layout" signal
    /// reports every new size.
    fn connect_float_layout(&self) {
        if self.imp().float_layout_connected.replace(true) {
            return;
        }
        match self.native().and_then(|native| native.surface()) {
            Some(surface) => {
                let overlays = self.clone();
                surface.connect_layout(move |_, _, _| overlays.queue_float_positions());
            }
            None => {
                // Not attached to a window yet. `map` tries again.
                self.imp().float_layout_connected.set(false);
            }
        }
    }

    /// Apply the remembered positions once the toolbar overlay has been allocated.
    ///
    /// The application is deferred: a resize is announced before the widgets are allocated for
    /// it, so the panels can only be placed inside a range the overlay has.
    ///
    /// It is also delayed until the surface stops changing size. A window opens at its default
    /// size, is maximized, and the compositor may resize it again while it animates in, and a
    /// position applied at every one of those sizes makes the panels glide across the window
    /// after every start. A settle is the size the window keeps.
    pub(crate) fn queue_float_positions(&self) {
        if let Some(source) = self.imp().float_apply_source.take() {
            source.remove();
        }
        let overlays = self.clone();
        let source = glib::timeout_add_local_once(Duration::from_millis(100), move || {
            overlays.imp().float_apply_source.take();
            let overlay = overlays.imp().toolbar_overlay.get();
            // The size that is compared is the window's, not the overlay's: a placement that
            // changes the overlay's own size must not count as a new size to place the panels for,
            // or the panel would slide across the window one reapply at a time.
            let size = overlays
                .native()
                .and_then(|native| native.surface())
                .map(|surface| (surface.width(), surface.height()))
                .unwrap_or((0, 0));
            if size.0 > 0
                && size.1 > 0
                && overlay.width() > 0
                && overlay.height() > 0
                && size != overlays.imp().float_apply_size.get()
            {
                if overlays.apply_float_positions() {
                    overlays.imp().float_apply_size.set(size);
                    overlays.imp().float_apply_tries.set(0);
                } else if overlays.imp().float_apply_tries.get() < FLOAT_APPLY_TRIES {
                    // A panel is not allocated yet: it has no size to take its range from, and
                    // the surface may not resize again before it does. Try until it is, then
                    // give up rather than spin on a panel that never gets one.
                    overlays
                        .imp()
                        .float_apply_tries
                        .set(overlays.imp().float_apply_tries.get() + 1);
                    overlays.queue_float_positions();
                }
            }
        });
        self.imp().float_apply_source.replace(Some(source));
    }

    /// The window edge a release at `at` would dock `panel` to, and the distance to that edge.
    fn dock_target(
        &self,
        size: (f64, f64),
        start: (f64, f64),
        press: (f64, f64),
        at: (f64, f64),
    ) -> (f64, ToolbarDock) {
        let overlay = self.imp().toolbar_overlay.get();
        let center_x = start.0 + at.0 - press.0 + size.0 / 2.0;
        let center_y = start.1 + at.1 - press.1 + size.1 / 2.0;
        let edges = [
            (center_y, ToolbarDock::Top),
            (overlay.height() as f64 - center_y, ToolbarDock::Bottom),
            (center_x, ToolbarDock::Left),
            (overlay.width() as f64 - center_x, ToolbarDock::Right),
        ];
        edges
            .iter()
            .copied()
            .fold((f64::MAX, ToolbarDock::Floating), |best, edge| {
                if edge.0 < best.0 { edge } else { best }
            })
    }

    fn setup_toolbars(&self) {
        for panel in [
            ToolbarPanel::PenPicker,
            ToolbarPanel::ColorPicker,
            ToolbarPanel::PensSideBar,
        ] {
            self.panel_handle(panel).set_cursor_from_name(Some("grab"));
        }

        // The drag is driven from raw events on the whole overlays widget, on neither a panel
        // handle nor the toolbar overlay: a docked panel lives in a dock slot that is a sibling of
        // the overlay, so an overlay-owned gesture never sees a press on it. A `GestureDrag` is no
        // good either: floating a docked panel reparents it into the overlay and hides it for the
        // move, which drops the pointer focus of the press target, and GTK cancels a gesture whose
        // sequence loses its target mid-way - the drag then reports no offset update at all. Raw
        // events keep arriving whichever child the pointer is over, and reparenting does not stop
        // them.
        let dragged = Rc::new(Cell::new(None::<ToolbarPanel>));
        // Where the press landed, in overlays coordinates.
        let press_at = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
        // The panel's position in overlay coordinates where the drag started.
        let drag_start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
        // The position the panel has to move to, applied once per frame: a pen reports several
        // motions per frame, and one move per event costs a whole frame for each.
        let pending_move = Rc::new(Cell::new(None::<(i32, i32)>));
        let move_tick = Rc::new(RefCell::new(None::<gtk4::TickCallbackId>));
        // The picture of the dragged panel and its size, taken while the panel is still on screen.
        // A widget that moves is damaged only where it now is, so the panel is not moved at all:
        // it is hidden for the drag and the canvas paints its picture at the pointer.
        let drag_picture = Rc::new(RefCell::new(None::<(gtk4::gdk::Paintable, (f64, f64))>));
        let drag_size = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
        let preview_started = Rc::new(Cell::new(false));
        // The strip of the edge a release would dock the panel to, shown while the drag is in
        // progress. Each strip keeps a fixed position and is only shown or hidden: a widget that
        // moves while the pointer is down keeps the pixels of the position it left behind, because
        // the frame's damage then covers only where the widget now is.
        let indicators = [
            (ToolbarDock::Top, Align::Fill, Align::Start),
            (ToolbarDock::Bottom, Align::Fill, Align::End),
            (ToolbarDock::Left, Align::Start, Align::Fill),
            (ToolbarDock::Right, Align::End, Align::Fill),
        ]
        .into_iter()
        .map(|(dock, halign, valign)| {
            // A box, not a drawing area: the strip is painted by the CSS class, so it follows the
            // theme's accent colour instead of one hardcoded wash.
            let strip = Box::new(gtk4::Orientation::Horizontal, 0);
            strip.set_can_target(false);
            strip.set_visible(false);
            strip.add_css_class("dock_indicator");
            strip.set_halign(halign);
            strip.set_valign(valign);
            if matches!(dock, ToolbarDock::Top | ToolbarDock::Bottom) {
                strip.set_size_request(-1, DOCK_INDICATOR_THICKNESS);
            } else {
                strip.set_size_request(DOCK_INDICATOR_THICKNESS, -1);
            }
            self.imp().toolbar_overlay.get().add_overlay(&strip);
            (dock, strip)
        })
        .collect::<Vec<_>>();
        *self.imp().dock_indicators.borrow_mut() = indicators;

        let controller = gtk4::EventControllerLegacy::new();
        controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let on_event = {
            let overlays = self.clone();
            let dragged = dragged.clone();
            let press_at = press_at.clone();
            let drag_start = drag_start.clone();
            let pending_move = pending_move.clone();
            let move_tick = move_tick.clone();
            let drag_picture = drag_picture.clone();
            let drag_size = drag_size.clone();
            let preview_started = preview_started.clone();
            move |_controller: &gtk4::EventControllerLegacy,
                  event: &gtk4::gdk::Event|
                  -> glib::Propagation {
                // `Event::position` is in surface coordinates; the handle bounds are in the
                // overlays widget's own space, so the point is mapped through the window first.
                let (surface_x, surface_y) = event.position().unwrap_or_default();
                let (x, y) = overlays
                    .root()
                    .and_then(|root| {
                        root.upcast::<glib::Object>()
                            .downcast::<gtk4::Window>()
                            .ok()
                    })
                    .and_then(|root| {
                        root.compute_point(
                            &overlays,
                            &gtk4::graphene::Point::new(surface_x as f32, surface_y as f32),
                        )
                    })
                    .map(|point| (f64::from(point.x()), f64::from(point.y())))
                    .unwrap_or((surface_x, surface_y));

                match event.event_type() {
                    gtk4::gdk::EventType::ButtonPress => {
                        dragged.set(None);
                        pending_move.set(None);
                        drag_picture.replace(None);
                        preview_started.set(false);
                        // A drag cut short by a lost release leaves the strip on screen.
                        overlays.set_dock_indicator(ToolbarDock::Floating);
                        if let Some(id) = move_tick.borrow_mut().take() {
                            id.remove();
                        }
                        let primary = event
                            .downcast_ref::<gtk4::gdk::ButtonEvent>()
                            .is_some_and(|button| button.button() == gtk4::gdk::BUTTON_PRIMARY);
                        if !primary || overlays.imp().toolbars_locked.get() {
                            return glib::Propagation::Proceed;
                        }

                        // Press coordinates and the handle bounds are both taken in the overlays
                        // widget's space.
                        let hit = [
                            ToolbarPanel::PenPicker,
                            ToolbarPanel::ColorPicker,
                            ToolbarPanel::PensSideBar,
                        ]
                        .into_iter()
                        .find(|panel| {
                            overlays
                                .panel_handle(*panel)
                                .compute_bounds(&overlays)
                                .is_some_and(|bounds| {
                                    let (left, top) =
                                        (f64::from(bounds.x()), f64::from(bounds.y()));
                                    x >= left
                                        && x <= left + f64::from(bounds.width())
                                        && y >= top
                                        && y <= top + f64::from(bounds.height())
                                })
                        });
                        let Some(panel) = hit else {
                            return glib::Propagation::Proceed;
                        };

                        // Float the panel where it is, so a docked panel can be dragged out too. The
                        // position can be negative: a panel docked above or left of the overlay
                        // starts outside it, and the drag has to pick it up without a jump.
                        let widget = overlays.panel_widget(panel);
                        let (start_x, start_y) = overlays.position_in_overlay(&widget);
                        // A drag cut short by a lost release leaves the panel hidden; the next
                        // press brings it back before anything else reads its position.
                        widget.set_visible(true);
                        // The panel is on screen and drawn here, so its picture is taken now: it
                        // is hidden for the drag, and a hidden widget paints nothing.
                        // The picture covers the panel's whole box, border included, while
                        // `width()` and `height()` leave the CSS border out: drawing the picture
                        // at the content size squeezes the panel by the border width. Its own size
                        // draws it one to one.
                        let picture = gtk4::WidgetPaintable::new(Some(&widget)).current_image();
                        let size = (
                            f64::from(picture.intrinsic_width()),
                            f64::from(picture.intrinsic_height()),
                        );
                        drag_size.set(size);
                        drag_picture.replace(Some((picture, size)));
                        press_at.set((x, y));
                        drag_start.set((start_x, start_y));
                        overlays
                            .imp()
                            .docks
                            .borrow_mut()
                            .insert(panel, ToolbarDock::Floating);
                        // A panel already floating in the overlay stays where it is: reparenting
                        // it would hide and show it in the middle of the drag. Its margins only
                        // position it when it is aligned to the start.
                        let overlay = overlays.imp().toolbar_overlay.get();
                        if widget.parent().is_some_and(|parent| parent == overlay) {
                            overlay.set_measure_overlay(&widget, false);
                            widget.set_halign(Align::Start);
                            widget.set_valign(Align::Start);
                            widget.set_margin_end(0);
                            widget.set_margin_bottom(0);
                            widget.set_margin_start(start_x.round() as i32);
                            widget.set_margin_top(start_y.round() as i32);
                        } else {
                            overlays.reparent_panel(
                                panel,
                                ToolbarDock::Floating,
                                Some((start_x, start_y)),
                            );
                        }
                        dragged.set(Some(panel));
                        // The handle cursor is "grab"; keep the same feedback while the panel moves.
                        overlays.set_cursor_from_name(Some("grabbing"));
                    }
                    gtk4::gdk::EventType::MotionNotify => {
                        let Some(panel) = dragged.get() else {
                            return glib::Propagation::Proceed;
                        };
                        if overlays.imp().toolbars_locked.get() {
                            return glib::Propagation::Proceed;
                        }

                        let target = overlays.dock_target(
                            drag_size.get(),
                            drag_start.get(),
                            press_at.get(),
                            (x, y),
                        );
                        let shift_held = event
                            .modifier_state()
                            .contains(gtk4::gdk::ModifierType::SHIFT_MASK);
                        let dock = dock_at_release(
                            target,
                            dock_release_docks(overlays.dock_on_drop(), shift_held),
                        )
                        .unwrap_or(ToolbarDock::Floating);
                        overlays.set_dock_indicator(dock);

                        let widget = overlays.panel_widget(panel);
                        let (press_x, press_y) = press_at.get();
                        let (start_x, start_y) = drag_start.get();
                        let (next_x, next_y) = (start_x + x - press_x, start_y + y - press_y);

                        // Floating a panel that is on screen is queued to the next main loop
                        // iteration, so the first moves land while the panel still sits in its dock
                        // slot, where margins mean nothing: the queued float places it.
                        if !widget.is_ancestor(&overlays.imp().toolbar_overlay.get()) {
                            return glib::Propagation::Proceed;
                        }

                        pending_move.set(Some((next_x.round() as i32, next_y.round() as i32)));
                        if move_tick.borrow().is_none() {
                            let overlays_for_tick = overlays.clone();
                            let dragged_for_tick = dragged.clone();
                            let pending_move_for_tick = pending_move.clone();
                            let move_tick_for_tick = move_tick.clone();
                            let drag_picture_for_tick = drag_picture.clone();
                            let preview_started_for_tick = preview_started.clone();
                            let id = overlays.add_tick_callback(move |_, _| {
                                let Some((move_x, move_y)) = pending_move_for_tick.take() else {
                                    *move_tick_for_tick.borrow_mut() = None;
                                    return glib::ControlFlow::Break;
                                };
                                if let Some(panel) = dragged_for_tick.get() {
                                    let widget = overlays_for_tick.panel_widget(panel);
                                    let overlay = overlays_for_tick.imp().toolbar_overlay.get();
                                    if widget.is_ancestor(&overlay) {
                                        let canvas = overlays_for_tick
                                            .root()
                                            .and_then(|root| root.downcast::<RnAppWindow>().ok())
                                            .and_then(|appwindow| appwindow.active_tab_canvas());
                                        let picture = drag_picture_for_tick
                                            .borrow()
                                            .as_ref()
                                            .map(|(picture, size)| (picture.clone(), *size));
                                        match (canvas, picture) {
                                            (Some(canvas), Some((picture, size)))
                                                if picture.intrinsic_width() > 0 =>
                                            {
                                                let at = overlay
                                                    .compute_point(
                                                        &canvas,
                                                        &gtk4::graphene::Point::new(
                                                            move_x as f32,
                                                            move_y as f32,
                                                        ),
                                                    )
                                                    .map(|at| {
                                                        (f64::from(at.x()), f64::from(at.y()))
                                                    });
                                                let Some(at) = at else {
                                                    return glib::ControlFlow::Continue;
                                                };
                                                if preview_started_for_tick.replace(true) {
                                                    canvas.move_drag_preview(at);
                                                } else {
                                                    // The panel is not moved at all: a widget that
                                                    // moves keeps the pixels of the place it left,
                                                    // and under a pen, which moves it far from frame
                                                    // to frame, they read as flicker. The canvas
                                                    // repaints itself as a whole, so the picture is
                                                    // drawn there and the panel waits out the drag
                                                    // hidden.
                                                    canvas.start_drag_preview(picture, size, at);
                                                    widget.set_visible(false);
                                                }
                                            }
                                            _ => {
                                                // No canvas to draw on, or no picture taken: fall
                                                // back to moving the panel itself.
                                                widget.set_margin_start(move_x);
                                                widget.set_margin_top(move_y);
                                            }
                                        }
                                    }
                                }
                                glib::ControlFlow::Continue
                            });
                            *move_tick.borrow_mut() = Some(id);
                        }
                    }
                    gtk4::gdk::EventType::ButtonRelease => {
                        overlays.set_cursor_from_name(None);
                        overlays.set_dock_indicator(ToolbarDock::Floating);
                        if let Some(id) = move_tick.borrow_mut().take() {
                            id.remove();
                        }
                        // The picture was drawn by the canvas in the panel's place; the panel
                        // itself is shown again where the release puts it.
                        if preview_started.replace(false)
                            && let Some(canvas) = overlays
                                .root()
                                .and_then(|root| root.downcast::<RnAppWindow>().ok())
                                .and_then(|appwindow| appwindow.active_tab_canvas())
                        {
                            canvas.end_drag_preview();
                        }
                        let Some(panel) = dragged.take() else {
                            return glib::Propagation::Proceed;
                        };
                        let widget = overlays.panel_widget(panel);
                        widget.set_visible(true);
                        pending_move.set(None);
                        if overlays.imp().toolbars_locked.get() {
                            return glib::Propagation::Proceed;
                        }

                        let overlay = overlays.imp().toolbar_overlay.get();
                        let (press_x, press_y) = press_at.get();
                        let (start_x, start_y) = drag_start.get();
                        let (offset_x, offset_y) = (x - press_x, y - press_y);
                        let overlay_width = overlay.width() as f64;
                        let overlay_height = overlay.height() as f64;
                        let size = drag_size.get();
                        let target = overlays.dock_target(
                            size,
                            (start_x, start_y),
                            (press_x, press_y),
                            (x, y),
                        );
                        let shift_held = event
                            .modifier_state()
                            .contains(gtk4::gdk::ModifierType::SHIFT_MASK);
                        let dock = dock_at_release(
                            target,
                            dock_release_docks(overlays.dock_on_drop(), shift_held),
                        );

                        if let Some(dock) = dock {
                            overlays.set_panel_dock(panel, dock);
                        } else {
                            // Keep it floating, but inside the overlay. A release that beats the
                            // queued float sets the position it was queued with instead of margins
                            // the dock ignores.
                            let max_x = (overlay_width - size.0).max(0.0);
                            let max_y = (overlay_height - size.1).max(0.0);
                            let x = (start_x + offset_x).clamp(0.0, max_x);
                            let y = (start_y + offset_y).clamp(0.0, max_y);
                            if widget.is_ancestor(&overlay) {
                                overlays.remember_float_position(panel, (x, y), size);
                                widget.set_margin_start(x as i32);
                                widget.set_margin_top(y as i32);
                                overlay.queue_draw();
                            } else {
                                overlays.float_panel_at(panel, x, y, size);
                            }
                        }

                        // A released drag is a layout the user means to keep: write it now, so a
                        // quit that never reports the window as inactive still keeps it.
                        if let Some(appwindow) = overlays
                            .root()
                            .and_then(|root| root.downcast::<RnAppWindow>().ok())
                        {
                            appwindow.save_toolbar_layout();
                        }
                    }
                    _ => {}
                }

                glib::Propagation::Proceed
            }
        };
        controller.connect_event(on_event);
        self.add_controller(controller);
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        imp.colorpicker.get().init(appwindow);
        imp.penssidebar.get().init(appwindow);
        imp.penpicker.get().init(appwindow);
        imp.penssidebar.get().brush_page().init(appwindow);
        imp.penssidebar.get().shaper_page().init(appwindow);
        imp.penssidebar.get().typewriter_page().init(appwindow);
        imp.penssidebar.get().eraser_page().init(appwindow);
        imp.penssidebar.get().selector_page().init(appwindow);
        imp.penssidebar.get().tools_page().init(appwindow);

        // The panels are pointer tools. A click that took the keyboard focus would leave the
        // canvas unfocused, and the bare tool keys (1-6, b/s/t/e/v/x) are only handled by the
        // canvas's key controller: picking a color would kill them until the canvas is clicked
        // again. Keep tab-focus intact, just don't move focus on a click.
        for panel in [
            imp.penpicker_box.clone(),
            imp.colorpicker_box.clone(),
            imp.sidebar_box.clone(),
        ] {
            disable_focus_on_click(&panel);
        }

        self.setup_colorpicker(appwindow);
        self.setup_tabview(appwindow);
        self.setup_toolbars();
        self.imp().setup_toolbar_overlay();
    }

    fn setup_colorpicker(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();

        imp.colorpicker.connect_notify_local(
            Some("stroke-color"),
            clone!(
                #[weak]
                appwindow,
                move |colorpicker, _paramspec| {
                    let Some(canvas) = appwindow.active_tab_canvas() else {
                        return;
                    };
                    let stroke_color = colorpicker.stroke_color().into_compose_color();
                    let current_pen_style = canvas.engine_ref().current_pen_style_w_override();

                    match current_pen_style {
                        PenStyle::Typewriter => {
                            let widget_flags = canvas.engine_mut().text_change_color(stroke_color);
                            appwindow.handle_widget_flags(widget_flags, &canvas);
                        }
                        PenStyle::Selector => {
                            let widget_flags = canvas
                                .engine_mut()
                                .change_selection_stroke_colors(stroke_color);
                            appwindow.handle_widget_flags(widget_flags, &canvas);
                        }
                        PenStyle::Brush | PenStyle::Shaper | PenStyle::Eraser | PenStyle::Tools => {
                        }
                    }

                    // We have a global colorpicker, so we apply it to all styles
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .set_all_stroke_colors(stroke_color);
                }
            ),
        );

        imp.colorpicker.connect_notify_local(
            Some("fill-color"),
            clone!(
                #[weak]
                appwindow,
                move |colorpicker, _paramspec| {
                    let Some(canvas) = appwindow.active_tab_canvas() else {
                        return;
                    };
                    let fill_color = colorpicker.fill_color().into_compose_color();
                    let stroke_style = canvas.engine_ref().current_pen_style_w_override();

                    match stroke_style {
                        PenStyle::Selector => {
                            let widget_flags =
                                canvas.engine_mut().change_selection_fill_colors(fill_color);
                            appwindow.handle_widget_flags(widget_flags, &canvas);
                        }
                        PenStyle::Typewriter
                        | PenStyle::Brush
                        | PenStyle::Shaper
                        | PenStyle::Eraser
                        | PenStyle::Tools => {}
                    }

                    // We have a global colorpicker, so we apply it to all styles
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .set_all_fill_colors(fill_color);
                }
            ),
        );
    }

    fn setup_tabview(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();

        imp.tabview.connect_selected_page_notify(clone!(
            #[weak]
            appwindow,
            move |_| {
                let Some(active_tab_page) = appwindow.active_tab_page() else {
                    return;
                };
                let active_canvaswrapper = active_tab_page
                    .child()
                    .downcast::<RnCanvasWrapper>()
                    .unwrap();
                appwindow.tabs_set_unselected_inactive();
                let widget_flags = active_canvaswrapper.canvas().engine_mut().set_active(true);
                appwindow.handle_widget_flags(widget_flags, &active_canvaswrapper.canvas());
                appwindow.refresh_ui();
            }
        ));

        imp.tabview.connect_page_attached(clone!(
            #[weak]
            appwindow,
            move |_, page, _| {
                let canvaswrapper = page.child().downcast::<RnCanvasWrapper>().unwrap();
                canvaswrapper.init_reconnect(&appwindow);
                canvaswrapper.connect_to_tab_page(page);
                let widget_flags = canvaswrapper.canvas().engine_mut().set_active(true);
                appwindow.handle_widget_flags(widget_flags, &canvaswrapper.canvas());
            }
        ));

        imp.tabview.connect_page_detached(clone!(move |_, page, _| {
            let canvaswrapper = page.child().downcast::<RnCanvasWrapper>().unwrap();
            let _ = canvaswrapper.canvas().engine_mut().set_active(false);
            canvaswrapper.disconnect_connections();
        }));

        imp.tabview.connect_close_page(clone!(
            #[weak]
            appwindow,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |_, page| {
                glib::spawn_future_local(clone!(
                    #[weak]
                    appwindow,
                    #[weak]
                    page,
                    async move {
                        let close_finish_confirm = if page
                            .child()
                            .downcast::<RnCanvasWrapper>()
                            .unwrap()
                            .canvas()
                            .unsaved_changes()
                        {
                            dialogs::dialog_close_tab(&appwindow, &page).await
                        } else {
                            true
                        };

                        appwindow.close_tab_finish(&page, close_finish_confirm);
                    }
                ));

                glib::Propagation::Stop
            }
        ));

        imp.tabview.connect_setup_menu(clone!(
            #[weak]
            appwindow,
            move |tabview, page| {
                if let Some(page) = page {
                    let action_active_tab_move_left = appwindow
                        .lookup_action("active-tab-move-left")
                        .unwrap()
                        .downcast::<gio::SimpleAction>()
                        .unwrap();
                    let action_active_tab_move_right = appwindow
                        .lookup_action("active-tab-move-right")
                        .unwrap()
                        .downcast::<gio::SimpleAction>()
                        .unwrap();
                    let action_active_tab_close = appwindow
                        .lookup_action("active-tab-close")
                        .unwrap()
                        .downcast::<gio::SimpleAction>()
                        .unwrap();
                    let action_active_tab_move_window = appwindow
                        .lookup_action("active-tab-move-to-new-window")
                        .unwrap()
                        .downcast::<gio::SimpleAction>()
                        .unwrap();

                    tabview.set_selected_page(page);

                    let n_pages = tabview.n_pages();
                    let pos = tabview.page_position(page);
                    action_active_tab_move_left.set_enabled(pos > 0);
                    action_active_tab_move_right.set_enabled(pos + 1 < n_pages);
                    action_active_tab_close.set_enabled(n_pages > 1);
                    action_active_tab_move_window.set_enabled(n_pages > 1);
                }
            }
        ));

        imp.tabview.connect_create_window(clone!(
            #[weak]
            appwindow,
            #[upgrade_or_default]
            move |_tab| {
                use crate::RnApp;
                if let Some(rn_app_out) = appwindow.application() {
                    let rnapp = rn_app_out.downcast::<RnApp>().unwrap();
                    Some(rnapp.new_appwindow_init_return_tab())
                } else {
                    None
                }
            }
        ));
    }

    pub(crate) fn progressbar_start_pulsing(&self) {
        const PULSE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);

        self.imp()
            .progresspulses_active
            .set(self.imp().progresspulses_active.get().saturating_add(1));

        if let Some(src) =
            self.imp()
                .progresspulse_id
                .replace(Some(glib::source::timeout_add_local(
                    PULSE_INTERVAL,
                    clone!(
                        #[weak(rename_to=appwindow)]
                        self,
                        #[upgrade_or]
                        glib::ControlFlow::Break,
                        move || {
                            appwindow.progressbar().pulse();

                            glib::ControlFlow::Continue
                        }
                    ),
                )))
        {
            src.remove();
        }
    }

    pub(crate) fn progressbar_finish(&self) {
        const FINISH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

        self.progressbar().set_fraction(1.);
        self.imp()
            .progresspulses_active
            .set(self.imp().progresspulses_active.get().saturating_sub(1));

        if self.imp().progresspulses_active.get() == 0 {
            if let Some(src) = self.imp().progresspulse_id.take() {
                src.remove();
            }
            glib::source::timeout_add_local_once(
                FINISH_TIMEOUT,
                clone!(
                    #[weak(rename_to=appwindow)]
                    self,
                    move || {
                        appwindow.progressbar().set_fraction(0.);
                    }
                ),
            );
        }
    }

    #[allow(unused)]
    pub(crate) fn progressbar_abort(&self) {
        self.imp()
            .progresspulses_active
            .set(self.imp().progresspulses_active.get().saturating_sub(1));

        if self.imp().progresspulses_active.get() == 0 {
            if let Some(src) = self.imp().progresspulse_id.take() {
                src.remove();
            }
            self.progressbar().set_fraction(0.);
        }
    }

    pub(crate) fn dispatch_toast_w_button<F: Fn(&adw::Toast) + 'static>(
        &self,
        text: &str,
        button_label: &str,
        button_callback: F,
        timeout: Option<Duration>,
    ) -> glib::WeakRef<adw::Toast> {
        let toast = adw::Toast::builder()
            .title(text)
            .priority(adw::ToastPriority::High)
            .button_label(button_label)
            .timeout(timeout.map(|t| t.as_secs() as u32).unwrap_or(0))
            .build();
        toast.connect_button_clicked(button_callback);
        let toast_ref = glib::WeakRef::new();
        toast_ref.set(Some(&toast));
        self.toast_overlay().add_toast(toast);
        toast_ref
    }

    /// Ensures that only one toast per `singleton_toast` is queued at the same time by dismissing the previous toast.
    ///
    /// `singleton_toast` is a weak reference to a `Toast` which should be held somewhere by the caller.
    /// On subsequent calls the held reference will be replaced by one of the new dispatched toast.
    /// The caller should not modify this weak reference themselves.
    pub(crate) fn dispatch_toast_w_button_singleton<F: Fn(&adw::Toast) + 'static>(
        &self,
        text: &str,
        button_label: &str,
        button_callback: F,
        timeout: Option<Duration>,
        singleton_toast: &glib::WeakRef<adw::Toast>,
    ) {
        if let Some(previous_toast) = singleton_toast.upgrade() {
            previous_toast.dismiss();
        }
        singleton_toast.set(
            self.dispatch_toast_w_button(text, button_label, button_callback, timeout)
                .upgrade()
                .as_ref(),
        );
    }

    pub(crate) fn dispatch_toast_text(&self, text: &str, timeout: Option<Duration>) -> adw::Toast {
        let toast = adw::Toast::builder()
            .title(text)
            .priority(adw::ToastPriority::High)
            .timeout(timeout.map(|t| t.as_secs() as u32).unwrap_or(0))
            .build();
        self.toast_overlay().add_toast(toast.clone());
        toast
    }

    pub(crate) fn dispatch_toast_text_singleton(
        &self,
        text: &str,
        timeout: Option<Duration>,
        singleton_toast: &mut Option<adw::Toast>,
    ) {
        if let Some(previous_toast) = singleton_toast {
            previous_toast.dismiss();
        }
        *singleton_toast = Some(self.dispatch_toast_text(text, timeout));
    }

    pub(crate) fn dispatch_toast_error(&self, err: &str) -> adw::Toast {
        let toast = adw::Toast::builder()
            .title(err)
            .priority(adw::ToastPriority::High)
            .timeout(0)
            .build();
        self.toast_overlay().add_toast(toast.clone());
        error!("{err}");
        toast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three dockable toolbars, in a fixed order so failures name the same panel.
    const PANELS: [ToolbarPanel; 3] = [
        ToolbarPanel::PenPicker,
        ToolbarPanel::ColorPicker,
        ToolbarPanel::PensSideBar,
    ];

    fn panel_parent(overlays: &RnOverlays, panel: ToolbarPanel) -> Widget {
        overlays
            .panel_widget(panel)
            .parent()
            .expect("every toolbar must stay parented")
    }

    /// The binary registers the widgets' templates and icons in `main`; a test binary has to do
    /// the same before it builds any widget.
    fn init_gtk() -> bool {
        if gtk4::init().is_err() {
            eprintln!("no GTK display available, skipping the toolbar dock test");
            return false;
        }
        if let Err(err) = gio::resources_register_include!("compiled.gresource") {
            eprintln!("failed to register compiled gresource, Err: {err:?}");
            return false;
        }
        true
    }

    /// Every GTK flow runs from this one test: GTK can only be initialized on one thread, and
    /// libtest runs each test function on its own thread.
    #[test]
    fn gtk_toolbar_flows() {
        // Without a display (CI, a headless shell) there is nothing to check here.
        if !init_gtk() {
            return;
        }
        toolbars_move_between_dock_slots_and_the_overlay();
        locking_moves_every_toolbar_to_the_top_and_hides_handles();
        docks_and_floats_a_panel_that_is_on_screen();
        the_dock_indicator_marks_only_the_target_edge();
        default_margins_yield_to_a_stored_position();
    }

    fn toolbars_move_between_dock_slots_and_the_overlay() {
        let overlays = RnOverlays::new();
        let imp = overlays.imp();
        let overlay: Widget = imp.toolbar_overlay.clone().upcast();
        let dock = |dock: ToolbarDock| -> Widget { overlays.dock_slot(dock).unwrap().upcast() };

        // Every panel starts floating in the overlay, and its handle is shown.
        for panel in PANELS {
            assert_eq!(panel_parent(&overlays, panel), overlay);
            assert!(overlays.panel_handle(panel).is_visible());
        }

        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Left);
        assert_eq!(
            panel_parent(&overlays, ToolbarPanel::PenPicker),
            dock(ToolbarDock::Left)
        );
        assert!(overlays.dock_slot(ToolbarDock::Left).unwrap().is_visible());

        overlays.set_panel_dock(ToolbarPanel::ColorPicker, ToolbarDock::Bottom);
        assert_eq!(
            panel_parent(&overlays, ToolbarPanel::ColorPicker),
            dock(ToolbarDock::Bottom)
        );
        assert!(overlays.imp().dock_bottom_scroller.is_visible());

        // The sidebar fills the height of a side dock, the other panels keep their natural size.
        overlays.set_panel_dock(ToolbarPanel::PensSideBar, ToolbarDock::Right);
        assert_eq!(
            panel_parent(&overlays, ToolbarPanel::PensSideBar),
            dock(ToolbarDock::Right)
        );
        assert!(overlays.panel_widget(ToolbarPanel::PensSideBar).vexpands());
        assert!(!overlays.panel_widget(ToolbarPanel::PenPicker).vexpands());

        // An emptied dock collapses.
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Floating);
        assert!(!overlays.dock_slot(ToolbarDock::Left).unwrap().is_visible());
        assert_eq!(panel_parent(&overlays, ToolbarPanel::PenPicker), overlay);

        // Floating can place a panel at an exact overlay position. The overlay has no size in
        // this test, so the position comes from the pixels alone.
        overlays.float_panel_at(ToolbarPanel::PenPicker, 40.0, 50.0, (0.0, 0.0));
        let penpicker = overlays.panel_widget(ToolbarPanel::PenPicker);
        assert_eq!((penpicker.margin_start(), penpicker.margin_top()), (40, 50));
        assert_eq!(penpicker.halign(), Align::Start);
        assert_eq!(penpicker.valign(), Align::Start);
    }

    fn locking_moves_every_toolbar_to_the_top_and_hides_handles() {
        let overlays = RnOverlays::new();
        let top: Widget = overlays.dock_slot(ToolbarDock::Top).unwrap().upcast();

        // The state the window restores from settings before the lock is applied.
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Left);
        overlays.set_panel_dock(ToolbarPanel::ColorPicker, ToolbarDock::Right);
        overlays.set_panel_dock(ToolbarPanel::PensSideBar, ToolbarDock::Top);
        overlays.set_toolbars_locked(true);

        for panel in PANELS {
            assert_eq!(panel_parent(&overlays, panel), top);
            assert!(!overlays.panel_handle(panel).is_visible());
        }
        assert!(overlays.imp().dock_top_scroller.is_visible());
        assert!(!overlays.dock_slot(ToolbarDock::Right).unwrap().is_visible());

        // Unlocking leaves the panels docked where they are and brings the handles back.
        overlays.set_toolbars_locked(false);
        assert!(!overlays.toolbars_locked());
        for panel in PANELS {
            assert_eq!(panel_parent(&overlays, panel), top);
            assert!(overlays.panel_handle(panel).is_visible());
        }
    }

    /// A panel that is already on screen must survive being reparented.
    ///
    /// Dragging a docked panel out, and the settings that dock panels while the window is up,
    /// both reparent a mapped widget. That reparent is deferred to the next main loop iteration,
    /// so the assertions here settle the loop after each dock change.
    fn docks_and_floats_a_panel_that_is_on_screen() {
        let overlays = RnOverlays::new();
        let window = gtk4::Window::builder().child(&overlays).build();
        window.present();

        // Map the window, so the panels are mapped too, as they are when a user drags them.
        let context = glib::MainContext::default();
        for _ in 0..500 {
            if overlays.is_mapped() {
                break;
            }
            context.iteration(false);
        }
        if !overlays.is_mapped() {
            eprintln!("the window did not map, skipping the mapped panel test");
            return;
        }

        let dock = |dock: ToolbarDock| -> Widget { overlays.dock_slot(dock).unwrap().upcast() };
        let overlay: Widget = overlays.imp().toolbar_overlay.clone().upcast();
        // A reparent of a mapped panel is queued on the main loop.
        let settle = || {
            for _ in 0..20 {
                context.iteration(false);
            }
        };

        // Docked out of the overlay, and floated back into it.
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Left);
        settle();
        assert_eq!(
            panel_parent(&overlays, ToolbarPanel::PenPicker),
            dock(ToolbarDock::Left)
        );
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Floating);
        settle();
        assert_eq!(panel_parent(&overlays, ToolbarPanel::PenPicker), overlay);

        // And the other way around: a floating panel dropped onto an edge.
        overlays.set_panel_dock(ToolbarPanel::ColorPicker, ToolbarDock::Bottom);
        settle();
        assert_eq!(
            panel_parent(&overlays, ToolbarPanel::ColorPicker),
            dock(ToolbarDock::Bottom)
        );

        // And the overlay must not clip a panel at a negative offset: a docked panel floats out
        // of a top or left slot at one until the pointer pulls it inside.
        assert_eq!(
            overlays.imp().toolbar_overlay.get().overflow(),
            gtk4::Overflow::Visible
        );
    }

    /// Shift inverts the preference, so one gesture always docks and the other always floats.
    #[test]
    fn shift_inverts_docking_on_drop() {
        assert!(!dock_release_docks(false, false));
        assert!(dock_release_docks(false, true));
        assert!(dock_release_docks(true, false));
        assert!(!dock_release_docks(true, true));
    }

    /// A release docks only when docking is armed and the panel is inside the snap zone.
    #[test]
    fn a_release_docks_only_inside_the_snap_zone() {
        let target = |distance: f64| (distance, ToolbarDock::Top);
        assert_eq!(dock_at_release(target(0.0), true), Some(ToolbarDock::Top));
        assert_eq!(
            dock_at_release(target(DOCK_SNAP_DISTANCE), true),
            Some(ToolbarDock::Top)
        );
        assert_eq!(
            dock_at_release(target(DOCK_SNAP_DISTANCE + 0.5), true),
            None
        );
        // A disarmed release floats however close it is.
        assert_eq!(dock_at_release(target(0.0), false), None);
    }

    /// The strip marks the edge the release would use, and no strip is shown while floating.
    fn the_dock_indicator_marks_only_the_target_edge() {
        let overlays = RnOverlays::new();
        overlays.setup_toolbars();
        let indicators = overlays.imp().dock_indicators.borrow();
        assert_eq!(indicators.len(), 4);
        for (_, strip) in indicators.iter() {
            assert!(strip.has_css_class("dock_indicator"));
            assert!(!strip.is_visible());
        }

        overlays.set_dock_indicator(ToolbarDock::Left);
        let shown = indicators
            .iter()
            .filter(|(_, strip)| strip.is_visible())
            .map(|(dock, _)| *dock)
            .collect::<Vec<_>>();
        assert_eq!(shown, vec![ToolbarDock::Left]);

        overlays.set_dock_indicator(ToolbarDock::Floating);
        assert!(indicators.iter().all(|(_, strip)| !strip.is_visible()));
    }

    #[test]
    fn a_stored_position_is_a_fraction_of_the_travel_range() {
        assert_eq!(position_fraction(0.0, 400.0), 0.0);
        assert_eq!(position_fraction(200.0, 400.0), 0.5);
        assert_eq!(position_fraction(400.0, 400.0), 1.0);
        // A position past the end of the range and a range that has not been laid out yet both
        // stay valid.
        assert_eq!(position_fraction(600.0, 400.0), 1.0);
        assert_eq!(position_fraction(10.0, 0.0), 0.0);

        assert_eq!(fraction_position(0.5, 400.0), 200.0);
        assert_eq!(fraction_position(1.0, 400.0), 400.0);
        assert_eq!(fraction_position(2.0, 400.0), 400.0);

        // The same fraction of a wider range keeps the panel in the same relative place.
        let stored = position_fraction(100.0, 400.0);
        assert_eq!(fraction_position(stored, 800.0), 200.0);
    }

    fn default_margins_yield_to_a_stored_position() {
        let overlays = RnOverlays::new();
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Floating);
        assert!(overlays.uses_default_margins(ToolbarPanel::PenPicker));

        overlays.set_panel_position(ToolbarPanel::PenPicker, (0.5, 0.5));
        assert!(!overlays.uses_default_margins(ToolbarPanel::PenPicker));
        assert_eq!(
            overlays.panel_position(ToolbarPanel::PenPicker),
            Some((0.5, 0.5))
        );

        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Top);
        assert!(!overlays.uses_default_margins(ToolbarPanel::PenPicker));
    }
}
