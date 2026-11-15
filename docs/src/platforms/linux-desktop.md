# Desktop

The desktop runner opens a native window on Linux (Wayland or X11), macOS and
Windows.

## What is verified

The test suite runs on Linux, macOS and Windows in CI on every change. It drives
interfaces without opening a window, including the notes app at a desktop
window size. No CI job opens a real window, so windowing, GPU rendering and
system dialogs are checked by hand.

## Linux packages

```bash
sudo apt install -y libwayland-dev libxkbcommon-dev libfontconfig1-dev libfreetype-dev libx11-dev
```

## Starting a window

```rust
use craftui::prelude::*;

fn main() -> craftui::core::Result<()> {
    let window = WindowConfig {
        title: "My app".to_string(),
        width: 1100.0,
        height: 720.0,
        ..WindowConfig::default()
    };
    run_app(window, |cx, tree, _ctx| build_my_ui(cx, tree))
}
```

`WindowConfig` also has `resizable` and `force_cpu`. Set `force_cpu` to draw
with the CPU renderer, which is useful in a virtual machine or over remote
desktop where no usable GPU is present.

## Run

```bash
cargo run -p craftui-example-notes --features desktop
# or, from a project folder:
craftui run --platform desktop
```

## Rendering

The window is drawn with `wgpu`, which uses Vulkan on Linux, Metal on macOS and
Direct3D 12 on Windows. A CPU renderer is used when `force_cpu` is set.

## System services

- **Clipboard** uses the Wayland and X11 protocols directly on Linux.
- **File dialogs** go through `xdg-desktop-portal` on Linux, so they match the
  desktop's own file chooser.
- **More windows** can be opened with `WindowLauncher`.

## Editing `.cui` while the app runs

With the `hot-reload` feature (on by default), `run_hot_reload_cui` watches the
`.cui` file it was given and rebuilds the interface when that file changes,
without recompiling Rust. `craftui preview <file.cui>` opens a single file the
same way.

`run_app`, which the window example and the Notes app use, does **not** watch
anything: editing a `.cui` file leaves a running app unchanged, even with the
feature enabled. Use `craftui preview` or `run_hot_reload_cui` when you want
live reload.

Reload interprets the `.cui` file at run time, while a normal build compiles it
to Rust. The two paths are separate implementations, so check a change in a
real build before you rely on it.
