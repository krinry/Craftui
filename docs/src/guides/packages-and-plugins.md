# Packages & Plugins (`crates.io`, `MethodChannel`, and `HtmlView`)

CraftUI avoids building an isolated package ecosystem from scratch by letting you combine three existing ecosystems:

1. **`crates.io` (150,000+ Rust crates):** Native support for HTTP (`reqwest`), databases (`rusqlite`, `sqlx`), serialization (`serde`), audio (`rodio`), image processing (`image`), and crypto.
2. **`MethodChannel` Native Plugins (`craftui-core`):** A Flutter-inspired `MethodChannel` / `PluginRegistry` bridge for calling platform-native Android (Kotlin/Java), iOS (Swift/ObjC), or Desktop APIs—making it straightforward to port native handlers from `pub.dev` plugins without embedding a Dart VM.
3. **`npm` & Web APIs (`HtmlView`):** Embed rich web views or existing JS/Tailwind components with `window.__TAURI__`-compatible IPC.

---

## 1. Using `crates.io` Packages

Add any Rust crate to your CraftUI project using the CLI:

```bash
craftui add reqwest --features json
craftui add serde --features derive
```

This runs `cargo add` for your crate and records it under `[dependencies]` in `craftui.toml`.

### Connecting Background Crates to UI Signals

Use `ResourceState<T>` from `craftui_reactive` (re-exported by `craftui`) with `UiDispatcher` to drive `.cui` UI state cleanly from background threads:

```rust
use craftui::reactive::{ResourceState, Scope, Signal};

fn fetch_profile(cx: Scope, state: Signal<ResourceState<String>>) {
    state.set(ResourceState::Loading);
    // Spawn on a background thread / tokio runtime, then dispatch back to the UI thread:
    let dispatcher = craftui::reactive::ui_dispatcher();
    std::thread::spawn(move || {
        let result = Ok::<String, String>("Loaded from crates.io".into());
        let next = match result {
            Ok(data) => ResourceState::Ready(data),
            Err(err) => ResourceState::Error(err),
        };
        if let Some(d) = dispatcher {
            d.post(move || state.set(next));
        }
    });
}
```

---

## 2. Porting `pub.dev` Native Plugins via `MethodChannel`

Pure-Dart `pub.dev` packages require a Dart VM and cannot run directly inside a Rust binary. However, **platform plugins** on `pub.dev` (camera, biometrics, notifications, share, URL launcher, etc.) are thin wrappers over native Kotlin/Swift code communicating over a `MethodChannel`.

CraftUI provides `CraftPlugin`, `MethodCall`, and `PluginRegistry` in `craftui-core`:

```rust
use craftui::core::{CraftPlugin, MethodCall, PluginRegistry, Result};
use serde_json::json;

struct DeviceBatteryPlugin;

impl CraftPlugin for DeviceBatteryPlugin {
    fn name(&self) -> &str {
        "dev.craftui.battery"
    }

    fn handle(&self, call: &MethodCall) -> Result<serde_json::Value> {
        match call.method.as_str() {
            "getBatteryLevel" => Ok(json!({ "level": 92 })),
            other => Err(craftui::core::CraftError::Platform(format!(
                "unimplemented method `{other}` on channel `{}`",
                call.channel
            ))),
        }
    }
}

fn register(registry: &PluginRegistry) {
    registry.register(DeviceBatteryPlugin);
    let res = registry.invoke("dev.craftui.battery", "getBatteryLevel", json!({}));
    println!("{res:?}");
}
```

You can attach a `PluginRegistry` to `PlatformServices::with_plugins(...)` so every platform host shares the same channel interface.

## 3. Built-in First-Party Plugins (`craftui-core`)

`PluginRegistry::with_builtin_plugins()` (or `registry.register_builtin_plugins()`) registers in-memory/mock-capable defaults for headless and desktop runs, matching the live native handlers in Android `MainActivity.kt` and iOS `CraftUIPermissions.swift`:

| Channel | Typed Client (`craftui_core`) | Methods | Native Backends |
| :--- | :--- | :--- | :--- |
| `dev.craftui.camera` | `CameraClient` | `capturePhoto`, `pickImage` | Android `TakePicturePreview`/`GetContent`, iOS `UIImagePickerController` |
| `dev.craftui.biometrics` | `BiometricsClient` | `isAvailable`, `authenticate` | Android `BiometricPrompt` / `BiometricManager`, iOS `LAContext` |
| `dev.craftui.notifications` | `NotificationsClient` | `schedule`, `cancel`, `cancelAll` | Android `NotificationManager` + `NotificationChannel`, iOS `UNUserNotificationCenter` |
| `dev.craftui.url_launcher` | `UrlLauncherClient` | `canLaunch`, `launch` | Android `Intent.ACTION_VIEW`, iOS `UIApplication.shared.open` |
