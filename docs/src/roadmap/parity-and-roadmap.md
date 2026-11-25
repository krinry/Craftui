# Parity & Roadmap (Detailed Master Plan to 100%)

This document provides an honest, engineering-level assessment of where **CraftUI** (`v0.1.0`) stands today compared to **Slint**, **Flutter**, and **Tauri 2.0**, along with the exact step-by-step blueprint to reach full production parity.

---

## Current Parity Snapshot (Updated After Step 4)

| Area | Parity | Closest Benchmark | Current Reality |
| :--- | :---: | :--- | :--- |
| **Core Architecture & Reactivity** | **82–86%** | Slint / Dioxus | Fine-grained `Signal`, `Computed`, `Effect`, `ResourceState<T>`, and generational `Scope` arena; `taffy` Flexbox + CSS Grid layout + runtime `set_locale`/`set_direction` RTL flipping; `LocalizationCatalog` JSON catalog + `{param}` interpolation + BCP-47 RTL detection; `parley` + `fontique` text shaping; `accesskit` accessibility; `.cui` AOT codegen + unified hot-reload interpreter + `@children` slot composition. |
| **Tooling, DX & AI** | **76–80%** | Slint / Flutter | `craftui` CLI (`new --template counter/dashboard/mobile/plugin` + `--plugin`, `init`, `add`, `setup`, `doctor`, `dev`, `preview`, `check`, `render`, `build`, `permissions`, `mcp`), `craftui-lsp` (`tower-lsp`), VS Code extension + snippets, Craftpad studio, and built-in **MCP stdio server**. Full workspace metadata (`description`, `license`, `repository`, `version`) configured for `crates.io`. |
| **UI & Rendering** | **52–58%** | Slint / Flutter | Pure-WGSL instanced SDF GPU shader (`gpu-sdf`), `wgpu` surface pipeline, `vello_cpu` / `tiny-skia` CPU fallbacks, SVG icons, shadows, backdrop blur, `DesignSystem::Material3` / `Cupertino` / `WinUi` palettes, RTL text alignment mirroring, and `PlatformView` video overlay holes. Still lacks custom user WGSL shader effects, advanced clip/mask filters, and OS-native widget chrome drawing. |
| **Widgets & App Features** | **64–70%** | Flutter | **47 `WidgetKind` primitives** including `MediaPlayer` (`VideoPlayer`/`AudioPlayer` with `PlatformViewId` + transport controls), `RichTextEditor` (`B`/`I`/`U` toolbar + styled runs + IME), `DatePicker`, `TimePicker`, `TreeView`, `ReorderableList`, `MenuBar`, `Draggable`, `DropTarget`, `VirtualList`, `DataTable`, `SplitPane`, `TextInput`, `TextArea`, `RichText`, `Canvas`, `Overlay`, `ToastHost`, `CustomWidget` + `craftui_widgets::form` reactive `FieldValidator`/`FormGroup`. Still missing native OS System Tray and hardware video codecs (relies on host `PlatformView`). |
| **Multi-Platform Power** | **50–54%** | Flutter / Tauri | **Desktop (~74%):** `winit` + `wgpu` + `rfd` + clipboard + `accesskit` + Multi-Window `WindowLauncher` + RTL + `WinUi`/`Cupertino` presets.<br>**Web WASM (~66%):** `wasm-bindgen` + Craftpad + headless Chrome CI + lean `gpu-sdf`/`cpu-tiny-skia` builds (**4.1 MB raw / ~1.21 MB gzipped** for `notes`).<br>**Android (~38–42%):** `GameActivity` + JNI + Gradle generator + live Kotlin `dispatchCraftUiMethodChannel` & JNI forwarding + `android-emulator-smoke` CI job in `ci.yml` (template & JNI verified locally; live emulator run is CI-only).<br>**iOS (~30–35%):** `IosAppHost` + C-FFI + Metal hook + `CraftUIMethodChannelRouter` Swift handler (`camera`, `biometrics`, `notifications`, `url_launcher`) + updated `CraftUI.h` header (not yet compiled/tested on macOS/Xcode). |
| **Ecosystem & Packages** | **28–32%** | pub.dev / crates.io | `crates.io` dependency support (`craftui add`), `craftui new --plugin` scaffolding, async `MethodChannel` pipelines on Rust/Android/iOS, **4 built-in plugins** (`camera`, `biometrics`, `notifications`, `url_launcher`), and workspace crates metadata ready (`cargo publish` still pending live token/release). Community plugin volume remains far smaller than `pub.dev` or `npm`. |

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
   - Verified every `BUILTIN_WIDGETS` item validates and instantiates a node in both `codegen.rs` and `interpreter.rs` (smoke parity coverage; convenience aliases such as `CheckBoxTile` map to layout containers).

### Step 3 — Live Mobile Plugins & Real-Device CI (Completed on Branch)
1. **Kotlin & Swift `MethodChannel` Dispatchers:**
   - Implemented `dispatchCraftUiMethodChannel` in generated `MainActivity.kt` (`tools/craftui-cli/src/platform/android.rs`) and live JNI command forwarding (`forward_method_channel_to_kotlin` in `crates/craftui-platform-android/src/runtime.rs`).
   - Added `craftui_ios_app_deliver_method_channel_result` to `CraftUI.h` and `CraftUIMethodChannelRouter` in `CraftUIPermissions.swift` / `CraftUIViewController.swift` (`tools/craftui-cli/src/platform/ios.rs`).
2. **4 Official Built-in `MethodChannel` Plugins (`crates/craftui-core/src/builtin_plugins.rs`):**
   - `dev.craftui.camera` (`CameraPlugin` / `CameraClient`: `capturePhoto`, `pickImage`)
   - `dev.craftui.biometrics` (`BiometricsPlugin` / `BiometricsClient`: `isAvailable`, `authenticate` via `BiometricPrompt` on Android and `LAContext` on iOS)
   - `dev.craftui.notifications` (`NotificationsPlugin` / `NotificationsClient`: `schedule`, `cancel`, `cancelAll` via `NotificationManagerCompat` and `UNUserNotificationCenter`)
   - `dev.craftui.url_launcher` (`UrlLauncherPlugin` / `UrlLauncherClient`: `canLaunch`, `launch` via `Intent.ACTION_VIEW` and `UIApplication.shared.open`)
3. **Automated Android Emulator CI (`android-emulator-smoke` in `.github/workflows/ci.yml`):**
   - Added a `macos-14` `android-emulator-smoke` job running `reactivecircus/android-emulator-runner@v2` (`api-level: 34`, `arch: arm64-v8a`) to install, launch, verify `pidof dev.craftui.notes`, and scan `adb logcat` for fatal exceptions on the built Notes APK. Note: iOS Xcode simulator CI (`macos-latest` Swift compilation) remains pending for Step 4+.

### Step 4 — i18n/RTL, Media & Rich Editor, Adaptive Presets & Plugin CLI (Completed on Branch)
1. **i18n / Localization & Full RTL (`crates/craftui-core/src/i18n.rs`, `craftui-widgets`):**
   - Added `LocalizationCatalog` (JSON locale loading, regional subtag -> fallback locale chain, `{param}` interpolation) and `is_rtl_locale` / `locale_direction` helpers.
   - Added `WidgetTree::set_direction` / `set_locale` + automatic `taffy` flex reversal and `effective_text_align` Start/End mirroring for `Text`, `RichText`, `TextInput`, and `RichTextEditor`.
2. **Media & Rich Editing (`crates/craftui-widgets/src/media_editor.rs` + DSL/Runtime):**
   - Added `WidgetKind::MediaPlayer` (`VideoPlayer`, `AudioPlayer`, `MediaPlayer`) with `MediaController` transport state (`play/pause`, `seek_fraction`, `mute`, `PlatformViewId` video surface hole emission) across widgets, validator, AOT codegen, and hot-reload interpreter.
   - Added `WidgetKind::RichTextEditor` with `RichEditorController` (`B`/`I`/`U` formatting toolbar, `StyledSpanRange` management, cursor/keyboard/Ctrl+B/I/U & IME input).
3. **Adaptive Design Presets & Plugin Scaffolding (`crates/craftui-core/src/theme.rs`, `tools/craftui-cli`):**
   - Added `DesignSystem::{Material3, Cupertino, WinUi}`, `ThemeTokens::cupertino_light/dark`, `winui_light/dark`, `from_preset`, and `WidgetTree::set_design_preset`.
   - Added `craftui new <name> --plugin` / `--template plugin` scaffolding (`CraftPlugin` Rust crate + Kotlin/Swift stubs + `craftui.toml`) and completed `description` metadata across workspace `Cargo.toml` files (`crates.io` live publishing still requires maintainer credentials/release).

### Remaining Path from ~60–65% to 90–100% (Beyond Step 4)

#### Phase A — Real CI Validation, iOS Build Check & Example Dogfooding (Highest Priority)
1. **Full CI & Live Emulator/Device Verification:**
   - Verify `notes-web`, `notes-android`, and `android-emulator-smoke` (`macos-14` ARM64 emulator) end-to-end on GitHub Actions CI, plus add an `ios-sim` compilation check (`cargo build --target aarch64-apple-ios-sim` + `swiftc` check for generated Swift templates) on `macos-14` / `macos-latest`.
2. **Real App Dogfooding (`examples/showcase` / `examples/notes`):**
   - Exercise `MenuBar`, `Draggable`/`DropTarget`, `FormGroup`, `DatePicker`, `TimePicker`, `MediaPlayer`, `RichTextEditor`, live `set_locale("ar-SA")` RTL toggling, and `Material3`/`Cupertino`/`WinUi` switching inside interactive example apps.

#### Phase B — High-Impact Product Gaps (~65% -> ~80%+)
1. **Desktop System Tray & Global Shortcuts (`craftui-platform-desktop`):**
   - Native OS tray icon + context menu + show/hide window toggles and global keyboard shortcuts for Tauri-level desktop parity (~74% -> ~90%).
2. **True Adaptive Widget Chrome (`paint.rs` + `DesignSystem`):**
   - Adapt widget geometry and visuals (e.g., iOS pill toggles/sliders for `Cupertino`, Fluent toggles/buttons for `WinUi`) rather than swapping only colors and radii.
3. **`crates.io` Multi-Crate Publish Pipeline:**
   - Verify `cargo publish --dry-run` across all 15 workspace crates in topological dependency order and publish releases so `craftui new` projects compile externally without workspace overrides.

#### Phase C — Deep Rendering, Editor & Media Polish (~80% -> ~95%+)
1. **Custom WGSL Shader Hooks & Effects:** User-extensible fragment shaders, blur/mask filters, and richer `Canvas` effects.
2. **`RichTextEditor` Span Undo/Redo & Rich Clipboard:** Snapshot `StyledSpanRange` alongside plain text edits so `Ctrl+Z` / `Ctrl+Y` undo/redo formatting, plus scrollable multi-line viewport & rich copy/cut.
3. **Real Hardware Media Backends:** Connect `MediaPlayer` `PlatformView` textures to desktop media playback, Android `ExoPlayer`/`MediaPlayer`, and iOS `AVPlayer`.
