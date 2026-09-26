//! Workaround for the Wayland freeze of egui#5145
//! (<https://github.com/emilk/egui/issues/5145>): with vsync on, Mesa's (and NVIDIA's) EGL
//! swap waits for the compositor's frame callback, which never comes for a minimized or hidden
//! window. All viewports share one thread, so when the Settings window (or the main window
//! while Settings is open) is minimized, the whole app stops. With swap interval 0 the swap no
//! longer waits, so on Wayland vsync is turned off and [`FrameLimiter`] caps the frame rate
//! instead (a sleep: `request_repaint_after` caps nothing, since other code keeps calling
//! `request_repaint`). Details: `docs/research/wayland-viewport-freeze.md` on the branch
//! `research/wayland-viewport-freeze`.
//!
//! Remove the cap (and turn vsync back on) once <https://github.com/emilk/egui/pull/8631>,
//! which makes eframe itself avoid the blocking swap on Wayland, ships in a released eframe.

use std::ffi::OsString;
use std::time::{Duration, Instant};

/// Shortest time between two frames while vsync is off: about 120 frames per second.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// Whether parterre runs in a Wayland session, where vsync must be off. Windows, macOS and
/// X11 sessions keep vsync on.
pub fn wayland_session() -> bool {
    cfg!(all(unix, not(target_os = "macos"))) && is_wayland_session(|name| std::env::var_os(name))
}

/// Whether the environment `var` describes a Wayland session: winit picks Wayland when
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty. `XDG_SESSION_TYPE=wayland`
/// also counts, so that X11 forced inside a Wayland session (XWayland) gets the workaround too;
/// without it a hidden window slows it to a crawl.
fn is_wayland_session(var: impl Fn(&str) -> Option<OsString>) -> bool {
    let set = |name| var(name).is_some_and(|value| !value.is_empty());
    set("WAYLAND_DISPLAY")
        || set("WAYLAND_SOCKET")
        || var("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
}

/// Caps the frame rate while vsync is off, by sleeping away what is left of
/// [`MIN_FRAME_INTERVAL`] since the previous frame.
#[derive(Debug, Default)]
pub struct FrameLimiter {
    last_frame: Option<Instant>,
}

impl FrameLimiter {
    /// Call once per frame: returns when the next frame may start.
    pub fn wait(&mut self) {
        if let Some(last) = self.last_frame {
            std::thread::sleep(MIN_FRAME_INTERVAL.saturating_sub(last.elapsed()));
        }
        self.last_frame = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(vars: &[(&str, &str)]) -> bool {
        is_wayland_session(|name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        })
    }

    #[test]
    fn wayland_display_or_socket_means_wayland() {
        assert!(session(&[("WAYLAND_DISPLAY", "wayland-0")]));
        assert!(session(&[("WAYLAND_SOCKET", "3")]));
        assert!(session(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0")
        ]));
    }

    #[test]
    fn empty_wayland_variables_do_not_count() {
        assert!(!session(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", ":0")]));
        assert!(!session(&[("WAYLAND_SOCKET", "")]));
    }

    #[test]
    fn x11_forced_in_a_wayland_session_counts() {
        assert!(session(&[
            ("WAYLAND_DISPLAY", ""),
            ("DISPLAY", ":0"),
            ("XDG_SESSION_TYPE", "wayland"),
        ]));
    }

    #[test]
    fn x11_and_unknown_sessions_are_not_wayland() {
        assert!(!session(&[]));
        assert!(!session(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "x11")]));
        assert!(!session(&[("XDG_SESSION_TYPE", "tty")]));
    }

    #[test]
    fn limiter_sleeps_only_the_remainder() {
        let mut limiter = FrameLimiter::default();
        let start = Instant::now();
        limiter.wait();
        assert!(
            start.elapsed() < MIN_FRAME_INTERVAL,
            "the first frame waits for nothing"
        );
        limiter.wait();
        assert!(start.elapsed() >= MIN_FRAME_INTERVAL);

        limiter.last_frame = Some(Instant::now() - 2 * MIN_FRAME_INTERVAL);
        let late = Instant::now();
        limiter.wait();
        assert!(
            late.elapsed() < MIN_FRAME_INTERVAL,
            "a late frame waits for nothing"
        );
    }
}
