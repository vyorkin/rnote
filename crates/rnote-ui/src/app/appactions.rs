// Imports
use crate::RnApp;
use adw::prelude::*;
use gtk4::{gio, glib, glib::clone};

impl RnApp {
    pub(crate) fn setup_actions(&self) {
        let action_quit = gio::SimpleAction::new("quit", None);
        self.add_action(&action_quit);

        let action_color_scheme = gio::SimpleAction::new_stateful(
            "color-scheme",
            Some(&glib::VariantType::new("s").unwrap()),
            &String::from("system").to_variant(),
        );
        self.add_action(&action_color_scheme);

        let action_new_window = gio::SimpleAction::new("new-window", None);
        self.add_action(&action_new_window);

        action_color_scheme
            .bind_property("state", &self.style_manager(), "color-scheme")
            .transform_to(move |_, val: glib::Variant| {
                match val.get::<String>().unwrap().as_str() {
                    "default" => Some(adw::ColorScheme::Default),
                    "force-light" => Some(adw::ColorScheme::ForceLight),
                    "force-dark" => Some(adw::ColorScheme::ForceDark),
                    _ => None,
                }
            })
            .transform_from(move |_, val: adw::ColorScheme| match val {
                adw::ColorScheme::Default => Some(String::from("default").to_value()),
                adw::ColorScheme::ForceLight => Some(String::from("force-light").to_value()),
                adw::ColorScheme::ForceDark => Some(String::from("force-dark").to_value()),
                _ => None,
            })
            .sync_create()
            .bidirectional()
            .build();

        // Quit App
        action_quit.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                // Request closing all windows. They then get the chance to display a save dialog on unsaved changes
                for appwindow in app.windows() {
                    appwindow.close();
                }

                if app.windows().is_empty() {
                    app.quit();
                }
            }
        ));

        action_new_window.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                app.new_appwindow_init_show();
            }
        ));
    }

    // Accelerators / Keyboard Shortcuts
    pub(crate) fn setup_action_accels(&self) {
        self.set_accels_for_action("app.quit", &["<Ctrl>q"]);
        self.set_accels_for_action("app.new-window", &["<Ctrl>n"]);

        // The zen shortcuts are settings, so they are applied here and again
        // whenever the user assigns a new combination in the settings panel.
        self.apply_zen_accels();
        if let Some(settings) = self.app_settings() {
            for key in ["zen-accel-active-monitor", "zen-accel-all-monitors"] {
                settings.connect_changed(
                    Some(key),
                    clone!(
                        #[weak(rename_to = app)]
                        self,
                        move |_, _| app.apply_zen_accels()
                    ),
                );
            }
        }
    }

    /// Apply the two zen accelerators from their settings, falling back to the
    /// schema default when a stored value is not a usable accelerator.
    fn apply_zen_accels(&self) {
        let settings = self.app_settings();
        let active_monitor = Self::zen_accel(settings.as_ref(), "zen-accel-active-monitor");
        let all_monitors = Self::zen_accel(settings.as_ref(), "zen-accel-all-monitors");
        self.set_accels_for_action("win.zen-mode-active-monitor", &[&active_monitor]);
        self.set_accels_for_action("win.zen-mode-all-monitors", &[&all_monitors]);
    }

    /// The built-in defaults, repeated here because a compiled schema from
    /// before these keys existed (an install that was not refreshed since) has
    /// no default to fall back on, and the shortcuts must still work.
    fn zen_accel_default(key: &str) -> &'static str {
        match key {
            "zen-accel-active-monitor" => "<Ctrl><Shift>m",
            _ => "<Ctrl><Shift>f",
        }
    }

    /// The accelerator a zen shortcut is bound to: the stored one when it is a
    /// usable accelerator, the schema default otherwise, and the built-in as the
    /// last resort.
    pub(crate) fn zen_accel(settings: Option<&gio::Settings>, key: &str) -> String {
        let stored = settings
            .map(|settings| settings.string(key).to_string())
            .unwrap_or_default();
        if gtk4::accelerator_parse(&stored).is_some() {
            return stored;
        }
        settings
            .and_then(|settings| settings.default_value(key))
            .and_then(|value| value.get::<String>())
            .filter(|default| gtk4::accelerator_parse(default).is_some())
            .unwrap_or_else(|| Self::zen_accel_default(key).to_string())
    }
}
