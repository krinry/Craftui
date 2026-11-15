# Contributing to CraftUI

Thank you for helping. This page tells you how to build, test and submit a
change.

## Setup

You need Rust 1.89 or newer. On Linux, also install the windowing and font
libraries:

```bash
sudo apt install -y libwayland-dev libxkbcommon-dev libfontconfig1-dev libfreetype-dev libx11-dev
```

Clone the repository and build:

```bash
git clone https://github.com/krinry/Craftui
cd Craftui
cargo build --workspace
```

## Before you open a pull request

CI runs these checks; run them locally first.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

If you changed a platform crate, also check its target:

```bash
cargo clippy -p craftui-platform-web --target wasm32-unknown-unknown --all-targets -- -D warnings
cargo clippy -p craftui-platform-android --target aarch64-linux-android --all-targets -- -D warnings
```

`Cargo.lock` is committed and CI builds with `--locked`. If you add or update a
dependency, commit the updated lock file in the same change.

## Where things are

| Path | Contents |
|---|---|
| `crates/craftui` | The crate applications depend on. Re-exports the others. |
| `crates/craftui-core` | Shared types: geometry, colours, events, theme, platform services. |
| `crates/craftui-reactive` | Signals, computed values, effects, scopes. |
| `crates/craftui-layout` | Flexbox and grid layout (taffy). |
| `crates/craftui-text` | Text shaping and layout (parley), embedded fallback font. |
| `crates/craftui-render` | Paint scene and the GPU and CPU renderers. |
| `crates/craftui-widgets` | The widget tree, every built-in widget, input handling. |
| `crates/craftui-a11y` | Accessibility tree (AccessKit). |
| `crates/craftui-dsl` | The `.cui` language: lexer, parser, validator, code generator. |
| `crates/craftui-macros` | The `cui!` macro. |
| `crates/craftui-runtime` | Interprets `.cui` at run time for hot reload and preview. |
| `crates/craftui-platform-*` | Desktop, Android, iOS and Web runners. |
| `tools/craftui-cli` | The `craftui` command: new, run, build, check, preview, doctor. |
| `tools/craftui-lsp` | Language server for `.cui` files. |
| `editors/vscode` | VS Code extension. |
| `examples/` | Example apps. `examples/notes` is the reference app. |
| `docs/` | The book (mdBook). |

## Tests

- Put unit tests next to the code and integration tests in the crate's `tests/`
  folder.
- UI behaviour can be tested without a window: build a `WidgetTree`, lay it out,
  send events, inspect the result. `examples/notes/src/lib.rs` has a reusable
  harness.
- A bug fix should come with a test that fails without the fix.
- Tests must not depend on a GPU, a display, the network or the wall clock.

## Changing the `.cui` language

A language change usually touches four places: `parser.rs`, `validator.rs`,
`codegen.rs` in `crates/craftui-dsl`, and the run-time interpreter in
`crates/craftui-runtime` so that hot reload behaves the same as a real build.
Add cases to `crates/craftui-dsl/tests/language.rs` and update
`docs/src/dsl/syntax-guide.md`.

Every code block tagged `cui` in any Markdown file is compiled by
`crates/craftui-dsl/tests/docs_examples.rs`. Write complete components in those
blocks, and tag fragments as `text`.

## Documentation

Write in plain, direct English. Describe what the code does today. If something
is planned, partial or unverified, say so in those words. Do not state
performance or size figures without saying how they were measured.

## Dependencies and licence

The project is MIT licensed. Dependencies must be under MIT, Apache-2.0, BSD,
Zlib, ISC or a similarly permissive licence; GPL, LGPL and AGPL are rejected by
`cargo deny check licenses`, which CI runs.

## Commits and pull requests

- Keep each pull request to one topic.
- Explain what changed and why in the description, and how you tested it.
- CI must be green before a merge.
