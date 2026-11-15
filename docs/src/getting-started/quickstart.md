# Quickstart: your first app

This page takes you from nothing to a running counter app, and explains each
file on the way. It assumes the tools from
[Installation](installation-linux-android.md) are in place.

## 1. Create a project

Create the project **beside** the `Craftui` checkout, not inside it. A package
inside that directory is caught by the repository's workspace, which does not
list it, and Cargo then refuses to build it.

```bash
cd ..                # leave the Craftui checkout first
craftui new my_app
cd my_app
```

`craftui new` accepts `--template counter` (the default), `--template dashboard`
or `--template mobile`. All three create the same layout:

```text
my_app/
├── Cargo.toml      Rust package: depends on craftui, build-depends on craftui-dsl
├── craftui.toml    App name, id, version and permissions, used when packaging
├── build.rs        Compiles every .cui file in ui/ to Rust before the crate builds
├── src/
│   └── main.rs     Starts the app and connects Rust code to the UI
└── ui/
    └── main.cui    The user interface
```

> CraftUI is not on crates.io yet. Until it is, point the two dependencies in
> `Cargo.toml` at a checkout of this repository:
>
> ```toml
> [dependencies]
> craftui = { path = "../Craftui/crates/craftui" }
>
> [build-dependencies]
> craftui-dsl = { path = "../Craftui/crates/craftui-dsl" }
> ```

## 2. The user interface: `ui/main.cui`

```cui
export component MainView {
    in-out property <int> count: 0;

    SafeArea {
        Column {
            width: 100%;
            height: 100%;
            background: Theme.background;
            padding: 24dp;
            gap: 16dp;

            Text {
                text: "CraftUI App";
                font_size: 24;
                color: Theme.text_primary;
            }
            Text {
                text: count;
                font_size: 20;
                color: Theme.primary;
            }
            Row {
                gap: 12dp;

                Button {
                    text: "Increment";
                    background: Theme.primary;
                    on_tap => { count = count + 1; }
                }
                Button {
                    text: "Reset";
                    on_tap => { count = 0; }
                }
            }
        }
    }
}
```

Reading it from the top:

- `component MainView` declares a piece of UI. `export` makes the generated Rust
  type public.
- `in-out property <int> count: 0;` is the component's state: a whole number
  that starts at zero.
- `SafeArea` keeps its content clear of notches and system bars on phones.
- `Column` and `Row` arrange their children vertically and horizontally.
- `text: count;` is a binding. Whenever `count` changes, this text updates. You
  never refresh it yourself.
- `on_tap => { ... }` runs when the button is pressed.

[The `.cui` language](../dsl/syntax-guide.md) covers everything else.

## 3. The Rust side: `src/main.rs`

```rust
use craftui::prelude::*;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Starter".to_string(),
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let ui = generated::MainView::new(cx);
            ui.build(cx, tree)
        },
    )
}
```

`build.rs` turned `ui/main.cui` into `main.rs` inside Cargo's output directory;
`include!` brings it in. The generated `MainView` struct has one field per
property, each a reactive signal, so Rust can read and change the UI state:

```rust
let ui = generated::MainView::new(cx);
ui.count.set(10);                 // the text on screen will show 10
let current = ui.count.get();
```

[Reactivity and Rust](../dsl/reactivity.md) shows how to connect real
application logic.

## 4. Run it

```bash
cargo run                          # desktop window
craftui run --platform web         # builds to WebAssembly and serves it on http://localhost:8080
craftui run --platform android     # builds an APK, installs it on a connected device and starts it
```

The web target needs `wasm-bindgen-cli`; Android needs the SDK, the NDK and a
device or emulator visible to `adb`. Run `craftui setup --check-only` to see
what is missing.

## 5. Edit and check

While you work on a `.cui` file:

```bash
craftui check ui/main.cui          # report syntax and type errors without building
craftui preview ui/main.cui        # live preview window that reloads when the file is saved
```

## Where next

- [The `.cui` language](../dsl/syntax-guide.md): the complete language.
- [Widget catalog](../widgets/catalog.md): every widget with its properties.
- [Building the Notes app](../guides/notes-app.md): a complete app for Android
  and the web, explained step by step.
