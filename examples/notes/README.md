# CraftUI Notes

A small notes app that runs on **Android**, in the **browser** and on the
**desktop** from one codebase. It is the reference example for building a real
app with CraftUI: every part is tested, and CI builds the Android APK and the
web bundle on every change.

What it does:

- Create, edit and delete notes. Every keystroke is saved.
- Pin notes to keep them at the top. Filter to pinned notes only.
- Search titles and text.
- Light and dark theme, remembered between runs.
- On a phone the list and the editor take turns; from 720dp wide they sit side
  by side.

## Run it

All commands are run from the repository root.

### Desktop

```bash
cargo run -p craftui-example-notes
```

### Web

Needs the `wasm32-unknown-unknown` target and a `wasm-bindgen-cli` whose version
matches the `wasm-bindgen` entry in `Cargo.lock`.

```bash
rustup target add wasm32-unknown-unknown
cargo run -p craftui-cli -- build --platform web --release \
    -p craftui-example-notes -o dist/notes-web
```

`dist/notes-web` then holds `index.html`, `craftui_app.js` and
`craftui_app_bg.wasm`. Serve the folder with any static file server, for example
`python3 -m http.server -d dist/notes-web 8080`, and open
<http://localhost:8080>. To publish the app, upload the folder to any static
host.

### Android

Needs JDK 17, the Android SDK and NDK (`ANDROID_HOME`, `ANDROID_NDK_HOME`),
Gradle 8, and the `aarch64-linux-android` Rust target.

```bash
rustup target add aarch64-linux-android
cargo run -p craftui-cli -- build --platform android --release \
    -p craftui-example-notes -o dist/notes-android
gradle -p dist/notes-android assembleDebug
adb install -r dist/notes-android/app/build/outputs/apk/debug/app-debug.apk
```

The first command compiles the Rust code to `libcraftui_example_notes.so` and
writes a Gradle project around it; Gradle packages the APK.

You can also download ready-made builds: each CI run on GitHub attaches the
artifacts `craftui-notes-android-debug-apk` and `craftui-notes-web`.

## How it is organised

| File | Role |
|---|---|
| `src/model.rs` | The notes and every rule about them: ordering, search, pinning, JSON. No UI code. |
| `src/storage.rs` | Saves one string. A file on desktop and Android, `localStorage` in the browser, memory in tests. |
| `ui/notes.cui` | The screen. Compiled to Rust by `build.rs`. |
| `src/lib.rs` | `NotesController` connects model, storage and screen. Also the Web and Android entry points, and the UI tests. |
| `src/main.rs` | Desktop entry point. |
| `craftui.toml` | App name, id (`dev.craftui.notes`) and Android SDK levels, used for packaging. |

The model is the single source of truth. The controller publishes what the
screen should show (a list of cards, a few labels and flags) into the
component's properties, and the screen reports what the user did through
callbacks. See the [step-by-step guide](../../docs/src/guides/notes-app.md).

## Where notes are stored

| Platform | Location |
|---|---|
| Android | `notes.json` in the app's private files directory. Removed when the app is uninstalled. |
| Web | `localStorage` key `craftui-notes/v1`, per browser and per site. |
| Linux | `$XDG_DATA_HOME/craftui-notes/notes.json`, or `~/.local/share/craftui-notes/notes.json` |
| macOS | `~/Library/Application Support/craftui-notes/notes.json` |
| Windows | `%APPDATA%\craftui-notes\notes.json` |

If saved data cannot be read, the app leaves it untouched, shows a warning, and
does not save during that session, so nothing is overwritten.

## Tests

```bash
cargo test -p craftui-example-notes
```

The tests cover the model, storage, and the UI itself: they mount the real
screen without a window, tap buttons, type text, resize between phone and
desktop sizes, and check what is shown and what was saved. One test renders each
screen to a PNG; set `CRAFTUI_SNAPSHOT_DIR=some/folder` to keep the images.

CI adds two end-to-end checks:

- **Web**: `.github/scripts/web-smoke.mjs` loads the built bundle in headless
  Chrome, creates a note by clicking and typing, reloads the page, and checks
  the note is still there.
- **Android**: the APK is built, and its package id, native library and entry
  points are verified.

## Known limits

- The Android build has only been compiled and packaged in CI. It has not been
  run on a device or emulator by the test suite, so touch and keyboard behaviour
  on a real phone is unverified.
- The APK requests `INTERNET` and `USE_BIOMETRIC` although the app uses neither.
  The CLI's Android template always adds the first, and the biometrics library
  it links adds the second.
- Notes are plain text. There is no sync, export or rich formatting.
- Only `arm64-v8a` is built for Android.
