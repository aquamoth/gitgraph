//! The desktop's light or dark preference where winit can't tell. On Windows and macOS winit
//! reports the system theme and its changes, and egui follows them by itself. On Linux and the
//! BSDs winit reports nothing, so egui's "follow system" always came out dark. There the
//! preference comes from the XDG desktop portal (`org.freedesktop.appearance color-scheme`,
//! provided by GNOME, KDE and others): read once at startup, then kept up to date from the
//! portal's `SettingChanged` signal, so that switching to or from night mode shows up at once.
//! Both go through the `gdbus` tool rather than a D-Bus library, which would add some 45 crates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{self, Theme};

/// The desktop's latest preference, if known.
#[derive(Debug, Default)]
pub struct SystemTheme {
    state: Arc<AtomicU8>,
    /// The `gdbus monitor` process reporting changes, stopped on drop.
    #[cfg(all(unix, not(target_os = "macos")))]
    monitor: Option<std::process::Child>,
}

// `state` holds 0 while the preference is unknown.
const LIGHT: u8 = 1;
const DARK: u8 = 2;

impl SystemTheme {
    /// Reads the desktop's preference now (so the first frame already has the right theme)
    /// and keeps watching it, repainting `ctx` when it changes. Where winit knows the system
    /// theme (Windows, macOS) this does nothing and [`Self::get`] stays `None`.
    pub fn watch(ctx: &egui::Context) -> SystemTheme {
        #[allow(unused_mut)]
        let mut theme = SystemTheme::default();
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            theme.monitor = portal::watch(ctx, &theme.state);
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = ctx;
        theme
    }

    pub fn get(&self) -> Option<Theme> {
        match self.state.load(Ordering::Relaxed) {
            LIGHT => Some(Theme::Light),
            DARK => Some(Theme::Dark),
            _ => None,
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl Drop for SystemTheme {
    fn drop(&mut self) {
        // (Should parterre die without dropping this, the monitor still ends at the portal's
        // next signal, when it finds nobody reading its output.)
        if let Some(mut monitor) = self.monitor.take() {
            let _ = monitor.kill();
            let _ = monitor.wait();
        }
    }
}

#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn encode(theme: Theme) -> u8 {
    match theme {
        Theme::Light => LIGHT,
        Theme::Dark => DARK,
    }
}

/// Parses the portal's `color-scheme` value in `gdbus` output: `(<uint32 1>,)` from
/// `ReadOne`, `(<<uint32 1>>,)` from the older `Read`, or the tail of a signal line.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn parse_color_scheme(output: &str) -> Option<Theme> {
    let (_, rest) = output.split_once("uint32 ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<u32>().ok()? {
        1 => Some(Theme::Dark),
        // 0 is "no preference", which GNOME shows as its default (light) style.
        0 | 2 => Some(Theme::Light),
        _ => None,
    }
}

/// Parses a line of `gdbus monitor` output, if it announces a new `color-scheme`, e.g.
/// `/org/freedesktop/portal/desktop: org.freedesktop.portal.Settings.SettingChanged
/// ('org.freedesktop.appearance', 'color-scheme', <uint32 1>)`.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn parse_signal(line: &str) -> Option<Theme> {
    let (_, value) = line.split_once(
        "org.freedesktop.portal.Settings.SettingChanged ('org.freedesktop.appearance', \
         'color-scheme', ",
    )?;
    parse_color_scheme(value)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod portal {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    use eframe::egui::{self, Theme};

    use super::{encode, parse_color_scheme, parse_signal};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";

    /// Stores the current preference in `state` and starts following its changes. Returns the
    /// monitor process, or `None` if there is no portal (or no `gdbus`), leaving it to winit
    /// and egui.
    pub fn watch(ctx: &egui::Context, state: &Arc<AtomicU8>) -> Option<Child> {
        // Start listening before reading, so that no change falls in between.
        let mut monitor = Command::new("gdbus")
            .args([
                "monitor",
                "--session",
                "--dest",
                DEST,
                "--object-path",
                PATH,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut lines = BufReader::new(monitor.stdout.take()?).lines();
        // Its first line ("Monitoring signals on ...") says it is listening.
        let current = lines.next().and_then(Result::ok).and_then(|_| read());
        let Some(current) = current else {
            let _ = monitor.kill();
            let _ = monitor.wait();
            return None;
        };
        state.store(encode(current), Ordering::Relaxed);
        let state = state.clone();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("system-theme".into())
            .spawn(move || {
                // Ends when the monitor does.
                for theme in lines.map_while(Result::ok).filter_map(|l| parse_signal(&l)) {
                    let new = encode(theme);
                    if state.swap(new, Ordering::Relaxed) != new {
                        ctx.request_repaint();
                    }
                }
            });
        Some(monitor)
    }

    fn read() -> Option<Theme> {
        ["ReadOne", "Read"].into_iter().find_map(|method| {
            let output = Command::new("gdbus")
                .args([
                    "call",
                    "--session",
                    "--timeout",
                    "1",
                    "--dest",
                    DEST,
                    "--object-path",
                    PATH,
                    "--method",
                    &format!("org.freedesktop.portal.Settings.{method}"),
                    "org.freedesktop.appearance",
                    "color-scheme",
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| parse_color_scheme(&String::from_utf8_lossy(&output.stdout)))
                .flatten()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_portal_replies() {
        assert_eq!(parse_color_scheme("(<uint32 1>,)\n"), Some(Theme::Dark));
        assert_eq!(parse_color_scheme("(<<uint32 2>>,)\n"), Some(Theme::Light));
    }

    #[test]
    fn no_preference_is_light() {
        assert_eq!(parse_color_scheme("(<uint32 0>,)\n"), Some(Theme::Light));
    }

    #[test]
    fn unknown_replies_are_ignored() {
        assert_eq!(parse_color_scheme("(<uint32 7>,)"), None);
        assert_eq!(parse_color_scheme("Error: No such interface"), None);
    }

    #[test]
    fn parses_color_scheme_signals_only() {
        let signal = |group: &str, key: &str, value: &str| {
            format!(
                "/org/freedesktop/portal/desktop: org.freedesktop.portal.Settings.SettingChanged \
                 ('{group}', '{key}', {value})"
            )
        };
        assert_eq!(
            parse_signal(&signal(
                "org.freedesktop.appearance",
                "color-scheme",
                "<uint32 1>"
            )),
            Some(Theme::Dark)
        );
        assert_eq!(
            parse_signal(&signal(
                "org.freedesktop.appearance",
                "accent-color",
                "<(0.2, 0.5, 0.9)>"
            )),
            None
        );
        assert_eq!(
            parse_signal(&signal(
                "org.gnome.desktop.interface",
                "cursor-size",
                "<uint32 24>"
            )),
            None
        );
        assert_eq!(
            parse_signal("The name org.freedesktop.portal.Desktop is owned by :1.42"),
            None
        );
    }
}
