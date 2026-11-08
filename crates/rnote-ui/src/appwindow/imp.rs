// Imports
use crate::{RnMainHeader, RnOverlays, RnSidebar, config, dialogs};
use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk4::{
    Align, ArrowType, CompositeTemplate, CornerType, CssProvider, EventControllerKey, PackType,
    PadActionType, PadController, PositionType, PropagationPhase, gdk, gio, glib, glib::clone,
};
use once_cell::sync::Lazy;
use rnote_engine::document::DocumentConfig;
use rnote_engine::engine::EngineConfigShared;
use rnote_engine::pens::PenStyle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use tracing::{debug, error, trace};

#[derive(Debug, CompositeTemplate)]
#[template(resource = "/com/github/flxzt/rnote/ui/appwindow.ui")]
pub(crate) struct RnAppWindow {
    pub(crate) engine_config: EngineConfigShared,
    pub(crate) document_config_preset: RefCell<DocumentConfig>,
    pub(crate) pen_sounds: Cell<bool>,
    pub(crate) snap_positions: Cell<bool>,
    pub(crate) pen_style: Cell<PenStyle>,
    pub(crate) autosave: Cell<bool>,
    pub(crate) autosave_interval_secs: Cell<u32>,
    pub(crate) righthanded: Cell<bool>,
    pub(crate) block_pinch_zoom: Cell<bool>,
    pub(crate) respect_borders: Cell<bool>,
    pub(crate) touch_drawing: Cell<bool>,
    pub(crate) focus_mode: Cell<bool>,
    pub(crate) zen_mode: Cell<bool>,
    /// Which zen variant the user asked for: the whole desktop area, or a
    /// fullscreen window on the monitor it is on. Only read while zen mode is on.
    pub(crate) zen_span: Cell<bool>,
    /// Whether the window was fullscreened when zen mode was entered, so leaving zen
    /// restores the state it replaced.
    pub(crate) zen_restore_fullscreen: Cell<bool>,
    /// Set when fullscreen itself ends zen (F11), where restoring fullscreen on the way
    /// out would immediately undo the user's action.
    pub(crate) zen_skip_fullscreen_restore: Cell<bool>,
    /// Whether the window currently covers every monitor through the compositor.
    pub(crate) zen_spanning: Cell<bool>,
    /// The compositor state the span replaced, put back when the span ends.
    pub(crate) zen_span_restore: RefCell<Option<super::span::SpanRestore>>,
    pub(crate) devel_mode: Cell<bool>,
    pub(crate) visual_debug: Cell<bool>,

    pub(crate) lock_pen: Cell<bool>,
    pub(crate) lock_eraser: Cell<bool>,

    pub(crate) drawing_pad_controller: RefCell<Option<PadController>>,
    pub(crate) autosave_source_id: RefCell<Option<glib::SourceId>>,
    pub(crate) periodic_configsave_source_id: RefCell<Option<glib::SourceId>>,
    pub(crate) save_in_progress: Cell<bool>,
    pub(crate) save_in_progress_toast: RefCell<Option<adw::Toast>>,
    pub(crate) close_in_progress: Cell<bool>,

    #[template_child]
    pub(crate) overview: TemplateChild<adw::TabOverview>,
    #[template_child]
    pub(crate) main_header: TemplateChild<RnMainHeader>,
    #[template_child]
    pub(crate) split_view: TemplateChild<adw::OverlaySplitView>,
    #[template_child]
    pub(crate) sidebar: TemplateChild<RnSidebar>,
    #[template_child]
    pub(crate) tabbar: TemplateChild<adw::TabBar>,
    #[template_child]
    pub(crate) overlays: TemplateChild<RnOverlays>,
}

impl Default for RnAppWindow {
    fn default() -> Self {
        Self {
            engine_config: EngineConfigShared::default(),
            document_config_preset: RefCell::new(DocumentConfig::default()),
            pen_sounds: Cell::new(true),
            snap_positions: Cell::new(true),
            pen_style: Cell::new(PenStyle::default()),
            autosave: Cell::new(true),
            autosave_interval_secs: Cell::new(super::RnAppWindow::AUTOSAVE_INTERVAL_DEFAULT),
            righthanded: Cell::new(true),
            block_pinch_zoom: Cell::new(false),
            respect_borders: Cell::new(false),
            touch_drawing: Cell::new(false),
            focus_mode: Cell::new(false),
            zen_mode: Cell::new(false),
            zen_span: Cell::new(false),
            zen_restore_fullscreen: Cell::new(false),
            zen_skip_fullscreen_restore: Cell::new(false),
            zen_spanning: Cell::new(false),
            zen_span_restore: RefCell::new(None),
            devel_mode: Cell::new(false),
            visual_debug: Cell::new(false),

            lock_pen: Cell::new(false),
            lock_eraser: Cell::new(false),

            drawing_pad_controller: RefCell::new(None),
            autosave_source_id: RefCell::new(None),
            periodic_configsave_source_id: RefCell::new(None),
            save_in_progress: Cell::new(false),
            save_in_progress_toast: RefCell::new(None),
            close_in_progress: Cell::new(false),

            overview: TemplateChild::<adw::TabOverview>::default(),
            main_header: TemplateChild::<RnMainHeader>::default(),
            split_view: TemplateChild::<adw::OverlaySplitView>::default(),
            sidebar: TemplateChild::<RnSidebar>::default(),
            tabbar: TemplateChild::<adw::TabBar>::default(),
            overlays: TemplateChild::<RnOverlays>::default(),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for RnAppWindow {
    const NAME: &'static str = "RnAppWindow";
    type Type = super::RnAppWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for RnAppWindow {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        let _windowsettings = obj.settings();

        if config::PROFILE == "devel" {
            obj.add_css_class("devel");
        }

        // Load the application css
        let css = CssProvider::new();
        css.load_from_string(include_str!("../../data/style.css"));

        let display = gdk::Display::default().unwrap();
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        #[cfg(target_os = "windows")]
        {
            let css = CssProvider::new();
            css.load_from_string(&include_str!("../../data/style-win.css"));
            let display = gdk::Display::default().unwrap();
            gtk4::style_context_add_provider_for_display(
                &display,
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        self.setup_input();
        self.setup_overview();
        self.setup_split_view();
        self.setup_tabbar();

        obj.watch_zen_span_monitors();
        super::span::repair_stranded_zen_bar();

        // Zen mode is entered together with fullscreen: hiding the header in a windowed
        // state would leave the window with no buttons and nothing to drag. Leaving
        // fullscreen with F11 therefore leaves zen too, or the window would stay without
        // chrome. The property is cleared first, so the exit below does not restore the
        // fullscreen the user just gave up.
        //
        // A window that spans every monitor is not fullscreen - the compositor owns its
        // geometry - so the unfullscreen a span dispatches on entry must not end zen.
        // F11 there gives up the span, because a fullscreen window covers one output;
        // zen ends with it and the fullscreen the user asked for stays.
        obj.connect_fullscreened_notify(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            move |_| {
                if appwindow.imp().zen_spanning.get() {
                    if appwindow.is_fullscreen() {
                        appwindow.imp().zen_skip_fullscreen_restore.replace(true);
                        appwindow.set_property("zen-mode", false);
                        appwindow.imp().zen_skip_fullscreen_restore.replace(false);
                    }
                    return;
                }
                if appwindow.property::<bool>("zen-mode") && !appwindow.is_fullscreen() {
                    appwindow.imp().zen_skip_fullscreen_restore.replace(true);
                    appwindow.set_property("zen-mode", false);
                    appwindow.imp().zen_skip_fullscreen_restore.replace(false);
                }
            }
        ));

        // A window that closes while it spans must not take the bar it hid with
        // it. Restoring here, while the window is still alive, also gives the
        // compositor dispatches a window to address.
        let weak_window = obj.downgrade();
        obj.connect_close_request(move |_| {
            if let Some(appwindow) = weak_window.upgrade()
                && appwindow.imp().zen_spanning.get()
            {
                appwindow.leave_zen_span();
            }
            glib::Propagation::Proceed
        });
    }

    fn dispose(&self) {
        self.dispose_template();
        while let Some(child) = self.obj().first_child() {
            child.unparent();
        }
    }

    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![
                glib::ParamSpecBoolean::builder("pen-sounds")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("snap-positions")
                    .default_value(false)
                    .build(),
                glib::ParamSpecVariant::builder("pen-style", &PenStyle::static_variant_type())
                    .default_value(Some(&PenStyle::default().to_variant()))
                    .build(),
                glib::ParamSpecBoolean::builder("autosave")
                    .default_value(false)
                    .build(),
                glib::ParamSpecUInt::builder("autosave-interval-secs")
                    .minimum(5)
                    .maximum(u32::MAX)
                    .default_value(super::RnAppWindow::AUTOSAVE_INTERVAL_DEFAULT)
                    .build(),
                glib::ParamSpecBoolean::builder("righthanded")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("block-pinch-zoom")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("respect-borders")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("touch-drawing")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("focus-mode")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("zen-mode")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("zen-span")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("devel-mode")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("visual-debug")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("save-in-progress")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("lock-pen")
                    .default_value(false)
                    .build(),
                glib::ParamSpecBoolean::builder("lock-eraser")
                    .default_value(true)
                    .build(),
            ]
        });
        PROPERTIES.as_ref()
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "pen-sounds" => self.pen_sounds.get().to_value(),
            "snap-positions" => self.snap_positions.get().to_value(),
            "pen-style" => self.pen_style.get().to_variant().to_value(),
            "autosave" => self.autosave.get().to_value(),
            "autosave-interval-secs" => self.autosave_interval_secs.get().to_value(),
            "righthanded" => self.righthanded.get().to_value(),
            "block-pinch-zoom" => self.block_pinch_zoom.get().to_value(),
            "respect-borders" => self.respect_borders.get().to_value(),
            "touch-drawing" => self.touch_drawing.get().to_value(),
            "focus-mode" => self.focus_mode.get().to_value(),
            "zen-mode" => self.zen_mode.get().to_value(),
            "zen-span" => self.zen_span.get().to_value(),
            "devel-mode" => self.devel_mode.get().to_value(),
            "visual-debug" => self.visual_debug.get().to_value(),
            "save-in-progress" => self.save_in_progress.get().to_value(),
            "lock-pen" => self.lock_pen.get().to_value(),
            "lock-eraser" => self.lock_eraser.get().to_value(),
            _ => unimplemented!(),
        }
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        let obj = self.obj();
        match pspec.name() {
            "pen-sounds" => {
                let pen_sounds: bool = value.get().expect("The value needs to be of type `bool`");
                self.pen_sounds.replace(pen_sounds);
                if let Some(canvas) = obj.active_tab_canvas() {
                    canvas
                        .engine_mut()
                        .set_pen_sounds(pen_sounds, crate::env::pkg_data_dir().ok());
                }
            }
            "snap-positions" => {
                let snap_positions: bool =
                    value.get().expect("The value needs to be of type `bool`");
                self.snap_positions.replace(snap_positions);
                self.engine_config.write().snap_positions = snap_positions;
            }
            "pen-style" => {
                let pen_style = PenStyle::from_variant(
                    &value
                        .get()
                        .expect("The value needs to be of type `Variant`"),
                )
                .unwrap();
                self.pen_style.replace(pen_style);
                if let Some(canvas) = obj.active_tab_canvas() {
                    // don't change the style if the current style with override is already the same
                    // (e.g. when switched to from the pen button, not by clicking the pen page)
                    if pen_style != canvas.engine_ref().current_pen_style_w_override() {
                        let mut widget_flags = canvas.engine_mut().change_pen_style(pen_style);
                        widget_flags |= canvas.engine_mut().change_pen_style_override(None);
                        obj.handle_widget_flags(widget_flags, &canvas);
                    }
                }
            }
            "autosave" => {
                let autosave = value
                    .get::<bool>()
                    .expect("The value needs to be of type `bool`");

                self.autosave.replace(autosave);

                if autosave {
                    self.update_autosave_handler();
                } else if let Some(autosave_source_id) = self.autosave_source_id.borrow_mut().take()
                {
                    autosave_source_id.remove();
                }
            }
            "autosave-interval-secs" => {
                let autosave_interval_secs = value
                    .get::<u32>()
                    .expect("The value needs to be of type `u32`");

                self.autosave_interval_secs.replace(autosave_interval_secs);

                if self.autosave.get() {
                    self.update_autosave_handler();
                }
            }
            "righthanded" => {
                let righthanded = value
                    .get::<bool>()
                    .expect("The value needs to be of type `bool`");

                self.righthanded.replace(righthanded);

                self.handle_righthanded_property(righthanded);
            }
            "block-pinch-zoom" => {
                let block_pinch_zoom: bool =
                    value.get().expect("The value needs to be of type `bool`");
                self.block_pinch_zoom.replace(block_pinch_zoom);
            }
            "respect-borders" => {
                let respect_borders: bool =
                    value.get().expect("The value needs to be of type `bool`");
                self.respect_borders.replace(respect_borders);
            }
            "touch-drawing" => {
                let touch_drawing: bool =
                    value.get().expect("The value needs to be of type `bool`");
                self.touch_drawing.replace(touch_drawing);
            }
            "focus-mode" => {
                let focus_mode: bool = value.get().expect("The value needs to be of type `bool`");
                self.focus_mode.replace(focus_mode);

                self.overlays.set_toolbars_visible(!focus_mode);
            }
            "zen-mode" => {
                let zen_mode: bool = value.get().expect("The value needs to be of type `bool`");
                if self.zen_mode.replace(zen_mode) == zen_mode {
                    return;
                }

                // The header bar is the window's client-side decoration, and the tab bar
                // sits right below it. Zen keeps the toolbars, which are overlay children
                // of `RnOverlays` and not part of the window chrome.
                self.main_header.set_visible(!zen_mode);
                self.tabbar.set_visible(!zen_mode);

                if zen_mode {
                    self.zen_restore_fullscreen.replace(obj.is_fullscreen());
                    // A spanning zen covers the whole desktop when the compositor can
                    // give one window the union of all monitor geometries. Otherwise,
                    // and for the active-monitor variant, it stays a fullscreen window
                    // on the monitor the window is on.
                    if !self.zen_span.get() || !obj.enter_zen_span() {
                        obj.set_fullscreened(true);
                    }
                } else {
                    if self.zen_spanning.get() {
                        obj.leave_zen_span();
                    }
                    if !self.zen_skip_fullscreen_restore.get() {
                        obj.set_fullscreened(self.zen_restore_fullscreen.get());
                    }
                }
            }
            "zen-span" => {
                let zen_span: bool = value.get().expect("The value needs to be of type `bool`");
                if self.zen_span.replace(zen_span) == zen_span {
                    return;
                }
                if !self.zen_mode.get() {
                    return;
                }

                // The variant switched while zen mode is on: take the new one and
                // keep zen itself on.
                if zen_span {
                    if !obj.enter_zen_span() {
                        obj.set_fullscreened(true);
                    }
                } else {
                    if self.zen_spanning.get() {
                        obj.leave_zen_span();
                    }
                    obj.set_fullscreened(true);
                }
            }
            "devel-mode" => {
                let devel_mode = value
                    .get::<bool>()
                    .expect("The value needs to be of type `bool`");
                self.devel_mode.replace(devel_mode);
                let action_devel_menu = obj
                    .lookup_action("devel-menu")
                    .unwrap()
                    .downcast::<gio::SimpleAction>()
                    .unwrap();
                // Enable the devel menu action to reveal it in the app menu
                action_devel_menu.set_enabled(devel_mode);

                // Always disable visual-debugging when disabling the developer mode
                if !devel_mode {
                    debug!("Disabling developer mode, disabling visual debugging.");
                    obj.set_visual_debug(false);
                }
            }
            "visual-debug" => {
                let visual_debug = value
                    .get::<bool>()
                    .expect("The value needs to be of type `bool`");
                self.visual_debug.replace(visual_debug);
                self.engine_config.write().visual_debug = visual_debug;
                if let Some(canvas) = obj.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
            "save-in-progress" => {
                let save_in_progress = value
                    .get::<bool>()
                    .expect("The value needs to be of type `bool`");
                self.save_in_progress.replace(save_in_progress);
            }
            "lock-pen" => {
                let lock_pen: bool = value.get().expect("The value needs to be of type `bool`");
                self.lock_pen.replace(lock_pen);
            }
            "lock-eraser" => {
                let lock_eraser: bool = value.get().expect("The value needs to be of type `bool`");
                self.lock_eraser.replace(lock_eraser);
            }
            _ => unimplemented!(),
        }
    }
}

impl WidgetImpl for RnAppWindow {}

impl WindowImpl for RnAppWindow {
    fn close_request(&self) -> glib::Propagation {
        let obj = self.obj().to_owned();
        if self.close_in_progress.get() {
            return glib::Propagation::Stop;
        }

        if obj.tabs_any_saves_in_progress() {
            obj.connect_notify_local(Some("save-in-progress"), move |appwindow, _| {
                if !appwindow.save_in_progress() {
                    if appwindow.tabs_any_unsaved_changes() {
                        appwindow.imp().close_in_progress.set(false);
                        appwindow.main_header().headerbar().set_sensitive(true);
                        appwindow.sidebar().headerbar().set_sensitive(true);
                        if let Some(toast) = appwindow.imp().save_in_progress_toast.take() {
                            toast.dismiss();
                        }

                        glib::spawn_future_local(clone!(
                            #[weak]
                            appwindow,
                            async move {
                                dialogs::dialog_close_window(&appwindow).await;
                            }
                        ));
                    } else {
                        appwindow.close_force();
                    }
                }
            });
            self.close_in_progress.set(true);
            self.main_header.headerbar().set_sensitive(false);
            self.sidebar.headerbar().set_sensitive(false);
            obj.overlays().dispatch_toast_text_singleton(
                &gettext("Saves are in progress, waiting before closing..."),
                None,
                &mut self.save_in_progress_toast.borrow_mut(),
            );
        } else if obj.tabs_any_unsaved_changes() {
            glib::spawn_future_local(clone!(
                #[weak(rename_to=appwindow)]
                obj,
                async move {
                    dialogs::dialog_close_window(&appwindow).await;
                }
            ));
        } else {
            obj.close_force();
        }

        // Inhibit (Overwrite) the default handler. This handler is then responsible for destroying the window.
        glib::Propagation::Stop
    }
}

impl ApplicationWindowImpl for RnAppWindow {}
impl AdwWindowImpl for RnAppWindow {}
impl AdwApplicationWindowImpl for RnAppWindow {}

impl RnAppWindow {
    fn update_autosave_handler(&self) {
        let obj = self.obj();

        if let Some(removed_id) = self.autosave_source_id.borrow_mut().replace(
            glib::source::timeout_add_seconds_local(
                self.autosave_interval_secs.get(),
                clone!(#[weak(rename_to=appwindow)] obj, #[upgrade_or] glib::ControlFlow::Break, move || {
                    // save for all tabs opened in the current window that have unsaved changes
                    let tabs = appwindow.get_all_tabs();

                    for (i, tab) in tabs.iter().enumerate() {
                        let canvas = tab.canvas();
                        if canvas.unsaved_changes() && let Some(output_file) = canvas.output_file() {
                            trace!(
                                "there are unsaved changes on the tab {:?} with a file on disk, saving",i
                            );
                            glib::spawn_future_local(clone!(#[weak] canvas, #[weak] appwindow ,async move {
                                if let Err(e) = canvas.save_document_to_file(&output_file).await {
                                    error!("Saving document failed, Err: `{e:?}`");
                                    canvas.set_output_file(None);
                                    appwindow
                                        .overlays()
                                        .dispatch_toast_error(&gettext("Saving document failed"));
                                };
                            }));
                        }
                    }

                    glib::ControlFlow::Continue
                }),
            ),
        ) {
            removed_id.remove();
        }
    }

    fn setup_input(&self) {
        let obj = self.obj();
        let drawing_pad_controller = PadController::new(&*obj, None);

        drawing_pad_controller.set_action(
            PadActionType::Button,
            0,
            -1,
            &gettext("Button 1"),
            "drawing-pad-pressed-button-0",
        );
        drawing_pad_controller.set_action(
            PadActionType::Button,
            1,
            -1,
            &gettext("Button 2"),
            "drawing-pad-pressed-button-1",
        );
        drawing_pad_controller.set_action(
            PadActionType::Button,
            2,
            -1,
            &gettext("Button 3"),
            "drawing-pad-pressed-button-2",
        );
        drawing_pad_controller.set_action(
            PadActionType::Button,
            3,
            -1,
            &gettext("Button 4"),
            "drawing-pad-pressed-button-3",
        );

        obj.add_controller(drawing_pad_controller.clone());
        self.drawing_pad_controller
            .replace(Some(drawing_pad_controller));

        // Space held turns a pen or mouse drag on the canvas into a view pan. It is
        // tracked at the window, because key events go to the focused widget and after
        // clicking a toolbar button that is not the canvas. A text entry keeps its
        // space: only the canvas treats the held key as a pan modifier.
        let space_pan_controller = EventControllerKey::builder()
            .name("space_pan_controller")
            .propagation_phase(PropagationPhase::Capture)
            .build();
        space_pan_controller.connect_key_pressed(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let blocking_modifiers = gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::HYPER_MASK
                    | gdk::ModifierType::META_MASK;

                if key != gdk::Key::space || modifiers.intersects(blocking_modifiers) {
                    return glib::Propagation::Proceed;
                }

                // A text entry types a space. The typewriter's text box keeps focus on
                // the canvas, so its state is checked separately.
                if focus_is_text_input(&appwindow) {
                    return glib::Propagation::Proceed;
                }

                // Every canvas is armed: a split view has more than one, and the drag
                // may be started in either of them.
                for wrapper in appwindow.get_all_tabs() {
                    wrapper.canvas().set_space_pan_held(true);
                }
                glib::Propagation::Stop
            }
        ));
        space_pan_controller.connect_key_released(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            move |_, key, _, _| {
                if key == gdk::Key::space {
                    for wrapper in appwindow.get_all_tabs() {
                        wrapper.canvas().set_space_pan_held(false);
                    }
                }
            }
        ));
        obj.add_controller(space_pan_controller);

        // `Ctrl`+`C` held gives the lasso (the selector) without switching the tool for
        // good: the style is a hold layer that disappears when `Ctrl` is released. A
        // change that arrives in the middle of a stroke waits for the stroke to end, so
        // the stroke is neither cancelled nor drawn with the wrong pen. What the lasso
        // selected is copied when the hold ends, so the clipboard still gets the
        // selection this key is known for; `Ctrl`+`C` pressed on an already selected
        // selector copies at once, as it always did.
        let lasso_hold = Rc::new(Cell::new(false));
        let lasso_hold_controller = EventControllerKey::builder()
            .name("lasso_hold_controller")
            .propagation_phase(PropagationPhase::Capture)
            .build();
        lasso_hold_controller.connect_key_pressed(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            #[strong]
            lasso_hold,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if !modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    || !matches!(key.to_unicode(), Some('c' | 'C'))
                {
                    return glib::Propagation::Proceed;
                }

                // A text entry copies its text. The typewriter's text box keeps focus on
                // the canvas, so its state is checked separately.
                if focus_is_text_input(&appwindow) {
                    return glib::Propagation::Proceed;
                }

                // The selector is already the current pen and holds a selection: this is
                // the ordinary copy, not a request for the lasso.
                let copies_selection = appwindow.active_tab_canvas().is_some_and(|canvas| {
                    let engine = canvas.engine_ref();
                    engine.current_pen_style_w_override() == PenStyle::Selector
                        && engine.has_selection()
                });
                if copies_selection {
                    if gtk4::prelude::WidgetExt::activate_action(
                        &appwindow,
                        "win.clipboard-copy",
                        None,
                    )
                    .is_ok()
                    {
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Proceed;
                }

                for wrapper in appwindow.get_all_tabs() {
                    let canvas = wrapper.canvas();
                    let widget_flags = canvas
                        .engine_mut()
                        .change_pen_style_hold(Some(PenStyle::Selector));
                    appwindow.handle_widget_flags(widget_flags, &canvas);
                }
                lasso_hold.set(true);
                glib::Propagation::Stop
            }
        ));
        lasso_hold_controller.connect_key_released(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            #[strong]
            lasso_hold,
            move |_, key, _, _| {
                if !matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) || !lasso_hold.get() {
                    return;
                }
                lasso_hold.set(false);

                // The lasso is copied as the hold ends, because this very chord no longer
                // reaches the copy action. A stroke still down copies nothing: its
                // selection only exists once it is finished.
                let lasso_selection = appwindow.active_tab_canvas().is_some_and(|canvas| {
                    let engine = canvas.engine_ref();
                    !engine.pen_state_down() && engine.has_selection()
                });
                if lasso_selection {
                    // The copy reads the selection from the pen, so the hold is released
                    // only once it is in the clipboard. Dropping it first reinstalls the
                    // pen and drops the selection.
                    glib::spawn_future_local(clone!(#[weak] appwindow, async move {
                        appwindow.copy_clipboard_content().await;

                        for wrapper in appwindow.get_all_tabs() {
                            let canvas = wrapper.canvas();
                            let widget_flags = canvas.engine_mut().change_pen_style_hold(None);
                            appwindow.handle_widget_flags(widget_flags, &canvas);
                        }
                    }));
                    return;
                }

                for wrapper in appwindow.get_all_tabs() {
                    let canvas = wrapper.canvas();
                    let widget_flags = canvas.engine_mut().change_pen_style_hold(None);
                    appwindow.handle_widget_flags(widget_flags, &canvas);
                }
            }
        ));
        obj.add_controller(lasso_hold_controller);

        // A Space held while the window is left has no release delivered here, so the
        // flag would stay set and the next drag would pan unexpectedly. A held `Ctrl`+`C`
        // has the same problem: its release goes to the newly focused window.
        obj.connect_notify_local(
            Some("is-active"),
            clone!(
                #[weak(rename_to=appwindow)]
                obj,
                #[strong]
                lasso_hold,
                move |obj, _| {
                    if !obj.is_active() {
                        lasso_hold.set(false);
                        for wrapper in appwindow.get_all_tabs() {
                            let canvas = wrapper.canvas();
                            canvas.set_space_pan_held(false);
                            let widget_flags = canvas.engine_mut().reset_pen_style_hold();
                            appwindow.handle_widget_flags(widget_flags, &canvas);
                        }
                    }
                }
            ),
        );

        // Escape is left to the canvas alone: it ends text editing, cancels a selection and
        // returns a non-brush pen to the brush, and it does not leave zen mode either way.
        // Zen is left with `<Ctrl><Shift>f`, F11 or the compositor, which all keep the way back
        // to the hidden header explicit.
    }

    fn setup_overview(&self) {
        self.overview.set_view(Some(&self.overlays.tabview()));

        let obj = self.obj();

        // Create new tab via tab overview
        self.overview.connect_create_tab(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            #[upgrade_or_panic]
            move |_| {
                let wrapper = appwindow.new_canvas_wrapper();
                appwindow.append_wrapper_new_tab(&wrapper)
            }
        ));
    }

    fn setup_tabbar(&self) {
        self.tabbar.set_view(Some(&self.overlays.tabview()));
    }

    fn setup_split_view(&self) {
        let obj = self.obj();
        let split_view = self.split_view.get();
        let left_sidebar_reveal_toggle = obj.main_header().left_sidebar_reveal_toggle();
        let right_sidebar_reveal_toggle = obj.main_header().right_sidebar_reveal_toggle();

        left_sidebar_reveal_toggle
            .bind_property("active", &right_sidebar_reveal_toggle, "active")
            .sync_create()
            .bidirectional()
            .build();

        left_sidebar_reveal_toggle
            .bind_property("active", &split_view, "show-sidebar")
            .sync_create()
            .bidirectional()
            .build();
        right_sidebar_reveal_toggle
            .bind_property("active", &split_view, "show-sidebar")
            .sync_create()
            .bidirectional()
            .build();

        let update_widgets = move |split_view: &adw::OverlaySplitView,
                                   appwindow: &super::RnAppWindow| {
            let sidebar_position = split_view.sidebar_position();
            let sidebar_collapsed = split_view.is_collapsed();
            let sidebar_shown = split_view.shows_sidebar();

            let sidebar_appmenu_visibility = !sidebar_collapsed && sidebar_shown;
            let sidebar_left_close_button_visibility =
                (sidebar_position == PackType::End) && sidebar_collapsed && sidebar_shown;
            let sidebar_right_close_button_visibility =
                (sidebar_position == PackType::Start) && sidebar_collapsed && sidebar_shown;

            appwindow
                .main_header()
                .appmenu()
                .set_visible(!sidebar_appmenu_visibility);
            appwindow
                .sidebar()
                .appmenu()
                .set_visible(sidebar_appmenu_visibility);
            appwindow
                .sidebar()
                .left_close_button()
                .set_visible(sidebar_left_close_button_visibility);
            appwindow
                .sidebar()
                .right_close_button()
                .set_visible(sidebar_right_close_button_visibility);

            if sidebar_position == PackType::End {
                appwindow
                    .sidebar()
                    .left_close_button()
                    .set_icon_name("dir-right-symbolic");
                appwindow
                    .sidebar()
                    .right_close_button()
                    .set_icon_name("dir-right-symbolic");
            } else {
                appwindow
                    .sidebar()
                    .left_close_button()
                    .set_icon_name("dir-left-symbolic");
                appwindow
                    .sidebar()
                    .right_close_button()
                    .set_icon_name("dir-left-symbolic");
            }
        };

        let sidebar_expanded_shown = Rc::new(Cell::new(false));

        self.split_view.connect_show_sidebar_notify(clone!(
            #[strong]
            sidebar_expanded_shown,
            #[weak(rename_to=appwindow)]
            obj,
            move |split_view| {
                if !split_view.is_collapsed() {
                    sidebar_expanded_shown.set(split_view.shows_sidebar());
                }
                update_widgets(split_view, &appwindow);
            }
        ));

        self.split_view.connect_sidebar_position_notify(clone!(
            #[weak(rename_to=appwindow)]
            obj,
            move |split_view| {
                update_widgets(split_view, &appwindow);
            }
        ));

        self.split_view.connect_collapsed_notify(clone!(
            #[strong]
            sidebar_expanded_shown,
            #[weak(rename_to=appwindow)]
            obj,
            move |split_view| {
                if split_view.is_collapsed() {
                    // Always hide sidebar when transitioning from non-collapsed to collapsed.
                    split_view.set_show_sidebar(false);
                } else {
                    // show sidebar again when it was shown before it was collapsed
                    if sidebar_expanded_shown.get() {
                        split_view.set_show_sidebar(true);
                    }
                    // update the shown state for when the sidebar was toggled shown in the collapsed state
                    sidebar_expanded_shown.set(split_view.shows_sidebar());
                }
                update_widgets(split_view, &appwindow);
            }
        ));
    }

    fn handle_righthanded_property(&self, righthanded: bool) {
        let obj = self.obj();

        if righthanded {
            obj.split_view().set_sidebar_position(PackType::Start);
            obj.main_header()
                .left_sidebar_reveal_toggle()
                .set_visible(true);
            obj.main_header()
                .right_sidebar_reveal_toggle()
                .set_visible(false);

            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().workspacesbar());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().corner_filler());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().dir_box());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().files_scroller());
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().corner_filler(),
                0,
                0,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().workspacesbar(),
                0,
                1,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().dir_box(),
                2,
                0,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().files_scroller(),
                2,
                1,
                1,
                1,
            );
            obj.sidebar()
                .workspacebrowser()
                .files_scroller()
                .set_placement(CornerType::TopRight);
            obj.sidebar()
                .workspacebrowser()
                .workspacesbar()
                .workspaces_scroller()
                .set_placement(CornerType::TopRight);

            obj.sidebar()
                .settings_panel()
                .settings_scroller()
                .set_placement(CornerType::TopRight);

            if obj.overlays().panel_dock(crate::ToolbarPanel::PensSideBar)
                == crate::ToolbarDock::Floating
            {
                obj.overlays().sidebar_box().set_halign(Align::Start);
            }
            obj.overlays()
                .sidebar_scroller()
                .set_placement(CornerType::TopRight);
            obj.overlays()
                .penssidebar()
                .brush_page()
                .brushconfig_menubutton()
                .set_direction(ArrowType::Right);
            obj.overlays()
                .penssidebar()
                .brush_page()
                .stroke_width_picker()
                .set_position(PositionType::Left);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .shapeconfig_menubutton()
                .set_direction(ArrowType::Right);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .shapebuildertype_menubutton()
                .set_direction(ArrowType::Right);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .stroke_width_picker()
                .set_position(PositionType::Left);
            obj.overlays()
                .penssidebar()
                .typewriter_page()
                .emojichooser_menubutton()
                .set_direction(ArrowType::Right);
            obj.overlays()
                .penssidebar()
                .eraser_page()
                .stroke_width_picker()
                .set_position(PositionType::Left);
            obj.overlays()
                .penssidebar()
                .tools_page()
                .verticalspace_menubutton()
                .set_direction(ArrowType::Right);
        } else {
            obj.split_view().set_sidebar_position(PackType::End);
            obj.main_header()
                .left_sidebar_reveal_toggle()
                .set_visible(false);
            obj.main_header()
                .right_sidebar_reveal_toggle()
                .set_visible(true);

            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().files_scroller());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().dir_box());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().corner_filler());
            obj.sidebar()
                .workspacebrowser()
                .grid()
                .remove(&obj.sidebar().workspacebrowser().workspacesbar());
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().dir_box(),
                0,
                0,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().files_scroller(),
                0,
                1,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().corner_filler(),
                2,
                0,
                1,
                1,
            );
            obj.sidebar().workspacebrowser().grid().attach(
                &obj.sidebar().workspacebrowser().workspacesbar(),
                2,
                1,
                1,
                1,
            );
            obj.sidebar()
                .workspacebrowser()
                .files_scroller()
                .set_placement(CornerType::TopLeft);
            obj.sidebar()
                .workspacebrowser()
                .workspacesbar()
                .workspaces_scroller()
                .set_placement(CornerType::TopLeft);

            obj.sidebar()
                .settings_panel()
                .settings_scroller()
                .set_placement(CornerType::TopLeft);

            if obj.overlays().panel_dock(crate::ToolbarPanel::PensSideBar)
                == crate::ToolbarDock::Floating
            {
                obj.overlays().sidebar_box().set_halign(Align::End);
            }
            obj.overlays()
                .sidebar_scroller()
                .set_placement(CornerType::TopLeft);
            obj.overlays()
                .penssidebar()
                .brush_page()
                .brushconfig_menubutton()
                .set_direction(ArrowType::Left);
            obj.overlays()
                .penssidebar()
                .brush_page()
                .stroke_width_picker()
                .set_position(PositionType::Right);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .shapeconfig_menubutton()
                .set_direction(ArrowType::Left);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .shapebuildertype_menubutton()
                .set_direction(ArrowType::Left);
            obj.overlays()
                .penssidebar()
                .shaper_page()
                .stroke_width_picker()
                .set_position(PositionType::Right);
            obj.overlays()
                .penssidebar()
                .typewriter_page()
                .emojichooser_menubutton()
                .set_direction(ArrowType::Left);
            obj.overlays()
                .penssidebar()
                .eraser_page()
                .stroke_width_picker()
                .set_position(PositionType::Right);
            obj.overlays()
                .penssidebar()
                .tools_page()
                .verticalspace_menubutton()
                .set_direction(ArrowType::Left);
        }
    }
}

/// Whether focus is in a text entry, where a bare key types instead of acting on the
/// canvas. The typewriter's text box keeps focus on the canvas, so its state is
/// checked separately from the widget type.
fn focus_is_text_input(appwindow: &super::RnAppWindow) -> bool {
    let focus = gtk4::prelude::RootExt::focus(appwindow);
    if focus
        .as_ref()
        .is_some_and(|focus| focus.is::<gtk4::Editable>())
    {
        return true;
    }
    focus
        .and_then(|focus| focus.downcast::<crate::RnCanvas>().ok())
        .is_some_and(|canvas| canvas.engine_ref().penholder.is_handling_text_input())
}
