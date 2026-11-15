# Craftpad: Web Studio & Playground

**Craftpad** is the official interactive web playground and desktop studio for CraftUI. It allows developers to author, experiment with, and preview `.cui` declarative components in real-time inside any modern web browser or as a native desktop application.

---

## 1. Key Capabilities

- **Zero-Install Prototyping**: Test CraftUI directly in the browser via WebAssembly (`wasm32-unknown-unknown`) without installing Rust, Cargo, or the Android NDK.
- **Side-by-Side Live Preview**: Edit `.cui` code in the left pane and watch the interactive GPU/CPU canvas render on the right at 60 FPS.
- **Built-in Template Catalog**: Switch between verified component recipes with one click:
  - **Counter**: Two-way reactive state binding and callbacks.
  - **Login Form**: Structured forms with validation states and checkboxes.
  - **Settings Card**: Toggles, sliders, and dark mode controls.
  - **Search & Filter**: Search bars with responsive chips.
- **Instant Error Diagnostics**: Underlines syntax errors, unknown widget properties, and type mismatches with exact line and column numbers.
- **One-Click Export**: Copy clean, verified `.cui` markup or download ready-to-run projects.

---

## 2. Running Craftpad Locally

### On Desktop
To launch Craftpad as a native desktop application with live preview:

```bash
cargo run -p craftui-example-craftpad
```

### On the Web (Local Server)
To build and serve Craftpad as a WebAssembly web application:

```bash
craftui run --platform web -p craftui-example-craftpad --port 8080
```

Then open `http://localhost:8080` in Chrome, Firefox, Safari, or Edge.

---

## 3. Architecture & Integration

Craftpad is built entirely with CraftUI itself! Its screen layout is described in `examples/craftpad/ui/craftpad.cui` and compiled into AOT Rust code at build time.

```mermaid
flowchart LR
    A["DSL Source (.cui)"] --> B["craftui-dsl Parser & Validator"]
    B --> C["craftui-runtime HotReloadSession"]
    C --> D["craftui-render (SDF / Canvas)"]
    D --> E["Browser Canvas (WASM) / Desktop Window"]
```

When deployed to GitHub Pages or any static host, the compiled `craftui_app.wasm` and `index.html` run 100% client-side with zero backend server dependencies.
