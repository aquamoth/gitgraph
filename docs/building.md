# Building

## Linux (development machine)

```sh
cargo build --release
./target/release/parterre ~/some/repo
```

Only a Rust toolchain is required. At runtime the window needs the usual desktop libraries:
Wayland or X11, libxkbcommon, and OpenGL (EGL/GLX), all of which are present on any desktop.

## Windows

Native build on Windows with the MSVC toolchain. Prerequisites:

1. The MSVC C++ build tools and a Windows SDK. Either install *Build Tools for Visual Studio*
   with the "Desktop development with C++" workload, or add the components to an existing
   Visual Studio (from an elevated prompt):

   ```powershell
   & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify `
     --installPath "C:\Program Files\Microsoft Visual Studio\18\Professional" `
     --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
     --add Microsoft.VisualStudio.Component.Windows11SDK.26100 --passive --norestart
   ```

2. Rust from [rustup](https://rustup.rs) with the default `x86_64-pc-windows-msvc` host.
   `rust-toolchain.toml` pulls in rustfmt and clippy.

Then:

```powershell
cargo build --release
.\target\release\parterre.exe C:\path\to\repo
```

The result is a single self-contained `target\release\parterre.exe`. `.cargo/config.toml` links
the C runtime statically, so it runs without the Visual C++ Redistributable. Only `git` must be
on `PATH`.

Release builds use the GUI subsystem (no console window), and git is started with
`CREATE_NO_WINDOW` so no console flashes. To still show `--help`, errors and `--export` output
in a terminal, the program attaches to its parent's console at startup
(`crates/parterre/src/console.rs`, the workspace's only `unsafe`). It releases the console
before opening an interactive window, so closing the terminal doesn't close the window.
Shells don't wait for GUI programs, so the output may appear after the next prompt; press
Enter to get a fresh prompt. Debug builds are ordinary console programs.

Don't use the `x86_64-pc-windows-gnu` host toolchain as a shortcut around installing the MSVC
tools. Its bundled `dlltool` needs an assembler (`as.exe`) that the toolchain doesn't ship.
`windows-link` always uses `raw-dylib`, so the build fails with
`error calling dlltool` / `CreateProcess` unless a full MinGW-w64 is on `PATH`.

Cross-checking from Linux works without extra tools:

```sh
rustup target add x86_64-pc-windows-gnu
cargo check --workspace --target x86_64-pc-windows-gnu
```

Producing a Windows `.exe` from Linux needs a linker. Options: `sudo apt install mingw-w64`
and then `cargo build --target x86_64-pc-windows-gnu`, or `cargo-zigbuild`, or `cargo-xwin`
(which downloads the MSVC CRT; that requires accepting Microsoft's license).

## macOS

Should build with `cargo build --release`. Not yet tried.

## Checking visuals without a human

```sh
cargo run --release -- ~/repo --screenshot out.png --window-size 1400x900 [--fit] [--theme dark]
```

This renders a few frames, saves the window to `out.png`, and exits. `--demo-drag DX,DY` drags
the centre node first, to show the physics.

## Icon

`crates/parterre-core/src/icon.rs` draws the app icon in code. The window icon is rasterised
from it at startup, and

```sh
cargo run --release -p parterre-core --example icon_assets
```

writes `packaging/icon/`: the SVG, PNGs from 16 to 512 px, `parterre.ico` and `parterre.icns`.
Rerun it after changing the drawing and commit the results.

`crates/parterre/build.rs` embeds `parterre.ico` in the Windows executable through
`crates/parterre/parterre.rc`. That needs `rc.exe` from the Windows SDK (installed with the
build tools above), or `x86_64-w64-mingw32-windres` for the GNU target; without one the build
only warns and the `.exe` has no icon. The `.icns` waits for a macOS `.app` bundle.

On Linux, `packaging/linux/install.sh` installs the release binary into `~/.local/bin`, and the
desktop entry and the icon where the desktop finds them; `--uninstall` removes them again. A
desktop shows a window's icon through the desktop entry, which it loads only if the entry's
`Exec` can be found, so the script writes the binary's absolute path into the entry rather than
relying on `~/.local/bin` being on the session's PATH. On Wayland the entry is the only source
of the icon, since GNOME never uses the icon a window sets on itself. A running parterre shows
it after a restart.
