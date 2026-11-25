//! WebAssembly platform adapter (`wasm-bindgen` + `web-sys` Canvas/DPR observer +
//! async WebGPU/WebGL2 + 2D Canvas fallback + hidden `<input>` DOM `input`/IME bridge +
//! `#craftui-a11y-root` DOM ARIA mirror + CSS cursor sync + browser clipboard/paste +
//! `window.location.hash` router sync + RAII listener lifecycle + monotonic timers).

pub mod dom;
pub mod host;
pub mod route;
#[cfg(target_arch = "wasm32")]
pub mod runner;
pub mod services;

#[cfg(test)]
mod tests;

pub use dom::{
    WebAriaNode, WebClipboard, craft_cursor_to_css, craft_role_to_aria, dom_button_to_craft,
    dom_composition_to_ime, dom_input_to_events, dom_key_to_craft, dom_keyboard_event_to_craft,
    dom_pointer_event, dom_wheel_event,
};
pub use host::WebAppHost;
pub use route::{BrowserRouteSync, HiddenImeInputState, RouteBridge, WebBridgeConfig};
#[cfg(target_arch = "wasm32")]
pub use runner::wasm_runner::{WebRunnerHandle, mount_web_app, start_web_app};
pub use services::{
    WebBridgeState, WebDeviceCapabilities, WebHapticsBackend, WebPermissionBackend,
    WebSecureStorage, WebShareBackend, parse_web_permission_state, web_permission_descriptor,
};
