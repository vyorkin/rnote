//! The zen accelerators match by keycode as well as by keyval.
//!
//! A GTK accelerator names a keyval, and a keyval belongs to the layout that is
//! active: with `ru` on, the F key types `а`, so `<Ctrl><Shift>f` never matches
//! and the shortcut is dead until the layout is switched back - which the
//! `Ctrl+Shift+Space` that switches it sits on the shortcut's own modifiers to
//! do. The keycode does not depend on the layout, so the window watches it too
//! and activates the action when a keyval match cannot happen.

use super::RnAppWindow;
use crate::RnApp;
use gtk4::{EventControllerKey, PropagationPhase, gdk, gio, glib, glib::clone, prelude::*};

/// The setting holding the accelerator that enters zen on the active monitor.
pub(crate) const ZEN_ACCEL_ACTIVE_MONITOR: &str = "zen-accel-active-monitor";
/// The setting holding the keycode recorded with that accelerator.
pub(crate) const ZEN_ACCEL_ACTIVE_MONITOR_KEYCODE: &str = "zen-accel-active-monitor-keycode";
/// The setting holding the accelerator that enters zen over all monitors.
pub(crate) const ZEN_ACCEL_ALL_MONITORS: &str = "zen-accel-all-monitors";
/// The setting holding the keycode recorded with that accelerator.
pub(crate) const ZEN_ACCEL_ALL_MONITORS_KEYCODE: &str = "zen-accel-all-monitors-keycode";

impl RnAppWindow {
    /// Watch the keycodes of the two zen combinations.
    ///
    /// The controller only fires where GTK's own accelerator matching cannot: a
    /// keyval the active layout spells otherwise. Everything else is left to the
    /// accelerator, so a layout that already works keeps working and the action
    /// is never activated twice.
    pub(crate) fn setup_zen_accel_keycodes(&self) {
        let controller = EventControllerKey::builder()
            .name("zen_accel_keycode_controller")
            .propagation_phase(PropagationPhase::Capture)
            .build();
        controller.connect_key_pressed(clone!(
            #[weak(rename_to = appwindow)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, keycode, state| {
                let Some(settings) = appwindow.app().app_settings() else {
                    return glib::Propagation::Proceed;
                };
                let offset = keycode_offset();
                for (accel_key, keycode_key, action) in [
                    (
                        ZEN_ACCEL_ACTIVE_MONITOR,
                        ZEN_ACCEL_ACTIVE_MONITOR_KEYCODE,
                        "zen-mode-active-monitor",
                    ),
                    (
                        ZEN_ACCEL_ALL_MONITORS,
                        ZEN_ACCEL_ALL_MONITORS_KEYCODE,
                        "zen-mode-all-monitors",
                    ),
                ] {
                    let accel = RnApp::zen_accel(Some(&settings), accel_key);
                    let Some(accel) = gtk4::accelerator_parse(&accel) else {
                        continue;
                    };
                    let recorded = recorded_keycode(&settings, keycode_key);
                    if combination_matches(accel, recorded, keyval, keycode, state, offset) {
                        // An action window that went away has nothing to report.
                        let _ = WidgetExt::activate_action(&appwindow, action, None);
                        return glib::Propagation::Stop;
                    }
                }
                glib::Propagation::Proceed
            }
        ));
        self.add_controller(controller);
    }
}

/// Whether a key event is the combination by its keycode.
///
/// `accel` is the parsed combination, `recorded` the keycode the settings panel
/// saw when the combination was recorded (`0` when there is none), `offset` the
/// platform's keycode offset.
///
/// The keycode of a combination that names a letter or a digit is known from the
/// US-QWERTY positions, so a recorded keycode only serves a key the table does
/// not carry: a value left over from an older combination cannot make the wrong
/// key fire.
fn combination_matches(
    accel: (gdk::Key, gdk::ModifierType),
    recorded: u32,
    keyval: gdk::Key,
    keycode: u32,
    state: gdk::ModifierType,
    offset: u32,
) -> bool {
    let (accel_keyval, accel_mods) = accel;

    // GTK resolves this event on its own when the keyval is the accelerator's.
    if keyval == accel_keyval {
        return false;
    }
    // A layout that spells both keys in ASCII is the kind GTK's matching serves:
    // the accelerator works there already, and a second activation would undo it.
    if is_ascii_keyval(keyval) && is_ascii_keyval(accel_keyval) {
        return false;
    }
    if state & combination_modifiers() != accel_mods & combination_modifiers() {
        return false;
    }

    let expected =
        us_keycode(accel_keyval, offset).or_else(|| (recorded > 1).then_some(recorded));
    expected == Some(keycode)
}

/// The modifiers a combination can ask for. Caps Lock, Num Lock and the like are
/// not part of a combination, so a key caught with one of them on still matches.
fn combination_modifiers() -> gdk::ModifierType {
    gdk::ModifierType::CONTROL_MASK
        | gdk::ModifierType::SHIFT_MASK
        | gdk::ModifierType::ALT_MASK
        | gdk::ModifierType::SUPER_MASK
        | gdk::ModifierType::HYPER_MASK
        | gdk::ModifierType::META_MASK
}

/// Whether the keyval types an ASCII character. A Latin layout spells its keys
/// in ASCII, and there GTK's accelerator matching is the one that works.
fn is_ascii_keyval(keyval: gdk::Key) -> bool {
    keyval.to_unicode().is_some_and(|c| c.is_ascii())
}

/// The keycode of a keyval on the US-QWERTY positions, which is the key a
/// combination names: `<Ctrl><Shift>f` is the F key, wherever the layout puts
/// its own letters. `None` for a key the table does not carry; such a key keeps
/// being matched by keyval alone, exactly as before.
fn us_keycode(keyval: gdk::Key, offset: u32) -> Option<u32> {
    let position = match keyval.to_unicode()?.to_ascii_lowercase() {
        'q' => 16,
        'w' => 17,
        'e' => 18,
        'r' => 19,
        't' => 20,
        'y' => 21,
        'u' => 22,
        'i' => 23,
        'o' => 24,
        'p' => 25,
        'a' => 30,
        's' => 31,
        'd' => 32,
        'f' => 33,
        'g' => 34,
        'h' => 35,
        'j' => 36,
        'k' => 37,
        'l' => 38,
        'z' => 44,
        'x' => 45,
        'c' => 46,
        'v' => 47,
        'b' => 48,
        'n' => 49,
        'm' => 50,
        '1' => 2,
        '2' => 3,
        '3' => 4,
        '4' => 5,
        '5' => 6,
        '6' => 7,
        '7' => 8,
        '8' => 9,
        '9' => 10,
        '0' => 11,
        _ => return None,
    };
    Some(position + offset)
}

/// GDK reports the keycode the platform's own: the evdev code on Wayland, the X
/// keycode on X11, which is the evdev code plus eight.
fn keycode_offset() -> u32 {
    match gdk::Display::default() {
        Some(display) if display.name().starts_with("wayland") => 0,
        _ => 8,
    }
}

/// Whether the compiled schema carries the key. A schema that predates the key
/// (an install that installs the app without installing its schema) leaves the
/// shortcut on the keyval path instead of failing the whole call.
fn has_key(settings: &gio::Settings, key: &str) -> bool {
    settings
        .settings_schema()
        .is_some_and(|schema| schema.has_key(key))
}

/// The keycode recorded with a combination, `0` when nothing was recorded or the
/// compiled schema predates the key.
pub(crate) fn recorded_keycode(settings: &gio::Settings, key: &str) -> u32 {
    if !has_key(settings, key) {
        return 0;
    }
    u32::try_from(settings.int(key)).unwrap_or(0)
}

/// Record the keycode of a combination next to the combination itself, so the
/// same physical key keeps working in a layout that spells it otherwise.
pub(crate) fn set_recorded_keycode(settings: &gio::Settings, key: &str, keycode: u32) {
    if !has_key(settings, key) {
        return;
    }
    let _ = settings.set_int(key, keycode.min(i32::MAX as u32) as i32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::translate::FromGlib;

    fn keyval(code: u32) -> gdk::Key {
        unsafe { gdk::Key::from_glib(code) }
    }

    // The keyvals a key press carries. `0x66` is `f` in a Latin layout, `0x6c6`
    // is Cyrillic `ф`, which is what the same F key types with `ru` on.
    const LATIN_F: u32 = 0x66;
    const LATIN_UPPER_F: u32 = 0x46;
    const LATIN_U: u32 = 0x75;
    const CYRILLIC_EF: u32 = 0x6c6;

    fn ctrl_shift() -> gdk::ModifierType {
        gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK
    }

    /// `Ctrl`+`Shift`+`F` as GTK parses it: the keyval is lowercased and the
    /// shift is part of the modifiers.
    fn ctrl_shift_f() -> (gdk::Key, gdk::ModifierType) {
        (keyval(LATIN_F), ctrl_shift())
    }

    #[test]
    fn the_russian_spelling_of_the_key_still_fires_the_shortcut() {
        assert!(combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            33,
            ctrl_shift(),
            0
        ));
    }

    #[test]
    fn the_latin_layout_keeps_the_accelerator_it_already_had() {
        // Pressed in a Latin layout, GTK's own matching handles the key.
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(LATIN_UPPER_F),
            33,
            ctrl_shift(),
            0
        ));
    }

    #[test]
    fn a_latin_layout_that_puts_another_letter_on_the_key_is_left_alone() {
        // Dvorak types `u` on the F key: the combination names `f`, and firing
        // here would put a second key on the shortcut.
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(LATIN_U),
            33,
            ctrl_shift(),
            0
        ));
    }

    #[test]
    fn another_key_with_the_same_modifiers_does_not_fire() {
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            34,
            ctrl_shift(),
            0
        ));
    }

    #[test]
    fn a_combination_needs_exactly_its_modifiers() {
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            33,
            gdk::ModifierType::CONTROL_MASK,
            0
        ));
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            33,
            ctrl_shift() | gdk::ModifierType::ALT_MASK,
            0
        ));
    }

    #[test]
    fn lock_modifiers_are_not_part_of_a_combination() {
        assert!(combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            33,
            ctrl_shift() | gdk::ModifierType::LOCK_MASK,
            0
        ));
    }

    #[test]
    fn the_platform_offset_lands_on_the_same_key() {
        // X11 reports the evdev keycode plus eight.
        assert!(combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            41,
            ctrl_shift(),
            8
        ));
        assert!(!combination_matches(
            ctrl_shift_f(),
            0,
            keyval(CYRILLIC_EF),
            33,
            ctrl_shift(),
            8
        ));
    }

    #[test]
    fn a_recorded_keycode_serves_a_combination_the_table_does_not_carry() {
        // `Ctrl`+`Shift`+`Ф`: no US-QWERTY position, so the recorded keycode is
        // the only thing that can match it.
        let accel = (keyval(0x6e6), ctrl_shift());

        assert!(!combination_matches(accel, 0, keyval(LATIN_UPPER_F), 33, ctrl_shift(), 0));
        assert!(combination_matches(accel, 33, keyval(LATIN_UPPER_F), 33, ctrl_shift(), 0));
        assert!(!combination_matches(accel, 33, keyval(LATIN_UPPER_F), 34, ctrl_shift(), 0));
    }

    #[test]
    fn a_table_key_ignores_a_stale_recorded_keycode() {
        // The setting was edited by hand, the recorded keycode is the old key's.
        assert!(!combination_matches(
            ctrl_shift_f(),
            37,
            keyval(CYRILLIC_EF),
            37,
            ctrl_shift(),
            0
        ));
        assert!(combination_matches(
            ctrl_shift_f(),
            37,
            keyval(CYRILLIC_EF),
            33,
            ctrl_shift(),
            0
        ));
    }
}
