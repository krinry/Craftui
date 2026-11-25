//! Desktop platform runner (`winit` + `wgpu`/`vello`/`gpu-sdf` + `accesskit_winit` + `notify` hot-reload)
//! supporting Linux (Wayland/X11), macOS, and Windows with multi-window support, in-process native OS
//! clipboard (`x11-clipboard`/`wl-clipboard-rs`/`objc2-app-kit`/`windows-sys`), `rfd` native file
//! pickers (`xdg-portal` + `wayland`, AppKit, Win32 IFileDialog), cursor synchronization, and timer scheduling.

pub mod clipboard;
pub mod events;
pub mod file_dialog;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
pub mod permissions;
pub mod presenter;
pub mod runner;
pub mod services;

#[cfg(test)]
mod tests;

pub use clipboard::DesktopClipboard;
pub use events::{
    DesktopUserEvent, translate_modifiers, translate_mouse_button, translate_scroll_delta,
    translate_winit_cursor, translate_winit_key,
};
pub use file_dialog::{DesktopFileDialog, build_rfd_dialog};
#[cfg(feature = "hot-reload")]
pub use hot_reload::should_trigger_hot_reload;
pub use permissions::DesktopPermissionBackend;
pub use presenter::pack_rgba8_to_softbuffer_0rgb;
pub use runner::{AppContext, WindowConfig, WindowLauncher, run_app};
pub use winit::window::WindowId;
#[cfg(feature = "hot-reload")]
pub use runner::{run_hot_reload_cui, run_live_preview_cui};
pub use services::{
    DesktopDeviceCapabilities, DesktopSecureStorage, DesktopShareBackend, default_desktop_services,
};
