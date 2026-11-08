//! Zen mode across every monitor.
//!
//! A Wayland client cannot place or resize its own window, so a zen view that
//! covers all monitors is arranged through the compositor. Hyprland's IPC can
//! make the window floating and give it the union of the output geometries, and
//! it renders that one surface across the seam. Compositors without that IPC (or
//! a single-monitor desktop) fall back to a fullscreen zen window on one output.
//!
//! The state taken over here is remembered per window, so leaving the span puts
//! the geometry, the floating state and the decoration back as they were.
//!
//! The Omarchy bar is a layer surface above a floating window, so the span hides
//! it through Omarchy's own toggle and shows it again on exit, unless it was
//! already hidden when the span began. A window that closes while spanning
//! restores the bar too, before the compositor forgets its surface.
//!
//! The compositor dispatch is a request, not a guarantee: after it the window is
//! checked against the wanted geometry once, retried once and only then accepted,
//! so a lost race falls back to the fullscreen zen instead of a half-applied
//! span with the bar gone.

use super::RnAppWindow;
use glib::subclass::prelude::*;
use gtk4::{gdk, glib, glib::clone, prelude::*};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, warn};

/// The window state the compositor had before zen took it over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpanRestore {
    floating: bool,
    at: (i32, i32),
    size: (i32, i32),
    /// Set when the span hid a bar that was visible, so only that bar is shown
    /// again; a bar the user had hidden stays hidden.
    hid_bar: bool,
}

/// The union of every monitor's geometry, in layout coordinates.
#[derive(Debug, Clone, Copy)]
struct SpanRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct HyprClient {
    address: String,
    #[serde(default)]
    pid: i32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    at: [i32; 2],
    #[serde(default)]
    size: [i32; 2],
    #[serde(default)]
    floating: bool,
    #[serde(default)]
    fullscreen: i32,
}

impl RnAppWindow {
    /// Watch the monitor list and re-span when it changes, so plugging a monitor
    /// in or out while in zen keeps the window on the whole desktop area.
    pub(crate) fn watch_zen_span_monitors(&self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        display.monitors().connect_items_changed(clone!(
            #[weak(rename_to = appwindow)]
            self,
            move |_, _, _, _| appwindow.reapply_zen_span()
        ));
    }

    /// Make the window cover every monitor. Returns `false` when the compositor
    /// cannot do it, in which case the caller falls back to a fullscreen zen.
    pub(crate) fn enter_zen_span(&self) -> bool {
        let Some(rect) = monitors_union() else {
            return false;
        };
        let Some(client) = own_client(self) else {
            debug!("zen span: the window was not found in the compositor's clients");
            return false;
        };

        let imp = self.imp();
        *imp.zen_span_restore.borrow_mut() = Some(SpanRestore {
            floating: client.floating,
            at: (client.at[0], client.at[1]),
            size: (client.size[0], client.size[1]),
            hid_bar: false,
        });
        imp.zen_spanning.set(true);

        // The span is one compositor transaction that the compositor can also
        // lose to a competing layout change, so ask once and check the result.
        let mut spans =
            self.dispatch_zen_span(&client, &rect) && span_in_place(&rect, &client.address);
        if !spans {
            std::thread::sleep(std::time::Duration::from_millis(120));
            spans = self.dispatch_zen_span(&client, &rect) && span_in_place(&rect, &client.address);
        }
        if !spans {
            warn!("zen span: the compositor did not keep the window on the whole desktop area");
            self.leave_zen_span();
            return false;
        }

        // Only hide the bar once the span itself is in place, so a refused span
        // never leaves the desktop without its bar.
        if !bar_hidden() && set_bar_hidden(true) {
            if let Some(restore) = imp.zen_span_restore.borrow_mut().as_mut() {
                restore.hid_bar = true;
            }
        }

        true
    }

    /// Ask the compositor for the span itself: unfullscreen, float, resize, move,
    /// drop the border.
    fn dispatch_zen_span(&self, client: &HyprClient, rect: &SpanRect) -> bool {
        let address = format!("address:{}", client.address);
        // A fullscreen window cannot be moved or resized, and the span relies on
        // both. The unfullscreen notification this dispatch causes arrives after
        // `zen_spanning` is set, so the fullscreen handler ignores it.
        let left_fullscreen = client.fullscreen == 0
            && !self.is_fullscreen()
            || dispatch(
                "fullscreen",
                &format!(
                    "hl.dsp.window.fullscreen({{ mode = \"fullscreen\", action = \"unset\", window = \"{address}\" }})"
                ),
            );
        left_fullscreen
            && dispatch(
                "float",
                &format!(
                    "hl.dsp.window.float({{ action = \"set\", window = \"{address}\" }})"
                ),
            )
            && dispatch(
                "resize",
                &format!(
                    "hl.dsp.window.resize({{ x = {}, y = {}, window = \"{address}\" }})",
                    rect.width, rect.height
                ),
            )
            && dispatch(
                "move",
                &format!(
                    "hl.dsp.window.move({{ x = {}, y = {}, window = \"{address}\" }})",
                    rect.x, rect.y
                ),
            )
            && dispatch(
                "decorate",
                &format!(
                    "hl.dsp.window.set_prop({{ prop = \"decorate\", value = \"0\", window = \"{address}\" }})"
                ),
            )
    }

    /// Put the window back where the compositor had it before the span.
    pub(crate) fn leave_zen_span(&self) {
        let imp = self.imp();
        imp.zen_spanning.set(false);

        let Some(restore) = imp.zen_span_restore.borrow_mut().take() else {
            return;
        };
        if restore.hid_bar {
            set_bar_hidden(false);
        }
        let Some(client) = own_client(self) else {
            warn!("zen span: the window was not found in the compositor's clients");
            return;
        };

        let address = format!("address:{}", client.address);
        // The window was decorated when it was handed over. "unset" drops the
        // override instead of pinning a value, so window rules and the config
        // decide the decoration again.
        dispatch(
            "decorate",
            &format!(
                "hl.dsp.window.set_prop({{ prop = \"decorate\", value = \"unset\", window = \"{address}\" }})"
            ),
        );
        if restore.floating {
            dispatch(
                "resize",
                &format!(
                    "hl.dsp.window.resize({{ x = {}, y = {}, window = \"{address}\" }})",
                    restore.size.0, restore.size.1
                ),
            );
            dispatch(
                "move",
                &format!(
                    "hl.dsp.window.move({{ x = {}, y = {}, window = \"{address}\" }})",
                    restore.at.0, restore.at.1
                ),
            );
        } else {
            // A tiled window goes back into the layout it was taken out of.
            dispatch(
                "float",
                &format!("hl.dsp.window.float({{ action = \"unset\", window = \"{address}\" }})"),
            );
        }
    }

    /// Follow a monitor change while the window spans the desktop.
    pub(crate) fn reapply_zen_span(&self) {
        if !self.imp().zen_spanning.get() {
            return;
        }
        let Some(rect) = monitors_union() else {
            // The last monitor went away: a single fullscreen is the closest
            // zen left, and it needs no span.
            self.leave_zen_span();
            self.set_fullscreened(true);
            return;
        };
        let Some(client) = own_client(self) else {
            return;
        };
        let address = format!("address:{}", client.address);
        dispatch(
            "resize",
            &format!(
                "hl.dsp.window.resize({{ x = {}, y = {}, window = \"{address}\" }})",
                rect.width, rect.height
            ),
        );
        dispatch(
            "move",
            &format!(
                "hl.dsp.window.move({{ x = {}, y = {}, window = \"{address}\" }})",
                rect.x, rect.y
            ),
        );
    }
}

/// A compositor that never answers the span is normal off Hyprland; on Hyprland
/// it is a fault worth reporting.
fn report(problem: &str) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        warn!("{problem}");
    } else {
        debug!("{problem}");
    }
}

/// Run a dispatcher through `hyprctl`, which speaks Lua on the Hyprland this
/// fork targets. `true` only when the compositor answered "ok".
fn dispatch(name: &str, request: &str) -> bool {
    match Command::new("hyprctl").args(["dispatch", request]).output() {
        Ok(output) if output.status.success() => {
            let answer = String::from_utf8_lossy(&output.stdout);
            if answer.trim() == "ok" {
                true
            } else {
                report(&format!(
                    "zen span: `{name}` dispatch refused: {}{}",
                    answer.trim(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
                false
            }
        }
        Ok(output) => {
            report(&format!(
                "zen span: `{name}` dispatch failed: {}{}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            false
        }
        Err(err) => {
            report(&format!(
                "zen span: `{name}` dispatch could not run hyprctl: {err}"
            ));
            false
        }
    }
}

fn clients() -> Option<Vec<HyprClient>> {
    let output = Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Whether the compositor reports the window floating over the whole span area.
/// Hyprland can round the requested size by a pixel, so positions and sizes are
/// compared within a hair.
fn span_in_place(rect: &SpanRect, address: &str) -> bool {
    let near = |a: i32, b: i32| (a - b).abs() <= 2;
    let Some(client) =
        clients().and_then(|all| all.into_iter().find(|client| client.address == *address))
    else {
        return false;
    };
    client.floating
        && near(client.at[0], rect.x)
        && near(client.at[1], rect.y)
        && near(client.size[0], rect.width)
        && near(client.size[1], rect.height)
}

/// The compositor's view of this window. A process can hold several app windows,
/// so when the pid alone is ambiguous the title (and last, focus) decides.
fn own_client(window: &RnAppWindow) -> Option<HyprClient> {
    let pid = std::process::id() as i32;
    let all = clients()?;
    let candidates: Vec<&HyprClient> = all.iter().filter(|client| client.pid == pid).collect();

    match candidates.as_slice() {
        [] => None,
        [only] => Some((*only).clone()),
        many => {
            if let Some(active) = active_client() {
                if let Some(client) = many.iter().find(|client| client.address == active) {
                    return Some((*client).clone());
                }
            }
            let title = window
                .title()
                .map(|title| title.to_string())
                .unwrap_or_default();
            many.iter()
                .find(|client| !title.is_empty() && client.title == title)
                .map(|client| (*client).clone())
                .or_else(|| many.first().map(|client| (*client).clone()))
        }
    }
}

fn active_client() -> Option<String> {
    let output = Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let active: HyprClient = serde_json::from_slice(&output.stdout).ok()?;
    Some(active.address)
}

/// Omarchy's flag for a hidden bar.
fn bar_flag() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/state/omarchy/toggles/bar-off"))
}

fn bar_hidden() -> bool {
    bar_flag().is_some_and(|flag| flag.exists())
}

/// Hide or show the Omarchy bar the way its own toggle does. Omarchy's script
/// also nudges the shell to re-read the flag, which the file watcher can miss
/// when a span begins and ends in quick succession; without it the flag alone
/// is still what the bar watches.
fn set_bar_hidden(hidden: bool) -> bool {
    // The flag names the bar, not its state: `on` writes a `bar-off` flag, which
    // hides the bar, and `off` removes it.
    let action = if hidden { "on" } else { "off" };
    if Command::new("omarchy-toggle-bar")
        .arg(action)
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return true;
    }

    let Some(flag) = bar_flag() else {
        return false;
    };
    if hidden {
        flag.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::File::create(&flag).is_ok()
    } else {
        std::fs::remove_file(&flag).is_ok() || !flag.exists()
    }
}

/// The bounding box of every monitor, in layout coordinates: one window that
/// covers it covers the whole desktop area.
fn monitors_union() -> Option<SpanRect> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    if monitors.n_items() < 2 {
        return None;
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for index in 0..monitors.n_items() {
        let monitor = monitors.item(index)?.downcast::<gdk::Monitor>().ok()?;
        let geometry = monitor.geometry();
        if geometry.width() <= 0 || geometry.height() <= 0 {
            return None;
        }
        min_x = min_x.min(geometry.x());
        min_y = min_y.min(geometry.y());
        max_x = max_x.max(geometry.x() + geometry.width());
        max_y = max_y.max(geometry.y() + geometry.height());
    }

    Some(SpanRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}
