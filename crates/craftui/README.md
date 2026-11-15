# craftui

The crate that applications depend on. It re-exports the rest of the workspace
and provides `craftui::prelude`.

```rust
use craftui::prelude::*;
```

## Modules

| Path | Crate | Contents |
|---|---|---|
| `craftui::core` | `craftui-core` | Geometry, colours, events, theme tokens, platform services. |
| `craftui::reactive` | `craftui-reactive` | `Signal`, `Computed`, effects, `Scope`. |
| `craftui::layout` | `craftui-layout` | `LayoutStyle` and the layout engine. |
| `craftui::text` | `craftui-text` | Text shaping, measuring and editing state. |
| `craftui::render` | `craftui-render` | `PaintScene` and the renderers. |
| `craftui::widgets` | `craftui-widgets` | `WidgetTree` and every built-in widget. |
| `craftui::a11y` | `craftui-a11y` | Accessibility tree. |
| `craftui::dsl` | `craftui-dsl` | The `.cui` compiler. Feature `dsl`. |
| `craftui::runtime` | `craftui-runtime` | Run-time `.cui` interpreter. Feature `hot-reload`. |
| `craftui::platform_desktop`, `platform_android`, `platform_ios`, `platform_web` | `craftui-platform-*` | Platform runners, each behind its feature. |

## Features

| Feature | Default | Effect |
|---|---|---|
| `desktop` | yes | Desktop window runner (`run_app`). Turn off for Android and Web builds. |
| `android` | no | Android runner. |
| `ios` | no | iOS runner. |
| `web` | no | Web runner. |
| `dsl` | yes | The `.cui` compiler, for use from `build.rs`. |
| `hot-reload` | yes | Reload `.cui` files while the app runs. Implies `dsl`. |
| `embedded-fallback-font` | yes | Bundles a font so text renders without system fonts. Needed on the Web and in headless tests. |
| `gpu-sdf` | yes | The default GPU renderer. |
| `gpu-vello` | yes | The Vello GPU renderer. |
| `cpu-vello` | yes | Vello CPU renderer, used as a fallback and for headless rendering. |
| `cpu-tiny-skia` | yes | tiny-skia CPU renderer. |

A package that targets all three platform families usually disables the
defaults and re-enables what it needs, with `desktop` behind a feature of its
own. `examples/notes/Cargo.toml` shows the arrangement.

## Starting an app

```rust
use craftui::prelude::*;

mod ui {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(WindowConfig::default(), |cx, tree, _ctx| {
        let view = ui::MainView::new(cx);
        view.build(cx, tree)
    })
}
```

See the book in `docs/` for the language, the widgets and the platform guides.
