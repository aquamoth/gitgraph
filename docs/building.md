# Building

## Linux (development machine)

```sh
cargo build --release
./target/release/gitgraph ~/some/repo
```

Only a Rust toolchain is required. At runtime the window needs the usual desktop libraries:
Wayland or X11, libxkbcommon, and OpenGL (EGL/GLX), all of which are present on any desktop.

## Windows

Native build on Windows (MSVC toolchain from rustup):

```powershell
cargo build --release
.\target\release\gitgraph.exe C:\path\to\repo
```

Release builds use the GUI subsystem (no console window), and git is started with
`CREATE_NO_WINDOW` so no console flashes.

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
