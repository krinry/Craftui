# Permissions

CraftUI has one list of permission names that it maps to each platform. You
declare what the app needs in `craftui.toml` and request it at run time from
Rust.

---

## 1. The permission names

Print the full list with the CLI:

```bash
craftui permissions list
```

A selection:

| Permission Key | Description | Android Mapping | iOS Usage Description |
|---|---|---|---|
| `camera` | Camera capture | `android.permission.CAMERA` | `NSCameraUsageDescription` |
| `microphone` | Audio recording | `android.permission.RECORD_AUDIO` | `NSMicrophoneUsageDescription` |
| `location_when_in_use` | Foreground GPS | `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` | `NSLocationWhenInUseUsageDescription` |
| `photos` | Photo library read | `READ_MEDIA_IMAGES` (API 33+), `READ_EXTERNAL_STORAGE` (API <= 32) | `NSPhotoLibraryUsageDescription` |
| `biometrics` | Fingerprint / Face | `USE_BIOMETRIC` | `NSFaceIDUsageDescription` |
| `contacts` | Address book | `READ_CONTACTS`, `WRITE_CONTACTS` | `NSContactsUsageDescription` |
| `bluetooth` | BLE & Classic | `BLUETOOTH_SCAN`, `BLUETOOTH_CONNECT`, `BLUETOOTH_ADVERTISE` (API 31+) | `NSBluetoothAlwaysUsageDescription` |
| `vibrate` | Haptic feedback | `android.permission.VIBRATE` | *(Built-in CoreHaptics)* |
| `notifications` | Push alerts | `POST_NOTIFICATIONS` (API 33+) | *(UserNotifications Framework)* |

---

## 2. Manifest Reference (`craftui.toml`)

Enable permissions with human-readable rationale strings in your `craftui.toml`:

```toml
[app]
name = "MyPhotoApp"
id = "dev.craftui.photo"
version = "1.0.0"

[permissions]
camera = "Allows you to take pictures directly in the app."
photos = "Allows you to choose photos from your gallery."
vibrate = "Provides haptic feedback when taking pictures."
```

Check the manifest for unknown names and missing descriptions:
```bash
craftui permissions check
```

---

## 3. Requesting permissions in Rust

`PlatformServices` checks and requests permissions at run time. The calls are
ordinary functions that return the status directly:

```rust
use craftui::core::{Permission, PermissionStatus, PlatformServices};

fn take_photo(services: &PlatformServices) {
    match services.check_permission(&Permission::Camera) {
        PermissionStatus::Granted => capture_image(),
        PermissionStatus::NotDetermined | PermissionStatus::Denied => {
            if services.request_permission(&Permission::Camera) == PermissionStatus::Granted {
                capture_image();
            }
        }
        // PermanentlyDenied, Restricted, Limited, Provisional, Unsupported
        other => println!("camera is not available: {other:?}"),
    }
}
```

`request_permissions(&[...])` asks for several at once. On platforms where the
answer arrives later, `request_permission_async` and `request_permissions_async`
take a `UiDispatcher` and call you back on the UI thread. Inside an app the
services are available as `tree.services`.

What each platform does with a request depends on its backend; the desktop
backend is an in-memory stand-in intended for development and tests.
