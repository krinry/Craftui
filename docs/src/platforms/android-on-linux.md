# Android

A CraftUI Android app is a native library (`.so`) that draws the whole
interface itself, loaded by a small generated Gradle project.

## What is verified

CI builds `examples/notes` into a debug APK on every change and inspects it:
package `dev.craftui.notes`, one `arm64-v8a` library that exports
`android_main`, target SDK 35, size about 12 MB.

The APK has **not** been run on a phone or emulator in CI. Treat behaviour on a
real device as untested until you have tried it.

## What you need

- The Rust target: `rustup target add aarch64-linux-android`
- The Android SDK and NDK. Set `ANDROID_HOME` (or `ANDROID_SDK_ROOT`) and
  `ANDROID_NDK_HOME` (or `ANDROID_NDK_ROOT`).
- JDK 17 and Gradle 8.7 or newer. The generated project uses the Android Gradle
  Plugin 8.6.1, which needs 8.7+; CI uses 8.10.2.

`craftui doctor` reports which of these it can find.

## The entry point

The library exports `android_main`. It starts the Android runner with a closure
that builds the interface:

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: craftui::platform_android::android_activity::AndroidApp) {
    craftui::platform_android::android_runtime::run_android_app(
        app,
        |cx, tree, _textures, _dispatcher| build_my_ui(cx, tree),
    );
}
```

`examples/notes/src/lib.rs` has a complete version, including where it reads
the app's private data directory.

The package must build a `cdylib`:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

## Build an APK

```bash
craftui build --platform android --release -p craftui-example-notes -o dist/notes-android
gradle -p dist/notes-android assembleDebug
```

The first command compiles the library with the NDK compiler and writes a
Gradle project to the output folder. The second packages it. The APK is at
`dist/notes-android/app/build/outputs/apk/debug/app-debug.apk`.

Install it on a connected device:

```bash
adb install -r dist/notes-android/app/build/outputs/apk/debug/app-debug.apk
```

`craftui run --platform android -p craftui-example-notes` does the build, runs
Gradle, and when a device is connected installs the APK with `adb install -r`
and starts it. Pass `-p` with your own package name: without it the CLI builds
the default `craftui-platform-android` package instead of your app.

Only `arm64-v8a` is built. Emulators on x86-64 machines need an ARM system
image.

## App settings (`craftui.toml`)

```toml
[app]
name = "CraftUI Notes"
id = "dev.craftui.notes"
version = "0.1.0"

[android]
min_sdk = 26
target_sdk = 35
version_code = 1

[permissions]
# camera = "Used to scan documents"
```

Each entry under `[permissions]` becomes the matching `<uses-permission>` in the
generated manifest. `craftui permissions list` shows the names.

## Known limits

- The generated project always declares `INTERNET`, and links a biometrics
  library that adds `USE_BIOMETRIC` and `USE_FINGERPRINT`, even when
  `[permissions]` is empty.
- Only debug builds are produced by the documented commands. Release signing is
  not covered.
- Only the `arm64-v8a` ABI is built.
