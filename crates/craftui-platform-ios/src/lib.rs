//! iOS platform adapter (`UIKit` lifecycle C-ABI entrypoints, `CAMetalLayer` + `CADisplayLink`
//! frame pacing, Dynamic Island / Home Indicator SafeArea, `UITextInput` IME, and `CVPixelBuffer` external textures).

pub mod ffi;
pub mod host;
pub mod services;
pub mod types;

#[cfg(test)]
mod tests;

pub use ffi::*;
#[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
pub use host::ios_metal;
pub use host::{IosAppHost, IosMetalLayerDescriptor};
pub use services::{
    IosBridgeState, IosDeviceCapabilities, IosHapticsBackend, IosPermissionBackend,
    IosPlatformCommand, IosSecureStorage, IosShareBackend, lifecycle_event_from_code,
    network_status_from_code, permission_status_from_code, permission_status_to_code,
};
pub use types::{
    CvPixelBufferBridge, CvPixelBufferFormat, CvPixelBufferFrame, IosBuildUiCallback,
    IosImeCommand, IosKeyboardFrame, IosPlatformViewMode, IosSafeAreaInsets,
    bgra_frame_to_packed_rgba,
};
