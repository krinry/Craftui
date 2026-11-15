# CraftUI — Engineering Roadmap, License Governance & Risk Plan (`ROADMAP.md`)

## 1. Execution Strategy: Avoid Burnout via Strict Phasing

Building a cross-platform GUI framework is notorious for scope explosion. The golden rule for CraftUI is **Vertical Slices + Early Dogfooding**: ship a tiny end-to-end pipeline first, validate on real apps (like ScanCraft), and expand platform/widget breadth incrementally.

---

## 2. Phased Milestones

### Milestone 0: Workspace & License Guardrails (`Week 1`)
* **Deliverables:**
  - Initialize Cargo workspace (`crates/craftui-*`).
  - Configure `deny.toml` (`cargo-deny`) blocking GPL/LGPL/AGPL and unapproved licenses.
  - Setup GitHub Actions CI matrix (`ubuntu-latest`, `windows-latest`, `macos-latest`, `wasm32-unknown-unknown`, `aarch64-linux-android`).
* **Exit Gate:** `cargo test --workspace` and `cargo deny check licenses` pass cleanly.

### Milestone 1: Desktop Vertical Slice (`Weeks 2–5`)
* **Deliverables:**
  - `craftui-reactive`: `SlotMap` Signal arena (`Signal`, `Computed`, `Effect`, `Scope`).
  - `craftui-layout`: `taffy` integration (`Row`, `Column`, `Stack`, `Padding`, `Size`).
  - `craftui-render`: `winit` + `wgpu` + `vello` GPU renderer (rectangles, rounded borders, shadows, image textures) + `tiny-skia` / `vello_cpu` software fallback switch.
  - `craftui-text`: `parley` basic single/multi-line text rendering.
  - `craftui-widgets`: `View`, `Text`, `Button`, `Image`, `Canvas` (basic paths).
* **Exit Gate:** Interactive Counter + Image Brightness Preview app runs at 120 FPS on Linux/Windows/macOS.

### Milestone 2: Core Widgets, Scrolling & Text Input (`Weeks 6–10`)
* **Deliverables:**
  - Gesture disambiguation (`Tap`, `Drag`, `PinchZoom`, `Wheel`).
  - `ScrollView` with smooth momentum & clipping + `VirtualList` (windowed item recycling for large lists).
  - Interactive `TextInput` (cursor, selection, clipboard, keyboard events).
  - `ThemeScope` reactive Light/Dark token switching & animation easing primitives (`Spring`, `EaseInOut`).
* **Exit Gate:** Multi-screen sample app with 10,000-item scrollable list, search input, and animated theme switch.

### Milestone 3: `.cui` DSL, Codegen & Hot-Reload (`Weeks 11–15`)
* **Deliverables:**
  - `craftui-macros`: `cui!` proc-macro for inline Rust UI.
  - `craftui-dsl`: `.cui` parser (`logos` + `rowan` lossless CST for future LSP), semantic validator, and `build.rs` AOT Rust codegen.
  - `craftui-runtime` + `craftui-cli`: File watcher reloading `.cui` files into a running window in < 100ms.
* **Exit Gate:** Edit `.cui` colors/layout/text in editor and observe instant live UI update without Rust recompilation.

### Milestone 4: Web (WASM) & Android Support (`Weeks 16–22`)
* **Deliverables:**
  - **Web (`craftui-platform-web`):** `wasm32-unknown-unknown` target, WebGPU + WebGL2 fallback, DPR scaling, hidden `<input>` bridge for mobile web keyboards, `wasm-opt` size optimization.
  - **Android (`craftui-platform-android`):** `android-activity` (`GameActivity`) integration, soft keyboard (IME) height insets, `SafeArea` cutout support, touch gestures, APK packager in `craftui-cli`, and **`PlatformView` external `SurfaceTexture` slot** for CameraX/Video.
* **Exit Gate:** Same `.cui` + Rust app runs natively on Desktop, Android APK, and Browser WASM.

### Milestone 5: iOS, Mobile Permissions, Accessibility & Real-World Dogfooding *(Completed — 2026-11-11)*
* **Deliverables:**
  - **iOS (`craftui-platform-ios`):** `UIKit` + `Metal` `wgpu` surface, touch/IME/SafeArea, C-ABI bridge exports (`craftui_ios_app_*`), Swift permission broker, and XCFramework/IPA tooling.
  - **Mobile Permissions & Manifest Engine:** 35 standard permissions catalog, zero-dependency `craftui.toml` manifest parser & validator, automatic Android `<uses-permission>` & iOS `NS*UsageDescription` generation.
  - **Phase 5 Widgets & DX:** `SafeArea`, `SegmentedControl`, `Accordion`, `DataTable` (sortable), `craftui-cli` (`new --template mobile`, `init`, `setup`, `run`, `permissions list|check`), official VS Code extension (`editors/vscode/`), and `.github/workflows/ci.yml`.
  - **Accessibility (`craftui-a11y`):** `accesskit` incremental tree synchronization for screen readers.
  - **Flagship Dogfooding & Showcase:**
    - Showcase updated with "Mobile & Native" tab demonstrating permissions, `SafeArea`, `SegmentedControl`, `Accordion`, `DataTable`, native haptics, share sheet, and device telemetry.
* **Exit Gate:** Zero-crash 60 FPS demo across Desktop + Android + Web + iOS, passing 100% of workspace tests and strict clippy, ready for public `v0.1.0` MIT release.

---

## 3. Automated License Governance (`deny.toml`)

Add this exact guard to the workspace root so no dependency ever breaks your MIT guarantee:

```toml
[licenses]
version = 2
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "CC0-1.0",
]
confidence-threshold = 0.8
```

Run `cargo deny check licenses` in every CI job.

---

## 4. Technical Risk & Mitigation Matrix

| Risk | Severity | Impact | Mitigation Strategy |
| :--- | :---: | :--- | :--- |
| **Old Android / VM GPUs fail `vello` compute shaders** | High | Blank screen or shader compile crash | Build `RenderBackend` trait from Day 1; detect compute capability at startup and seamlessly drop to `vello_cpu` / `tiny-skia` software renderer. |
| **Mobile IME & Soft Keyboard Bugs** | High | Broken typing, hidden text fields, missing swipe/autocorrect | Study `egui`/`winit`/`slint` Android IME implementations early; prioritize `GameActivity` + `InputConnection` + `SafeArea` bottom-inset signals. |
| **WASM Binary Bloat (> 5 MB)** | Medium | Slow initial web page load | Keep heavy font blobs external (lazy fetch system/CDN fonts), disable unused `wgpu` features, strip debug symbols, enforce `wasm-opt -Oz` + Brotli budget checks in CI. |
| **Rust Proc-Macro & `wgpu` Compile Times** | Medium | Sluggish developer iteration | Split `craftui-dsl` AOT vs `craftui-runtime` hot-reload; use dynamic linking (`dylib`) in dev workspace profiles; keep generic monomorphization minimal in `craftui-widgets`. |
| **LSP & Tooling Maintenance Burden** | Medium | Developers avoid `.cui` without IDE support | Use `rowan` lossless syntax tree in `craftui-dsl` from day one so the same parser powers compiler errors, formatter, and `craftui-lsp`, while offering `cui!` macro as immediate pure-Rust alternative. |

---

## 5. Recommended First Coding Session Checklist

Before writing widget #2, build the smallest proving loop:
1. `cargo new craftui-lab` with `winit` + `wgpu` + `vello` (and `tiny-skia` flag).
2. Render 1 rounded colored rectangle + 1 `parley` text string + 1 `taffy` flex row.
3. Wire 1 `SlotMap` `Signal<i32>` that increments on click and marks only that text node dirty.
4. Measure frame time and stripped binary size — **this establishes your performance baseline before adding DSL syntax or extra platforms.**
