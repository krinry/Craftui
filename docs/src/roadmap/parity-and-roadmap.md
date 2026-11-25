# Parity & Roadmap

This document provides an honest, engineering-level assessment of where **CraftUI** (`v0.1.0`) stands today compared to **Slint**, **Flutter**, and **Tauri 2.0**, along with the concrete roadmap to close the remaining gaps.

---

## Current Parity Snapshot (Updated After Phase 2)

| Area | Parity | Closest Benchmark | Current Reality |
| :--- | :---: | :--- | :--- |
| **Core Architecture & Reactivity** | **70–75%** | Slint / Dioxus | Fine-grained `Signal`, `Computed`, `Effect`, `ResourceState<T>`, and generational `Scope` arena; `taffy` Flexbox + CSS Grid layout; `parley` + `fontique` text shaping + fallback font APIs; `accesskit` accessibility; `.cui` lexer/parser/validator/codegen + hot-reload interpreter + `@children` slot composition. |
| **Tooling, DX & AI** | **70–75%** | Slint / Flutter | `craftui` CLI (`new`, `init`, `add`, `setup`, `doctor`, `dev`, `preview`, `check`, `render`, `build`, `permissions`, `mcp`), `craftui-lsp` (`tower-lsp`), VS Code extension + snippets, Craftpad browser studio, and a built-in **MCP stdio server**. |
| **UI & Rendering** | **45–50%** | Slint / Flutter | Pure-WGSL instanced SDF GPU shader (`gpu-sdf`), `wgpu` surface pipeline, `vello_cpu` / `tiny-skia` CPU fallbacks, SVG icons, shadows, and backdrop blur. Lacks complex shader effects, advanced clip/mask layers, and native OS look-and-feel (Cupertino / WinUI). |
| **Widgets & App Features** | **45–50%** | Flutter | 41 `WidgetKind` primitives (including `DatePicker`, `TimePicker`, `TreeView`, `VirtualList`, `DataTable`, `SplitPane`, `TextInput`, `TextArea`, `RichText`, `Canvas`, `Overlay`, `ToastHost`, `CustomWidget`) + Material 3 `.cui` library (`std_material.rs`) + `@children` wrapper composition. Still missing `ReorderableList`, widget-to-widget Drag-and-Drop, Multi-window, and native OS MenuBar/Tray. |
| **Multi-Platform Power** | **32–38%** | Flutter / Tauri | **Desktop (~65%):** `winit` + `wgpu` + `rfd` + clipboard + `accesskit`.<br>**Web WASM (~50%):** `wasm-bindgen` + Craftpad + headless Chrome CI.<br>**Android (~28–32%):** `GameActivity` + JNI + Gradle template generator + `MethodChannel` host wiring; lacks real-device CI & production native plugins.<br>**iOS (~22–28%):** `IosAppHost` + C-FFI + Metal hook + `MethodChannel` host wiring; early bridge stage without Xcode device CI. |
| **Ecosystem & Packages** | **12–18%** | pub.dev / crates.io | First-class `crates.io` support via `craftui add <crate>` + `craftui.toml` `[dependencies]` + `ResourceState<T>`, and `MethodChannel` (`PluginRegistry`) wired into `PlatformServices`, `AndroidHost`, and `IosAppHost`. Real-world score remains early until ready-made Android Kotlin / iOS Swift plugin packages ship. |

---

## Roadmap

### Phase 1 — Ecosystem & Async Ergonomics (Completed on Branch)
- **First-class `crates.io` integration:** `craftui add <crate>`, `[dependencies]` / `[plugins]` in `craftui.toml`, and `ResourceState<T>` in `craftui-reactive`.
- **Flutter-compatible `MethodChannel` bridge:** `PluginRegistry`, `CraftPlugin`, and `MethodCall` in `craftui-core` wired into `PlatformServices`, `AndroidHost`, and `IosAppHost`.

### Phase 2 — Widget & DSL Parity (Partially Completed; Remaining Items Below)
- **Completed:**
  - Child-slot composition (`@children`) across `.cui` parser, validator, AOT codegen (`build_with_children`), and hot-reload interpreter.
  - `DatePicker`, `TimePicker`, and `TreeView` interactive widgets in `craftui-widgets`, `.cui` DSL, runtime interpreter, and `craftui-lsp`.
- **Remaining:**
  - `ReorderableList` and widget-to-widget Drag-and-Drop.
  - Multi-window management & native OS menu bar / system tray integration.
  - Unify remaining differences between AOT `codegen.rs` and hot-reload `interpreter.rs`.

### Phase 3 — Mobile & Web Production Hardening
- **Android & iOS real-device CI & live JNI/Swift `MethodChannel` dispatchers:** Connect `PluginRegistry` over JNI (`MainActivity.kt`) and Swift C-FFI so Kotlin/Swift plugins work out-of-the-box without writing Rust boilerplate.
- **Core native plugins:** Official plugins for Camera, Biometrics, Local Notifications, and Deep Links.
- **Web WASM size optimization:** Target `< 2 MB` gzipped WASM bundles via feature-splitting and `wasm-opt`.
