# CraftUI

A declarative UI framework written in Rust. One codebase draws its own
interface on desktop (Linux, macOS, Windows), Android and the Web. MIT licensed.

You describe the interface in `.cui` files, which are compiled to Rust when the
app is built, and keep the app's logic in ordinary Rust.

```cui
export component Counter {
    in-out property <int> count: 0;

    Column {
        gap: 12;
        padding: 24;

        Text { text: "Count: " + count; font_size: 24; }
        Button { text: "Add one"; on_tap => { count += 1; } }
    }
}
```

> **Status: early.** Version 0.1.0, not published on crates.io, and the API
> will change. Read the [status table](#status) before choosing it for a
> project.

## Try the example app

`examples/notes` is a notes app with search, pinning, an editor and saved data.
It runs on all three platform families from the same code.

```bash
# Desktop
cargo run -p craftui-example-notes --features desktop

# Web: writes dist/notes-web, then serve that folder
cargo run -p craftui-cli -- build --platform web --release -p craftui-example-notes -o dist/notes-web

# Android: writes a Gradle project, then builds the APK
cargo run -p craftui-cli -- build --platform android --release -p craftui-example-notes -o dist/notes-android
gradle -p dist/notes-android assembleDebug
```

Requirements and details are in [`examples/notes/README.md`](examples/notes/README.md).

## Status

What the project's CI checks on every change, and what it does not.

| Area | Checked automatically | Not checked |
|---|---|---|
| Framework (Linux) | Formatting, clippy with warnings denied, the whole test suite. UI tests run without a window. | Real windows, GPU output, system dialogs. |
| Web | The notes app is built to WebAssembly and driven in headless Chrome: it starts, paints, opens the editor, accepts typing, saves, and restores after a reload. | Other browsers; real GPUs; touch input; mobile browsers. |
| Android | The notes app is built into a debug APK and the package is inspected (id, native library, entry point, SDK level). | **Running on a device or emulator.** Release signing. |
| macOS, Windows | Clippy with warnings denied and the whole test suite, on every change. | Real windows, GPU output, system dialogs. |
| iOS | Nothing beyond compiling shared code. A platform crate and project templates exist. | Building, linking and running an iOS app. |

Known gaps:

- Some widget names are accepted by the language but not implemented yet; see
  the end of the [widget catalog](docs/src/widgets/catalog.md).
- A component cannot take child elements from the place it is used.
- Editing `.cui` files while the app runs uses a separate interpreter from the
  build-time compiler, and the two can differ.
- The Android project template declares `INTERNET` and biometric permissions
  even when the app asks for none.
- The WebAssembly file for the notes app is about 6.2 MB before compression.

## What is in the box

- **The `.cui` language**: components, typed properties, callbacks, two-way
  bindings (`<=>`), `if` / `for`, imports, theme tokens, `animate` blocks.
  Errors are reported at build time with file and line.
- **Reactive state**: signals, computed values and effects. Only the parts of
  the interface that read a changed value are updated.
- **Widgets**: layout (`Row`, `Column`, `Stack`, `Grid`, `ScrollView`,
  `SafeArea`), text and input (`Text`, `TextInput`, `TextArea`, `Checkbox`,
  `Switch`, `Slider`, `Select`), lists (`VirtualList`, `VirtualGrid`,
  `DataTable`), overlays (`Modal`, `BottomSheet`, `ToastHost`), `Canvas`, and a
  Material-style component library written in `.cui`.
- **Rendering**: a GPU renderer on `wgpu`, an optional Vello renderer, and CPU
  renderers for machines without a GPU and for tests.
- **Themes**: light, dark, AMOLED and high-contrast token sets.
- **Platform services**: clipboard, file dialogs, permissions, haptics, sharing,
  secure storage. How complete each one is differs by platform.
- **Tools**: the `craftui` command (`new`, `run`, `build`, `check`, `preview`,
  `doctor`, `permissions`), a language server, and a VS Code extension in
  `editors/vscode`.

## Start a project

```bash
git clone https://github.com/krinry/Craftui
cd Craftui
cargo install --path tools/craftui-cli

# Create the app beside the clone, not inside it: a package inside `Craftui`
# would be caught by that workspace, which does not list it, and Cargo would
# reject the build.
cd ..
craftui new my_app
cd my_app
```

Because CraftUI is not on crates.io, open the new project's `Cargo.toml` and
point the `craftui` dependency at your clone:

```toml
craftui = { path = "../Craftui/crates/craftui" }
```

Then:

```bash
craftui check ui/main.cui     # validate the interface file
craftui run --platform desktop
```

## Documentation

The book is in [`docs/`](docs/src/SUMMARY.md):

- [Installation](docs/src/getting-started/installation-linux-android.md)
- [Quickstart](docs/src/getting-started/quickstart.md)
- [The `.cui` language](docs/src/dsl/syntax-guide.md)
- [Reactivity and Rust](docs/src/dsl/reactivity.md)
- [Widget catalog](docs/src/widgets/catalog.md)
- [Building the notes app](docs/src/guides/notes-app.md)
- Platform guides: [desktop](docs/src/platforms/linux-desktop.md),
  [Android](docs/src/platforms/android-on-linux.md),
  [Web](docs/src/platforms/web.md)

Every `cui` example in the documentation is compiled by the test suite, so the
examples stay valid as the language changes.

Design notes and plans written before and during development are in
[`docs/design/`](docs/design/README.md). They describe intent and are not always
accurate about the current code; where they disagree with the book, the book is
right.

## Developing CraftUI

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the layout of the repository and
[`CHANGELOG.md`](CHANGELOG.md) for recent changes.

## License

[MIT](LICENSE). Dependencies under GPL, LGPL or AGPL are not accepted; CI
enforces this with `cargo deny`.
