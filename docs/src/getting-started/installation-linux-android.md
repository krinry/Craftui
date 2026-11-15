# Installation

This page sets up a machine to build CraftUI apps for desktop, the Web and
Android. The package lists are the ones the project's CI uses on Ubuntu.

## 1. Rust

CraftUI needs Rust 1.89 or newer.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

## 2. Linux system libraries

```bash
sudo apt update
sudo apt install -y build-essential pkg-config \
    libwayland-dev libxkbcommon-dev libfontconfig1-dev libfreetype-dev libx11-dev
```

macOS and Windows need no extra packages for desktop builds.

## 3. Get CraftUI

CraftUI is not published on crates.io yet. Clone the repository and install the
command-line tool from it:

```bash
git clone https://github.com/krinry/Craftui
cd Craftui
cargo install --path tools/craftui-cli
```

This installs the `craftui` command. Check your setup:

```bash
craftui doctor
```

`doctor` lists the Rust toolchain, the installed targets and the Android tools
it found, and tells you what is missing.

## 4. Web builds (optional)

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
```

The `wasm-bindgen-cli` version must equal the `wasm-bindgen` version in
`Cargo.lock`; the build fails with a clear message when they differ.

## 5. Android builds (optional)

You need the Android SDK and NDK, JDK 17 and Gradle 8. Android Studio installs
the first two, or use the command-line SDK tools.

```bash
rustup target add aarch64-linux-android
```

Add to your shell profile, with the NDK version you installed:

```bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/<version>"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

To install on a phone, enable USB debugging and check that it is listed:

```bash
adb devices
```

## Next

Continue with the [quickstart](quickstart.md), or build the
[notes app](../guides/notes-app.md) for all three platforms.
