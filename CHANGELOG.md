# Changelog

Notable changes to CraftUI. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added

- `examples/notes`: a notes app for Android, the Web and desktop with model,
  storage and UI tests. It is the reference example.
- Continuous integration (`.github/workflows/ci.yml`): formatting, clippy with
  warnings denied, the workspace test suite, licence checks, a lock-file check,
  WebAssembly and Android target checks, a benchmark smoke run, and two
  end-to-end jobs that build the notes app as a web bundle (tested in headless
  Chrome) and as an Android APK.
- `WidgetTree::is_visible` to ask whether a node is currently shown.
- `LayoutStyle::flex_basis`, `LayoutStyle::fill_remaining` and
  `LayoutStyle::with_min_size`.
- Tests for the reactive engine, the layout engine and the app manifest.
- Tests for the `.cui` language (`crates/craftui-dsl/tests/language.rs`) and a
  test that compiles every `cui` example in the documentation.
- `CONTRIBUTING.md`, this changelog, a README for the notes example, and a
  step-by-step guide to the notes app in the book.

### Fixed

- `Window.width`, `Window.is_compact` and the other `Window.*` values were wrong
  until the first resize, because platform runners set the window size without
  updating the reactive copy. On the Web this made a phone-sized page use the
  desktop layout.
- A branch of `if ... else` could not fill its parent: `flex_grow` and
  percentage sizes had no effect, because the wrapper around the branch was
  sized to its content. The wrapper now takes `width`, `height` and `flex_grow`
  from the branches.
- A scrolling list in an `if ... else` branch was laid out at the full height
  of its content, so a long list ran off the screen and could not be scrolled.
  The wrapper now takes exactly the space its parent has left.
- Text fields had no accessible name. They now use their placeholder, so screen
  readers and the web accessibility layer can identify them.
- Code generated from `.cui` files no longer produces clippy warnings in
  applications that deny warnings.
- The CLI no longer falls back to a hard-coded Android SDK path from the
  original author's machine.

### Changed

- Windows and macOS are tested on every pull request, not only after a merge.
  Clippy findings in the Windows and macOS clipboard code and a keyboard test
  that assumed Ctrl on every platform were fixed.
- Merged upstream `krinry/Craftui` main (whiteboard layout and renderer fixes).
- The book's language guide, reactivity page, quickstart and widget catalog were
  rewritten against the real grammar. The earlier versions described syntax that
  never compiled.
- `Cargo.lock` is committed and CI builds with `--locked`.
- Size and performance figures in the documentation were replaced by values
  measured in CI.
