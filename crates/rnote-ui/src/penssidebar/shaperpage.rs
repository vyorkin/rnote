// Imports
use crate::{
    RnAppWindow, RnGroupedIconPicker, RnStrokeWidthPicker,
    groupediconpicker::GroupedIconPickerGroupData,
};
use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk4::{
    Button, CompositeTemplate, MenuButton, Popover, StringList, ToggleButton, Widget, glib,
    glib::clone,
};
use num_traits::cast::ToPrimitive;
use rnote_compose::builders::ShapeBuilderType;
use rnote_compose::constraints::ConstraintRatio;
use rnote_compose::style::rough::roughoptions::FillStyle;
use rnote_compose::style::smooth::{LineCap, LineStyle, SmoothOptions};
use rnote_engine::pens::pensconfig::ShaperConfig;
use rnote_engine::pens::pensconfig::shaperconfig::ShaperStyle;
use rnote_engine::strokes::{AxisEnd, AxisTicks};
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/penssidebar/shaperpage.ui")]
    pub(crate) struct RnShaperPage {
        #[template_child]
        pub(crate) shapebuildertype_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) shapebuildertype_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) shapebuildertype_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) shapebuildertype_picker: TemplateChild<RnGroupedIconPicker>,

        #[template_child]
        pub(crate) shapeconfig_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) shapeconfig_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) shapeconfig_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) shaperstyle_smooth_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) shaperstyle_rough_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) highlight_mode_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) highlight_opacity_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) pen_pressure_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_ticks_row: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub(crate) axis_tick_spacing_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) axis_tick_size_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) axis_tick_numbering_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_tick_label_every_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) axis_tick_font_size_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) axis_tick_constant_spacing_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_arrows_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_arrow_size_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) axis_labels_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_label_x_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub(crate) axis_label_y_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub(crate) axis_label_z_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub(crate) axis_third_into_page_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) axis_right_handed_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) smoothstyle_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub(crate) smoothstyle_line_cap_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) smoothstyle_line_style_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) smoothstyle_dash_length_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) smoothstyle_dash_gap_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) shaper_line_style_solid_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) shaper_line_style_dashed_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) shaper_line_style_dotted_toggle: TemplateChild<ToggleButton>,
        /// Set while the line style controls are written from the shaper config, so that the
        /// handlers they fire do not write the very same line style back into the config.
        pub(crate) line_style_syncing: Cell<bool>,
        #[template_child]
        pub(crate) roughstyle_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub(crate) roughstyle_fillstyle_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) roughstyle_hachure_angle_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) constraint_enabled_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) constraint_one_to_one_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) constraint_three_to_two_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) constraint_golden_row: TemplateChild<adw::SwitchRow>,

        #[template_child]
        pub(crate) stroke_width_picker: TemplateChild<RnStrokeWidthPicker>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnShaperPage {
        const NAME: &'static str = "RnShaperPage";
        type Type = super::RnShaperPage;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnShaperPage {
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

    impl WidgetImpl for RnShaperPage {}
}

glib::wrapper! {
    pub(crate) struct RnShaperPage(ObjectSubclass<imp::RnShaperPage>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnShaperPage {
    fn default() -> Self {
        Self::new()
    }
}

impl RnShaperPage {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn shapeconfig_menubutton(&self) -> MenuButton {
        self.imp().shapeconfig_menubutton.get()
    }

    pub(crate) fn shapebuildertype_menubutton(&self) -> MenuButton {
        self.imp().shapebuildertype_menubutton.get()
    }

    pub(crate) fn shaper_style(&self) -> Option<ShaperStyle> {
        if self.imp().shaperstyle_smooth_toggle.is_active() {
            Some(ShaperStyle::Smooth)
        } else if self.imp().shaperstyle_rough_toggle.is_active() {
            Some(ShaperStyle::Rough)
        } else {
            None
        }
    }

    pub(crate) fn set_shaper_style(&self, style: ShaperStyle) {
        match style {
            ShaperStyle::Smooth => self.imp().shaperstyle_smooth_toggle.set_active(true),
            ShaperStyle::Rough => self.imp().shaperstyle_rough_toggle.set_active(true),
        }
    }

    fn apply_shaper_style_selection(&self, appwindow: &RnAppWindow, shaper_style: ShaperStyle) {
        appwindow
            .engine_config()
            .write()
            .pens_config
            .shaper_config
            .style = shaper_style;
        self.stroke_width_picker().deselect_setters();

        match shaper_style {
            ShaperStyle::Smooth => {
                let stroke_width = appwindow
                    .engine_config()
                    .read()
                    .pens_config
                    .shaper_config
                    .smooth_options
                    .stroke_width;
                let page = self.imp();
                page.roughstyle_group.set_visible(false);
                page.smoothstyle_group.set_visible(true);
                page.stroke_width_picker.set_stroke_width(stroke_width);
            }
            ShaperStyle::Rough => {
                let stroke_width = appwindow
                    .engine_config()
                    .read()
                    .pens_config
                    .shaper_config
                    .rough_options
                    .stroke_width;
                let page = self.imp();
                page.smoothstyle_group.set_visible(false);
                page.roughstyle_group.set_visible(true);
                page.stroke_width_picker.set_stroke_width(stroke_width);
            }
        }
        self.sync_line_style(appwindow);
    }

    pub(crate) fn shapebuildertype(&self) -> Option<ShapeBuilderType> {
        let icon_name = self.imp().shapebuildertype_picker.picked()?;
        ShapeBuilderType::from_icon_name(icon_name.as_str())
    }

    pub(crate) fn set_shapebuildertype(&self, builder_type: ShapeBuilderType) {
        self.imp()
            .shapebuildertype_picker
            .set_picked(Some(builder_type.to_icon_name()));
    }

    pub(crate) fn smoothstyle_line_cap(&self) -> LineCap {
        LineCap::try_from(self.imp().smoothstyle_line_cap_row.get().selected()).unwrap()
    }

    pub(crate) fn smoothstyle_line_style(&self) -> LineStyle {
        LineStyle::try_from(self.imp().smoothstyle_line_style_row.get().selected())
            .unwrap_or_default()
    }

    /// Shows the given line style on both controls, the combo row and the quick toggles, and the
    /// lengths that go with it.
    fn set_line_style(&self, line_style: LineStyle, dash_length: f64, dash_gap: f64) {
        let imp = self.imp();
        imp.line_style_syncing.set(true);

        imp.smoothstyle_line_style_row
            .set_selected(line_style.to_u32().unwrap());
        imp.shaper_line_style_solid_toggle
            .set_active(line_style == LineStyle::Solid);
        imp.shaper_line_style_dotted_toggle
            .set_active(line_style.is_dotted());
        imp.shaper_line_style_dashed_toggle
            .set_active(line_style == LineStyle::Dashed);

        // A dashed line is the only one with dashes to measure, and a solid one has no spaces.
        imp.smoothstyle_dash_length_row
            .set_visible(line_style == LineStyle::Dashed);
        imp.smoothstyle_dash_gap_row
            .set_visible(line_style != LineStyle::Solid);
        imp.smoothstyle_dash_gap_row
            .set_title(&gettext(if line_style.is_dotted() {
                "Space Between Dots"
            } else {
                "Space Between Dashes"
            }));

        imp.smoothstyle_dash_length_row.set_value(dash_length);
        imp.smoothstyle_dash_gap_row.set_value(dash_gap);

        imp.line_style_syncing.set(false);
    }

    fn apply_line_style(&self, appwindow: &RnAppWindow, line_style: LineStyle) {
        appwindow
            .engine_config()
            .write()
            .pens_config
            .shaper_config
            .update_line_style(line_style);
        self.sync_line_style(appwindow);
    }

    /// Shows the line style of the current shape style, the cap it implies, and enables the line
    /// controls only when the style has a line at all: a rough shape and a coordinate system have
    /// none.
    fn sync_line_style(&self, appwindow: &RnAppWindow) {
        let (line_style, dash_length, dash_gap, line_cap) = {
            let config = appwindow.engine_config();
            let config = config.read();
            let smooth_options = &config.pens_config.shaper_config.smooth_options;

            (
                config.pens_config.shaper_config.line_style(),
                config.pens_config.shaper_config.dash_length(),
                config.pens_config.shaper_config.dash_gap(),
                smooth_options.line_cap,
            )
        };
        let has_line_style = line_style.is_some();
        let imp = self.imp();

        imp.line_style_syncing.set(true);
        imp.smoothstyle_line_cap_row
            .set_selected(line_cap.to_u32().unwrap());
        imp.line_style_syncing.set(false);

        imp.smoothstyle_line_cap_row.set_sensitive(has_line_style);
        imp.smoothstyle_line_style_row.set_sensitive(has_line_style);
        imp.smoothstyle_dash_length_row
            .set_sensitive(has_line_style);
        imp.smoothstyle_dash_gap_row.set_sensitive(has_line_style);
        imp.shaper_line_style_solid_toggle
            .set_sensitive(has_line_style);
        imp.shaper_line_style_dashed_toggle
            .set_sensitive(has_line_style);
        imp.shaper_line_style_dotted_toggle
            .set_sensitive(has_line_style);

        self.set_line_style(
            line_style.unwrap_or_default(),
            dash_length.unwrap_or(SmoothOptions::DEFAULT_DASH_LENGTH),
            dash_gap.unwrap_or(SmoothOptions::DEFAULT_DASH_GAP),
        );
    }

    fn connect_line_style_toggle(
        &self,
        toggle: &ToggleButton,
        line_style: LineStyle,
        appwindow: &RnAppWindow,
    ) {
        toggle.connect_toggled(clone!(
            #[weak(rename_to=shaperpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() || shaperpage.imp().line_style_syncing.get() {
                    return;
                }

                shaperpage.apply_line_style(&appwindow, line_style);
            }
        ));
    }

    pub(crate) fn roughstyle_fillstyle(&self) -> FillStyle {
        FillStyle::try_from(self.imp().roughstyle_fillstyle_row.get().selected()).unwrap()
    }

    pub(crate) fn set_roughstyle_fillstyle(&self, fill_style: FillStyle) {
        let position = fill_style.to_u32().unwrap();

        self.imp()
            .roughstyle_fillstyle_row
            .get()
            .set_selected(position);
    }

    pub(crate) fn stroke_width_picker(&self) -> RnStrokeWidthPicker {
        self.imp().stroke_width_picker.get()
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        let shapeconfig_popover = imp.shapeconfig_popover.get();
        let shapebuildertype_popover = imp.shapebuildertype_popover.get();

        // Popovers
        imp.shapeconfig_popover_close_button.connect_clicked(clone!(
            #[weak]
            shapeconfig_popover,
            move |_| {
                shapeconfig_popover.popdown();
            }
        ));

        imp.shapebuildertype_popover_close_button
            .connect_clicked(clone!(
                #[weak]
                shapebuildertype_popover,
                move |_| {
                    shapebuildertype_popover.popdown();
                }
            ));

        // Stroke width
        imp.stroke_width_picker.spinbutton().set_range(
            ShaperConfig::STROKE_WIDTH_MIN,
            ShaperConfig::STROKE_WIDTH_MAX,
        );
        // set value after the range!
        imp.stroke_width_picker
            .set_stroke_width(SmoothOptions::default().stroke_width);

        imp.stroke_width_picker.connect_notify_local(
            Some("stroke-width"),
            clone!(
                #[weak]
                appwindow,
                move |picker, _| {
                    let stroke_width = picker.stroke_width();
                    let shaper_style = appwindow
                        .engine_config()
                        .read()
                        .pens_config
                        .shaper_config
                        .style;

                    match shaper_style {
                        ShaperStyle::Smooth => {
                            let smooth_options: &mut SmoothOptions = &mut appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .shaper_config
                                .smooth_options;

                            smooth_options.stroke_width = stroke_width;
                            smooth_options.update_piet_stroke_style();
                        }
                        ShaperStyle::Rough => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .shaper_config
                                .rough_options
                                .stroke_width = stroke_width;
                        }
                    }
                }
            ),
        );

        // Shaper style
        imp.shaperstyle_smooth_toggle.connect_toggled(clone!(
            #[weak(rename_to=shaperpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(shaper_style) = shaperpage.shaper_style() else {
                    return;
                };

                shaperpage.apply_shaper_style_selection(&appwindow, shaper_style);
            }
        ));

        imp.shaperstyle_rough_toggle.connect_toggled(clone!(
            #[weak(rename_to=shaperpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(shaper_style) = shaperpage.shaper_style() else {
                    return;
                };

                shaperpage.apply_shaper_style_selection(&appwindow, shaper_style);
            }
        ));

        // Highlighter options
        imp.highlight_mode_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .highlight_mode = row.is_active();
            }
        ));

        imp.highlight_opacity_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .highlight_opacity = row.value().round() / 100.0;
            }
        ));

        // Pen pressure
        imp.pen_pressure_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .pen_pressure = row.is_active();
            }
        ));

        // Coordinate system ticks
        imp.axis_ticks_row.connect_enable_expansion_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_ticks_enabled = row.enables_expansion();
            }
        ));
        imp.axis_tick_spacing_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_tick_spacing = row.value();
            }
        ));
        imp.axis_tick_size_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_tick_size = row.value();
            }
        ));
        imp.axis_tick_numbering_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_tick_numbering = row.is_active();
            }
        ));
        imp.axis_tick_numbering_row
            .bind_property("active", &*imp.axis_tick_label_every_row, "sensitive")
            .sync_create()
            .build();
        imp.axis_tick_numbering_row
            .bind_property("active", &*imp.axis_tick_font_size_row, "sensitive")
            .sync_create()
            .build();
        imp.axis_tick_label_every_row.set_range(
            AxisTicks::LABEL_EVERY_MIN as f64,
            AxisTicks::LABEL_EVERY_MAX as f64,
        );
        imp.axis_tick_label_every_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_tick_label_every = row.value().round() as u32;
            }
        ));
        imp.axis_tick_font_size_row
            .set_range(AxisTicks::FONT_SIZE_MIN, AxisTicks::FONT_SIZE_MAX);
        imp.axis_tick_font_size_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_tick_font_size = row.value();
            }
        ));
        imp.axis_tick_constant_spacing_row
            .connect_active_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .axis_tick_constant_spacing = row.is_active();
                }
            ));

        // Ends of the coordinate system axes
        imp.axis_arrows_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_arrows = row.is_active();
            }
        ));
        imp.axis_arrows_row
            .bind_property("active", &*imp.axis_arrow_size_row, "sensitive")
            .sync_create()
            .build();
        imp.axis_arrow_size_row
            .set_range(AxisEnd::ARROW_SIZE_MIN, AxisEnd::ARROW_SIZE_MAX);
        imp.axis_arrow_size_row.connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_arrow_size = row.value();
            }
        ));
        imp.axis_labels_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_labels = row.is_active();
            }
        ));
        for (index, row) in [
            &*imp.axis_label_x_row,
            &*imp.axis_label_y_row,
            &*imp.axis_label_z_row,
        ]
        .into_iter()
        .enumerate()
        {
            imp.axis_labels_row
                .bind_property("active", row, "sensitive")
                .sync_create()
                .build();
            row.connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .axis_label_names[index] = row.text().trim().to_string();
                }
            ));
        }
        imp.axis_third_into_page_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_third_into_page = row.is_active();
            }
        ));
        imp.axis_right_handed_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .axis_right_handed = row.is_active();
            }
        ));

        // Smooth style
        // Line cap
        imp.smoothstyle_line_cap_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=shaperpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    if shaperpage.imp().line_style_syncing.get() {
                        return;
                    }

                    let line_cap = shaperpage.smoothstyle_line_cap();

                    // A straight cap cannot end in dots, which turns a dotted style back to solid.
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .smooth_options
                        .update_line_cap(line_cap);
                    shaperpage.sync_line_style(&appwindow);
                }
            ));

        // Line style
        imp.smoothstyle_line_style_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=shaperpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    if shaperpage.imp().line_style_syncing.get() {
                        return;
                    }

                    let line_style = shaperpage.smoothstyle_line_style();

                    shaperpage.apply_line_style(&appwindow, line_style);
                }
            ));
        // The quick toggles next to the tool, so that the line style can be chosen without opening
        // the popover.
        self.connect_line_style_toggle(
            &imp.shaper_line_style_solid_toggle.get(),
            LineStyle::Solid,
            appwindow,
        );
        self.connect_line_style_toggle(
            &imp.shaper_line_style_dashed_toggle.get(),
            LineStyle::Dashed,
            appwindow,
        );
        self.connect_line_style_toggle(
            &imp.shaper_line_style_dotted_toggle.get(),
            LineStyle::Dotted,
            appwindow,
        );
        imp.smoothstyle_dash_length_row.connect_changed(clone!(
            #[weak(rename_to=shaperpage)]
            self,
            #[weak]
            appwindow,
            move |row| {
                if shaperpage.imp().line_style_syncing.get() {
                    return;
                }

                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .update_dash_length(row.value());
            }
        ));
        imp.smoothstyle_dash_gap_row.connect_changed(clone!(
            #[weak(rename_to=shaperpage)]
            self,
            #[weak]
            appwindow,
            move |row| {
                if shaperpage.imp().line_style_syncing.get() {
                    return;
                }

                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .shaper_config
                    .update_dash_gap(row.value());
            }
        ));
        self.sync_line_style(appwindow);

        // Rough style
        // Fill style
        imp.roughstyle_fillstyle_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=shaperpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .rough_options
                        .fill_style = shaperpage.roughstyle_fillstyle();
                }
            ));

        // Hachure angle
        imp.roughstyle_hachure_angle_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .rough_options
                        .hachure_angle = row
                        .value()
                        .round()
                        .to_radians()
                        .clamp(-std::f64::consts::PI, std::f64::consts::PI);
                }
            ));

        // shape builder type
        imp.shapebuildertype_picker.set_groups(
            shape_builder_type_icons_get_groups(),
            shape_builder_type_icons_to_display_name,
        );

        imp.shapebuildertype_picker.connect_notify_local(
            Some("picked"),
            clone!(
                #[weak(rename_to=shaperpage)]
                self,
                #[weak]
                appwindow,
                move |picker, _| {
                    if let Some(buildertype) = shaperpage.shapebuildertype()
                        && let Some(icon_name) = picker.picked()
                    {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .builder_type = buildertype;
                        shaperpage
                            .imp()
                            .shapebuildertype_menubutton
                            .set_icon_name(&icon_name);
                        // A coordinate system draws solid axes, so its line controls go away.
                        shaperpage.sync_line_style(&appwindow);
                    }
                }
            ),
        );

        // Constraints
        imp.constraint_enabled_row
            .get()
            .connect_active_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .shaper_config
                        .constraints
                        .enabled = row.is_active();
                }
            ));

        imp.constraint_one_to_one_row
            .get()
            .connect_active_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    if row.is_active() {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .insert(ConstraintRatio::OneToOne);
                    } else {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .remove(&ConstraintRatio::OneToOne);
                    }
                }
            ));

        imp.constraint_three_to_two_row
            .get()
            .connect_active_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    if row.is_active() {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .insert(ConstraintRatio::ThreeToTwo);
                    } else {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .remove(&ConstraintRatio::ThreeToTwo);
                    }
                }
            ));

        imp.constraint_golden_row
            .get()
            .connect_active_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    if row.is_active() {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .insert(ConstraintRatio::Golden);
                    } else {
                        appwindow
                            .engine_config()
                            .write()
                            .pens_config
                            .shaper_config
                            .constraints
                            .ratios
                            .remove(&ConstraintRatio::Golden);
                    }
                }
            ));
    }

    pub(crate) fn refresh_ui(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();

        let shaper_config = appwindow
            .engine_config()
            .read()
            .pens_config
            .shaper_config
            .clone();

        self.set_shaper_style(shaper_config.style);

        match shaper_config.style {
            ShaperStyle::Smooth => {
                imp.stroke_width_picker
                    .set_stroke_width(shaper_config.smooth_options.stroke_width);
            }
            ShaperStyle::Rough => {
                imp.stroke_width_picker
                    .set_stroke_width(shaper_config.rough_options.stroke_width);
            }
        }

        // builder type
        self.set_shapebuildertype(shaper_config.builder_type);

        // Smooth style
        self.sync_line_style(appwindow);

        // Rough style
        self.set_roughstyle_fillstyle(shaper_config.rough_options.fill_style);
        imp.roughstyle_hachure_angle_row
            .set_value(shaper_config.rough_options.hachure_angle.to_degrees());

        // Highlighter opacity
        imp.highlight_mode_row
            .set_active(shaper_config.highlight_mode);
        imp.highlight_opacity_row
            .set_value((shaper_config.highlight_opacity * 100.0).round());

        // Pen pressure and coordinate system ticks
        imp.pen_pressure_row.set_active(shaper_config.pen_pressure);
        imp.axis_ticks_row
            .set_enable_expansion(shaper_config.axis_ticks_enabled);
        imp.axis_tick_spacing_row
            .set_value(shaper_config.axis_tick_spacing);
        imp.axis_tick_size_row
            .set_value(shaper_config.axis_tick_size);
        imp.axis_tick_numbering_row
            .set_active(shaper_config.axis_tick_numbering);
        imp.axis_tick_label_every_row
            .set_value(shaper_config.axis_tick_label_every as f64);
        imp.axis_tick_font_size_row
            .set_value(shaper_config.axis_tick_font_size);
        imp.axis_tick_constant_spacing_row
            .set_active(shaper_config.axis_tick_constant_spacing);
        imp.axis_arrows_row.set_active(shaper_config.axis_arrows);
        imp.axis_arrow_size_row
            .set_value(shaper_config.axis_arrow_size);
        imp.axis_labels_row.set_active(shaper_config.axis_labels);
        for (row, name) in [
            &*imp.axis_label_x_row,
            &*imp.axis_label_y_row,
            &*imp.axis_label_z_row,
        ]
        .into_iter()
        .zip(&shaper_config.axis_label_names)
        {
            // Setting the same text again would move the cursor while typing
            if row.text() != *name {
                row.set_text(name);
            }
        }
        imp.axis_third_into_page_row
            .set_active(shaper_config.axis_third_into_page);
        imp.axis_right_handed_row
            .set_active(shaper_config.axis_right_handed);

        // Constraints
        imp.constraint_enabled_row
            .set_active(shaper_config.constraints.enabled);
        imp.constraint_one_to_one_row.set_active(
            shaper_config
                .constraints
                .ratios
                .contains(&ConstraintRatio::OneToOne),
        );
        imp.constraint_three_to_two_row.set_active(
            shaper_config
                .constraints
                .ratios
                .contains(&ConstraintRatio::ThreeToTwo),
        );
        imp.constraint_golden_row.set_active(
            shaper_config
                .constraints
                .ratios
                .contains(&ConstraintRatio::Golden),
        );
    }
}

fn shape_builder_type_icons_get_groups() -> Vec<GroupedIconPickerGroupData> {
    vec![
        GroupedIconPickerGroupData {
            name: gettext("Miscellaneous"),
            icons: StringList::new(&[
                "shapebuilder-line-symbolic",
                "shapebuilder-arrow-symbolic",
                "shapebuilder-rectangle-symbolic",
                "shapebuilder-grid-symbolic",
            ]),
        },
        GroupedIconPickerGroupData {
            name: gettext("Coordinate Systems"),
            icons: StringList::new(&[
                "shapebuilder-coordsystem2d-symbolic",
                "shapebuilder-coordsystem3d-symbolic",
                "shapebuilder-quadrantcoordsystem2d-symbolic",
            ]),
        },
        GroupedIconPickerGroupData {
            name: gettext("Ellipses"),
            icons: StringList::new(&[
                "shapebuilder-ellipse-symbolic",
                "shapebuilder-fociellipse-symbolic",
            ]),
        },
        GroupedIconPickerGroupData {
            name: gettext("Curves & Paths"),
            icons: StringList::new(&[
                "shapebuilder-quadbez-symbolic",
                "shapebuilder-cubbez-symbolic",
                "shapebuilder-polyline-symbolic",
                "shapebuilder-polygon-symbolic",
            ]),
        },
    ]
}

fn shape_builder_type_icons_to_display_name(icon_name: &str) -> String {
    match ShapeBuilderType::from_icon_name(icon_name)
        .expect("ShapeBuilderTypePicker failed, display name of unknown icon name requested")
    {
        ShapeBuilderType::Arrow => gettext("Arrow"),
        ShapeBuilderType::Line => gettext("Line"),
        ShapeBuilderType::Rectangle => gettext("Rectangle"),
        ShapeBuilderType::Grid => gettext("Grid"),
        ShapeBuilderType::CoordSystem2D => gettext("2D coordinate system"),
        ShapeBuilderType::CoordSystem3D => gettext("3D coordinate system"),
        ShapeBuilderType::QuadrantCoordSystem2D => gettext("2D single quadrant coordinate system"),
        ShapeBuilderType::Ellipse => gettext("Ellipse"),
        ShapeBuilderType::FociEllipse => gettext("Ellipse with foci"),
        ShapeBuilderType::QuadBez => gettext("Quadratic bezier curve"),
        ShapeBuilderType::CubBez => gettext("Cubic bezier curve"),
        ShapeBuilderType::Polyline => gettext("Polyline"),
        ShapeBuilderType::Polygon => gettext("Polygon"),
    }
}
