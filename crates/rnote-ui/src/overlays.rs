// Imports
use crate::RnPensSideBar;
use crate::canvaswrapper::RnCanvasWrapper;
use crate::{RnAppWindow, RnColorPicker, RnPenPicker, dialogs};
use core::time::Duration;
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
const DOCK_SNAP_DISTANCE: f64 = 64.0;

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

    impl WidgetImpl for RnOverlays {}
    impl RnOverlays {
        pub(crate) fn setup_toolbar_overlay(&self) {
            self.toolbar_overlay
                .set_measure_overlay(&*self.colorpicker_box, true);
            self.toolbar_overlay
                .set_measure_overlay(&*self.penpicker_box, true);
            self.toolbar_overlay
                .set_measure_overlay(&*self.sidebar_box, true);
        }
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
                // `add_overlay` reparents the panel out of its dock slot, so measure has to be
                // set after it is an overlay child.
                overlay.add_overlay(&widget);
                overlay.set_measure_overlay(&widget, true);

                // A dragged panel is placed at the position the drag started from; one that is
                // only floating goes back to its default corner.
                let (halign, valign, margin_start, margin_top, margin_end, margin_bottom) =
                    match (at, panel) {
                        (Some((x, y)), _) => (Align::Start, Align::Start, x as i32, y as i32, 0, 0),
                        (None, ToolbarPanel::PenPicker) => (Align::Center, Align::End, 6, 6, 6, 18),
                        (None, ToolbarPanel::ColorPicker) => {
                            (Align::Center, Align::Start, 18, 18, 18, 6)
                        }
                        (None, ToolbarPanel::PensSideBar) => {
                            (Align::End, Align::Center, 18, 72, 18, 72)
                        }
                    };

                widget.set_margin_top(margin_top);
                widget.set_margin_bottom(margin_bottom);
                widget.set_margin_start(margin_start);
                widget.set_margin_end(margin_end);
                widget.set_halign(halign);
                widget.set_valign(valign);
                widget.set_hexpand(false);
                widget.set_vexpand(false);
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
    fn float_panel_at(&self, panel: ToolbarPanel, x: f64, y: f64) {
        self.imp()
            .docks
            .borrow_mut()
            .insert(panel, ToolbarDock::Floating);
        self.reparent_panel(panel, ToolbarDock::Floating, Some((x, y)));
    }

    fn setup_toolbars(&self) {
        for panel in [
            ToolbarPanel::PenPicker,
            ToolbarPanel::ColorPicker,
            ToolbarPanel::PensSideBar,
        ] {
            self.panel_handle(panel).set_cursor_from_name(Some("grab"));
        }

        // The drag sits on the whole overlays widget, on neither a panel handle nor the toolbar
        // overlay: a docked panel lives in a dock slot that is a sibling of the overlay, so an
        // overlay-owned gesture never sees a press on it, and a handle-owned gesture dies when
        // floating the panel reparents the handle. The overlays widget is the common ancestor of
        // the slots and the overlay and never gets reparented.
        let dragged = Rc::new(Cell::new(None::<ToolbarPanel>));
        let start_margin = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
        let drag = gtk4::GestureDrag::new();

        drag.connect_drag_begin(clone!(
            #[weak(rename_to = overlays)]
            self,
            #[strong]
            dragged,
            #[strong]
            start_margin,
            move |gesture, start_x, start_y| {
                dragged.set(None);
                if overlays.imp().toolbars_locked.get() {
                    return;
                }

                // Press coordinates are relative to the widget the gesture sits on (the overlays
                // widget), so the handle bounds are taken in that same space.
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
                            let (x, y) = (f64::from(bounds.x()), f64::from(bounds.y()));
                            start_x >= x
                                && start_x <= x + f64::from(bounds.width())
                                && start_y >= y
                                && start_y <= y + f64::from(bounds.height())
                        })
                });
                let Some(panel) = hit else {
                    gesture.set_state(gtk4::EventSequenceState::Denied);
                    return;
                };

                // Float the panel where it is, so a docked panel can be dragged out too. The
                // position can be negative: a panel docked above or left of the overlay starts
                // outside it, and the drag has to pick it up without a jump.
                let widget = overlays.panel_widget(panel);
                let (x, y) = overlays.position_in_overlay(&widget);
                overlays.float_panel_at(panel, x, y);
                start_margin.set((x, y));
                dragged.set(Some(panel));
            }
        ));

        drag.connect_drag_update(clone!(
            #[weak(rename_to = overlays)]
            self,
            #[strong]
            dragged,
            #[strong]
            start_margin,
            move |_gesture, offset_x, offset_y| {
                let Some(panel) = dragged.get() else {
                    return;
                };
                if overlays.imp().toolbars_locked.get() {
                    return;
                }

                let widget = overlays.panel_widget(panel);
                let (start_x, start_y) = start_margin.get();
                widget.set_margin_start((start_x + offset_x) as i32);
                widget.set_margin_top((start_y + offset_y) as i32);
            }
        ));

        drag.connect_drag_end(clone!(
            #[weak(rename_to = overlays)]
            self,
            #[strong]
            dragged,
            #[strong]
            start_margin,
            move |gesture, _offset_x, _offset_y| {
                let Some(panel) = dragged.take() else {
                    return;
                };
                if overlays.imp().toolbars_locked.get() {
                    return;
                }

                let widget = overlays.panel_widget(panel);
                let overlay = overlays.imp().toolbar_overlay.get();
                let (offset_x, offset_y) = gesture.offset().unwrap_or_default();
                let (start_x, start_y) = start_margin.get();
                let overlay_width = overlay.width() as f64;
                let overlay_height = overlay.height() as f64;

                let center_x = start_x + offset_x + widget.width() as f64 / 2.0;
                let center_y = start_y + offset_y + widget.height() as f64 / 2.0;
                let edges = [
                    (center_y, ToolbarDock::Top),
                    (overlay_height - center_y, ToolbarDock::Bottom),
                    (center_x, ToolbarDock::Left),
                    (overlay_width - center_x, ToolbarDock::Right),
                ];
                let (distance, dock) =
                    edges
                        .iter()
                        .copied()
                        .fold((f64::MAX, ToolbarDock::Floating), |best, edge| {
                            if edge.0 < best.0 { edge } else { best }
                        });

                if distance <= DOCK_SNAP_DISTANCE {
                    overlays.set_panel_dock(panel, dock);
                } else {
                    // Keep it floating, but inside the overlay.
                    let max_x = (overlay_width - widget.width() as f64).max(0.0);
                    let max_y = (overlay_height - widget.height() as f64).max(0.0);
                    widget.set_margin_start((start_x + offset_x).clamp(0.0, max_x) as i32);
                    widget.set_margin_top((start_y + offset_y).clamp(0.0, max_y) as i32);
                }
            }
        ));

        self.add_controller(drag);
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

    #[test]
    fn toolbars_move_between_dock_slots_and_the_overlay() {
        // GTK has to be initialized before any widget is built. Without a display (CI, a
        // headless shell) there is nothing to check here.
        if !init_gtk() {
            return;
        }

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

        // Floating can place a panel at an exact overlay position.
        overlays.float_panel_at(ToolbarPanel::PenPicker, 40.0, 50.0);
        let penpicker = overlays.panel_widget(ToolbarPanel::PenPicker);
        assert_eq!((penpicker.margin_start(), penpicker.margin_top()), (40, 50));
        assert_eq!(penpicker.halign(), Align::Start);
        assert_eq!(penpicker.valign(), Align::Start);
    }

    #[test]
    fn locking_moves_every_toolbar_to_the_top_and_hides_handles() {
        if !init_gtk() {
            return;
        }

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

    /// A panel that is already on screen must survive being reparented, and a press on its
    /// handle must float it out of its dock.
    ///
    /// Dragging a docked panel out, and the settings that dock panels while the window is up,
    /// both reparent a mapped widget. That reparent is deferred to the next main loop iteration,
    /// so the assertions here settle the loop after each dock change.
    #[test]
    fn docks_and_floats_a_panel_that_is_on_screen() {
        if !init_gtk() {
            return;
        }

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

        // The drag a user starts on the handle floats the panel out of its dock slot: the gesture
        // sits on the overlays widget and finds the handle under the press point.
        overlays.setup_toolbars();
        overlays.set_panel_dock(ToolbarPanel::PenPicker, ToolbarDock::Left);
        settle();
        settle();

        let Some(bounds) = overlays
            .panel_handle(ToolbarPanel::PenPicker)
            .compute_bounds(&overlays)
        else {
            eprintln!("the handle has no allocation, skipping the handle drag test");
            return;
        };
        let press_x = f64::from(bounds.x() + bounds.width() / 2.0);
        let press_y = f64::from(bounds.y() + bounds.height() / 2.0);
        let drag = overlays
            .observe_controllers()
            .into_iter()
            .flatten()
            .find_map(|controller| controller.downcast::<gtk4::GestureDrag>().ok())
            .expect("the overlays widget owns the toolbar drag gesture");
        drag.emit_by_name::<()>("drag-begin", &[&press_x, &press_y]);
        settle();
        assert_eq!(panel_parent(&overlays, ToolbarPanel::PenPicker), overlay);
    }
}
