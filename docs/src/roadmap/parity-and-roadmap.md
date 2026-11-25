# Parity & Roadmap (Detailed Master Plan to 100%)

This document provides an honest, engineering-level assessment of where **CraftUI** (`v0.1.0`) stands today compared to **Slint**, **Flutter**, and **Tauri 2.0**, along with the exact step-by-step blueprint to reach full production parity.

---

## Current Parity Snapshot (Updated After Step 2)

| Area | Parity | Closest Benchmark | Current Reality |
| :--- | :---: | :--- | :--- |
| **Core Architecture & Reactivity** | **76–80%** | Slint / Dioxus | Fine-grained `Signal`, `Computed`, `Effect`, `ResourceState<T>`, and generational `Scope` arena; `taffy` Flexbox + CSS Grid layout; `parley` + `fontique` text shaping + fallback font APIs; `accesskit` accessibility; `.cui` lexer/parser/validator/codegen + unified hot-reload interpreter (every `BUILTIN_WIDGETS` & Material 3 variant aligned) + `@children` slot composition. |
| **Tooling, DX & AI** | **72–76%** | Slint / Flutter | `craftui` CLI (`new`, `init`, `add`, `setup`, `doctor`, `dev`, `preview`, `check`, `render`, `build`, `permissions`, `mcp`), `craftui-lsp` (`tower-lsp`), VS Code extension + snippets, Craftpad browser studio, and a built-in **MCP stdio server**. |
| **UI & Rendering** | **45–50%** | Slint / Flutter | Pure-WGSL instanced SDF GPU shader (`gpu-sdf`), `wgpu` surface pipeline, `vello_cpu` / `tiny-skia` CPU fallbacks, SVG icons, shadows, and backdrop blur. Lacks complex shader effects, advanced clip/mask layers, and native OS look-and-feel (Cupertino / WinUI). |
| **Widgets & App Features** | **55–60%** | Flutter | 45 `WidgetKind` primitives (including `DatePicker`, `TimePicker`, `TreeView`, `ReorderableList`, `MenuBar`, `Draggable`, `DropTarget`, `VirtualList`, `DataTable`, `SplitPane`, `TextInput`, `TextArea`, `RichText`, `Canvas`, `Overlay`, `ToastHost`, `CustomWidget`) + `craftui_widgets::form` reactive `FieldValidator`/`FormGroup` + Material 3 `.cui` library + `@children` wrapper composition. Still missing native OS System Tray, Rich Text *Editor*, and Media widgets. |
| **Multi-Platform Power** | **42–46%** | Flutter / Tauri | **Desktop (~72%):** `winit` + `wgpu` + `rfd` + clipboard + `accesskit` + Multi-Window `WindowLauncher` (`open_window`, `close_window`, `set_title`, context-injected `WindowId`).<br>**Web WASM (~65%):** `wasm-bindgen` + Craftpad + headless Chrome CI + lean `gpu-sdf`/`cpu-tiny-skia` Web builds (**4.1 MB raw / ~1.21 MB gzipped** for `notes`).<br>**Android (~30–35%):** `GameActivity` + JNI + Gradle template generator + async `InvokeMethodChannel` JNI bridge.<br>**iOS (~25–30%):** `IosAppHost` + C-FFI + Metal hook + async `InvokeMethodChannel` C-ABI JSON bridge. |
| **Ecosystem & Packages** | **15–20%** | pub.dev / crates.io | First-class `crates.io` support (`craftui add <crate>`, `[dependencies]`/`[plugins]` in `craftui.toml`, `ResourceState<T>`) plus async cross-language `MethodChannel` pipelines on Rust, Android JNI, and iOS C-FFI. |

---

## Master Execution Plan to 100% Parity

### Step 1 — Desktop, Widgets & Forms Parity (Completed on Branch)
1. **Multi-Window Support (`craftui-platform-desktop`):** `WindowLauncher` (`open_window`, `close_window`, `set_title`) + `WindowId` & `WindowLauncher` injected into `root_scope` context.
2. **Application `MenuBar` (`WidgetKind::MenuBar`):** Top horizontal menu bar with floating dropdown menus, outside-click/Escape dismissal, and `on_select("Menu/Item")`.
3. **Generic Cross-Widget Drag-and-Drop (`Draggable` & `DropTarget`):** Payload-based cross-widget drag sessions with floating drag pill and drop target highlight.
4. **Reactive Form Validation (`craftui_widgets::form`):** `ValidationRule` (`Required`, `MinLength`, `MaxLength`, `Email`, `Contains`), `FieldValidator`, and `FormGroup`.

### Step 2 — Web WASM Size Optimization & Unified DSL (Completed on Branch)
1. **Lean Web WASM Builds (`~1.21 MB` gzipped):**
   - Split `craftui-platform-web/Cargo.toml` default features to `["gpu-sdf", "cpu-tiny-skia"]` (with opt-in `full-renderers`), added `web-lean` feature to `craftui`, trimmed `examples/notes/Cargo.toml` so `vello`/`vello_cpu` only build for `desktop`, and switched `wasm-opt` to `-Oz --strip-debug` in `tools/craftui-cli/src/platform/web.rs`.
   - Verified `examples/notes` release WASM shrank from ~6.2 MB to **4.1 MB raw (`1,272,811` bytes / ~1.21 MB gzipped)**.
2. **Aligned AOT `codegen.rs` and Hot-Reload `interpreter.rs`:**
   - Added missing Material 3 widgets & aliases (`Card`, `ElevatedCard`, `FilledCard`, `OutlinedCard`, `ListTile`, `Chip`/`FilterChip`/`ActionChip`/`ChoiceChip`/`InputChip`, `AppBar`, `Drawer`, `FilledButton`..`OutlinedIconButton`, `HorizontalDivider`/`VerticalDivider`, `LinearProgressIndicator`, `CircularProgressIndicator`, semantic HTML tags) to `interpreter.rs`.
   - Fixed a latent `ThemeTokens::on_surface` typo in `codegen.rs` (`TonalButton`/`TonalIconButton`) -> `text_primary`.
   - Verified every `BUILTIN_WIDGETS` item passes in both `codegen.rs` and `interpreter.rs`.

### Step 3 — Live Mobile Plugins & Real-Device CI (Next Up — Target: Mobile ~65%, Ecosystem ~55%)
1. **Kotlin & Swift `MethodChannel` Dispatchers:**
   - Implement live `MainActivity.kt` command dispatcher and `CraftUIBridge.swift` JSON handler for `InvokeMethodChannel`.
2. **4 Official Built-in `MethodChannel` Plugins:**
   - `dev.craftui.camera` (Camera & Gallery PhotoPicker)
   - `dev.craftui.biometrics` (Fingerprint / FaceID)
   - `dev.craftui.notifications` (Local Scheduled Notifications)
   - `dev.craftui.url_launcher` (External URLs & Deep Links)
3. **Automated Android Emulator & iOS Simulator CI:**
   - Boot headless Android emulator in CI to install, launch, and smoke-test the APK (beyond static APK inspection).

### Step 4 — Polish, i18n/RTL, Media & 1.0 Release (Target: 90–100%)
1. **i18n / Localization & Full RTL (`Direction::Rtl`):**
   - String translation catalog loader (JSON/Fluent), locale signal switching, and automatic RTL layout/text mirroring in `taffy` + `parley`.
2. **Media & Rich Editing:**
   - Audio/Video playback surface backed by `PlatformView` textures and interactive `RichTextEditor` (styled spans + cursor editing).
3. **Native Adaptive Presets & `crates.io` Publishing:**
   - Cupertino (iOS/macOS) and WinUI (Windows) token/component presets alongside Material 3.
   - Publish `craftui-*` crates to `crates.io` and add `craftui new --plugin` scaffolding template.
