# CraftUI for Visual Studio Code

Official Visual Studio Code language extension for [CraftUI](https://github.com/krinry/Craftui) declarative UI applications (`.cui`).

## Features

- **Syntax Highlighting**: Full TextMate grammar for `.cui` templates, properties, two-way bindings (`<=>`), handler closures (`=>`), and types.
- **Language Server Integration (`craftui-lsp`)**:
  - Live syntax & semantic diagnostics
  - Hover documentation for all 40+ built-in widgets (including `SafeArea`, `SegmentedControl`, `Accordion`, `DataTable`, `Spinner`, `Badge`, `SplitPane`, `ToastHost`)
  - Intelligent completion for widgets, properties, and events
  - Hex color pickers and document color decorators (`#RRGGBBAA`)
- **Direct CLI Commands**:
  - `CraftUI: Open Live Preview Window` (`craftui preview <file>`)
  - `CraftUI: Validate Current .cui File` (`craftui check <file>`)
  - `CraftUI: Audit Permissions (craftui.toml)` (`craftui permissions check craftui.toml`)
  - `CraftUI: Restart Language Server`

## Installation

Download the VSIX for your operating system and architecture from the project's
extension build artifacts, then run `code --install-extension <file>.vsix` or
use **Extensions: Install from VSIX** in VS Code. Platform-specific packages
built by the `VS Code extension packages` workflow include `craftui-lsp` and
`craftui`; no local Rust toolchain is needed. Explicit `craftui.lspPath` and
`craftui.cliPath` settings override bundled binaries.

For development, symlink or copy `editors/vscode` to your VS Code extensions folder:
```sh
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/craftui
```

Build `craftui` and `craftui-lsp`:
```sh
cargo build --release -p craftui-cli -p craftui-lsp
```
Unbundled development installs discover `craftui-lsp` and `craftui` binaries in your workspace `target/debug/` or `target/release/` directories or system `PATH`.
