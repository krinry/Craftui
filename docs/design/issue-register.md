# CraftUI — Issue Register & Production Roadmap

> Last Audited: 2026-11-11  
> Purpose: Honest baseline audit of every bug, bottleneck, bloat source, visual gap, and platform gap in CraftUI, plus the phased plan to turn it into a top-tier MIT-licensed UI framework comparable to Slint, GPUI, and Flutter.

---

## Part 1: Verified Issue Register (Current Codebase Audit)

### 1. Performance & Power Bottlenecks
- [x] **PERF-01 (Critical): Per-frame text re-shaping & full scene rebuild** — Bounded 1024-entry `ShapedLayoutCache` added in `craftui-text` and wired across `WidgetTree`, `VelloCpuBackend`, and `GpuSurfacePipeline`; idle frames skip scene rebuilds across Desktop, Web, Android, and iOS.
- [x] **PERF-02 (High): Unconditional full-tree Taffy layout sync** — `LayoutEngine::compute_layout` now tracks `dirty_styles` and environment changes per node and eliminates per-node child `Vec` clones.
- [x] **PERF-03 (High): Unconditional full AccessKit tree rebuild** — Added `A11yBridge::build_incremental_update` and `a11y_active` tracking so AccessKit diffs only when state or focus changes while AT is active.
- [x] **PERF-04 (Critical): Always-on 60fps idle spin loops** — Replaced with 530ms demand-driven caret blink timers, `ControlFlow::Wait`/`WaitUntil` on Desktop, dynamic `poll_events` timeouts on Android, on-demand `requestAnimationFrame` + `setTimeout` on Web, and `needs_redraw` + deduplicated `SyncText` on iOS.

### 2. "Lightweight" & Footprint Contradictions
- [x] **BLOAT-01 (Critical): Unconditional triple renderer bundling** — Added strict Cargo feature flags in `craftui-render` (`gpu-sdf`, `gpu-vello`, `cpu-vello`, `cpu-tiny-skia`) and built the pure-WGSL instanced SDF + glyph-atlas + Kawase backdrop-blur renderer (`gpu_sdf_backend.rs` + `sdf_quad.wgsl`).
- [x] **BLOAT-02 (High): `TinySkiaBackend` incomplete** — Completed `TinySkiaBackend` with glyph rasterization (`glyph_raster.rs`), `DrawText`, rounded/spread/inset `DropShadow`, `PushClip`, `PushLayer`, `BackdropBlur`, `Transform2D`, and multi-stop gradients.
- [x] **BLOAT-03 (High): Compiler & AST interpreter shipped in every app** — Gated `craftui-dsl`, `craftui-runtime`, and `notify` behind `dsl` and `hot-reload` features across `craftui` and `craftui-platform-desktop`; verified `--no-default-features --features desktop,gpu-sdf` excludes `vello`, `craftui-dsl`, `craftui-runtime`, and `notify`.
- [x] **BLOAT-04 (Critical): Zero bundled fallback font** — Embedded permissive 65 KB Bitstream `Vera.ttf` behind default `embedded-fallback-font` feature in `craftui-text` so WASM and minimal Linux always render text out of the box.

### 3. Visual Quality & "Beautiful UI" Gaps
- [x] **VIS-01 (High): Missing Transform2D stack** — Added `Transform2D` (`translate`, `scale`, `rotate`, `skew`, `around_pivot`) with `PushTransform`/`PopTransform` across `craftui-core`, `PaintScene`, `WidgetTree`, GPU (`gpu-sdf` / `vello`), and CPU backends.
- [x] **VIS-02 (High): Missing Backdrop Blur (Glassmorphism) & Blend Modes** — Implemented `BackdropBlur` (Kawase dual-filter GPU pass in `gpu-sdf` + separable box blur in software backends) and `BlendMode` compositing across `BoxStyle`, `PaintScene`, GPU, and CPU renderers.
- [x] **VIS-03 (Medium): Limited `DropShadow` & huge opacity layer bounds** — Upgraded `BoxShadow` with per-corner `CornerRadii`, `spread`, and `inset` shadows, computed tight subtree opacity layer bounds in `WidgetTree`, and configured explicit sRGB/non-sRGB WGSL blending discipline.
- [x] **VIS-04 (Critical): No declarative widget/property animation system** — Completed across `WidgetTree`, `.cui` (`animate <prop> { duration: ...; easing: ...; }`), `codegen.rs`, `craftui-runtime`, `cui!` macro (`animate <prop> { ... }`), and `craftui-lsp` completions/validation.
- [x] **VIS-05 (Medium): Composable widget & control limits** — `Button` renders child subtrees when provided and draws a high-contrast dual focus ring visible in both dark and light themes.

### 4. Concrete Widget, DSL & Runtime Bugs
- [x] **BUG-01 (High): `ScrollView` drag viewport bug** — `WidgetTree::handle_drag` now clamps scroll offset against the `ScrollView` (`nid`) viewport rect instead of the dragged child's rect.
- [x] **BUG-02 (High): `for`-loop keys include array index** — `reactive_for_keyed`, `codegen.rs`, `craftui-runtime`, and `cui!` now key items by explicit `key: <expr>` identity (reusing `NodeId`+`Scope` and updating `Signal<T>`/`Signal<usize>` in place), with duplicate-key occurrence fallback.
- [x] **BUG-03 (Critical): Typos silently compile** — Replaced flat `BUILTIN_PROPS` with strict per-widget `props_for_widget`/`events_for_widget` schemas, `Severity::Error` diagnostics, ternary `? :` validation, scope/identifier checks, and zero-division guards in `validator.rs` + `craftui-lsp`.
- [x] **BUG-04 (High): AOT vs Runtime divergence & static properties** — Unified `:` (one-way) vs `<=>` (two-way) enforcement, dynamic reactive layout/box/transform/glass/text bindings, `Button` empty child labels, `Canvas` callbacks, and `PlatformViewId::from_raw` across `codegen.rs`, `craftui-runtime`, and `cui!`.
- [x] **BUG-05 (Medium): Handler statement limits & `cui!` gaps** — Supported nested/indexed/compound (`+=`, `-=`, `*=`, `/=`) handler assignments and `if/else` in `.cui`; brought full parity (`if/else`, keyed `for`, `<=>`, transforms, glassmorphism, `animate` blocks) to `cui!`.
- [x] **BUG-06 (Medium): Editor & Overlay defects** — Fixed in Phase 1 + Phase 3 (`HotReloadSession` preserves widget state across live reloads and `resolver.rs` fixes local-before-import priority).

### 5. Platform Integration & Tooling Gaps
- [x] **PLAT-01 (High): Blocking desktop OS dialogs & clipboard** — Replaced external CLI subprocess spawns with in-process `DesktopClipboard` (`wl-clipboard-rs` + `x11-clipboard` on Linux/BSD, `objc2-app-kit` `NSPasteboard` on macOS, `windows-sys` Win32 `CF_UNICODETEXT` on Windows, with headless fallback cache), in-process `rfd` `DesktopFileDialog` (`xdg-portal` + `wayland` / AppKit / Win32 `IFileDialog`) paired with off-UI-thread `PlatformServices::pick_open_file_async` / `pick_open_files_async` / `pick_save_file_async` via `UiDispatcher`, and per-window `sync_window_cursor` (`DesktopWindowState.last_cursor` -> `winit::Window::set_cursor`) plus multi-window `WindowLauncher`.
- [x] **PLAT-02 (High): Incomplete Web bridge** — Implemented `#craftui-a11y-root` DOM ARIA mirror (`craftui-a11y` `A11yRole` -> `craft_role_to_aria`, `WebAppHost::extract_aria_tree`, snapshot-diffed `sync_dom_a11y_tree`), CSS cursor sync (`craft_cursor_to_css` + `sync_canvas_cursor`), async `WebClipboard` hydration (`navigator.clipboard.read_text` + trusted DOM `paste` listener + `write_text`), and RAII `DomListener` / `WebRunnerHandle` cleanup (removing DOM listeners, `WebScheduler` RAF/timers, hidden `<input>`, and ARIA root with no leaked listeners).
- [x] **PLAT-03 (High): Prototype Android & iOS bridges** — Added full Android keymap (`map_activity_keycode` covering `A..Z` with `Shift ^ CapsLock`, digits, US QWERTY & numpad punctuation, `Cut`/`Copy`/`Paste`, `DpadCenter` -> `Enter`), full-buffer `GameTextInput` `push_ime_text_state` + `AndroidImeCommand::SyncTextState { text, selection_start, selection_end, compose_region }` without echo loops, and `AndroidHost` error tracking (`record_error`/`last_error`/`clear_error`). Added iOS `IosAppHost::attach_metal` `Result<()>` error propagation, `last_error`/`clear_error`, C-ABI exports (`craftui_ios_app_attach_metal`, `craftui_ios_app_last_error`, `craftui_ios_app_clear_error`, `craftui_ios_app_set_marked_text`, plus `craftui_ios_app_tick` error flag bit `0b100`), and CLI-generated Android `GameActivity` Gradle/Kotlin + iOS Swift/UIKit templates. *(Honest GPU note: primary UI surfaces render directly via `wgpu` on `ANativeWindow` / `CAMetalLayer`; external `PlatformView` feed traits `ExternalSurfaceTextureBridge` / `CvPixelBufferBridge` still perform CPU RGBA/BGRA copies, leaving zero-copy `AHardwareBuffer` / `CVMetalTextureCache` interop as a future optimization.)*
- [x] **PLAT-04 (High): `craftui build` lacks real packaging** — Implemented `craftui doctor [--json]` (toolchain, Rust targets, NDK/tools, `.cui` validation), Android NDK auto-discovery (`discover_android_ndk` / `configure_android_ndk_env`), and `craftui build --platform desktop|web|android|ios [--out] [--release] [--package] [--skip-cargo]` emitting Web `dist/` (`index.html`, `wasm-bindgen`, optional `wasm-opt`), Android Gradle/Kotlin `GameActivity` projects + `jniLibs/arm64-v8a`, and iOS C-ABI `CraftUI.h` / `CraftUIViewController.swift` / `Info.plist` templates, plus modernized `examples/showcase` and Phase 4 widgets (`ToastManager`/`ToastHost`, `Spinner`, `SplitPane`, `Badge`, `CursorIcon` propagation).

---

## Part 2: Phased Production Roadmap

### Phase 1 — Performance Engine, Demand-Driven Power & Core Bug Fixes *(Completed)*
- Add LRU **ShapedLayoutCache** inside `craftui-text` + embed permissive 65 KB **`Vera.ttf`** fallback font (feature-gated `embedded-fallback-font`).
- Make `LayoutEngine::compute_layout` dirty-aware and eliminate per-frame child `Vec` clones; add incremental dirty tracking to `A11yBridge`.
- Replace all idle 60fps spin loops (Desktop caret blink timer at 530ms, Android dynamic `poll_events` timeout/blocking wait, Web on-demand `requestAnimationFrame`, iOS dirty-checked `SyncText` + redraw flags).
- Fix `ScrollView` drag viewport bug, `Select` Z-order hit-test & scrolling, `Button` child composability & theme-aware focus ring, `TextEditorState` undo/redo + word motion + cursor preservation + cross-OS `Cmd`/`Ctrl` shortcuts, and property-effect replacement hygiene.

### Phase 2 — Lightweight Feature-Gating & Next-Gen Hybrid GPU Renderer *(Completed)*
- Add strict Cargo feature flags in `craftui-render` (`gpu-sdf`, `gpu-vello`, `cpu-vello`, `cpu-tiny-skia`) and `craftui` (`dsl`, `hot-reload`).
- Upgrade `PaintScene` & `BoxStyle` with `Transform2D` push/pop stack, rich `BoxShadow` (per-corner `CornerRadii`, `spread`, `inset`, multi-shadow), `BackdropBlur` (frosted glass), `BlendMode`, and tight bounded opacity layers.
- Build **Instanced SDF Quad + Glyph Atlas + Kawase BackdropBlur GPU Renderer (`gpu-sdf`)** using pure WGSL render pipelines (works on Desktop Vulkan/Metal/DX12, Mobile Vulkan/GLES, WebGPU, **and WebGL2** without compute shaders!), plus complete `TinySkiaBackend` (text, shadows, gradients, clips, opacity).
- Wire first-class **Property Animation & Transition Engine (`AnimatedProperty` + `TransitionSpec`)** with spring/tween transitions on widgets, modals, sheets, tabs, switches, checkboxes, buttons, and `Router` transitions.

### Phase 3 — Strict `.cui` Compiler, Declarative Animations, LSP & State-Preserving Hot-Reload *(Completed)*
- Replace flat `BUILTIN_PROPS` with typed per-widget schemas; check expressions, scopes, member accesses, types, and two-way `<=>` writability; emit `Severity::Error` on invalid code.
- Fix `resolver.rs` import priority; implement `for` stable identity keys (`key: expr` without index); support all reactive layout/visual/transform props, `animate` blocks, rich handler statements (`item.field = ...`, `+=`, `if/else`), `Button` children, `Canvas` callbacks, and `PlatformView` IDs in both AOT `codegen.rs` and `craftui-runtime`.
- Upgrade `cui!` macro (`if`, `for`, two-way bindings, transforms/animations) and state-preserving `HotReloadSession` + context-aware `craftui-lsp`.

### Phase 4 — Production Polish, Native Bridges & Multi-Platform CLI Bundling *(Completed — 2026-11-11)*
- Added Phase 4 widgets and interaction polish across `WidgetTree`, `.cui` DSL, `craftui-runtime`, `cui!`, and `craftui-lsp`: `ToastManager`/`ToastHost`, `Spinner`, `SplitPane`, `Badge`, and end-to-end `CursorIcon` propagation.
- Upgraded platform bridges: Desktop (in-process `DesktopClipboard`, `rfd` `DesktopFileDialog` + off-UI-thread async file pickers on `PlatformServices`, per-window `sync_window_cursor`, multi-window `WindowLauncher`), Web (`#craftui-a11y-root` DOM ARIA mirror, CSS cursor sync, async `WebClipboard` + DOM `paste` hydration, RAII `DomListener`/`WebRunnerHandle`), Android (full keymap including punctuation/`Cut`/`Copy`/`Paste`/`DpadCenter`, `GameTextInput` `compose_region` & full-buffer `SyncTextState`, error tracking), and iOS (`attach_metal` `Result<()>` + C-ABI error & `set_marked_text` entrypoints). Primary UI surfaces target `ANativeWindow` / `CAMetalLayer` via `wgpu`; zero-copy `AHardwareBuffer` / `CVMetalTextureCache` interop for external `PlatformView` textures remains a future optimization.
- Upgraded `craftui-cli` (`doctor [--json]`, NDK auto-discovery, `build --platform desktop|web|android|ios [--out] [--release] [--package] [--skip-cargo]`) emitting Web `dist/`, Android Gradle/Kotlin `GameActivity` + `jniLibs`, and iOS Swift/UIKit + C-ABI templates, and modernized `examples/showcase` with glassmorphism, spring motion, transforms, Phase 4 widgets, and `.cui` strict validation.

### Phase 5 — Release-Grade Hardening, Native Mobile Permissions/Capabilities & DX Excellence *(Completed — 2026-11-11)*
- **35 Standard Mobile & Web Permissions**: Complete `craftui_core::Permission` catalog covering all native Android `<uses-permission>` declarations (with exact API bounds including `maxSdkVersion="32"`/`"30"`/`"29"`), iOS `Info.plist` `NS*UsageDescription` privacy strings, and W3C Web Permissions API descriptors.
- **Zero-Boilerplate Manifest Engine (`craftui.toml`)**: Implemented zero-dependency `AppManifest` in `tools/craftui-cli/src/manifest.rs` parsing `[app]`, `[android]`, `[ios]`, and `[permissions]`. Automatically emits valid Android `AndroidManifest.xml` and iOS `Info.plist`.
- **Phase 5 Widgets**: `SafeArea`, `SegmentedControl`, `Accordion`, and `DataTable`.
- **Production CLI & Tooling**: `craftui-cli` (`new --template counter|dashboard|mobile`, `init`, `setup`, `run`, `permissions list|check`, `doctor [--json]`), and pure Node LSP VS Code extension (`editors/vscode/`).
- **Showcase Modernization**: `examples/showcase` updated with 5th "Mobile & Native" tab.

### Phase 6 — Flagship Real-World Dogfood App: "ScanCraft" *(In Progress)*
- **Real-World Application Scope**: Build a full-fledged mobile & desktop document scanner and photo filter suite in `examples/scancraft`.
- **Core Features**:
  - Live viewfinder & capture simulation / gallery picker (`Permission::Camera`, `Permission::Photos`).
  - Interactive 2D image processing canvas: crop boundaries, 90° rotation, real-time brightness/contrast adjustment, grayscale, and document thresholding.
  - Document library with sortable, filterable, selectable `DataTable` and `VirtualList`.
  - Edge-to-edge `SafeArea` integration, native haptic feedback (`Vibrate`), native share sheet export, and real-time battery/storage telemetry.
- **Developer Host Focus**: Primary tier on **Linux Desktop** and **Android** (physical device + emulator via ADB/NDK). No macOS or iOS setup required.

### Phase 7 — Official Comprehensive Documentation Book *(In Progress)*
- Complete mdBook documentation tree in `docs/`:
  - Quickstart for Linux & Android (`craftui new`, `craftui doctor`, `craftui run`).
  - `.cui` DSL Language Guide & Reactivity architecture.
  - Complete 25+ Widget Catalog with interactive properties, events, and a11y roles.
  - Android & Linux Platform deployment guides.
  - Mobile Permissions & Manifest security reference.

### Verification Matrix

| Target / Environment | Verification Scope | What Was Verified vs. Limitations |
|---|---|---|
| **Linux Host (`x86_64-unknown-linux-gnu`)** | `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, feature-gating (`--no-default-features`, `--features desktop,dsl,hot-reload`), `examples/showcase` AOT build/tests, `craftui doctor` / `check` / `build --skip-cargo` CLI smoke tests, `deny.toml` license audit | Full unit, integration, widget, DSL, CLI packaging, and software/GPU-offscreen tests run natively on Linux; `DesktopClipboard` and `DesktopFileDialog` exercise headless fallback paths in CI (live Wayland/X11/portal UI dialogs require an interactive desktop session). |
| **Cross-Target: Web (`wasm32-unknown-unknown`)** | `cargo check` (default & `--no-default-features --features gpu-sdf`) and `cargo clippy -p craftui-platform-web --target wasm32-unknown-unknown -- -D warnings`, plus `craftui build --platform web --skip-cargo` bundle generation | Cross-target compilation and strict clippy verified from Linux host; DOM ARIA, CSS cursor, `navigator.clipboard`, and WebGPU/WebGL2 presentation require execution in a live browser runtime. |
| **Cross-Target: Android (`aarch64-linux-android` via NDK 28.2)** | `cargo check` & `cargo clippy` (`default` and `--no-default-features --features gpu-sdf`) using NDK `28.2.13676358` (`aarch64-linux-android24-clang`), host-side `AndroidHost` tests, and `craftui build --platform android --skip-cargo` template emission | Cross-target NDK compilation/linting and Gradle/`GameActivity` project generation verified on Linux host; on-device `ANativeWindow` Vulkan/GLES rendering and `GameTextInput` IME require an Android device or emulator. |
| **Non-Host Apple Targets (`x86_64-apple-darwin`, `aarch64-apple-darwin`, `aarch64-apple-ios`)** | Host-side `IosAppHost` / `CvPixelBufferBridge` / C-ABI unit tests (`craftui_ios_app_*`) and CLI `CraftUI.h` / `CraftUIViewController.swift` / `Info.plist` generation (`--skip-cargo`) | **Requires macOS + Xcode CI runner** for native `rustc` cross-compilation (`aarch64-apple-ios`), Swift/UIKit + `CAMetalLayer` linking (`xcodebuild`), macOS `NSPasteboard` / AppKit `rfd` compilation, and live execution. |
| **Non-Host Windows Target (`x86_64-pc-windows-msvc`)** | Platform-neutral core/widget/CLI tests on Linux host | **Requires Windows CI runner** for compiling and executing Win32 `CF_UNICODETEXT` clipboard (`windows-sys`), `IFileDialog` (`rfd`), and DX12/Vulkan desktop windowing. |
