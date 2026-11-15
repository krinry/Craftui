# CraftUI — Product Requirements Document (PRD)

> **Working Codename:** `CraftUI`  
> **License:** MIT (Zero GPL/LGPL/AGPL dependencies — enforced via `deny.toml`)  
> **Target Platforms:** Desktop (Windows, macOS, Linux), Mobile (Android, iOS), Web (WASM + WebGPU/WebGL2)  
> **Core Mission:** Slint-level declarative ergonomics & tiny footprint + GPUI-level instanced GPU rendering beauty + Flutter-level 60–120 FPS fluid motion across all platforms.

---

## 1. Executive Summary & Product Vision

**CraftUI** is an open-source, **MIT-licensed**, compiled declarative UI framework built in pure Rust. Its goal is to build **genuinely beautiful, silky-smooth, lightweight native & web applications** from a single codebase across **Desktop, Mobile (Android & iOS), and Web (WebAssembly)** — with zero WebView overhead and zero commercial licensing fees.

Unlike early prototypes that re-shape text every frame or bundle monolithic compute pipelines into every target, CraftUI combines three proven production architectures:
1. **Slint-style Compile-Time Safety & Declarative Animations:** `.cui` files compile to typed Rust + fine-grained `SlotMap` signals with first-class `animate ... { duration / spring }` transitions, strict compile-time diagnostics (hard errors on typos/type mismatches), and zero interpreter bloat in release builds.
2. **GPUI/Makepad-style Instanced SDF + Glyph Atlas GPU Pipeline:** 95% of modern UI (rounded cards, layered/inset shadows, crisp borders, frosted-glass backdrop blur, gradients, text glyphs, textures) renders via lightweight batched **WGSL Instanced SDF + Glyph Atlas shaders** compatible with Vulkan, Metal, DX12, WebGPU, **and WebGL2** — with optional `vello` vector path and CPU software (`vello_cpu`/`tiny-skia`) backends gated by Cargo features.
3. **Demand-Driven Power & Native OS Integration:** True **0% idle CPU/GPU usage** (timer-driven caret blink and dirty-only layout/a11y/paint updates), bundled tiny fallback font (`Vera.ttf`, 65 KB) so text never renders blank on Web/minimal OSes, async OS dialogs/clipboard, and real app packaging via `craftui-cli`.

---

## 2. Competitive Positioning

| Framework | License | Core Paradigm | Rendering Strategy | Desktop / Mobile / Web | Key Limitation vs CraftUI Target |
| :--- | :--- | :--- | :--- | :---: | :--- |
| **Slint** | GPLv3 / Paid | `.slint` Compiler + Property Graph | FemtoVG / Skia / SW Damage | Yes / Yes / Yes | Copyleft GPLv3 or commercial fees for closed-source apps |
| **Zed GPUI** | Apache-2.0 | Retained View + Rust | Instanced SDF Quads + Glyph Atlas | Desktop only | No first-class Mobile/Web support or declarative markup DSL |
| **Makepad** | MIT | `live_design!` DSL | Custom GPU SDF Shaders | Yes / Yes / Yes | Non-standard ecosystem crates and steep API learning curve |
| **Flutter** | BSD-3 | 3-Tree (Widget/Element/Render) | Impeller / Skia | Yes / Yes / Yes | Dart VM runtime & larger mobile/web binary footprint |
| **Iced / Dioxus** | MIT | MVU / RSX VDOM | `wgpu` / WebView or Blitz | Desktop/Web / Beta | Mobile IME/touch/platform-view & lightweight SDF beauty lag behind |
| **CraftUI (Target)** | **MIT** | **`.cui` AOT + `cui!` + `SlotMap` Signals** | **Feature-Gated SDF+Atlas GPU / Vello / SW** | **Yes / Yes / Yes** | **Active evolution tracked in `ROADMAP.md`** |

---

## 3. Target Audience & Personas

1. **Design-Conscious Cross-Platform App Builders:** Need glassmorphism, spring micro-interactions, smooth route/modal transitions, crisp typography, and custom vector canvases without Electron/WebView bloat.
2. **Privacy, Media & Real-Time Tool Developers (Camera, Image Editors, Scanners, Audio/Video):** Need zero-copy GPU texture streaming, `PlatformView` composition (CameraX/Metal), multi-touch `GestureArena`, and MIT licensing for App Store / Play Store / commercial release.
3. **Lightweight Web & Utility Developers:** Need small WASM bundles, instant cold startup, deterministic memory, and zero-flicker light/dark/AMOLED themes.

---

## 4. Core Differentiators

1. **Feature-Gated Minimal Footprint:** Release builds opt into only the backend needed (`gpu-sdf` lightweight default vs full `vello` vector compute vs `cpu-vello` / `cpu-tiny-skia`) and strip `.cui` parser/interpreter crates completely out when `hot-reload` is disabled.
2. **Fine-Grained Retained Tree + Cached Text/Glyphs:** Signal updates dirty only affected nodes; Taffy sync, AccessKit updates, and Parley text shaping are cached across frames so idle and steady interactions minimize allocations.
3. **First-Class Declarative Motion & Visual Polish:** Transform matrix stack (`translate`, `scale`, `rotate`), multi-layered/inset `BoxShadow`, `BackdropBlur` frosted glass, blend modes, and property-level `Spring` / `Tween` animations directly in `.cui` and standard widgets.
4. **Guaranteed Typography Out of the Box:** Bundles a permissive 65 KB fallback font (`Vera.ttf`, Bitstream Vera License) alongside system font discovery (`fontique`) and complex script shaping (`parley` + `harfrust`).
5. **100% Permissive MIT Ecosystem:** Automated CI enforcement (`deny.toml`) permitting zero GPL/LGPL/AGPL dependencies.

---

## 5. Functional Requirements (FR)

### FR-1: Strict Declarative `.cui` Compiler & `cui!` Macro
* **FR-1.1 Lossless `.cui` DSL:** Components, `in`/`out`/`in-out`/`private` properties, two-way `<=>` bindings, one-way `:` bindings (strictly read-only), loops (`for item, idx in model` with stable identity `key:`), conditionals (`if` / `else`), inline handler statements (`struct.field = ...`, `+=`, `if/else`, callbacks), and declarative `animate <prop> { duration / spring }`.
* **FR-1.2 Strict Compile-Time Validation:** Per-widget property schemas, full identifier/scope resolution, member/enum/struct type checking, and `Severity::Error` diagnostics on typos or type mismatches (no silent string fallbacks in codegen).
* **FR-1.3 Zero-Overhead AOT & State-Preserving Hot-Reload:** `build.rs` AOT generates pure typed Rust structs; dev-mode `craftui-runtime` reloads `.cui` changes in `< 50ms` while preserving scroll positions, focus, and live signals.
* **FR-1.4 Complete `cui!` Proc-Macro:** Supports inline reactive bindings, `if`/`for` blocks, two-way bindings, transforms, and animations inside `.rs` files.

### FR-2: Fine-Grained Reactive State Engine (`craftui-reactive`)
* Generational `Copy` handles (`Signal<T>`, `ReadSignal<T>`, `Computed<T>`, `Effect`), `batch`, `untrack`, `Scope`-bound cleanup (zero duplicate effect leaks when restyling nodes), and thread-safe `UiDispatcher` for `rayon`/`tokio`/`std::thread`.

### FR-3: Dirty-Tracked Cross-Platform Layout (`craftui-layout`)
* Powered by `taffy` (Flexbox, CSS Grid, Z-`Stack`, `SafeArea` notch/keyboard insets, RTL mirroring, breakpoints) with **dirty-only style synchronization** and reusable traversal buffers.

### FR-4: Next-Gen Visual Scene & Hybrid GPU/CPU Renderer (`craftui-render`)
* **FR-4.1 Rich `PaintScene` Primitives:** `Transform2D` push/pop stack, rounded rects (`CornerRadii`), multi-shadow & inset `BoxShadow`, linear/radial gradients, **Kawase `BackdropBlur` (frosted glass)**, `BlendMode`, bounded opacity layers, vector paths (`BezPath`), SVG icons, images, and `PlatformViewHole`.
* **FR-4.2 Instanced SDF + Glyph Atlas GPU Backend (`gpu-sdf`):** Batched WGSL render pipeline working on Desktop, Android Vulkan/GLES, iOS Metal, WebGPU, and **WebGL2** without compute shaders.
* **FR-4.3 Optional Heavy Vector (`vello`) & Complete Software (`vello_cpu`, `tiny-skia`) Backends:** Feature-gated in `craftui-render/Cargo.toml`; software backends faithfully support text, clipping, gradients, opacity layers, and shadows.

### FR-5: Cached Text Shaping, Glyph Atlas & Complete Editor (`craftui-text`)
* **FR-5.1 Shaped-Layout Cache + Embedded Fallback:** LRU-cached `parley` shaping (`(text, style, max_width)`) + embedded 65 KB `Vera.ttf` fallback font.
* **FR-5.2 Production `TextEditorState`:** Multi-line editing, undo/redo stack, word-by-word navigation (`Ctrl/Alt + Arrow`), OS-aware shortcuts (`Cmd` on macOS, `Ctrl` on Win/Linux/Web), safe cursor preservation across external signal updates, selection highlights, and full CJK/mobile IME preedit composition.

### FR-6: Rich Animated Widget Kit & Navigation (`craftui-widgets`)
* Animated built-in micro-interactions (spring hover/press, sliding `Tabs` pill, animated `Switch`/`Checkbox`, scale+fade+blur `Modal`/`BottomSheet`, transition-capable `Router<R>`).
* Composable `Button` (accepts arbitrary child layouts or text label), scrollable `Select`, fixed child-aware `ScrollView` drag & momentum fling, `Toast` overlay manager, `Spinner`, `SplitPane`, `Badge`, cursor shapes (`CursorIcon`), and `Ctrl+Shift+I` Inspector overlay.

### FR-7: Demand-Driven All-Platform Shells & Packaging
* **FR-7.1 Zero Idle Spin Loops:** Desktop (`ControlFlow::Wait`/`WaitUntil`), Android (`poll_events` waking only on input/waker/animation/timer), Web (on-demand `requestAnimationFrame`), and iOS (`CADisplayLink` dirty gating) consume ~0% CPU/GPU at rest, including with focused `TextInput` (timer-based 530ms caret blink).
* **FR-7.2 Real OS Services:** Non-blocking/native Desktop clipboard & file picker (`arboard` / `rfd` or async OS portals without stalling UI thread), real browser clipboard & CSS cursor + **DOM ARIA Accessibility Mirror** on Web, full Android key/IME/Gradle/Kotlin bridge, and iOS UIKit/Metal/Swift template.
* **FR-7.3 Production `craftui-cli` & Context-Aware `craftui-lsp`:** Scaffolding multi-platform templates (`web/`, `android/`, `ios/`), `craftui doctor`, `craftui build` emitting deployable Web `dist/`, Android Gradle/APK projects, and iOS XCFramework setups, plus schema-driven LSP completion/validation.

---

## 6. Non-Functional Requirements (NFR)

* **NFR-1 Frame Budget & Idle Power:**
  * Steady UI layout + SDF/Atlas paint + submit: **<= 4.0ms** (Desktop/Modern Mobile, capable of 120–240 FPS).
  * Idle power consumption (no active animations): **0 redraws/sec**; blinking cursor wakes UI loop at `~1.9 Hz` (530ms toggle), never 60–120 Hz.
* **NFR-2 Feature-Gated Binary Size Targets (Stripped Release):**
  * Minimal `gpu-sdf` Desktop core (no `dsl`/`runtime`/`vello`): **<= 4–6 MB**.
  * Minimal `gpu-sdf` Web WASM (`opt-level = "z"`, `lto = "thin"`, `wasm-opt -Oz` + brotli): **<= 1.8 MB compressed**.
  * Full-featured `vello` vector Desktop build: **<= 12 MB**.
* **NFR-3 Safety & License Gate:**
  * Minimal isolated `unsafe` restricted to FFI/GPU surface creation (`wgpu`/JNI/ObjC).
  * `cargo-deny` allow-list strictly enforced (`MIT`, `Apache-2.0`, `BSD-2/3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, and Bitstream Vera font redistribution terms).
* **NFR-4 Platform Matrix:**
  * Desktop: Linux (Wayland/X11), macOS 12+, Windows 10+.
  * Mobile: Android 8.0+ (API 26+), iOS 15+.
  * Web: Chrome, Firefox, Safari, Edge (WebGPU & WebGL2/Canvas2D).
