# Avoiding the Wayland freeze in a second window

Research for issue #32. parterre's log window is going to be a second OS window (an egui
viewport). On Wayland, egui#5145 freezes the whole app when a child viewport is hidden and then
repainted. This note answers three questions: does parterre's Settings window freeze today, which
workaround holds up, and what does each workaround cost on the other platforms?

It builds on the #26 research,
[eframe-second-window.md](https://github.com/aquamoth/parterre/blob/research/eframe-second-window/docs/research/eframe-second-window.md).
The versions are the same: eframe/egui 0.36.2 with glow, winit 0.30.13, glutin 0.32.3.

Labels used below:
- **(observed)**: a result from a throwaway, uncommitted build of parterre run on this machine.
- **(derived)**: something I worked out from the code that no source says outright.
- No label: the linked source says it.

## Short answer

- **Yes, the Settings window freezes parterre today (observed).** I minimized the Settings window
  from a debug hook. parterre froze within half a second and never recovered. It even froze
  when the main window repainted only once a second. Minimizing the *main* window while
  Settings was open stopped both windows as well.
- **Recommended: on Linux in a Wayland session, set `glow_options.vsync = false` and cap the
  frame rate in `App::ui` with a sleep-based limiter (about 8 ms per frame).** Leave vsync on
  everywhere else.
  - With that change, none of the freeze cases froze (observed).
  - The cost during continuous animation was 14 % of a core instead of 11 % (observed).
  - Idle cost does not change. There is no tearing, because the compositor composes every frame
    (derived).
  - On X11, Windows and macOS nothing changes at all.
- **Two alternatives do not work in eframe 0.36:**
  - Capping with `request_repaint_after` alone. parterre and egui still call
    `request_repaint()`, and the loop ran at about 2000 fps (observed).
  - Skipping the repaint of a hidden child. Neither winit nor eframe can tell on Wayland that a
    window is hidden.
- **Upstream fix:** egui PR [#8631](https://github.com/emilk/egui/pull/8631) (open, created
  2026-09-25) makes glow do the same thing inside eframe: swap interval 0 on Wayland, with frame
  callbacks for pacing. No released eframe has it. When it ships, parterre's limiter can go.
- **`--screenshot` is unaffected.** Automated runs embed every viewport, so only one window is
  open. The screenshot is read from the back buffer before the swap, so the swap interval does
  not touch it (derived).

## 1. Does the Settings window freeze today?

### Method

The test was a throwaway build of parterre, not committed. It had:
- a hook in `App::ui` that opens Settings on frame 1;
- `ViewportCommand::Minimized(true)` sent to the Settings viewport (or to ROOT) on frame 120;
- a watchdog thread that counted frames and reported stalls.

Environment variables selected the repaint policy (continuous, or one repaint per second after
the minimize) and the value of `glow_options.vsync`. Each run lasted 6–10 s, with parterre's own
repository open.

Machine: GNOME Shell 46.0 on Wayland, 120 Hz. Two GPUs: NVIDIA RTX 4070 Ti and AMD Raphael.
Mesa `libegl-mesa0` 25.2.8. The client bound `wl_drm`, so Mesa EGL probably rendered (derived).

I could not test covering a window with another one. GNOME's `Shell.Eval` is locked, and the
session gives no way to raise a window from a script. After the coordinator asked me to stop
opening windows on the live desktop, I made no further runs. No headless Wayland compositor is
installed; Xvfb has no window manager, so it cannot minimize a window.

### Results (observed)

| Run | vsync | Result |
|---|---|---|
| Settings open, nothing minimized (6 runs) | on | 120 fps, about 10 % of a core. |
| Settings open, nothing minimized (1 earlier run) | on | Froze at frame 20 without any minimize. Cause unknown. |
| Settings minimized, continuous repaint (4 runs) | on | Froze 49 frames (0.4 s) after the minimize and never recovered. Even `process::exit` from the watchdog hung; it needed `SIGKILL`. |
| Settings minimized, one repaint per second afterwards | on | Froze 4 frames after the minimize. |
| Main window minimized, Settings open | on | Froze. The Settings window stops with it. |
| Main window minimized, no Settings | on | Froze. For a single window this is the normal wait for a hidden surface, and nobody sees it. |
| XWayland (`WAYLAND_DISPLAY=`), Settings minimized | on | Did not freeze, but crawled at about 1 fps. |
| XWayland, main window minimized | on | Crawled at about 9 fps. |
| Settings minimized, continuous repaint | **off** | No freeze. Unpaced: about 2000 fps, 104 % of a core. |
| Settings minimized, idle | off | No freeze, about 1 % of a core. |
| Main window minimized, Settings open | off | No freeze. |
| Settings minimized, `request_repaint_after(8 or 16 ms)` | off | No freeze, but still about 2000 fps: other code keeps calling `request_repaint()`. |
| Settings minimized, sleep limiter of 8 ms in `App::ui` | off | No freeze. 124 fps, **14 %** of a core. |
| Nothing minimized, vsync on (baseline for the row above) | on | 120 fps, **11 %** of a core. |
| XWayland, Settings minimized, sleep limiter of 8 ms | off | No freeze. 124 fps, 19 % of a core. |

`WAYLAND_DEBUG=1` shows the mechanism (observed):
- After `xdg_toplevel.set_minimized()` on the Settings surface, Mutter kept sending frame
  callbacks for about 0.4 s. That time probably covers the minimize animation (derived).
- Then the last `wl_surface.frame` callback requested for the Settings surface never arrived.
- The next swap on that surface waited for it forever.

This matches Mesa's Wayland swap path (mesa-25.2.0, `src/egl/drivers/dri2/platform_wayland.c`):
- With a swap interval above 0, each swap requests a frame callback
  ([L1786-L1791](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.0/src/egl/drivers/dri2/platform_wayland.c#L1786-L1791)).
- The next swap first dispatches until that callback arrives
  ([L1774](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.0/src/egl/drivers/dri2/platform_wayland.c#L1774),
  loop at [L1734-L1737](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.0/src/egl/drivers/dri2/platform_wayland.c#L1734-L1737)).
- With interval 0, the swap waits only for a `wl_display_sync` round trip, which a hidden surface
  also gets
  ([L1856-L1864](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.0/src/egl/drivers/dri2/platform_wayland.c#L1856-L1864)).
- Mesa allows at most interval 1 on Wayland
  ([L2247-L2255](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.0/src/egl/drivers/dri2/platform_wayland.c#L2247-L2255)).

glutin passes `SwapInterval::Wait(1)` / `DontWait` straight to `eglSwapInterval`
([glutin v0.32.3 egl/surface.rs#L369-L384](https://github.com/rust-windowing/glutin/blob/v0.32.3/glutin/src/api/egl/surface.rs#L369-L384)).
The glutin maintainer calls the block "expected behavior and can't be fixed other than not using
vsync" ([glutin#1591](https://github.com/rust-windowing/glutin/issues/1591), closed
2023-05-04). PR #8631 says NVIDIA's egl-wayland blocks the same way.

Why the *whole* app stops:
- All viewports share one GL context and one main thread.
- An immediate child is painted and swapped inside the parent's pass. The swap at
  [glow_integration.rs#L1735-L1737](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1735-L1737)
  blocks the pass, and with it the main window.

Two consequences for the log window (derived from the rows above):
- Minimizing the log window freezes parterre.
- Minimizing the **main** window freezes the log window too: its content cannot update, and the
  process cannot quit cleanly until the main window is shown again.

## 2. Why the "skip a hidden child" route is closed

- **winit cannot tell.** On Wayland `is_minimized()` returns `None`: "clients don't know whether
  they are minimized or not"
  ([W/…/wayland/window/mod.rs#L368-L371](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/linux/wayland/window/mod.rs#L368-L371)).
  `WindowEvent::Occluded` is "Unsupported" on Wayland
  ([W/src/event.rs#L421](https://github.com/rust-windowing/winit/blob/v0.30.13/src/event.rs#L421)).
  - winit reads the xdg-shell `SUSPENDED` state but uses it only to skip a resize
    ([state.rs#L388](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/linux/wayland/window/state.rs#L388)).
  - The same holds in 0.31.0-beta.3 and on master.
  - winit once had Occluded on Wayland but disabled it, because "drawing when getting `Occluded`
    with vsync will block indefinitely"
    ([winit#3441](https://github.com/rust-windowing/winit/pull/3441), merged 2024-01-30).
  - Re-adding it for `suspended` is open PR
    [winit#4709](https://github.com/rust-windowing/winit/pull/4709) (created 2026-09-23). Its
    author concluded that avoiding the block "is the app's job (`pre_present_notify`, or a
    non-FIFO present mode)".
  - Mutter advertises `xdg_wm_base` version 6, which has `suspended` (observed in the protocol
    log). I did not see whether it sends `suspended` when a window is minimized.
- **eframe could not use it anyway.**
  - `ViewportInfo::visible()` is unknown unless `minimized` or `occluded` is known
    ([E/…/viewport_info.rs#L95-L101](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/data/input/viewport_info.rs#L95-L101)).
  - The immediate renderer paints without checking visibility at all (#26 research, section 3).
  - The app cannot keep an immediate child from painting while it is open: it is painted
    whenever `show_viewport_immediate` is called (derived).
- **Covered windows get no signal at all.** A compositor may stop sending frame callbacks
  without telling the client that the window is hidden (PR #8631, observed on Hyprland). A
  visibility check would never catch that case.

## 3. Candidate workarounds and their costs

### A. vsync off in Wayland sessions, plus a sleep-based frame cap (recommended)

What to do (derived; this is a sketch, not code in the repo):

1. In `main.rs`, before `run_native`, detect a Wayland session. Then set
   `options.glow_options.vsync = !wayland_session` (the field is
   [egui_glow `GlowConfiguration::vsync`](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui_glow/src/lib.rs#L122)).
   - eframe reads it once, when it creates the GL context
     ([glow_integration.rs#L1060-L1064](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1060-L1064)),
     and applies it to every window's surface
     ([#L1364](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1364)).
     So the decision has to be made before the window exists, not in `App::new`.
   - winit picks Wayland when `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty
     ([W/…/linux/mod.rs#L735-L752](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/linux/mod.rs#L735-L752)).
   - Also treat `XDG_SESSION_TYPE=wayland` as Wayland. That covers X11 forced inside a Wayland
     session (XWayland), which crawled at about 1 fps with a hidden child (observed).
2. In `App::ui`, if vsync is off, sleep until 8 ms have passed since the previous frame.
   - One `ui` pass paints the main window and every immediate child, so one sleep paces them
     all (derived).
   - Every other place that asks for repaints keeps working unchanged.
3. Keep the limiter in automated runs too, so the "frame interval" line printed by
   `automation.rs` stays comparable between runs. Screenshot runs step physics with a fixed
   `dt` (`automation.rs:96-98`), so their images do not depend on the frame rate.

Why a sleep and not `request_repaint_after`:
- egui runs a pass as soon as *anyone* calls `request_repaint()`: parterre's physics
  (`app.rs:932-934`), the Settings window while it opens, egui's own animations, and pointer
  events.
- The observed run with `request_repaint_after(8 ms)` and vsync off still ran at about 2000 fps.
- eframe 0.36 has a `max_fps` option only for the web (`WebOptions::max_fps`), none for native
  (derived: `grep max_fps` finds nothing under `eframe/src/native`).

Costs:

| Platform | Cost |
|---|---|
| Wayland | 14 % instead of 11 % of a core while animating at about 120 fps (observed). Same idle cost (observed). No tearing: the compositor latches whole buffers, and egui does not use the tearing-control protocol (derived). Frames are no longer lined up with the display's refresh, so an animation can occasionally show a doubled or skipped frame (derived). Latency is the same or lower, because the swap no longer waits (derived). While the main window is minimized, eframe cannot tell and keeps painting at up to 125 fps as long as physics is moving; parterre goes idle once the layout settles (derived). |
| XWayland (X11 forced in a Wayland session) | 19 % of a core at 124 fps instead of crawling (observed). No tearing: Mutter composes XWayland windows too (derived). |
| X11 session, Windows, macOS | None: vsync stays on. |

The 8 ms cap wastes frames on a 60 Hz display, where 16 ms would do. eframe does not expose the
monitor's refresh rate. winit has `MonitorHandle::refresh_rate_millihertz`, but eframe does not
pass the winit window to the app (derived). A fixed 8 ms is the simple choice. The cost stays
small because parterre only animates while the layout settles or the view moves.

### B. Wait for the upstream fix, or patch eframe with it now

- **egui PR [#8631](https://github.com/emilk/egui/pull/8631)** "eframe: keep running on Wayland
  when frame callbacks stop". Open, created 2026-09-25, head `e2c5f07`. What it does:
  - glow calls `window.pre_present_notify()` before the swap in `run_ui_and_paint`, so winit
    paces `RedrawRequested` by the compositor's frame callbacks;
  - it forces `SwapInterval::DontWait` when the display is Wayland ("`glow_options.vsync`
    therefore has no effect on Wayland");
  - if a requested redraw has not arrived after 250 ms, it runs only `App::logic`.

  In its diff the only new `pre_present_notify` is in `run_ui_and_paint`, not in the
  immediate-viewport renderer. An immediate child is still painted every parent pass, but it no
  longer blocks, because the swap interval is 0 for all surfaces (derived).
- **Nothing released yet.** crates.io's newest eframe is 0.36.2 (2026-09-08). master after
  0.36.2 has no Wayland or swap-interval change in `glow_integration.rs`.
  - egui PR [#8171](https://github.com/emilk/egui/pull/8171) (merged 2026-05-19, "Possibly
    fixes #5145") changes only wgpu files.
  - #5145 itself is still open, with no workaround in its thread.
- **Patching now** with `[patch.crates-io]` pointing at a fork of 0.36.2 plus #8631 would work.
  - It gives true frame-callback pacing for the main window, and it stops painting a hidden main
    window.
  - It costs a git dependency and a rebase for every eframe update, for a PR nobody has
    reviewed yet (derived).
  - Not worth it while option A is about 15 lines of parterre code (derived).
- **Once #8631 ships:** `vsync = false` becomes a no-op on Wayland and can stay. The limiter
  then only matters for the rare case of X11 inside a Wayland session.

### C. Switch to the wgpu renderer

- egui-wgpu already calls `pre_present_notify` ([PR #8089](https://github.com/emilk/egui/pull/8089),
  merged 2026-04-12, so it is in 0.36.2).
- PR #8631 reports that with wgpu a hidden window stops getting `RedrawRequested` rather than
  blocking.
- Reasons against:
  - An immediate child is still presented from the parent's pass. wgpu's FIFO mode has had its
    own hangs on hidden Wayland windows ([wgpu#8597](https://github.com/gfx-rs/wgpu/issues/8597)).
    So wgpu does not clearly fix our case, and I did not test it (derived).
  - It swaps the whole GPU stack and adds a large dependency tree for one bug.
- Not recommended.

### D. Embed viewports on Wayland

- `ctx.set_embed_viewports(true)` in Wayland sessions would draw the log window inside the main
  window as an `egui::Window`. That is what `--screenshot` runs already do (`app.rs:264`).
- It avoids the bug completely, at no CPU cost.
- The cost: no separate OS window on Wayland, which is the main reason for #27's design. It also
  has the embedded-window quirks from the #26 research, section 4 (no close button, and the
  current viewport is the main window).
- Keep it as the fallback if option A shows a problem.

### What other toolkits do

All of them avoid a blocking EGL swap on Wayland:
- **SDL** sets the real interval to 0 and waits for the frame callback itself, with a timeout
  ([SDL_waylandopengles.c#L67-L180](https://github.com/libsdl-org/SDL/blob/a19bac65218bf583e8e5cb8c52e1c9603fa8d94c/src/video/wayland/SDL_waylandopengles.c#L67-L180)).
- **Qt** calls `eglSwapInterval(…, 0)` and emulates a blocking swap with a 100 ms wait
  ([qwaylandglcontext.cpp#L501-L507](https://github.com/qt/qtbase/blob/57f2d926cbe2e34c66ed71b966760fe09d19b36b/src/plugins/platforms/wayland/plugins/hardwareintegration/wayland-egl/qwaylandglcontext.cpp#L501-L507)).
- **GLFW** did the same in 2026: "setting the EGL swap interval to zero" with "a reasonable
  timeout" ([commit fdd14e6](https://github.com/glfw/glfw/commit/fdd14e65b1c29e4e6df875fb5669ec00d6793531),
  2026-02-17). That fixed "hide one window, the other stops responding"
  ([glfw#2640](https://github.com/glfw/glfw/issues/2640)).
- **alacritty** uses `SwapInterval::DontWait` plus `pre_present_notify`
  ([display/mod.rs#L512-L515](https://github.com/alacritty/alacritty/blob/d692748d3f61253ebe9f5094320120d22f6a046f/alacritty/src/display/mod.rs#L512-L515)).

Option A is the same idea without frame callbacks, which eframe does not let the app request.

## 4. `--screenshot`

- Automated runs call `set_embed_viewports(automation.is_active())` (`app.rs:264`). Settings and
  a future log window become `egui::Window`s inside the one real window, so the multi-window
  freeze cannot happen in them (derived).
- glow reads the screenshot with `read_screen_rgba` in `run_ui_and_paint` *before*
  `swap_buffers`
  ([glow_integration.rs#L806-L852](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L806-L852)).
  The swap interval therefore cannot change the image (derived).
- With vsync off, a screenshot run is also safer when its window starts hidden, for example on
  another workspace: the swap can no longer block (derived; PR #8631 reports that a vsync swap
  blocks for windows started on a hidden workspace).
- Not verified by a `--screenshot` run with vsync off. Such a run opens a window on the live
  desktop, and I had been told to stop doing that.

## Open points

- The covered-window case (Settings fully under the main window) was not tested. #5145 reports
  that it freezes; per Mesa's code it is the same frame-callback wait (derived).
- One of eight runs with vsync on froze at frame 20 with nothing minimized. It may be the same
  withheld-frame-callback problem for a newly mapped window that PR #8631 describes (derived,
  unconfirmed). vsync off would avoid it as well.
- Whether native Xorg (not XWayland) slows down with a minimized child was not tested.
- Windows and macOS were not tested. Keeping vsync on there leaves their behaviour exactly as
  it is today.
