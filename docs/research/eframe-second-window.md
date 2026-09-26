# A second native window (viewport) in eframe 0.36

Research for issue #26: can the eframe/egui that parterre uses open a second, separate OS window
(for a log window, later perhaps a diff window) reliably on Wayland, X11, Windows and macOS?

Everything below comes from reading the source at the exact versions in `Cargo.lock`, the
changelogs, and egui's issue tracker, plus a throwaway probe app run on this machine. A statement
I worked out from the code, and that no source says outright, is marked **(derived)**. A result
from the probe is marked **(observed)**. The probe ran only on Linux, so everything about Windows
and macOS comes from source and issues alone.

## Short answer

- **Yes, it works on all four platforms, and parterre already does it.** The Settings window is a
  native immediate viewport (`crates/parterre/src/app/settings_window.rs:78-117`). eframe turns
  real viewports on for every desktop OS, and our glow renderer supports both immediate and
  deferred viewports.
- **Wayland (and XWayland) has a serious bug: the whole app freezes when a child window is
  hidden (minimized, or per egui#5145 fully covered) and gets repainted.** I reproduced it with
  both an immediate and a deferred child (observed). Setting `glow_options.vsync = false` avoids
  it (observed), but then repaints are no longer paced by the display. The Settings window
  probably has this bug today (derived, not tested in parterre itself).
- **Which kind to use:** an **immediate** viewport for a log window. It shares state with the app
  directly, the Settings window already uses it, and its main cost (it repaints whenever the main
  window does, and the other way round) is small for a text list. A deferred viewport is the
  better fit for a diff window that is expensive to lay out, but it needs `Arc<Mutex<…>>` state
  and has open repaint bugs on Windows. It does not avoid the Wayland freeze.
- **Fallback:** without real viewports, egui draws the child as an `egui::Window` inside the main
  window. eframe only does this on non-desktop targets, or when the app asks for it.
  `--screenshot` asks for it.
- **`--screenshot` keeps working** because automated runs embed every viewport
  (`crates/parterre/src/app.rs:264`), so the second window appears inside the PNG. Even with a real
  second window open, a screenshot of the main window works (observed). A screenshot *of an
  immediate child* is never delivered on glow (source and observed).

## Versions and features in use

| Crate | Version (`Cargo.lock`) | Notes |
|---|---|---|
| eframe / egui / egui-winit / egui_glow | 0.36.2 | latest on crates.io as of 2026-09-26 (`cargo search`) |
| winit | 0.30.13 | plus `wayland-csd-adwaita` (`crates/parterre/Cargo.toml:31`) |
| glutin | 0.32.3 | |

The eframe features are `default_fonts, glow, persistence, wayland, x11` with default features off
(`Cargo.toml:25-30`), so **the renderer is glow (OpenGL via glutin)**. `wgpu` appears in
`Cargo.lock` but is not built for parterre (`cargo tree -p parterre -i wgpu` prints nothing).

Sources, pinned:

| Short name | Permalink base |
|---|---|
| E | https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/ (tag `0.36.2`) |
| W | https://github.com/rust-windowing/winit/blob/v0.30.13/ |

## 1. Does eframe open real windows on each platform?

- eframe calls `set_embed_viewports(!IS_DESKTOP)`, where `IS_DESKTOP` covers FreeBSD, Linux,
  macOS, OpenBSD and Windows
  ([E/crates/eframe/src/native/winit_integration.rs#L36-L46](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/winit_integration.rs#L36-L46)).
  So all four targets get real OS windows. The web target does not
  ([E/crates/egui/src/viewport.rs#L1-L9](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/viewport.rs#L1-L9)).
- The glow backend installs the immediate-viewport renderer
  ([E/crates/eframe/src/native/glow_integration.rs#L379](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L379)).
  All windows share one GL context, and eframe makes it current on each window's surface in turn
  (`change_gl_context`,
  [#L1004-L1037](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1004-L1037)).
- Multi-viewport support has been in eframe since 0.24 (egui PR
  [#3172](https://github.com/emilk/egui/pull/3172)). The tracking issue for the missing pieces is
  still open ([#3556](https://github.com/emilk/egui/issues/3556)). Among them: new windows are not
  positioned automatically ("they currently cover each other"), and viewport position and size are
  not stored in memory.
- Probe (observed, GNOME Shell 46.0 on Wayland, Mesa 25.2.8, 120 Hz): an immediate child and a
  deferred child both opened as separate windows under native Wayland and under XWayland
  (`WAYLAND_DISPLAY=` unset). Both got keyboard focus when they opened.

## 2. Immediate or deferred?

The authoritative description is the module docs
([E/crates/egui/src/viewport.rs#L11-L37](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/viewport.rs#L11-L37))
and the two functions
([E/crates/egui/src/context.rs#L4033-L4172](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4033-L4172)).
Both must be called every pass while the window should exist. If a pass does not call it, the
window is removed (`remove_viewports_not_in`,
[glow_integration.rs#L1440-L1451](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1440-L1451)).

| | Immediate (`show_viewport_immediate`) | Deferred (`show_viewport_deferred`) |
|---|---|---|
| Callback | `FnMut(&mut Ui, ViewportClass) -> T`, called right away inside the parent's pass. Can borrow `&mut self`. | `Fn(&mut Ui, ViewportClass) + Send + Sync + 'static`, stored and called later by eframe, perhaps many times. |
| State sharing | Direct, like any other ui code. | "You will need to wrap your viewport state in an `Arc<RwLock<T>>` or `Arc<Mutex<T>>`" ([context.rs#L4046](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4047)). |
| Repaint | Coupled: "whenever the parent viewports needs to be repainted, so will the child viewport, and vice versa" ([viewport.rs#L30](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/viewport.rs#L30)). A repaint request for the child makes glow repaint its parent instead ([glow_integration.rs#L588-L602](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L588-L602)). | Independent: "Deferred viewports are repainted independently of the parent viewport" ([viewport.rs#L22](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/viewport.rs#L22)). |
| Threading | Main thread only ([context.rs#L4104](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4104-L4105)). | Also runs on the main thread, from the event loop: `run_ui_and_paint` handles every window ([glow_integration.rs#L566-L880](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L566-L880)). `Send + Sync` is an API requirement, not a separate render thread **(derived)**. |
| `App::logic` / `App::ui` | Run as usual; the child's ui runs inside them. | Not called for the child's passes; only the stored callback is ([E/crates/eframe/src/native/epi_integration.rs#L288-L303](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/epi_integration.rs#L288-L303)). |
| `App::raw_input_hook` | **Not** run for the child: glow calls `egui_ctx.run_ui` directly ([glow_integration.rs#L1655-L1663](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1665-L1676)) **(derived)**. | Run for the child too, through `prepare_raw_input` ([epi_integration.rs#L352-L364](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/epi_integration.rs#L352-L364)) **(derived)**. |
| Screenshot command | Never delivered (see section 5). | Delivered in the child's own input (observed). |

Costs, and whether they matter for parterre:

- **Immediate: CPU.** The child is laid out, painted and swapped on every frame of the main window
  and the other way round. parterre repaints on every frame while the physics settles
  (`crates/parterre/src/app.rs:932-934`), so an open log window would be redrawn at the display
  rate during drags **(derived)**. For a text list drawn with `ScrollArea::show_rows`, that is
  cheap. For a large diff it may not be.
- **Immediate: vsync per window.** egui#5836 (open) says each immediate viewport's
  `swap_buffers` waits for vsync by itself, so two windows run at half the frame rate, three at a
  third ([#5836](https://github.com/emilk/egui/issues/5836); a related Windows report with
  monitors at different rates is [#4963](https://github.com/emilk/egui/issues/4963)). **Not
  reproduced here** (observed): with an immediate child open, the main window's mean frame time
  stayed at 8.3 ms (120 Hz) under both Wayland and XWayland.
- **Deferred: plumbing.** Log lines have to reach the callback through shared state. Anything that
  adds lines must call `ctx.request_repaint_of(log_viewport_id)`. A plain `request_repaint()`
  targets whichever viewport is current (`viewport_stack.last()`, else ROOT;
  [context.rs#L641-L643](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L641-L643),
  [#L1821-L1823](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L1821-L1823)).
  From a background thread that is usually ROOT, not the log window **(derived)**.
- **Deferred: open repaint bugs,** mostly reported on Windows:
  - the deferred window stops redrawing while the main window has focus ([#8466](https://github.com/emilk/egui/issues/8466), Windows);
  - the main window does not update while the pointer moves over a deferred window ([#7686](https://github.com/emilk/egui/issues/7686), Windows 11);
  - `request_repaint` animation works only in the focused viewport ([#4945](https://github.com/emilk/egui/issues/4945));
  - a deferred viewport shown from `App::logic` appears only once the main window is un-minimized ([#8470](https://github.com/emilk/egui/issues/8470)). The same issue reports that doing this with an immediate viewport can panic.

**Recommendation (derived):** use an **immediate** viewport for the log window, like the Settings
window, and draw it with `ScrollArea::show_rows`. Switch to deferred only if profiling shows the
coupled repaint costs something. A diff window is a better candidate for deferred.

## 3. Known bugs and missing features, per platform

Most window-management limits come from winit, because egui-winit maps each `ViewportCommand`
onto a winit call
([E/crates/egui-winit/src/lib.rs#L1728-L1945](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit/src/lib.rs#L1728-L1945)).

### Wayland (the user's session: GNOME 46)

- **Freeze when a child window is hidden (open,
  [#5145](https://github.com/emilk/egui/issues/5145), GNOME 46 / Mutter).** "When an immediate
  or deferred viewport is completely covered by the main window, the application stops responding
  until the viewport is brought to the front again." A commenter adds that a fullscreen child
  covering the main window freezes it too.
  - **Reproduced by proxy (observed):** in the probe the main window repaints continuously, and
    the probe sends `ViewportCommand::Minimized(true)` to the child.
    - Immediate child: the app froze about 40 frames later and never recovered.
    - Deferred child: it froze when the hidden child repainted a second time. That can happen with
      only on-demand repaints; one `request_repaint_of` was enough.
    - While frozen, the main thread sits in `poll` (`/proc/<pid>/task/*/wchan` =
      `poll_schedule_timeout`) at about 1 % CPU.
    - I did not test covering the window, only minimizing it.
  - **Why (derived):**
    - glow uses swap interval 1 by default (`SwapInterval::Wait(1)`,
      [glow_integration.rs#L1060-L1064](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1060-L1064);
      `vsync: true`,
      [E/crates/egui_glow/src/lib.rs#L142](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui_glow/src/lib.rs#L142)).
      On Wayland, Mesa's `eglSwapBuffers` then waits for the compositor's frame callback, and a
      hidden surface never gets one.
    - eframe cannot tell that the window is hidden. winit reports `is_minimized()` as "always
      `None`" on Wayland
      ([W/src/window.rs#L1074](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L1074)),
      and egui-winit turns that into `minimized = Some(false)`
      ([egui-winit lib.rs#L1387](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit/src/lib.rs#L1387)).
      `WindowEvent::Occluded` is "Unsupported" on Wayland
      ([W/src/event.rs#L421](https://github.com/rust-windowing/winit/blob/v0.30.13/src/event.rs#L421)).
      So `ViewportInfo::visible()` never turns false and eframe keeps painting.
    - The immediate renderer paints without any visibility check at all
      ([glow_integration.rs#L1603-L1746](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1603-L1746)).
  - **Workaround (observed):** with `NativeOptions::glow_options.vsync = false` neither case froze.
    The cost is that repaints are no longer paced: the probe's continuously repainting loop ran at
    about 0.3 ms per frame. parterre would then have to pace its physics animation itself, for
    example with `request_repaint_after(~16 ms)` instead of `request_repaint()` **(derived)**.
    Other options: embed viewports on Wayland (no second OS window there), or wait for an
    upstream fix.
  - **parterre today (derived, not tested in the app):** the Settings window is an immediate
    viewport, so minimizing it (Super+H), or clicking the main window until the Settings window is
    fully covered, should freeze parterre on the next repaint of the main window. A two-minute
    manual check would confirm it.
- **No positioning.** `with_position` is ignored ("Others: Ignored",
  [W/src/window.rs#L229-L246](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L229-L246)).
  `set_outer_position` is "Unsupported" and `outer_position` "Always returns NotSupportedError"
  ([#L699](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L699),
  [#L730](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L730)). The
  compositor decides where the log window goes; we cannot put it beside the main window. The probe
  saw `outer_rect = None` and `inner_rect = None` in the child's `ViewportInfo` under Wayland
  (observed); size is still available from `content_rect()`.
- **No programmatic focus.** `ViewportCommand::Focus` calls `focus_window`
  ([egui-winit lib.rs#L1895-L1899](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit/src/lib.rs#L1895-L1899)),
  which is "Unsupported" on Wayland
  ([W/src/window.rs#L1312](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L1312)).
  "Show log" cannot raise a log window that is already open. `RequestUserAttention` needs
  `xdg_activation_v1` ([#L1340-L1343](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L1342)).
  New windows did get focus when they opened (observed).
- **Icons.** `set_window_icon` is "Unsupported" on Wayland and macOS
  ([W/src/window.rs#L1200](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L1200)).
  On GNOME the icon comes from the desktop entry that matches the `app_id`, so a log window should
  get `.with_app_id(settings::APP_ID)` as the Settings window does (`settings_window.rs:87`).
  egui copies the parent's icon to a child that has none
  ([glow_integration.rs#L1518-L1523](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1517-L1522)),
  although [#3634](https://github.com/emilk/egui/issues/3634) (that it does not) is still open.
  parterre sets the icon explicitly anyway.
- **Resizing.** eframe has a Linux-only fix that resizes the GL surface after `InnerSize`
  ([glow_integration.rs#L1487-L1497](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1487-L1497),
  [#4196](https://github.com/emilk/egui/issues/4196)). Open:
  [#4220](https://github.com/emilk/egui/issues/4220) (dragging out of maximize does not resize).
- **Idle CPU.** [#8523](https://github.com/emilk/egui/issues/8523) (open, filed against 0.35)
  says the loop stays in `Poll` on Wayland after a burst of repaints. 0.36.2 changed this path:
  "Don't busy-loop a CPU core while waiting for a redraw"
  ([PR #8398](https://github.com/emilk/egui/pull/8398);
  [run.rs#L210-L216](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/run.rs#L210-L216)).
  Whether that fixes #8523 is unverified.
- Requested sizes are unreliable on this machine: children sometimes opened at the requested
  300×200 and sometimes at the same size as the main window (observed; the window manager ignores
  sizes, as noted in earlier parterre work).

### X11

- Positioning works (`with_position` sets the outer position,
  [W/src/window.rs#L245](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L245)),
  and so do `focus_window` and window icons. Under XWayland the probe saw real
  `outer_rect`/`inner_rect` values (observed).
- The hidden-window problem shows up under **XWayland** too, in a milder form (observed):
  - With the immediate child minimized, the main window crawled: 30 frames took 18 s instead of
    0.25 s.
  - With the deferred child minimized, the app kept running normally; the hidden child stopped
    painting.
  - I could not test a native Xorg session.
- `StartDrag` is guarded because on X11 input "will be permanently taken until the app is killed"
  otherwise ([egui-winit lib.rs#L1750-L1756](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit/src/lib.rs#L1750-L1756)).
  This only matters for custom title bars.

### Windows

Not tested; from source and issues only.

- glow cannot skip `make_current` on Windows, so every window switch costs a context switch
  ([glow_integration.rs#L1011-L1015](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1011-L1015)).
  [#4173](https://github.com/emilk/egui/issues/4173) (open) reports high CPU from
  `make_not_current` on Windows. A second window means more switches **(derived)**.
- Crash (access violation) when a **deferred** viewport is closed with its close button,
  0.28 and later ([#4842](https://github.com/emilk/egui/issues/4842), open, Windows 11).
- Deferred repaint problems: [#8466](https://github.com/emilk/egui/issues/8466) and
  [#7686](https://github.com/emilk/egui/issues/7686) (see section 2).
- `send_viewport_cmd` does not reach invisible viewports
  ([#3655](https://github.com/emilk/egui/issues/3655)). eframe now paints invisible windows
  directly for this reason ([run.rs#L200-L228](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/run.rs#L200-L228), [#5229](https://github.com/emilk/egui/issues/5229)).
- Fixed in the version we use: transparent child viewports with glow on Windows (0.36.2,
  [PR #8423](https://github.com/emilk/egui/pull/8423);
  [E/crates/eframe/CHANGELOG.md#L13](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/CHANGELOG.md#L13)).
- Wrong placement and size with several monitors at different DPI
  ([#4918](https://github.com/emilk/egui/issues/4918), open).

### macOS

Not tested; from source and issues only.

- glow clears the framebuffer late when there is more than one viewport, because "an early clear
  doesn't 'take' on Mac with multiple viewports"
  ([glow_integration.rs#L697-L698](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L697-L698)).
- Opening a child viewport while the main window is fullscreen or maximized makes the child
  flicker or aborts the fullscreen transition
  ([#8259](https://github.com/emilk/egui/issues/8259), open, reported with wgpu).
- Switching Spaces or minimizing to the Dock destroyed immediate viewports and re-centred them
  ([#8085](https://github.com/emilk/egui/issues/8085), glow, 0.34.1, still open). 0.36.0 stopped
  running any pass while nothing is visible
  ([PR #8387](https://github.com/emilk/egui/pull/8387)), and the pass is skipped only when no
  descendant is visible either
  ([glow_integration.rs#L614-L615](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L615-L624),
  [#L1584-L1600](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L1584-L1600)).
  That looks like it removes the cause described in #8085 **(derived, unverified on a Mac)**.
- Window icons: unsupported (see Wayland); macOS takes the icon from the bundle.
- `with_position` sets the *inner* position on macOS
  ([W/src/window.rs#L236-L240](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L236-L241)).

### glow or wgpu?

parterre builds glow only. Switching renderers would not avoid the problems above:

- #5836 (vsync per immediate viewport) is labelled `egui_glow` and `egui-wgpu`.
- #5145 (Wayland freeze) is filed against the `multiple_viewports` example without naming a
  renderer.
- The positioning, focus and icon limits come from winit.

Differences I found **(derived)**:

- wgpu handles screenshots of viewports differently. It drains the screenshot requests in its own
  paint path
  ([wgpu_integration.rs#L803-L823](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/wgpu_integration.rs#L803-L823)).
  I did not check whether that covers immediate viewports.
- eframe 0.36 made `vsync`/frame latency configurable at runtime for wgpu only
  ([CHANGELOG 0.35.0, PR #8114](https://github.com/emilk/egui/pull/8114)).

No reason found to change renderer for this feature.

## 4. What happens when a platform cannot do multiple viewports

- When `embed_viewports()` is true, or no immediate renderer is installed, both functions draw the
  child inside the current window as an `egui::Window`
  ([context.rs#L4068-L4073](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4070-L4074),
  [#L4124-L4138](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4124-L4137)).
  The callback receives `ViewportClass::EmbeddedWindow`.
- `Window::from_viewport` copies only title (or app id), sizes, `resizable`, decorations and the
  minimize button, which becomes "collapsible". "A lot of things not implemented yet"
  ([E/crates/egui/src/containers/window.rs#L126-L158](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/containers/window.rs#L126-L158)).
  The immediate path forces `collapsible(false)` and panics if the window is somehow collapsed
  ([context.rs#L4174-L4186](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui/src/context.rs#L4174-L4186)).
- **The embedded window has no close button** (`from_viewport` never calls `.open()`), so the app
  must offer its own way to close it **(derived)**.
- Inside an embedded callback the "current viewport" is still the parent. So
  `i.viewport().close_requested()` reports the *main* window's close, and `send_viewport_cmd` goes
  to the main window **(derived)**. The Settings window already guards both with
  `class != EmbeddedWindow` (`settings_window.rs:99-111`); a log window should do the same.
- For parterre this fallback only ever happens when we ask for it, because eframe enables real
  viewports on every desktop target (section 1).

## 5. `--screenshot` with a second viewport open

`--screenshot` is not headless: it opens the normal window, sends `ViewportCommand::Screenshot`
after some frames, reads `Event::Screenshot` from the root's input, saves it and closes
(`crates/parterre/src/automation.rs:159-197`).

- **Today:** automated runs call `set_embed_viewports(automation.is_active())`
  (`crates/parterre/src/app.rs:264`). Any viewport, including a future log window, becomes an
  embedded `egui::Window` inside the main window and is part of the PNG. The screenshot path is
  unchanged. Probe with embedding on (observed): screenshots of the main window arrived with the
  children drawn inside it.
- **If a native child were open during a screenshot run** (observed and source):
  - Screenshots of the main window still arrive, with an immediate child open and with a deferred
    child open, under Wayland and XWayland. They contain only the main window.
  - A screenshot sent to an **immediate** child never arrives. glow queues it in
    `actions_requested`
    ([egui-winit lib.rs#L1940-L1942](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit/src/lib.rs#L1940-L1942)),
    but only drains that queue in `run_ui_and_paint`
    ([glow_integration.rs#L806-L822](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/glow_integration.rs#L806-L818)),
    never in `render_immediate_viewport`. Automation waiting for it would hang.
  - A screenshot of a **deferred** child arrives in that child's own pass, not the main window's.
  - `App::raw_input_hook`, which injects the demo pointer events, would also run for a deferred
    child's input (section 2). That is another reason to keep embedding in automated runs.
  - A native child also brings the Wayland hidden-window freeze into unattended runs.
- **Recommendation:** keep `set_embed_viewports(true)` for automated runs, and keep the
  `class != EmbeddedWindow` guards in the child's code.

## Probe details

Throwaway crate in `/tmp/vp-test`, not committed. It used parterre's `Cargo.lock` and eframe
`=0.36.2` with `default_fonts, glow, wayland, x11`. The main window repainted continuously and
counted frames:

1. 90 frames alone.
2. 120 frames with an immediate child (300×200, `with_position([100, 100])`).
3. 120 frames with a deferred child that also repainted continuously.

The probe sent screenshots to the main window and to each child. Optional flags minimized a child,
used `embed_viewports`, turned vsync off, or made the deferred child repaint only on request.
Machine: GNOME Shell 46.0 (Wayland), Mesa `libegl-mesa0` 25.2.8, amdgpu and nvidia drivers
present (which one rendered was not checked), scale 1.25, 120 Hz.

| Run | Result |
|---|---|
| Wayland, default | Main window 8.3 ms per frame in all three phases. Screenshots: main window yes, immediate child no, deferred child yes. Child `outer_rect`/`inner_rect` = `None`. |
| XWayland, default | Same frame times and screenshot results. Child rects present. |
| Wayland, `embed_viewports(true)` | Children drawn inside the main window; screenshots of the main window work. |
| Wayland, immediate child minimized | Froze about 40 frames later; killed by `timeout` after 40 s. |
| Wayland, deferred child minimized (continuous or on-demand repaint) | Froze after the hidden child's second repaint. Main thread in `poll`, about 1 % CPU. |
| XWayland, immediate child minimized | Main window slowed to about 0.6 s per frame. |
| XWayland, deferred child minimized | No effect on the main window; the hidden child stopped painting. |
| Wayland, either child minimized, `vsync = false` | No freeze; the loop ran unpaced (about 0.3 ms per frame). |
