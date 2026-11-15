# Craftpad

Craftpad includes an experimental browser editor backed by the CraftUI runtime.
The web editor sends `.cui` source to the WASM interpreter and renders the
result to an interactive canvas. Edits are debounced; Run and Reload reapply
source while preserving component property state. Restart and switching
samples create fresh state. Errors are shown in Problems; a failed validation
leaves the last successfully mounted preview in place.

## Build the browser bundle

Install the `wasm32-unknown-unknown` Rust target and the version of
`wasm-bindgen-cli` matching `Cargo.lock`, then run:

```bash
scripts/build-craftpad-web.sh
python3 -m http.server --directory dist/craftpad-web 8080
```

Open `http://localhost:8080`. Deploy the contents of `dist/craftpad-web` to a
static host; `.wasm` files must be served with the `application/wasm` MIME
type. Local filesystem imports are not available in the browser; use inline
components or CraftUI's built-in modules.

## Desktop prototype

```bash
cargo run -p craftui-example-craftpad
```

The desktop studio validates the source but its preview pane is still a static
mockup. For a real native live preview with state preservation use
`craftui preview <file.cui>`.
