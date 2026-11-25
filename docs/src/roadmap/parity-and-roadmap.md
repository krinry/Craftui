# Parity & Roadmap

This document provides an honest, engineering-level assessment of where **CraftUI** (`v0.1.0`) stands today compared to **Slint**, **Flutter**, and **Tauri 2.0**, along with the concrete roadmap to close the remaining gaps.

---

## Current Parity Snapshot

| Area | Parity | Closest Benchmark | Current Reality |
| :--- | :---: | :--- | :--- |
| **Core Architecture & Reactivity** | **65–70%** | Slint / Dioxus | Fine-grained `Signal`, `Computed`, `Effect`, and generational `Scope` arena; `taffy` Flexbox + CSS Grid layout; `parley` + `fontique` text shaping; `accesskit` accessibility; `.cui` lexer/parser/validator/codegen + hot-reload interpreter. |
| **Tooling, DX & AI** | **65–75%** | Slint / Flutter | `craftui` CLI (`new`, `init`, `setup`, `doctor`, `dev`, `preview`, `check`, `render`, `build`, `permissions`, `mcp`), `craftui-lsp` (`tower-lsp`), VS Code extension + snippets, Craftpad browser studio, and a built-in **MCP stdio server**. |
| **UI & Rendering** | **45–50%** | Slint / Flutter | Pure-WGSL instanced SDF GPU shader (`gpu-sdf`), `wgpu` surface pipeline, `vello_cpu` / `tiny-skia` CPU fallbacks, SVG icons, shadows, and backdrop blur. Lacks complex shader effects, advanced clip/mask layers, and native OS look-and-feel (Cupertino / WinUI). |
| **Widgets & App Features** | **35–40%** | Flutter | 38 `WidgetKind` primitives (`Row`, `Column`, `Grid`, `Stack`, `ScrollView`, `VirtualList`, `DataTable`, `SplitPane`, `TextInput`, `TextArea`, `RichText`, `Canvas`, `Overlay`, `ToastHost`, `CustomWidget`) + Material 3 `.cui` library (`std_material.rs`). Missing DatePicker, TimePicker, TreeView, ReorderableList, Drag-and-Drop, Multi-window, and native OS MenuBar/Tray. |
| **Multi-Platform Power** | **30–35%** | Flutter / Tauri | **Desktop (~65%):** `winit` + `wgpu` + `rfd` + clipboard + `accesskit`.<br>**Web WASM (~50%):** `wasm-bindgen` + Craftpad + headless Chrome CI.<br>**Android (~25–30%):** `GameActivity` + JNI + Gradle template generator; lacks real-device CI & production native plugins.<br>**iOS (~20–25%):** `IosAppHost` + C-FFI + Metal hook; early bridge stage. |
| **Ecosystem & Packages** | **5–15%** | pub.dev / crates.io | Direct `crates.io` support via Cargo + `craftui add` CLI, and a `MethodChannel` (`PluginRegistry`) bridge in `craftui-core` for native Android/iOS/Desktop plugins. No centralized CraftUI package registry yet. |

---

## Roadmap

### Phase 1 — Ecosystem & Async Ergonomics (v0.2)
- **First-class `crates.io` integration:** `craftui add <crate>` and `[dependencies]` tracking in `craftui.toml`, paired with `ResourceState<T>` (`Loading` / `Ready` / `Error`) in `craftui-reactive`.
- **Flutter-compatible `MethodChannel` bridge:** `PluginRegistry` + `CraftPlugin` + `MethodCall` in `craftui-core` so native Android (Kotlin) and iOS (Swift) handlers—including handlers ported from `pub.dev` plugins—can be invoked cleanly from Rust.

### Phase 2 — Widget & DSL Parity (v0.3)
- **Child-slot composition (`@children`):** Allow `.cui` components to accept arbitrary child elements from call sites.
- **Missing production widgets:** `DatePicker`, `TimePicker`, `TreeView`, `ReorderableList`, and widget-to-widget Drag-and-Drop.
- **Unified DSL compiler/runtime:** Eliminate divergence between AOT `codegen.rs` and hot-reload `interpreter.rs`.
- **Multi-window & native menus:** Desktop `Window` management and OS menu bar / system tray integration.

### Phase 3 — Mobile & Web Production Hardening (v0.4)
- **Android & iOS real-device CI:** Automated emulator/simulator smoke tests and release signing workflows.
- **Core native plugins:** Official `MethodChannel` plugins for Camera, Biometrics, Local Notifications, and Deep Links.
- **Web WASM size optimization:** Target `< 2 MB` gzipped WASM bundles via feature-splitting and `wasm-opt`.
