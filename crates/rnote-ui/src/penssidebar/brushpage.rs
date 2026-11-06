// Imports
use crate::{RnAppWindow, RnStrokeWidthPicker};
use adw::prelude::*;
use gtk4::{
    Button, CompositeTemplate, ListBox, MenuButton, Popover, ToggleButton, Widget, glib,
    glib::clone, subclass::prelude::*,
};
use num_traits::cast::ToPrimitive;
use rnote_compose::builders::PenPathBuilderType;
use rnote_compose::style::PressureCurve;
use rnote_compose::style::smooth::LineStyle;
use rnote_compose::style::textured::{TexturedDotsDistribution, TexturedOptions};
use rnote_engine::pens::pensconfig::BrushConfig;
use rnote_engine::pens::pensconfig::brushconfig::{BrushStyle, FountainOptions, SolidOptions};

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/penssidebar/brushpage.ui")]
    pub(crate) struct RnBrushPage {
        #[template_child]
        pub(crate) brushstyle_solid_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushstyle_marker_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushstyle_textured_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushstyle_fountain_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushconfig_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) brushconfig_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) brushconfig_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) brush_buildertype_listbox: TemplateChild<ListBox>,
        #[template_child]
        pub(crate) brush_buildertype_simple: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) brush_buildertype_curved: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) brush_buildertype_modeled: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) solidstyle_pressure_curves_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) brush_line_style_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub(crate) brush_line_style_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) brush_line_style_solid_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brush_line_style_dashed_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brush_line_style_dotted_toggle: TemplateChild<ToggleButton>,
        /// Set while the line style controls are written from the brush config, so that the
        /// handlers they fire do not write the very same line style back into the config.
        pub(crate) line_style_syncing: std::cell::Cell<bool>,
        #[template_child]
        pub(crate) texturedstyle_density_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) texturedstyle_distribution_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) fountainstyle_pressure_influence_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) fountainstyle_speed_influence_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) fountainstyle_taper_length_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) stroke_width_picker: TemplateChild<RnStrokeWidthPicker>,
        #[template_child]
        pub(crate) ruler_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) ruler_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) ruler_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) ruler_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) ruler_snap_distance_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_angle_snap_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) ruler_show_dial_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) ruler_width_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_tick_spacing_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_body_opacity_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_scroll_step_row: TemplateChild<adw::SpinRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnBrushPage {
        const NAME: &'static str = "RnBrushPage";
        type Type = super::RnBrushPage;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnBrushPage {
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

    impl WidgetImpl for RnBrushPage {}
}

glib::wrapper! {
    pub(crate) struct RnBrushPage(ObjectSubclass<imp::RnBrushPage>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnBrushPage {
    fn default() -> Self {
        Self::new()
    }
}

impl RnBrushPage {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn brushconfig_menubutton(&self) -> MenuButton {
        self.imp().brushconfig_menubutton.get()
    }

    pub(crate) fn brush_style(&self) -> Option<BrushStyle> {
        if self.imp().brushstyle_marker_toggle.is_active() {
            Some(BrushStyle::Marker)
        } else if self.imp().brushstyle_solid_toggle.is_active() {
            Some(BrushStyle::Solid)
        } else if self.imp().brushstyle_textured_toggle.is_active() {
            Some(BrushStyle::Textured)
        } else if self.imp().brushstyle_fountain_toggle.is_active() {
            Some(BrushStyle::Fountain)
        } else {
            None
        }
    }

    pub(crate) fn set_brush_style(&self, brush_style: BrushStyle) {
        match brush_style {
            BrushStyle::Marker => self.imp().brushstyle_marker_toggle.set_active(true),
            BrushStyle::Solid => self.imp().brushstyle_solid_toggle.set_active(true),
            BrushStyle::Textured => self.imp().brushstyle_textured_toggle.set_active(true),
            BrushStyle::Fountain => self.imp().brushstyle_fountain_toggle.set_active(true),
        }
    }

    fn apply_brush_style_selection(&self, appwindow: &RnAppWindow, brush_style: BrushStyle) {
        appwindow
            .engine_config()
            .write()
            .pens_config
            .brush_config
            .style = brush_style;
        self.stroke_width_picker().deselect_setters();

        let stroke_width = appwindow
            .engine_config()
            .read()
            .pens_config
            .brush_config
            .stroke_width();
        self.imp()
            .stroke_width_picker
            .set_stroke_width(stroke_width);
        self.sync_line_style(appwindow);
    }

    pub(crate) fn buildertype(&self) -> Option<PenPathBuilderType> {
        PenPathBuilderType::try_from(
            self.imp().brush_buildertype_listbox.selected_row()?.index() as u32
        )
        .ok()
    }

    pub(crate) fn set_buildertype(&self, buildertype: PenPathBuilderType) {
        match buildertype {
            PenPathBuilderType::Simple => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_simple)),
            PenPathBuilderType::Curved => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_curved)),
            PenPathBuilderType::Modeled => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_modeled)),
        }
    }

    pub(crate) fn solidstyle_pressure_curve(&self) -> PressureCurve {
        PressureCurve::try_from(self.imp().solidstyle_pressure_curves_row.get().selected()).unwrap()
    }

    pub(crate) fn set_solidstyle_pressure_curve(&self, pressure_curve: PressureCurve) {
        let position = pressure_curve.to_u32().unwrap();

        self.imp()
            .solidstyle_pressure_curves_row
            .get()
            .set_selected(position);
    }

    pub(crate) fn texturedstyle_dots_distribution(&self) -> TexturedDotsDistribution {
        TexturedDotsDistribution::try_from(
            self.imp().texturedstyle_distribution_row.get().selected(),
        )
        .unwrap()
    }

    pub(crate) fn set_texturedstyle_distribution_variant(
        &self,
        distribution: TexturedDotsDistribution,
    ) {
        let position = distribution.to_u32().unwrap();

        self.imp()
            .texturedstyle_distribution_row
            .get()
            .set_selected(position);
    }

    pub(crate) fn stroke_width_picker(&self) -> RnStrokeWidthPicker {
        self.imp().stroke_width_picker.get()
    }

    pub(crate) fn line_style(&self) -> LineStyle {
        LineStyle::try_from(self.imp().brush_line_style_row.get().selected()).unwrap_or_default()
    }

    /// Shows the given line style on both controls, the combo row and the quick toggles.
    ///
    /// The three quick toggles stand for the line styles they can draw as an icon, so every
    /// dashed style shows as the dashed one: the variants are chosen in the combo row.
    fn set_line_style(&self, line_style: LineStyle) {
        let imp = self.imp();
        imp.line_style_syncing.set(true);

        imp.brush_line_style_row
            .get()
            .set_selected(line_style.to_u32().unwrap());
        imp.brush_line_style_solid_toggle
            .set_active(line_style == LineStyle::Solid);
        imp.brush_line_style_dotted_toggle
            .set_active(line_style.is_dotted());
        imp.brush_line_style_dashed_toggle
            .set_active(line_style != LineStyle::Solid && !line_style.is_dotted());

        imp.line_style_syncing.set(false);
    }

    fn apply_line_style(&self, appwindow: &RnAppWindow, line_style: LineStyle) {
        appwindow
            .engine_config()
            .write()
            .pens_config
            .brush_config
            .update_line_style(line_style);
        self.set_line_style(line_style);
    }

    /// Shows the line style of the current brush style, and enables the controls only when that
    /// style has one: a textured stroke is a texture, not a line.
    fn sync_line_style(&self, appwindow: &RnAppWindow) {
        let line_style = appwindow
            .engine_config()
            .read()
            .pens_config
            .brush_config
            .line_style();
        let has_line_style = line_style.is_some();
        let imp = self.imp();

        imp.brush_line_style_group.set_sensitive(has_line_style);
        imp.brush_line_style_solid_toggle.set_sensitive(has_line_style);
        imp.brush_line_style_dashed_toggle.set_sensitive(has_line_style);
        imp.brush_line_style_dotted_toggle.set_sensitive(has_line_style);

        self.set_line_style(line_style.unwrap_or_default());
    }

    fn connect_line_style_toggle(
        &self,
        toggle: &ToggleButton,
        line_style: LineStyle,
        appwindow: &RnAppWindow,
    ) {
        toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() || brushpage.imp().line_style_syncing.get() {
                    return;
                }

                brushpage.apply_line_style(&appwindow, line_style);
            }
        ));
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        let brushconfig_popover = imp.brushconfig_popover.get();

        // Popovers
        imp.brushconfig_popover_close_button.connect_clicked(clone!(
            #[weak]
            brushconfig_popover,
            move |_| {
                brushconfig_popover.popdown();
            }
        ));

        // Stroke width
        imp.stroke_width_picker
            .spinbutton()
            .set_range(BrushConfig::STROKE_WIDTH_MIN, BrushConfig::STROKE_WIDTH_MAX);
        // set value after the range!
        imp.stroke_width_picker
            .set_stroke_width(SolidOptions::default().stroke_width);

        imp.stroke_width_picker.connect_notify_local(
            Some("stroke-width"),
            clone!(
                #[weak]
                appwindow,
                move |picker, _| {
                    let stroke_width = picker.stroke_width();
                    let brush_style = appwindow
                        .engine_config()
                        .read()
                        .pens_config
                        .brush_config
                        .style;

                    match brush_style {
                        BrushStyle::Marker => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .marker_options
                                .stroke_width = stroke_width;
                        }
                        BrushStyle::Solid => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .solid_options
                                .stroke_width = stroke_width;
                        }
                        BrushStyle::Textured => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .textured_options
                                .stroke_width = stroke_width;
                        }
                        BrushStyle::Fountain => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .fountain_options
                                .stroke_width = stroke_width;
                        }
                    }
                }
            ),
        );

        // Line style
        imp.brush_line_style_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=brushpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    if brushpage.imp().line_style_syncing.get() {
                        return;
                    }

                    let line_style = brushpage.line_style();
                    brushpage.apply_line_style(&appwindow, line_style);
                }
            ));
        self.connect_line_style_toggle(
            &imp.brush_line_style_solid_toggle.get(),
            LineStyle::Solid,
            appwindow,
        );
        self.connect_line_style_toggle(
            &imp.brush_line_style_dashed_toggle.get(),
            LineStyle::DashedEquidistant,
            appwindow,
        );
        self.connect_line_style_toggle(
            &imp.brush_line_style_dotted_toggle.get(),
            LineStyle::Dotted,
            appwindow,
        );
        self.sync_line_style(appwindow);

        // Style
        imp.brushstyle_marker_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        imp.brushstyle_solid_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        imp.brushstyle_textured_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        imp.brushstyle_fountain_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        // Builder type
        imp.brush_buildertype_listbox.connect_row_selected(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |_, _| {
                let Some(buildertype) = brushpage.buildertype() else {
                    return;
                };
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .builder_type = buildertype;
            }
        ));

        // Solid style
        // Pressure curve
        imp.solidstyle_pressure_curves_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=brushpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .solid_options
                        .pressure_curve = brushpage.solidstyle_pressure_curve();
                }
            ));

        // Textured style
        // Density
        imp.texturedstyle_density_row
            .get()
            .set_range(TexturedOptions::DENSITY_MIN, TexturedOptions::DENSITY_MAX);
        // set value after the range!
        imp.texturedstyle_density_row
            .get()
            .set_value(TexturedOptions::default().density);

        imp.texturedstyle_density_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .textured_options
                    .density = row.value();
            }
        ));

        // dots distribution
        imp.texturedstyle_distribution_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=brushpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .textured_options
                        .distribution = brushpage.texturedstyle_dots_distribution();
                }
            ));

        // Fountain style
        // Pressure influence
        imp.fountainstyle_pressure_influence_row
            .get()
            .set_value(FountainOptions::default().pressure_influence);
        imp.fountainstyle_pressure_influence_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .fountain_options
                        .pressure_influence = row.value();
                }
            ));

        // Speed influence
        imp.fountainstyle_speed_influence_row
            .get()
            .set_value(FountainOptions::default().speed_influence);
        imp.fountainstyle_speed_influence_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .fountain_options
                        .speed_influence = row.value();
                }
            ));

        // Taper length
        imp.fountainstyle_taper_length_row
            .get()
            .set_value(FountainOptions::default().taper_length);
        imp.fountainstyle_taper_length_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .fountain_options
                        .taper_length = row.value();
                }
            ));

        // Ruler toggle: enables/disables the on-canvas ruler. The first time the
        // user enables it, seed the ruler position to the current viewport center.
        imp.ruler_toggle.connect_toggled(clone!(
            #[weak]
            appwindow,
            move |toggle| {
                let visible = toggle.is_active();
                let needs_seed = {
                    let mut config = appwindow.engine_config().write();
                    let ruler = &mut config.pens_config.brush_config.ruler_config;
                    ruler.visible = visible;
                    visible && ruler.anchor == p2d::math::Vector2::ZERO
                };
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    if needs_seed {
                        // The ruler position is in window coordinates — seed it to the center
                        // of the visible area, which is what the canvas wrapper covers.
                        let center_window = appwindow
                            .active_tab_wrapper()
                            .map(|w| {
                                p2d::math::Vector2::new(w.width() as f64, w.height() as f64) * 0.5
                            })
                            .unwrap_or_else(|| canvas.engine_ref().camera.size() * 0.5);
                        let mut c = appwindow.engine_config().write();
                        let r = &mut c.pens_config.brush_config.ruler_config;
                        r.anchor = center_window;
                        r.dial_pos = center_window;
                    }
                    canvas.queue_draw();
                }
            }
        ));

        let ruler_popover = imp.ruler_popover.get();
        imp.ruler_popover_close_button.connect_clicked(clone!(
            #[weak]
            ruler_popover,
            move |_| {
                ruler_popover.popdown();
            }
        ));

        imp.ruler_snap_distance_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .snap_distance = row.value();
            }
        ));

        imp.ruler_angle_snap_row.get().connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .angle_snap_enabled = row.is_active();
            }
        ));

        imp.ruler_show_dial_row.get().connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .show_dial = row.is_active();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        // Ruler width: the UI exposes the full width; storage uses the half-width.
        imp.ruler_width_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .body_half_width = row.value() * 0.5;
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_tick_spacing_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .tick_spacing = row.value();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_body_opacity_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .body_opacity = row.value();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_scroll_step_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .scroll_rotation_step_deg = row.value();
            }
        ));
    }

    pub(crate) fn refresh_ui(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        let brush_config = appwindow
            .engine_config()
            .read()
            .pens_config
            .brush_config
            .clone();

        self.set_solidstyle_pressure_curve(brush_config.solid_options.pressure_curve);
        imp.texturedstyle_density_row
            .set_value(brush_config.textured_options.density);
        self.set_texturedstyle_distribution_variant(brush_config.textured_options.distribution);

        imp.fountainstyle_pressure_influence_row
            .set_value(brush_config.fountain_options.pressure_influence);
        imp.fountainstyle_speed_influence_row
            .set_value(brush_config.fountain_options.speed_influence);
        imp.fountainstyle_taper_length_row
            .set_value(brush_config.fountain_options.taper_length);

        self.set_brush_style(brush_config.style);
        self.set_buildertype(brush_config.builder_type);

        imp.stroke_width_picker
            .set_stroke_width(brush_config.stroke_width());
        self.sync_line_style(appwindow);

        imp.ruler_toggle
            .set_active(brush_config.ruler_config.visible);
        imp.ruler_snap_distance_row
            .set_value(brush_config.ruler_config.snap_distance);
        imp.ruler_angle_snap_row
            .set_active(brush_config.ruler_config.angle_snap_enabled);
        imp.ruler_show_dial_row
            .set_active(brush_config.ruler_config.show_dial);
        imp.ruler_width_row
            .set_value(brush_config.ruler_config.body_half_width * 2.0);
        imp.ruler_tick_spacing_row
            .set_value(brush_config.ruler_config.tick_spacing);
        imp.ruler_body_opacity_row
            .set_value(brush_config.ruler_config.body_opacity);
        imp.ruler_scroll_step_row
            .set_value(brush_config.ruler_config.scroll_rotation_step_deg);
    }
}
