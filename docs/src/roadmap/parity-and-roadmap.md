# Parity & Roadmap

This document provides an honest, engineering-level assessment of where **CraftUI** (`v0.1.0`) stands today compared to **Slint**, **Flutter**, and **Tauri 2.0**, along with the concrete roadmap to close the remaining gaps.

---

## Current Parity Snapshot (Updated After ReorderableList & Native MethodChannel Queues)

| Area | Parity | Closest Benchmark | Current Reality |
| :--- | :---: | :--- | :--- |
| **Core Architecture & Reactivity** | **70–75%** | Slint / Dioxus | Fine-grained `Signal`, `Computed`, `Effect`, `ResourceState<T>`, and generational `Scope` arena; `taffy` Flexbox + CSS Grid layout; `parley` + `fontique` text shaping + fallback font APIs; `accesskit` accessibility; `.cui` lexer/parser/validator/codegen + hot-reload interpreter + `@children` slot composition. |
| **Tooling, DX & AI** | **70–75%** | Slint / Flutter | `craftui` CLI (`new`, `init`, `add`, `setup`, `doctor`, `dev`, `preview`, `check`, `render`, `build`, `permissions`, `mcp`), `craftui-lsp` (`tower-lsp`), VS Code extension + snippets, Craftpad browser studio, and a built-in **MCP stdio server**. |
| **UI & Rendering** | **45–50%** | Slint / Flutter | Pure-WGSL instanced SDF GPU shader (`gpu-sdf`), `wgpu` surface pipeline, `vello_cpu` / `tiny-skia` CPU fallbacks, SVG icons, shadows, and backdrop blur. Lacks complex shader effects, advanced clip/mask layers, and native OS look-and-feel (Cupertino / WinUI). |
| **Widgets & App Features** | **48–52%** | Flutter | 42 `WidgetKind` primitives (including `DatePicker`, `TimePicker`, `TreeView`, `ReorderableList`, `VirtualList`, `DataTable`, `SplitPane`, `TextInput`, `TextArea`, `RichText`, `Canvas`, `Overlay`, `ToastHost`, `CustomWidget`) + Material 3 `.cui` library (`std_material.rs`) + `@children` wrapper composition. Still missing generic cross-widget Drag-and-Drop, Multi-window, and native OS MenuBar/Tray. |
| **Multi-Platform Power** | **35–40%** | Flutter / Tauri | **Desktop (~65%):** `winit` + `wgpu` + `rfd` + clipboard + `accesskit`.<br>**Web WASM (~50%):** `wasm-bindgen` + Craftpad + headless Chrome CI.<br>**Android (~30–35%):** `GameActivity` + JNI + Gradle template generator + async `InvokeMethodChannel` JNI bridge (`MainActivity.kt` hook); still lacks real-device CI & ready-made plugins.<br>**iOS (~25–30%):** `IosAppHost` + C-FFI + Metal hook + async `InvokeMethodChannel` C-ABI JSON bridge; still lacks Xcode device CI. |
| **Ecosystem & Packages** | **15–20%** | pub.dev / crates.io | First-class `crates.io` support (`craftui add <crate>`, `[dependencies]`/`[plugins]` in `craftui.toml`, `ResourceState<T>`) plus async cross-language `MethodChannel` pipelines on Rust, Android JNI, and iOS C-FFI. Real-world score stays early until official Camera/Biometrics/Notifications plugin implementations ship. |

---

## Roadmap

### Phase 1 — Ecosystem & Async Ergonomics (Completed on Branch)
- **First-class `crates.io` integration:** `craftui add <crate>`, `[dependencies]` / `[plugins]` in `craftui.toml`, and `ResourceState<T>` in `craftui-reactive`.
- **Flutter-compatible `MethodChannel` bridge:** `PluginRegistry`, `CraftPlugin`, and `MethodCall` in `craftui-core` wired into `PlatformServices`, `AndroidHost`, and `IosAppHost`.

### Phase 2 — Widget & DSL Parity (Mostly Completed; Remaining Items Below)
- **Completed:**
  - Child-slot composition (`@children`) across `.cui` parser, validator, AOT codegen (`build_with_children`), and hot-reload interpreter.
  - `DatePicker`, `TimePicker`, `TreeView`, and `ReorderableList` (drag grip + keyboard reorder) across `craftui-widgets`, `.cui` DSL, runtime interpreter, and `craftui-lsp`.
- **Remaining:**
  - Generic cross-widget Drag-and-Drop (`Draggable` / `DragTarget`).
  - Multi-window management & native OS menu bar / system tray integration.
  - Unify remaining differences between AOT `codegen.rs` and hot-reload `interpreter.rs`.

### Phase 3 — Mobile & Web Production Hardening (In Progress)
- **Completed:**
  - Async cross-language `MethodChannel` command queues + JNI (`nativeOnMethodChannelResult`) and iOS C-ABI (`craftui_ios_app_deliver_method_channel_result`) roundtrip bridges.
- **Remaining:**
  - Android & iOS real-device/emulator CI smoke runs and release signing automation.
  - Official `MethodChannel` plugin packages for Camera, Biometrics, Local Notifications, and Deep Links.
  - Web WASM size optimization: target `< 2 MB` gzipped WASM bundles via feature-splitting and `wasm-opt`.
