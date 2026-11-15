//! WebAssembly platform adapter (`wasm-bindgen` + `web-sys` Canvas/DPR observer +
//! async WebGPU/WebGL2 + 2D Canvas fallback + hidden `<input>` DOM `input`/IME bridge +
//! `#craftui-a11y-root` DOM ARIA mirror + CSS cursor sync + browser clipboard/paste +
//! `window.location.hash` router sync + RAII listener lifecycle + monotonic timers).

use craftui_a11y::A11yRole;
use craftui_core::{
    AppLifecycleEvent, BatteryState, ClipboardBackend, CursorIcon, DeviceCapabilitiesBackend,
    DeviceInfo, HapticStyle, HapticsBackend, ImeEvent, KeyCode, KeyEvent, Modifiers, NetworkStatus,
    NodeId, Permission, PermissionBackend, PermissionStatus, PlatformServices, Point,
    PointerButton, PointerEvent, PointerId, PointerPhase, Rect, ScrollDelta, SecureStorageBackend,
    ShareBackend, SharePayload, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba};
use craftui_widgets::{Router, WidgetKind, WidgetTree};
use slotmap::Key;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Configuration for bootstrapping a CraftUI WASM application inside an HTML canvas element.
#[derive(Debug, Clone)]
pub struct WebBridgeConfig {
    pub canvas_id: String,
    pub sync_url_hash: bool,
    pub force_2d_fallback: bool,
}

impl Default for WebBridgeConfig {
    fn default() -> Self {
        Self {
            canvas_id: "craftui-canvas".to_string(),
            sync_url_hash: true,
            force_2d_fallback: false,
        }
    }
}

/// Synchronizes hidden `<input>` accessibility/IME overlay coordinates with the focused `TextInput`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HiddenImeInputState {
    pub visible: bool,
    pub x_dp: f32,
    pub y_dp: f32,
    pub width_dp: f32,
    pub height_dp: f32,
    pub value: String,
}

/// Type-erased router binding connecting CraftUI [`Router<R>`] state to browser URL hashes.
#[derive(Clone)]
pub struct RouteBridge {
    current_route: Rc<dyn Fn() -> String>,
    apply_route: Rc<dyn Fn(&str) -> bool>,
}

impl RouteBridge {
    /// Construct a [`RouteBridge`] for any custom route enum/struct `R`.
    pub fn new<R: Clone + PartialEq + 'static>(
        router: Router<R>,
        to_path: fn(&R) -> String,
        from_path: fn(&str) -> R,
    ) -> Self {
        Self {
            current_route: Rc::new(move || to_path(&router.current())),
            apply_route: Rc::new(move |path: &str| {
                let target = from_path(path);
                if router.current() != target {
                    router.push(target);
                    true
                } else {
                    false
                }
            }),
        }
    }

    pub fn current_route(&self) -> String {
        (self.current_route)()
    }

    pub fn navigate(&self, route: &str) -> bool {
        (self.apply_route)(route)
    }
}

/// Maps CraftUI router route strings to/from browser `window.location.hash` fragments (`#/route`).
pub struct BrowserRouteSync;

impl BrowserRouteSync {
    pub fn route_to_hash(route: &str) -> String {
        let trimmed = route.trim_start_matches('#').trim_start_matches('/');
        format!("#/{trimmed}")
    }

    pub fn hash_to_route(hash: &str) -> String {
        let trimmed = hash.trim_start_matches('#').trim_start_matches('/');
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{trimmed}")
        }
    }

    /// Connect a typed [`Router<R>`] in `scope` so WASM `hashchange` / `window.location.hash` stay synchronized.
    pub fn connect_router<R: Clone + PartialEq + 'static>(
        scope: Scope,
        router: Router<R>,
        to_path: fn(&R) -> String,
        from_path: fn(&str) -> R,
    ) {
        scope.provide_context(RouteBridge::new(router, to_path, from_path));
    }

    /// Connect a `Router<String>` directly in `scope` using raw path strings (e.g., `"/"`, `"/settings"`).
    pub fn connect_string_router(scope: Scope, router: Router<String>) {
        scope.provide_context(RouteBridge::new(
            router,
            |s| s.clone(),
            |path| path.to_string(),
        ));
    }
}

/// Map a CraftUI [`CursorIcon`] hint to the standard CSS `cursor` keyword.
pub fn craft_cursor_to_css(cursor: CursorIcon) -> &'static str {
    match cursor {
        CursorIcon::Default => "default",
        CursorIcon::Pointer => "pointer",
        CursorIcon::Text => "text",
        CursorIcon::Grab => "grab",
        CursorIcon::Grabbing => "grabbing",
        CursorIcon::ResizeHorizontal => "ew-resize",
        CursorIcon::ResizeVertical => "ns-resize",
        CursorIcon::NotAllowed => "not-allowed",
        CursorIcon::Crosshair => "crosshair",
    }
}

/// Map a CraftUI semantic [`A11yRole`] to a WAI-ARIA `role` attribute string.
pub fn craft_role_to_aria(role: A11yRole) -> &'static str {
    match role {
        A11yRole::Window => "application",
        A11yRole::Container => "group",
        A11yRole::Label => "note",
        A11yRole::Button => "button",
        A11yRole::TextInput | A11yRole::MultilineTextInput => "textbox",
        A11yRole::CheckBox => "checkbox",
        A11yRole::RadioButton => "radio",
        A11yRole::Switch => "switch",
        A11yRole::Slider => "slider",
        A11yRole::ComboBox => "combobox",
        A11yRole::TabList => "tablist",
        A11yRole::Tab => "tab",
        A11yRole::Menu => "menu",
        A11yRole::MenuItem => "menuitem",
        A11yRole::Tooltip => "tooltip",
        A11yRole::Divider => "separator",
        A11yRole::ProgressIndicator => "progressbar",
        A11yRole::Image => "img",
        A11yRole::ScrollView => "region",
        A11yRole::Canvas => "img",
        A11yRole::Dialog => "dialog",
        A11yRole::Alert => "alert",
        A11yRole::List => "list",
        A11yRole::ListItem => "listitem",
    }
}

/// Snapshot node used to synchronize an invisible DOM ARIA mirror under `#craftui-a11y-root`.
#[derive(Debug, Clone, PartialEq)]
pub struct WebAriaNode {
    pub id: u64,
    pub role: &'static str,
    pub raw_role: A11yRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub numeric_value: Option<f64>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub focusable: bool,
    pub focused: bool,
    pub rect: Rect,
}

/// Browser clipboard bridge backed by `navigator.clipboard` + trusted DOM `paste` events (on `wasm32`)
/// and a synchronous shared cache.
#[derive(Clone, Default)]
pub struct WebClipboard {
    cache: Arc<Mutex<String>>,
}

impl WebClipboard {
    pub fn new(cache: Arc<Mutex<String>>) -> Self {
        Self { cache }
    }

    /// Update the synchronous clipboard cache (e.g. from a DOM `ClipboardEvent` or async `readText()`).
    pub fn hydrate(&self, text: &str) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
    }

    /// Populate clipboard cache asynchronously via `navigator.clipboard.readText()` on `wasm32`.
    #[cfg(target_arch = "wasm32")]
    pub fn read_browser_clipboard_async<F>(cache: Arc<Mutex<String>>, on_ready: F)
    where
        F: FnOnce(String) + 'static,
    {
        if let Some(window) = web_sys::window() {
            let promise = window.navigator().clipboard().read_text();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(val) = wasm_bindgen_futures::JsFuture::from(promise).await
                    && let Some(text) = val.as_string()
                {
                    if let Ok(mut guard) = cache.lock() {
                        guard.clear();
                        guard.push_str(&text);
                    }
                    on_ready(text);
                }
            });
        }
    }
}

impl ClipboardBackend for WebClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.cache.lock().ok().map(|g| g.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(text);
        }
        true
    }
}

/// Map a CraftUI [`Permission`] to its standard W3C Permissions API descriptor name, if any.
pub fn web_permission_descriptor(permission: &Permission) -> Option<&'static str> {
    match permission {
        Permission::Camera => Some("camera"),
        Permission::Microphone => Some("microphone"),
        Permission::LocationWhenInUse | Permission::LocationCoarse | Permission::LocationAlways => {
            Some("geolocation")
        }
        Permission::Notifications => Some("notifications"),
        Permission::ClipboardRead => Some("clipboard-read"),
        Permission::Bluetooth | Permission::BluetoothScan | Permission::BluetoothConnect => {
            Some("bluetooth")
        }
        Permission::Nfc => Some("nfc"),
        Permission::Sensors | Permission::MotionActivity => Some("accelerometer"),
        Permission::MediaProjection => Some("display-capture"),
        Permission::BackgroundRefresh => Some("periodic-background-sync"),
        _ => None,
    }
}

/// Parse a DOM Permissions API / Notification API status string (`"granted"`, `"denied"`, `"prompt"`, `"default"`).
pub fn parse_web_permission_state(state: &str) -> PermissionStatus {
    match state.trim().to_ascii_lowercase().as_str() {
        "granted" => PermissionStatus::Granted,
        "denied" => PermissionStatus::Denied,
        "prompt" | "default" => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Unsupported,
    }
}

/// Shared Web platform bridge holding browser permission cache, session secrets, device info,
/// network/battery telemetry, and tab visibility lifecycle history.
#[derive(Debug)]
pub struct WebBridgeState {
    statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    secure_entries: Mutex<HashMap<String, String>>,
    device_info: Mutex<DeviceInfo>,
    network_status: Mutex<NetworkStatus>,
    battery_state: Mutex<BatteryState>,
    lifecycle_history: Mutex<Vec<AppLifecycleEvent>>,
}

impl Default for WebBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

impl WebBridgeState {
    pub fn new() -> Self {
        Self {
            statuses: Mutex::new(HashMap::new()),
            secure_entries: Mutex::new(HashMap::new()),
            device_info: Mutex::new(DeviceInfo {
                os: "Web".to_string(),
                model: "Browser".to_string(),
                os_version: String::new(),
                sdk_int: None,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            network_status: Mutex::new(NetworkStatus::Unknown),
            battery_state: Mutex::new(BatteryState::default()),
            lifecycle_history: Mutex::new(Vec::new()),
        }
    }

    pub fn set_permission_status(&self, permission: Permission, status: PermissionStatus) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(permission, status);
        }
    }

    pub fn permission_status(&self, permission: &Permission) -> Option<PermissionStatus> {
        self.statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
    }

    pub fn set_device_info(&self, info: DeviceInfo) {
        if let Ok(mut guard) = self.device_info.lock() {
            *guard = info;
        }
    }

    pub fn device_info(&self) -> DeviceInfo {
        self.device_info
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn set_network_status(&self, status: NetworkStatus) {
        if let Ok(mut guard) = self.network_status.lock() {
            *guard = status;
        }
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.network_status.lock().map(|g| *g).unwrap_or_default()
    }

    pub fn set_battery_state(&self, state: BatteryState) {
        if let Ok(mut guard) = self.battery_state.lock() {
            *guard = state;
        }
    }

    pub fn battery_state(&self) -> BatteryState {
        self.battery_state.lock().map(|g| *g).unwrap_or_default()
    }

    pub fn push_lifecycle_event(&self, event: AppLifecycleEvent) {
        if let Ok(mut guard) = self.lifecycle_history.lock() {
            guard.push(event);
        }
    }

    pub fn lifecycle_history(&self) -> Vec<AppLifecycleEvent> {
        self.lifecycle_history
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn hydrate_from_browser(self: &Arc<Self>, on_update: impl Fn() + Clone + 'static) {
        use wasm_bindgen::JsCast;
        let Some(window) = web_sys::window() else {
            return;
        };
        let nav = window.navigator();

        self.refresh_online_from_navigator(&nav);

        let platform = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("platform"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "Browser".to_string());
        let ua = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("userAgent"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        self.set_device_info(DeviceInfo {
            os: "Web".to_string(),
            model: platform,
            os_version: ua,
            sdk_int: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        });

        if let Some(notif_status) = read_browser_notification_permission(&window) {
            self.set_permission_status(Permission::Notifications, notif_status);
        }

        for perm in [
            Permission::Camera,
            Permission::Microphone,
            Permission::LocationWhenInUse,
            Permission::Notifications,
            Permission::ClipboardRead,
            Permission::MediaProjection,
        ] {
            query_browser_permission_async(self.clone(), perm, on_update.clone());
        }

        if let Ok(get_bat) =
            js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("getBattery"))
            && let Some(func) = get_bat.dyn_ref::<js_sys::Function>()
            && let Ok(prom_val) = func.call0(&nav)
            && let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>()
        {
            let bridge = self.clone();
            let wake = on_update;
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(mgr) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    let level =
                        js_sys::Reflect::get(&mgr, &wasm_bindgen::JsValue::from_str("level"))
                            .ok()
                            .and_then(|v| v.as_f64())
                            .map(|f| (f * 100.0).round().clamp(0.0, 100.0) as u8);
                    let charging =
                        js_sys::Reflect::get(&mgr, &wasm_bindgen::JsValue::from_str("charging"))
                            .ok()
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                    bridge.set_battery_state(BatteryState {
                        level_pct: level,
                        charging,
                    });
                    wake();
                }
            });
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn refresh_online_from_navigator(&self, nav: &web_sys::Navigator) {
        let online = js_sys::Reflect::get(nav, &wasm_bindgen::JsValue::from_str("onLine"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !online {
            self.set_network_status(NetworkStatus::Offline);
            return;
        }
        let conn_type = js_sys::Reflect::get(nav, &wasm_bindgen::JsValue::from_str("connection"))
            .ok()
            .and_then(|c| {
                js_sys::Reflect::get(&c, &wasm_bindgen::JsValue::from_str("type"))
                    .ok()
                    .and_then(|t| t.as_string())
            })
            .unwrap_or_default();
        let status = match conn_type.to_ascii_lowercase().as_str() {
            "cellular" => NetworkStatus::Cellular,
            "ethernet" => NetworkStatus::Ethernet,
            "wifi" => NetworkStatus::Wifi,
            "none" => NetworkStatus::Offline,
            _ => NetworkStatus::Wifi,
        };
        self.set_network_status(status);
    }
}

#[cfg(target_arch = "wasm32")]
fn read_browser_notification_permission(window: &web_sys::Window) -> Option<PermissionStatus> {
    let notif =
        js_sys::Reflect::get(window, &wasm_bindgen::JsValue::from_str("Notification")).ok()?;
    let perm = js_sys::Reflect::get(&notif, &wasm_bindgen::JsValue::from_str("permission"))
        .ok()?
        .as_string()?;
    Some(parse_web_permission_state(&perm))
}

#[cfg(target_arch = "wasm32")]
fn query_browser_permission_async(
    bridge: Arc<WebBridgeState>,
    permission: Permission,
    on_update: impl Fn() + 'static,
) {
    use wasm_bindgen::JsCast;
    let Some(descriptor) = web_permission_descriptor(&permission) else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let nav = window.navigator();
    let Ok(perms_obj) = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("permissions"))
    else {
        return;
    };
    let Ok(query_fn_val) =
        js_sys::Reflect::get(&perms_obj, &wasm_bindgen::JsValue::from_str("query"))
    else {
        return;
    };
    let Ok(query_fn) = query_fn_val.dyn_into::<js_sys::Function>() else {
        return;
    };
    let desc = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &desc,
        &wasm_bindgen::JsValue::from_str("name"),
        &wasm_bindgen::JsValue::from_str(descriptor),
    );
    let Ok(prom_val) = query_fn.call1(&perms_obj, &desc) else {
        return;
    };
    let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>() else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(status_obj) = wasm_bindgen_futures::JsFuture::from(promise).await
            && let Ok(state_val) =
                js_sys::Reflect::get(&status_obj, &wasm_bindgen::JsValue::from_str("state"))
            && let Some(state_str) = state_val.as_string()
        {
            bridge.set_permission_status(permission, parse_web_permission_state(&state_str));
            on_update();
        }
    });
}

/// Web browser permission bridge querying `navigator.permissions` & `Notification.permission`
/// without ever blocking the browser main thread.
#[derive(Clone, Debug)]
pub struct WebPermissionBackend {
    bridge: Arc<WebBridgeState>,
}

impl WebPermissionBackend {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }
}

impl PermissionBackend for WebPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        if let Some(cached) = self.bridge.permission_status(permission) {
            return cached;
        }
        #[cfg(target_arch = "wasm32")]
        if *permission == Permission::Notifications
            && let Some(win) = web_sys::window()
            && let Some(st) = read_browser_notification_permission(&win)
        {
            self.bridge.set_permission_status(permission.clone(), st);
            return st;
        }
        if web_permission_descriptor(permission).is_some() {
            PermissionStatus::NotDetermined
        } else {
            PermissionStatus::Unsupported
        }
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        let current = self.check(permission);
        if current != PermissionStatus::NotDetermined {
            return current;
        }
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if *permission == Permission::Notifications
                && let Some(window) = web_sys::window()
                && let Ok(notif) =
                    js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("Notification"))
                && let Ok(req_fn_val) = js_sys::Reflect::get(
                    &notif,
                    &wasm_bindgen::JsValue::from_str("requestPermission"),
                )
                && let Ok(req_fn) = req_fn_val.dyn_into::<js_sys::Function>()
                && let Ok(prom_val) = req_fn.call0(&notif)
                && let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>()
            {
                let bridge = self.bridge.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(res) = wasm_bindgen_futures::JsFuture::from(promise).await
                        && let Some(s) = res.as_string()
                    {
                        bridge.set_permission_status(
                            Permission::Notifications,
                            parse_web_permission_state(&s),
                        );
                        wasm_runner::request_all_frames();
                    }
                });
            } else {
                query_browser_permission_async(
                    self.bridge.clone(),
                    permission.clone(),
                    wasm_runner::request_all_frames,
                );
            }
        }
        current
    }

    fn should_show_rationale(&self, _permission: &Permission) -> bool {
        false
    }

    fn open_app_settings(&self) -> bool {
        false
    }
}

/// Web vibration haptics bridge calling `navigator.vibrate(ms)` when supported by mobile browsers.
#[derive(Clone, Copy, Debug, Default)]
pub struct WebHapticsBackend;

impl WebHapticsBackend {
    pub const fn duration_ms(style: HapticStyle) -> f64 {
        match style {
            HapticStyle::Selection => 8.0,
            HapticStyle::Light => 12.0,
            HapticStyle::Success => 18.0,
            HapticStyle::Medium => 25.0,
            HapticStyle::Warning => 35.0,
            HapticStyle::Heavy => 45.0,
            HapticStyle::Error => 60.0,
        }
    }
}

impl HapticsBackend for WebHapticsBackend {
    fn trigger(&self, #[allow(unused_variables)] style: HapticStyle) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window() {
                let nav = window.navigator();
                if let Ok(vibrate_val) =
                    js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("vibrate"))
                    && let Ok(vibrate_fn) = vibrate_val.dyn_into::<js_sys::Function>()
                    && let Ok(res) = vibrate_fn.call1(
                        &nav,
                        &wasm_bindgen::JsValue::from_f64(Self::duration_ms(style)),
                    )
                {
                    return res.as_bool().unwrap_or(false);
                }
            }
        }
        false
    }
}

/// Web Share API (`navigator.share`) bridge with clipboard copy fallback for browsers without Web Share.
#[derive(Clone, Default)]
pub struct WebShareBackend {
    clipboard: WebClipboard,
}

impl WebShareBackend {
    pub fn new(clipboard: WebClipboard) -> Self {
        Self { clipboard }
    }

    pub fn format_payload(payload: &SharePayload) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(title) = payload.title.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(title.trim().to_string());
        }
        if let Some(text) = payload.text.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(text.trim().to_string());
        }
        if let Some(url) = payload.url.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(url.trim().to_string());
        }
        if !payload.files.is_empty() {
            let paths: Vec<String> = payload
                .files
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            parts.push(paths.join(", "));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }
}

impl ShareBackend for WebShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        let Some(formatted) = Self::format_payload(payload) else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window() {
                let nav = window.navigator();
                if let Ok(share_val) =
                    js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("share"))
                    && let Ok(share_fn) = share_val.dyn_into::<js_sys::Function>()
                {
                    let data = js_sys::Object::new();
                    if let Some(ref title) = payload.title {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("title"),
                            &wasm_bindgen::JsValue::from_str(title),
                        );
                    }
                    if let Some(ref text) = payload.text {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("text"),
                            &wasm_bindgen::JsValue::from_str(text),
                        );
                    }
                    if let Some(ref url) = payload.url {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("url"),
                            &wasm_bindgen::JsValue::from_str(url),
                        );
                    }
                    if let Ok(prom_val) = share_fn.call1(&nav, &data) {
                        if let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>() {
                            wasm_bindgen_futures::spawn_local(async move {
                                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            });
                        }
                        return true;
                    }
                }
            }
        }
        let mut cb = self.clipboard.clone();
        cb.set_text(&formatted)
    }
}

/// Web credential store backed by an in-memory cache mirrored to browser `window.sessionStorage`.
#[derive(Clone, Debug)]
pub struct WebSecureStorage {
    bridge: Arc<WebBridgeState>,
}

impl WebSecureStorage {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }

    #[cfg(target_arch = "wasm32")]
    fn storage_key(key: &str) -> String {
        format!("craftui_secret:{key}")
    }
}

impl SecureStorageBackend for WebSecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        if let Ok(guard) = self.bridge.secure_entries.lock()
            && let Some(val) = guard.get(key)
        {
            return Some(val.clone());
        }
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(get_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("getItem"))
                && let Ok(get_fn) = get_fn_val.dyn_into::<js_sys::Function>()
                && let Ok(val) = get_fn.call1(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                )
                && let Some(s) = val.as_string()
            {
                if let Ok(mut guard) = self.bridge.secure_entries.lock() {
                    guard.insert(key.to_string(), s.clone());
                }
                return Some(s);
            }
        }
        None
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        let cached = if let Ok(mut guard) = self.bridge.secure_entries.lock() {
            guard.insert(key.to_string(), value.to_string());
            true
        } else {
            false
        };
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(set_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("setItem"))
                && let Ok(set_fn) = set_fn_val.dyn_into::<js_sys::Function>()
            {
                let _ = set_fn.call2(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                    &wasm_bindgen::JsValue::from_str(value),
                );
            }
        }
        cached
    }

    fn delete_secret(&self, key: &str) -> bool {
        #[allow(unused_mut)]
        let mut removed = self
            .bridge
            .secure_entries
            .lock()
            .map(|mut guard| guard.remove(key).is_some())
            .unwrap_or(false);
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(rem_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("removeItem"))
                && let Ok(rem_fn) = rem_fn_val.dyn_into::<js_sys::Function>()
            {
                let _ = rem_fn.call1(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                );
                removed = true;
            }
        }
        removed
    }
}

/// Web device telemetry adapter exposing Navigator platform, online/connection status, and battery level.
#[derive(Clone, Debug)]
pub struct WebDeviceCapabilities {
    bridge: Arc<WebBridgeState>,
}

impl WebDeviceCapabilities {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }
}

impl DeviceCapabilitiesBackend for WebDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        self.bridge.device_info()
    }

    fn network_status(&self) -> NetworkStatus {
        let cur = self.bridge.network_status();
        #[cfg(target_arch = "wasm32")]
        if cur == NetworkStatus::Unknown
            && let Some(window) = web_sys::window()
        {
            self.bridge
                .refresh_online_from_navigator(&window.navigator());
            return self.bridge.network_status();
        }
        cur
    }

    fn battery_state(&self) -> BatteryState {
        self.bridge.battery_state()
    }
}

/// Platform-neutral WASM host managing logical DPR sizing, hidden IME sync, router hash sync,
/// web permissions/capabilities bridge, and rendering.
pub struct WebAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    route_bridge: Option<RouteBridge>,
    last_synced_hash: Option<String>,
    pub clipboard_cache: Arc<Mutex<String>>,
    pub bridge: Arc<WebBridgeState>,
    needs_redraw: bool,
}

impl WebAppHost {
    pub fn new(dispatcher: UiDispatcher, css_width: f32, css_height: f32, dpr: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let clipboard_cache = Arc::new(Mutex::new(String::new()));
        let clipboard = WebClipboard::new(clipboard_cache.clone());
        let bridge = Arc::new(WebBridgeState::new());
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_clipboard(clipboard.clone())
                .with_permissions(WebPermissionBackend::new(bridge.clone()))
                .with_haptics(WebHapticsBackend)
                .with_share(WebShareBackend::new(clipboard))
                .with_secure_storage(WebSecureStorage::new(bridge.clone()))
                .with_device_capabilities(WebDeviceCapabilities::new(bridge.clone())),
        );
        tree.metrics = WindowMetrics {
            logical_size: Size::new(css_width.max(1.0), css_height.max(1.0)),
            scale_factor: dpr.max(1.0),
            ..Default::default()
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            route_bridge: None,
            last_synced_hash: None,
            clipboard_cache,
            bridge,
            needs_redraw: true,
        }
    }

    pub fn bridge(&self) -> Arc<WebBridgeState> {
        self.bridge.clone()
    }

    pub fn hydrate_permission_status(&mut self, permission: Permission, status: PermissionStatus) {
        self.bridge.set_permission_status(permission, status);
        self.needs_redraw = true;
    }

    pub fn push_lifecycle_event(&mut self, event: AppLifecycleEvent) {
        self.bridge.push_lifecycle_event(event);
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub fn handle_ui_event(&mut self, ev: &UiEvent) -> bool {
        let handled = self.tree.handle_event(ev);
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Resolve a [`RouteBridge`] from explicit attachment or `Scope` context (`RouteBridge` or `Router<String>`).
    fn active_route_bridge(&mut self) -> Option<RouteBridge> {
        if let Some(ref rb) = self.route_bridge {
            return Some(rb.clone());
        }
        if let Some(rb) = self.scope.use_context::<RouteBridge>() {
            self.route_bridge = Some(rb.clone());
            return Some(rb);
        }
        if let Some(router) = self.scope.use_context::<Router<String>>() {
            let rb = RouteBridge::new(router, |s| s.clone(), |p| p.to_string());
            self.route_bridge = Some(rb.clone());
            return Some(rb);
        }
        None
    }

    pub fn attach_route_bridge(&mut self, bridge: RouteBridge) {
        self.route_bridge = Some(bridge);
    }

    /// Apply a browser URL hash string (from initial load or `hashchange`) to the attached router.
    pub fn apply_url_hash(&mut self, hash: &str) -> bool {
        let Some(bridge) = self.active_route_bridge() else {
            return false;
        };
        if hash.trim().is_empty() {
            let cur_hash = BrowserRouteSync::route_to_hash(&bridge.current_route());
            self.last_synced_hash = Some(cur_hash);
            return false;
        }
        let route = BrowserRouteSync::hash_to_route(hash);
        let changed = bridge.navigate(&route);
        if changed {
            self.needs_redraw = true;
        }
        self.last_synced_hash = Some(BrowserRouteSync::route_to_hash(&bridge.current_route()));
        changed
    }

    /// Check whether the attached router moved to a new route and return the new `#/route` hash to apply.
    pub fn take_pending_hash_update(&mut self) -> Option<String> {
        let bridge = self.active_route_bridge()?;
        let target_hash = BrowserRouteSync::route_to_hash(&bridge.current_route());
        if self.last_synced_hash.as_deref() != Some(target_hash.as_str()) {
            self.last_synced_hash = Some(target_hash.clone());
            Some(target_hash)
        } else {
            None
        }
    }

    pub fn hydrate_clipboard(&self, text: &str) {
        if let Ok(mut guard) = self.clipboard_cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
    }

    pub fn hovered_cursor_css(&self) -> &'static str {
        craft_cursor_to_css(self.tree.hovered_cursor())
    }

    /// Extract visible semantic accessibility nodes from [`WidgetTree`] for DOM ARIA mirroring.
    pub fn extract_aria_tree(&self) -> Vec<WebAriaNode> {
        let mut out = Vec::new();
        let Some(root) = self.tree.root else {
            return out;
        };
        let mut seen = HashSet::new();
        self.collect_aria_recursive(root, &mut seen, &mut out);
        out
    }

    fn collect_aria_recursive(
        &self,
        id: NodeId,
        seen: &mut HashSet<NodeId>,
        out: &mut Vec<WebAriaNode>,
    ) {
        if !seen.insert(id) {
            return;
        }
        let Some(node) = self.tree.nodes.get(id) else {
            return;
        };
        if node.layout.hidden {
            return;
        }
        let keep = node.a11y.role != A11yRole::Container
            || node.a11y.label.is_some()
            || node.a11y.focusable
            || self.tree.focused == Some(id);
        if keep {
            out.push(WebAriaNode {
                id: id.data().as_ffi(),
                role: craft_role_to_aria(node.a11y.role),
                raw_role: node.a11y.role,
                label: node.a11y.label.clone(),
                value: node.a11y.value.clone(),
                numeric_value: node.a11y.numeric_value,
                min_value: node.a11y.min_value,
                max_value: node.a11y.max_value,
                checked: node.a11y.checked,
                disabled: node.a11y.disabled,
                focusable: node.a11y.focusable,
                focused: self.tree.focused == Some(id),
                rect: self.tree.rect(id),
            });
        }
        for &child in &node.children {
            self.collect_aria_recursive(child, seen, out);
        }
    }

    pub fn resize_css(&mut self, css_width: f32, css_height: f32, dpr: f32) -> (u32, u32) {
        let w = css_width.max(1.0);
        let h = css_height.max(1.0);
        let s = dpr.max(1.0);
        if (self.tree.metrics.logical_size.width - w).abs() > 0.01
            || (self.tree.metrics.logical_size.height - h).abs() > 0.01
            || (self.tree.metrics.scale_factor - s).abs() > 0.001
        {
            self.tree.handle_event(&UiEvent::Resize {
                width: w,
                height: h,
                scale_factor: s,
            });
            self.needs_redraw = true;
        }
        self.tree.metrics.physical_size()
    }

    pub fn hidden_ime_state(&self) -> HiddenImeInputState {
        let Some(focused) = self.tree.focused else {
            return HiddenImeInputState::default();
        };
        let Some(node) = self.tree.nodes.get(focused) else {
            return HiddenImeInputState::default();
        };
        match &node.kind {
            WidgetKind::TextInput { editor, .. } => {
                let r = self.tree.rect(focused);
                HiddenImeInputState {
                    visible: true,
                    x_dp: r.x,
                    y_dp: r.y,
                    width_dp: r.width,
                    height_dp: r.height,
                    value: editor.text.clone(),
                }
            }
            _ => HiddenImeInputState::default(),
        }
    }

    /// Advance one UI frame at `now_ms`, painting only when dirty, animating, or blink toggled.
    pub fn step_frame_with_redraw(&mut self, dt_sec: f32, now_ms: u64) -> (bool, bool) {
        let timers = craftui_reactive::tick_timers_at(now_ms);
        let jobs = self.dispatcher.drain();
        let blink = self.tree.tick_blink_timers(now_ms);
        let anim_tick = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let anim = anim_tick || self.tree.has_active_animations();
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || anim_tick || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }
        (anim, should_paint)
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> bool {
        let (anim, _) = self.step_frame_with_redraw(dt_sec, craftui_core::monotonic_ms());
        anim
    }

    /// Render the current scene to RGBA pixels for 2D `<canvas>` `ImageData` fallback or headless testing.
    pub fn render_rgba_2d(&mut self) -> (u32, u32, Vec<u8>) {
        let (pw, ph) = self.tree.metrics.physical_size();
        let bg = self.tree.theme().background;
        let sf = self.tree.metrics.scale_factor;
        let rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &self.scene,
            pw,
            ph,
            sf,
            bg,
            &self.textures,
            &mut self.tree.text_engine,
        );
        (pw, ph, rgba)
    }
}

/// Convert browser `KeyboardEvent.key` string into a CraftUI [`KeyEvent`].
pub fn dom_key_to_craft(key: &str, pressed: bool, repeat: bool, modifiers: Modifiers) -> KeyEvent {
    let cmd = modifiers.command();
    let code = match key {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Escape" => KeyCode::Escape,
        " " => KeyCode::Space,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "i" | "I" if cmd && modifiers.shift => KeyCode::KeyI,
        "a" | "A" if cmd => KeyCode::KeyA,
        "c" | "C" if cmd => KeyCode::KeyC,
        "v" | "V" if cmd => KeyCode::KeyV,
        "x" | "X" if cmd => KeyCode::KeyX,
        "y" | "Y" if modifiers.ctrl => KeyCode::KeyY,
        "z" | "Z" if cmd => KeyCode::KeyZ,
        s if s.chars().count() == 1 => KeyCode::Character(s.to_string()),
        _ => KeyCode::Unknown,
    };
    let text = if key.chars().count() == 1 {
        Some(key.to_string())
    } else {
        None
    };
    KeyEvent {
        code,
        text,
        pressed,
        repeat,
        modifiers,
    }
}

/// Convert mobile/virtual-keyboard DOM `InputEvent` updates on `<input>` into CraftUI key/IME events.
pub fn dom_input_to_events(
    prev: &str,
    current: &str,
    input_type: &str,
    data: Option<&str>,
) -> Vec<UiEvent> {
    let mut events = Vec::new();
    match input_type {
        "deleteContentBackward" | "deleteWordBackward" | "deleteSoftLineBackward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Backspace,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "deleteContentForward" | "deleteWordForward" | "deleteSoftLineForward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Delete,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "insertLineBreak" | "insertParagraph" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                text: Some("\n".to_string()),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        _ => {}
    }

    if let Some(text) = data
        && !text.is_empty()
        && matches!(
            input_type,
            "insertText" | "insertFromPaste" | "insertCompositionText" | ""
        )
    {
        events.push(UiEvent::Ime(ImeEvent::Commit(text.to_string())));
        return events;
    }

    if prev == current {
        return events;
    }

    // Fallback diff between `prev` and `current` for Android Chrome Gboard batch mutations
    let prev_chars: Vec<char> = prev.chars().collect();
    let curr_chars: Vec<char> = current.chars().collect();
    let common_prefix = prev_chars
        .iter()
        .zip(curr_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = prev_chars.len().saturating_sub(common_prefix);
    for _ in 0..removed {
        events.push(UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
    }
    if curr_chars.len() > common_prefix {
        let inserted: String = curr_chars[common_prefix..].iter().collect();
        events.push(UiEvent::Ime(ImeEvent::Commit(inserted)));
    }
    events
}

pub fn dom_button_to_craft(button: i16) -> Option<PointerButton> {
    match button {
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Secondary),
        3 => Some(PointerButton::Back),
        4 => Some(PointerButton::Forward),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn dom_pointer_event(
    pointer_id: i32,
    phase: PointerPhase,
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    button: i16,
    is_touch: bool,
    modifiers: Modifiers,
    timestamp_ms: u64,
) -> UiEvent {
    UiEvent::Pointer(PointerEvent {
        id: PointerId(pointer_id as u64),
        phase,
        position: Point::new(client_x - rect_left, client_y - rect_top),
        button: dom_button_to_craft(button),
        modifiers,
        is_touch,
        timestamp_ms,
    })
}

pub fn dom_wheel_event(
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    delta_x: f32,
    delta_y: f32,
) -> UiEvent {
    UiEvent::Scroll(ScrollDelta {
        x: -delta_x,
        y: -delta_y,
        position: Point::new(client_x - rect_left, client_y - rect_top),
    })
}

pub fn dom_composition_to_ime(phase_end: bool, data: String) -> UiEvent {
    if phase_end {
        UiEvent::Ime(ImeEvent::Commit(data))
    } else {
        let len = data.len();
        UiEvent::Ime(ImeEvent::Preedit(data, Some((len, len))))
    }
}

#[cfg(target_arch = "wasm32")]
pub mod wasm_runner {
    use super::*;
    use craftui_core::{NodeId, set_monotonic_ms_provider};
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    use craftui_render::GpuSurfacePipeline;
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::rc::{Rc, Weak};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{
        CanvasRenderingContext2d, ClipboardEvent as DomClipboardEvent, CompositionEvent, Element,
        EventTarget, HashChangeEvent, HtmlCanvasElement, HtmlElement, HtmlInputElement, ImageData,
        InputEvent as DomInputEvent, KeyboardEvent as DomKeyboardEvent,
        PointerEvent as DomPointerEvent, WheelEvent as DomWheelEvent,
    };

    thread_local! {
        static ACTIVE_SCHEDULERS: RefCell<Vec<Weak<WebScheduler>>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn request_all_frames() {
        ACTIVE_SCHEDULERS.with(|list| {
            for sched in list.borrow().iter().filter_map(|w| w.upgrade()) {
                sched.request_frame();
            }
        });
    }

    fn browser_now_ms() -> u64 {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now().max(0.0) as u64)
            .unwrap_or(0)
    }

    enum WebRenderBackend {
        Pending,
        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
        Gpu(Box<GpuSurfacePipeline>),
        Canvas2d(CanvasRenderingContext2d),
    }

    struct DomListener {
        target: EventTarget,
        event: &'static str,
        js_fn: js_sys::Function,
        _keepalive: Box<dyn Any>,
    }

    impl DomListener {
        fn attach<T: wasm_bindgen::convert::FromWasmAbi + 'static>(
            target: &EventTarget,
            event: &'static str,
            closure: Closure<dyn FnMut(T)>,
        ) -> Result<Self, wasm_bindgen::JsValue> {
            let js_fn: js_sys::Function =
                closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
            target.add_event_listener_with_callback(event, &js_fn)?;
            Ok(Self {
                target: target.clone(),
                event,
                js_fn,
                _keepalive: Box::new(closure),
            })
        }
    }

    impl Drop for DomListener {
        fn drop(&mut self) {
            let _ = self
                .target
                .remove_event_listener_with_callback(self.event, &self.js_fn);
        }
    }

    type RafClosure = Closure<dyn FnMut(f64)>;

    struct WebScheduler {
        window: web_sys::Window,
        raf_scheduled: Cell<bool>,
        disposed: Cell<bool>,
        raf_id: Cell<Option<i32>>,
        timeout_id: Cell<Option<i32>>,
        raf_cb: RefCell<Option<RafClosure>>,
        timer_cb: RefCell<Option<Closure<dyn FnMut()>>>,
    }

    impl WebScheduler {
        fn new(window: web_sys::Window) -> Rc<Self> {
            Rc::new(Self {
                window,
                raf_scheduled: Cell::new(false),
                disposed: Cell::new(false),
                raf_id: Cell::new(None),
                timeout_id: Cell::new(None),
                raf_cb: RefCell::new(None),
                timer_cb: RefCell::new(None),
            })
        }

        fn request_frame(&self) {
            if self.disposed.get() {
                return;
            }
            if let Some(tid) = self.timeout_id.take() {
                self.window.clear_timeout_with_handle(tid);
            }
            if self.raf_scheduled.get() {
                return;
            }
            if let Some(ref closure) = *self.raf_cb.borrow() {
                self.raf_scheduled.set(true);
                if let Ok(id) = self
                    .window
                    .request_animation_frame(closure.as_ref().unchecked_ref())
                {
                    self.raf_id.set(Some(id));
                }
            }
        }

        fn schedule_wake_after(&self, delay_ms: u64) {
            if self.disposed.get() || self.raf_scheduled.get() {
                return;
            }
            if let Some(tid) = self.timeout_id.take() {
                self.window.clear_timeout_with_handle(tid);
            }
            if let Some(ref tcb) = *self.timer_cb.borrow() {
                let ms = delay_ms.clamp(1, i32::MAX as u64) as i32;
                if let Ok(id) = self
                    .window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        tcb.as_ref().unchecked_ref(),
                        ms,
                    )
                {
                    self.timeout_id.set(Some(id));
                }
            }
        }

        fn dispose(&self) {
            self.disposed.set(true);
            self.raf_scheduled.set(false);
            if let Some(rid) = self.raf_id.take() {
                let _ = self.window.cancel_animation_frame(rid);
            }
            if let Some(tid) = self.timeout_id.take() {
                self.window.clear_timeout_with_handle(tid);
            }
            *self.raf_cb.borrow_mut() = None;
            *self.timer_cb.borrow_mut() = None;
        }
    }

    /// RAII lifecycle handle returned by [`mount_web_app`]; unmounts all DOM listeners, hidden IME
    /// input, ARIA mirror container, animation frames, and timers automatically on [`Drop`].
    pub struct WebRunnerHandle {
        scheduler: Rc<WebScheduler>,
        listeners: Vec<DomListener>,
        pointer_listeners: Rc<RefCell<Vec<DomListener>>>,
        hidden_input: HtmlInputElement,
        a11y_root: Element,
    }

    impl WebRunnerHandle {
        /// Explicitly retain the CraftUI web instance for the entire browser page lifetime.
        pub fn leak(self) {
            std::mem::forget(self);
        }
    }

    impl Drop for WebRunnerHandle {
        fn drop(&mut self) {
            self.scheduler.dispose();
            self.pointer_listeners.borrow_mut().clear();
            self.listeners.clear();
            self.hidden_input.remove();
            self.a11y_root.remove();
            ACTIVE_SCHEDULERS.with(|list| {
                list.borrow_mut().retain(|w| w.strong_count() > 0);
            });
        }
    }

    fn sync_canvas_cursor(
        canvas: &HtmlCanvasElement,
        host: &WebAppHost,
        last_cursor: &Cell<&'static str>,
    ) {
        let next = host.hovered_cursor_css();
        if last_cursor.get() != next {
            last_cursor.set(next);
            let _ = canvas.style().set_property("cursor", next);
        }
    }

    fn sync_dom_a11y_tree(
        document: &web_sys::Document,
        a11y_root: &Element,
        nodes: &[WebAriaNode],
        last_snapshot: &RefCell<Vec<WebAriaNode>>,
        canvas_rect_left: f32,
        canvas_rect_top: f32,
    ) {
        if *last_snapshot.borrow() == nodes {
            return;
        }
        *last_snapshot.borrow_mut() = nodes.to_vec();
        let root_node: &web_sys::Node = a11y_root.as_ref();
        root_node.set_text_content(None);

        for item in nodes {
            let tag = match item.raw_role {
                A11yRole::Button | A11yRole::Tab => "button",
                A11yRole::TextInput | A11yRole::MultilineTextInput => "input",
                _ => "div",
            };
            let Ok(el) = document.create_element(tag) else {
                continue;
            };
            let _ = el.set_attribute("data-craftui-node", &item.id.to_string());
            let _ = el.set_attribute("role", item.role);
            if let Some(ref label) = item.label {
                let _ = el.set_attribute("aria-label", label);
            }
            if let Some(ref val) = item.value {
                let _ = el.set_attribute("aria-valuetext", val);
            }
            if let Some(num) = item.numeric_value {
                let _ = el.set_attribute("aria-valuenow", &format!("{num:.2}"));
            }
            if let Some(min) = item.min_value {
                let _ = el.set_attribute("aria-valuemin", &format!("{min:.2}"));
            }
            if let Some(max) = item.max_value {
                let _ = el.set_attribute("aria-valuemax", &format!("{max:.2}"));
            }
            if let Some(checked) = item.checked {
                let _ = el.set_attribute("aria-checked", if checked { "true" } else { "false" });
            }
            if item.disabled {
                let _ = el.set_attribute("aria-disabled", "true");
            }
            if item.raw_role == A11yRole::Alert {
                let _ = el.set_attribute("aria-live", "polite");
            }
            if item.focusable || item.focused {
                let _ = el.set_attribute("tabindex", "0");
            }
            if let Some(hel) = el.dyn_ref::<HtmlElement>() {
                let st = hel.style();
                let _ = st.set_property("position", "fixed");
                let _ = st.set_property("left", &format!("{}px", canvas_rect_left + item.rect.x));
                let _ = st.set_property("top", &format!("{}px", canvas_rect_top + item.rect.y));
                let _ = st.set_property("width", &format!("{}px", item.rect.width.max(1.0)));
                let _ = st.set_property("height", &format!("{}px", item.rect.height.max(1.0)));
                let _ = st.set_property("pointer-events", "none");
            }
            let _ = root_node.append_child(el.as_ref());
        }
    }

    fn obtain_2d_context(
        canvas_slot: &Rc<RefCell<HtmlCanvasElement>>,
        host: &Rc<RefCell<WebAppHost>>,
        scheduler: &Rc<WebScheduler>,
        pointer_listeners: &Rc<RefCell<Vec<DomListener>>>,
        last_cursor: &Rc<Cell<&'static str>>,
    ) -> Option<CanvasRenderingContext2d> {
        let existing = canvas_slot.borrow().clone();
        if let Ok(Some(ctx)) = existing.get_context("2d")
            && let Ok(c2d) = ctx.dyn_into::<CanvasRenderingContext2d>()
        {
            return Some(c2d);
        }
        // If failed WebGPU/WebGL2 attempt locked the `<canvas>` context type, drop old listeners and replace cleanly.
        if let Ok(cloned_node) = existing.clone_node_with_deep(false)
            && let Ok(fresh_canvas) = cloned_node.dyn_into::<HtmlCanvasElement>()
        {
            fresh_canvas.set_width(existing.width());
            fresh_canvas.set_height(existing.height());
            pointer_listeners.borrow_mut().clear();
            let _ = existing.replace_with_with_node_1(&fresh_canvas);
            if let Ok(new_listeners) = attach_pointer_listeners(
                &fresh_canvas,
                host.clone(),
                scheduler.clone(),
                last_cursor.clone(),
            ) {
                *pointer_listeners.borrow_mut() = new_listeners;
            }
            *canvas_slot.borrow_mut() = fresh_canvas.clone();
            if let Ok(Some(ctx)) = fresh_canvas.get_context("2d") {
                return ctx.dyn_into::<CanvasRenderingContext2d>().ok();
            }
        }
        None
    }

    /// Mount and start a reactive CraftUI application inside an HTML5 `<canvas>` element, returning an
    /// RAII [`WebRunnerHandle`] that tears down DOM listeners, ARIA mirror, and RAF loops when dropped.
    pub fn mount_web_app<F>(
        config: WebBridgeConfig,
        build_ui: F,
    ) -> Result<WebRunnerHandle, wasm_bindgen::JsValue>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId + 'static,
    {
        let window =
            web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
        let document = window
            .document()
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;
        let el = document
            .get_element_by_id(&config.canvas_id)
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("canvas element not found"))?;
        let canvas: HtmlCanvasElement = el.dyn_into()?;

        let dpr = window.device_pixel_ratio() as f32;
        let rect = canvas.get_bounding_client_rect();
        let css_w = (rect.width() as f32).max(320.0);
        let css_h = (rect.height() as f32).max(240.0);

        set_monotonic_ms_provider(browser_now_ms);
        let scheduler = WebScheduler::new(window.clone());
        let sched_weak = Rc::downgrade(&scheduler);
        ACTIVE_SCHEDULERS.with(|list| {
            let mut list = list.borrow_mut();
            list.retain(|w| w.strong_count() > 0);
            list.push(sched_weak);
        });
        let dispatcher = UiDispatcher::with_waker(|| {
            ACTIVE_SCHEDULERS.with(|list| {
                for sched in list.borrow().iter().filter_map(|w| w.upgrade()) {
                    sched.request_frame();
                }
            });
        });
        let mut host = WebAppHost::new(dispatcher.clone(), css_w, css_h, dpr);
        let (pw, ph) = host.resize_css(css_w, css_h, dpr);
        canvas.set_width(pw);
        canvas.set_height(ph);

        let web_bridge = host.bridge();
        let sched_probe = scheduler.clone();
        web_bridge.hydrate_from_browser(move || {
            sched_probe.request_frame();
        });

        let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
        host.tree.set_root(root);

        if config.sync_url_hash
            && let Ok(hash) = window.location().hash()
        {
            host.apply_url_hash(&hash);
        }

        // Hidden <input> element overlay for mobile browser keyboards, CJK IME & paste interception
        let hidden_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
        hidden_input.set_type("text");
        let st = hidden_input.style();
        let _ = st.set_property("position", "fixed");
        let _ = st.set_property("opacity", "0");
        let _ = st.set_property("pointer-events", "none");
        let _ = st.set_property("width", "1px");
        let _ = st.set_property("height", "1px");

        // DOM ARIA accessibility mirror container (#craftui-a11y-root)
        let a11y_root = document.create_element("div")?;
        a11y_root.set_id("craftui-a11y-root");
        let _ = a11y_root.set_attribute("aria-label", "CraftUI Accessibility Tree");
        if let Some(hel) = a11y_root.dyn_ref::<HtmlElement>() {
            let ast = hel.style();
            let _ = ast.set_property("position", "fixed");
            let _ = ast.set_property("left", "0px");
            let _ = ast.set_property("top", "0px");
            let _ = ast.set_property("width", "1px");
            let _ = ast.set_property("height", "1px");
            let _ = ast.set_property("overflow", "hidden");
            let _ = ast.set_property("opacity", "0.001");
            let _ = ast.set_property("pointer-events", "none");
        }

        if let Some(body) = document.body() {
            let _ = body.append_child(&a11y_root);
            let _ = body.append_child(&hidden_input);
        }

        let host_rc = Rc::new(RefCell::new(host));
        let canvas_slot = Rc::new(RefCell::new(canvas.clone()));
        let last_hidden_val = Rc::new(RefCell::new(String::new()));
        let is_composing = Rc::new(Cell::new(false));
        let last_cursor = Rc::new(Cell::new("default"));
        let last_aria_snapshot = Rc::new(RefCell::new(Vec::<WebAriaNode>::new()));

        let pointer_listeners = Rc::new(RefCell::new(attach_pointer_listeners(
            &canvas,
            host_rc.clone(),
            scheduler.clone(),
            last_cursor.clone(),
        )?));

        let mut listeners = Vec::new();
        listeners.extend(attach_keyboard_ime_and_paste_listeners(
            &window,
            &hidden_input,
            host_rc.clone(),
            last_hidden_val.clone(),
            is_composing.clone(),
            scheduler.clone(),
        )?);

        let win_target: &EventTarget = window.as_ref();
        if config.sync_url_hash {
            let h_hash = host_rc.clone();
            let sched_hash = scheduler.clone();
            let hash_cb = Closure::wrap(Box::new(move |_ev: HashChangeEvent| {
                if let Some(win) = web_sys::window()
                    && let Ok(hash) = win.location().hash()
                    && h_hash.borrow_mut().apply_url_hash(&hash)
                {
                    sched_hash.request_frame();
                }
            }) as Box<dyn FnMut(HashChangeEvent)>);
            listeners.push(DomListener::attach(win_target, "hashchange", hash_cb)?);
        }

        {
            let h_res = host_rc.clone();
            let sched_res = scheduler.clone();
            let resize_cb = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
                h_res.borrow_mut().request_redraw();
                sched_res.request_frame();
            }) as Box<dyn FnMut(web_sys::Event)>);
            listeners.push(DomListener::attach(win_target, "resize", resize_cb)?);
        }

        {
            let b_online = web_bridge.clone();
            let sched_online = scheduler.clone();
            let online_cb = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
                if let Some(win) = web_sys::window() {
                    b_online.refresh_online_from_navigator(&win.navigator());
                }
                sched_online.request_frame();
            }) as Box<dyn FnMut(web_sys::Event)>);
            listeners.push(DomListener::attach(win_target, "online", online_cb)?);

            let b_offline = web_bridge.clone();
            let sched_offline = scheduler.clone();
            let offline_cb = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
                b_offline.set_network_status(NetworkStatus::Offline);
                sched_offline.request_frame();
            }) as Box<dyn FnMut(web_sys::Event)>);
            listeners.push(DomListener::attach(win_target, "offline", offline_cb)?);

            let doc_target: &EventTarget = document.as_ref();
            let b_vis = web_bridge.clone();
            let doc_vis = document.clone();
            let sched_vis = scheduler.clone();
            let vis_cb = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
                let state = js_sys::Reflect::get(
                    &doc_vis,
                    &wasm_bindgen::JsValue::from_str("visibilityState"),
                )
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
                let lifecycle = if state == "hidden" {
                    AppLifecycleEvent::Paused
                } else {
                    AppLifecycleEvent::Resumed
                };
                b_vis.push_lifecycle_event(lifecycle);
                sched_vis.request_frame();
            }) as Box<dyn FnMut(web_sys::Event)>);
            listeners.push(DomListener::attach(doc_target, "visibilitychange", vis_cb)?);
        }

        let backend_slot: Rc<RefCell<WebRenderBackend>> =
            Rc::new(RefCell::new(WebRenderBackend::Pending));
        let force_2d =
            config.force_2d_fallback || !cfg!(any(feature = "gpu-sdf", feature = "gpu-vello"));
        if force_2d {
            if let Some(c2d) = obtain_2d_context(
                &canvas_slot,
                &host_rc,
                &scheduler,
                &pointer_listeners,
                &last_cursor,
            ) {
                *backend_slot.borrow_mut() = WebRenderBackend::Canvas2d(c2d);
            }
        } else {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            {
                let backend_async = backend_slot.clone();
                let canvas_async = canvas_slot.clone();
                let host_async = host_rc.clone();
                let sched_async = scheduler.clone();
                let ptr_listeners_async = pointer_listeners.clone();
                let cursor_async = last_cursor.clone();
                let canvas_for_wgpu = canvas.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match GpuSurfacePipeline::new_async(
                        wgpu::SurfaceTarget::Canvas(canvas_for_wgpu),
                        pw,
                        ph,
                        false,
                    )
                    .await
                    {
                        Ok(gpu) => {
                            tracing::info!(
                                "CraftUI Web renderer initialized ({})",
                                gpu.active_backend_name()
                            );
                            *backend_async.borrow_mut() = WebRenderBackend::Gpu(Box::new(gpu));
                        }
                        Err(err) => {
                            tracing::warn!(
                                "WebGPU/WebGL2 async init failed ({err}); falling back to 2D canvas"
                            );
                            if let Some(c2d) = obtain_2d_context(
                                &canvas_async,
                                &host_async,
                                &sched_async,
                                &ptr_listeners_async,
                                &cursor_async,
                            ) {
                                *backend_async.borrow_mut() = WebRenderBackend::Canvas2d(c2d);
                            }
                        }
                    }
                    host_async.borrow_mut().request_redraw();
                    sched_async.request_frame();
                });
            }
        }

        {
            let sched_timer = scheduler.clone();
            let h_timer = host_rc.clone();
            *scheduler.timer_cb.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                sched_timer.timeout_id.set(None);
                h_timer.borrow_mut().request_redraw();
                sched_timer.request_frame();
            })
                as Box<dyn FnMut()>));
        }

        let sched_raf = scheduler.clone();
        let canvas_raf = canvas_slot;
        let window_raf = window;
        let doc_raf = document;
        let hidden_raf = hidden_input.clone();
        let a11y_raf = a11y_root.clone();
        let last_aria_raf = last_aria_snapshot;
        let last_cursor_raf = last_cursor;
        let sync_hash = config.sync_url_hash;
        let mut last_ts_ms = window_raf.performance().map(|p| p.now()).unwrap_or(0.0);

        *scheduler.raf_cb.borrow_mut() = Some(Closure::wrap(Box::new(move |ts: f64| {
            sched_raf.raf_id.set(None);
            sched_raf.raf_scheduled.set(false);
            if sched_raf.disposed.get() {
                return;
            }
            let dt_ms = (ts - last_ts_ms).max(1.0);
            last_ts_ms = ts;
            let dt_sec = ((dt_ms as f32) / 1000.0).clamp(0.001, 0.05);
            let now_ms = window_raf
                .performance()
                .map(|p| p.now() as u64)
                .unwrap_or(ts.max(0.0) as u64);

            let mut h = host_rc.borrow_mut();
            let dpr_now = window_raf.device_pixel_ratio() as f32;
            let cur_canvas = canvas_raf.borrow().clone();
            let bcr = cur_canvas.get_bounding_client_rect();
            let (pw, ph) = h.resize_css(bcr.width() as f32, bcr.height() as f32, dpr_now);
            if cur_canvas.width() != pw || cur_canvas.height() != ph {
                cur_canvas.set_width(pw);
                cur_canvas.set_height(ph);
                h.request_redraw();
            }

            let (anim, did_paint) = h.step_frame_with_redraw(dt_sec, now_ms);
            sync_canvas_cursor(&cur_canvas, &h, &last_cursor_raf);

            if sync_hash && let Some(new_hash) = h.take_pending_hash_update() {
                let _ = window_raf.location().set_hash(&new_hash);
            }

            let ime = h.hidden_ime_state();
            if ime.visible {
                let ist = hidden_raf.style();
                let _ = ist.set_property("left", &format!("{}px", bcr.left() as f32 + ime.x_dp));
                let _ = ist.set_property("top", &format!("{}px", bcr.top() as f32 + ime.y_dp));
                if !is_composing.get() && *last_hidden_val.borrow() != ime.value {
                    hidden_raf.set_value(&ime.value);
                    *last_hidden_val.borrow_mut() = ime.value.clone();
                }
                let hidden_node: &Element = hidden_raf.as_ref();
                let already_focused = doc_raf
                    .active_element()
                    .is_some_and(|ae| &ae == hidden_node);
                if !already_focused {
                    let _ = hidden_raf.focus();
                }
            }

            if did_paint {
                let aria_nodes = h.extract_aria_tree();
                sync_dom_a11y_tree(
                    &doc_raf,
                    &a11y_raf,
                    &aria_nodes,
                    &last_aria_raf,
                    bcr.left() as f32,
                    bcr.top() as f32,
                );

                match &mut *backend_slot.borrow_mut() {
                    WebRenderBackend::Pending => {}
                    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                    WebRenderBackend::Gpu(gpu) => {
                        gpu.resize(pw, ph);
                        let bg = h.tree.theme().background;
                        let host_ref = &mut *h;
                        let _ = gpu.render_frame(
                            &host_ref.scene,
                            dpr_now,
                            bg,
                            &host_ref.textures,
                            &mut host_ref.tree.text_engine,
                        );
                    }
                    WebRenderBackend::Canvas2d(ctx_2d) => {
                        let (w, h_px, rgba) = h.render_rgba_2d();
                        if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
                            wasm_bindgen::Clamped(&rgba),
                            w,
                            h_px,
                        ) {
                            let _ = ctx_2d.put_image_data(&img, 0.0, 0.0);
                        }
                    }
                }
            }

            if anim {
                sched_raf.request_frame();
            } else {
                let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
                if let Some(d) = h.tree.next_internal_delay_ms(now_ms) {
                    min_delay = Some(min_delay.map_or(d, |cur| cur.min(d)));
                }
                if let Some(delay) = min_delay {
                    sched_raf.schedule_wake_after(delay);
                }
            }
        }) as Box<dyn FnMut(f64)>));

        scheduler.request_frame();
        Ok(WebRunnerHandle {
            scheduler,
            listeners,
            pointer_listeners,
            hidden_input,
            a11y_root,
        })
    }

    /// Mount and leak a reactive CraftUI web application for the full browser page lifetime.
    pub fn start_web_app<F>(
        config: WebBridgeConfig,
        build_ui: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId + 'static,
    {
        mount_web_app(config, build_ui)?.leak();
        Ok(())
    }

    fn attach_pointer_listeners(
        canvas: &HtmlCanvasElement,
        host: Rc<RefCell<WebAppHost>>,
        scheduler: Rc<WebScheduler>,
        last_cursor: Rc<Cell<&'static str>>,
    ) -> Result<Vec<DomListener>, wasm_bindgen::JsValue> {
        let mut out = Vec::with_capacity(5);
        let canvas_target: &EventTarget = canvas.as_ref();
        for (ev_name, phase) in [
            ("pointerdown", PointerPhase::Down),
            ("pointermove", PointerPhase::Move),
            ("pointerup", PointerPhase::Up),
            ("pointercancel", PointerPhase::Cancel),
        ] {
            let h_c = host.clone();
            let c_el = canvas.clone();
            let sched_c = scheduler.clone();
            let cursor_c = last_cursor.clone();
            let cb = Closure::wrap(Box::new(move |e: DomPointerEvent| {
                let bcr = c_el.get_bounding_client_rect();
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                };
                let raw_button = if phase == PointerPhase::Move {
                    let b = e.buttons();
                    if b == 0 {
                        -1
                    } else if (b & 1) != 0 {
                        0
                    } else if (b & 2) != 0 {
                        2
                    } else if (b & 4) != 0 {
                        1
                    } else {
                        0
                    }
                } else {
                    e.button()
                };
                let ev = dom_pointer_event(
                    e.pointer_id(),
                    phase,
                    e.client_x() as f32,
                    e.client_y() as f32,
                    bcr.left() as f32,
                    bcr.top() as f32,
                    raw_button,
                    e.pointer_type() == "touch",
                    mods,
                    e.time_stamp() as u64,
                );
                let mut guard = h_c.borrow_mut();
                let changed = guard.handle_ui_event(&ev);
                sync_canvas_cursor(&c_el, &guard, &cursor_c);
                if changed {
                    sched_c.request_frame();
                }
            }) as Box<dyn FnMut(DomPointerEvent)>);
            out.push(DomListener::attach(canvas_target, ev_name, cb)?);
        }

        let h_w = host;
        let c_w = canvas.clone();
        let sched_w = scheduler;
        let wheel_cb = Closure::wrap(Box::new(move |e: DomWheelEvent| {
            let bcr = c_w.get_bounding_client_rect();
            let ev = dom_wheel_event(
                e.client_x() as f32,
                e.client_y() as f32,
                bcr.left() as f32,
                bcr.top() as f32,
                e.delta_x() as f32,
                e.delta_y() as f32,
            );
            if h_w.borrow_mut().handle_ui_event(&ev) {
                sched_w.request_frame();
            }
        }) as Box<dyn FnMut(DomWheelEvent)>);
        out.push(DomListener::attach(canvas_target, "wheel", wheel_cb)?);
        Ok(out)
    }

    fn attach_keyboard_ime_and_paste_listeners(
        window: &web_sys::Window,
        hidden_input: &HtmlInputElement,
        host: Rc<RefCell<WebAppHost>>,
        last_hidden_val: Rc<RefCell<String>>,
        is_composing: Rc<Cell<bool>>,
        scheduler: Rc<WebScheduler>,
    ) -> Result<Vec<DomListener>, wasm_bindgen::JsValue> {
        let mut out = Vec::with_capacity(7);
        let win_target: &EventTarget = window.as_ref();
        let hidden_target: &EventTarget = hidden_input.as_ref();

        for (ev_name, pressed) in [("keydown", true), ("keyup", false)] {
            let h_c = host.clone();
            let sched_k = scheduler.clone();
            let comp_k = is_composing.clone();
            let cb = Closure::wrap(Box::new(move |e: DomKeyboardEvent| {
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                };
                // Defer Cmd/Ctrl+V keydown to the trusted DOM `paste` event so live OS clipboard
                // contents from other tabs/apps arrive synchronously without double-pasting.
                if pressed
                    && !comp_k.get()
                    && mods.command()
                    && !mods.alt
                    && matches!(e.key().as_str(), "v" | "V")
                {
                    return;
                }
                let ke = dom_key_to_craft(&e.key(), pressed, e.repeat(), mods);
                if h_c.borrow_mut().handle_ui_event(&UiEvent::Key(ke)) {
                    sched_k.request_frame();
                }
            }) as Box<dyn FnMut(DomKeyboardEvent)>);
            out.push(DomListener::attach(win_target, ev_name, cb)?);
        }

        // Trusted DOM `paste` event hydrates clipboard_cache synchronously and dispatches Ctrl+V
        {
            let h_paste = host.clone();
            let sched_paste = scheduler.clone();
            let paste_cb = Closure::wrap(Box::new(move |e: DomClipboardEvent| {
                if let Some(dt) = e.clipboard_data()
                    && let Ok(text) = dt.get_data("text/plain")
                {
                    e.prevent_default();
                    let mut guard = h_paste.borrow_mut();
                    guard.hydrate_clipboard(&text);
                    let ke = KeyEvent {
                        code: KeyCode::KeyV,
                        text: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers {
                            ctrl: true,
                            ..Default::default()
                        },
                    };
                    if guard.handle_ui_event(&UiEvent::Key(ke)) {
                        sched_paste.request_frame();
                    }
                }
            }) as Box<dyn FnMut(DomClipboardEvent)>);
            out.push(DomListener::attach(win_target, "paste", paste_cb)?);
        }

        let comp_flag_start = is_composing.clone();
        let comp_start = Closure::wrap(Box::new(move |_e: CompositionEvent| {
            comp_flag_start.set(true);
        }) as Box<dyn FnMut(CompositionEvent)>);
        out.push(DomListener::attach(
            hidden_target,
            "compositionstart",
            comp_start,
        )?);

        let h_comp_upd = host.clone();
        let sched_comp_upd = scheduler.clone();
        let comp_upd = Closure::wrap(Box::new(move |e: CompositionEvent| {
            let ev = dom_composition_to_ime(false, e.data().unwrap_or_default());
            if h_comp_upd.borrow_mut().handle_ui_event(&ev) {
                sched_comp_upd.request_frame();
            }
        }) as Box<dyn FnMut(CompositionEvent)>);
        out.push(DomListener::attach(
            hidden_target,
            "compositionupdate",
            comp_upd,
        )?);

        let h_comp_end = host.clone();
        let comp_flag_end = is_composing.clone();
        let hidden_comp = hidden_input.clone();
        let last_val_comp = last_hidden_val.clone();
        let sched_comp_end = scheduler.clone();
        let comp_end = Closure::wrap(Box::new(move |e: CompositionEvent| {
            comp_flag_end.set(false);
            let ev = dom_composition_to_ime(true, e.data().unwrap_or_default());
            let mut guard = h_comp_end.borrow_mut();
            let changed = guard.handle_ui_event(&ev);
            let synced = guard.hidden_ime_state().value;
            hidden_comp.set_value(&synced);
            *last_val_comp.borrow_mut() = synced;
            if changed {
                sched_comp_end.request_frame();
            }
        }) as Box<dyn FnMut(CompositionEvent)>);
        out.push(DomListener::attach(
            hidden_target,
            "compositionend",
            comp_end,
        )?);

        // Mobile virtual keyboard DOM `input` event bridge on `<input>`
        let h_input = host;
        let hidden_inp = hidden_input.clone();
        let comp_flag_inp = is_composing;
        let last_val_inp = last_hidden_val;
        let sched_inp = scheduler;
        let on_input = Closure::wrap(Box::new(move |e: web_sys::Event| {
            if comp_flag_inp.get() {
                return;
            }
            let Ok(ie) = e.dyn_into::<DomInputEvent>() else {
                return;
            };
            if ie.is_composing() {
                return;
            }
            let current = hidden_inp.value();
            let prev = last_val_inp.borrow().clone();
            let input_type = ie.input_type();
            let data = ie.data();
            let events = dom_input_to_events(&prev, &current, &input_type, data.as_deref());
            *last_val_inp.borrow_mut() = current;
            let mut guard = h_input.borrow_mut();
            let mut changed = false;
            for ev in events {
                changed |= guard.handle_ui_event(&ev);
            }
            if changed {
                sched_inp.request_frame();
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        out.push(DomListener::attach(hidden_target, "input", on_input)?);

        Ok(out)
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_runner::{WebRunnerHandle, mount_web_app, start_web_app};

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;

    #[test]
    fn web_host_dpr_ime_overlay_hash_router_dom_input_and_2d_fallback_render() {
        reset_runtime();
        assert_eq!(BrowserRouteSync::route_to_hash("/settings"), "#/settings");
        assert_eq!(BrowserRouteSync::hash_to_route("#/settings"), "/settings");

        let mut host = WebAppHost::new(UiDispatcher::new(), 400.0, 300.0, 2.0);
        let router = Router::new(host.scope, "/".to_string());
        BrowserRouteSync::connect_string_router(host.scope, router);
        assert_eq!(host.take_pending_hash_update().as_deref(), Some("#/"));
        assert!(host.apply_url_hash("#/dashboard"));
        assert_eq!(router.current(), "/dashboard");
        router.push("/profile".to_string());
        assert_eq!(
            host.take_pending_hash_update().as_deref(),
            Some("#/profile")
        );

        let (pw, ph) = host.resize_css(400.0, 300.0, 2.0);
        assert_eq!((pw, ph), (800, 600));

        let sig = host.scope.signal("hello".to_string());
        let inp = host.tree.text_input(host.scope, "", "Search", Some(sig));
        host.tree.set_layout(
            inp,
            LayoutStyle::row().with_size(Length::Dp(240.0), Length::Dp(40.0)),
        );
        host.tree.set_root(inp);
        let _ = host.step_frame(0.016);

        let c = host.tree.rect(inp).center();
        let down = dom_pointer_event(
            1,
            PointerPhase::Down,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            0,
        );
        let up = dom_pointer_event(
            1,
            PointerPhase::Up,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            20,
        );
        host.tree.handle_event(&down);
        host.tree.handle_event(&up);

        let ime = host.hidden_ime_state();
        assert!(ime.visible);
        assert_eq!(ime.value, "hello");

        // CSS cursor tracking reflects hovered TextInput -> "text"
        let move_ev = dom_pointer_event(
            1,
            PointerPhase::Move,
            c.x,
            c.y,
            0.0,
            0.0,
            -1,
            false,
            Modifiers::default(),
            25,
        );
        host.handle_ui_event(&move_ev);
        assert_eq!(host.hovered_cursor_css(), "text");

        // Hydrated clipboard supports synchronous Ctrl+V paste into focused TextInput
        host.hydrate_clipboard(" world");
        host.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyV,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(sig.get(), "hello world");

        // DOM ARIA extraction exposes textbox role, focus state, and value
        let aria = host.extract_aria_tree();
        assert!(aria.iter().any(|n| n.role == "textbox" && n.focused));

        // Mobile virtual keyboard DOM `input` insert & backspace events
        host.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyA,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        host.tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit("hello".into())));
        for ev in dom_input_to_events("hello", "hello!", "insertText", Some("!")) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello!");
        for ev in dom_input_to_events("hello!", "hello", "deleteContentBackward", None) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello");

        let (rw, rh, rgba) = host.render_rgba_2d();
        assert_eq!((rw, rh), (800, 600));
        assert_eq!(rgba.len(), 800 * 600 * 4);

        // Demand-driven Web frame check: idle focused TextInput does not animate continuously,
        // and only triggers redraw at blink interval boundaries.
        host.tree.reset_caret_blink(1000);
        let (anim0, _painted0) = host.step_frame_with_redraw(0.016, 1000);
        assert!(!anim0);
        let (anim1, painted1) = host.step_frame_with_redraw(0.016, 1100);
        assert!(!anim1 && !painted1);
        let (anim2, painted2) = host.step_frame_with_redraw(0.016, 1600);
        assert!(!anim2 && painted2);

        let ky = dom_key_to_craft(
            "y",
            true,
            false,
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(ky.code, KeyCode::KeyY);
    }

    #[test]
    fn web_css_cursors_and_aria_roles_map_cleanly() {
        assert_eq!(craft_cursor_to_css(CursorIcon::Default), "default");
        assert_eq!(craft_cursor_to_css(CursorIcon::Pointer), "pointer");
        assert_eq!(craft_cursor_to_css(CursorIcon::Text), "text");
        assert_eq!(craft_cursor_to_css(CursorIcon::Grab), "grab");
        assert_eq!(craft_cursor_to_css(CursorIcon::Grabbing), "grabbing");
        assert_eq!(
            craft_cursor_to_css(CursorIcon::ResizeHorizontal),
            "ew-resize"
        );
        assert_eq!(craft_cursor_to_css(CursorIcon::ResizeVertical), "ns-resize");
        assert_eq!(craft_cursor_to_css(CursorIcon::NotAllowed), "not-allowed");
        assert_eq!(craft_cursor_to_css(CursorIcon::Crosshair), "crosshair");

        assert_eq!(craft_role_to_aria(A11yRole::Button), "button");
        assert_eq!(craft_role_to_aria(A11yRole::TextInput), "textbox");
        assert_eq!(craft_role_to_aria(A11yRole::CheckBox), "checkbox");
        assert_eq!(craft_role_to_aria(A11yRole::Switch), "switch");
        assert_eq!(craft_role_to_aria(A11yRole::Slider), "slider");
        assert_eq!(craft_role_to_aria(A11yRole::Alert), "alert");
    }

    #[test]
    fn web_host_supports_permissions_haptics_share_secure_storage_and_telemetry() {
        reset_runtime();
        let mut host = WebAppHost::new(UiDispatcher::new(), 390.0, 844.0, 3.0);
        assert_eq!(
            web_permission_descriptor(&Permission::Camera),
            Some("camera")
        );
        assert_eq!(
            web_permission_descriptor(&Permission::Notifications),
            Some("notifications")
        );
        assert_eq!(web_permission_descriptor(&Permission::SmsSend), None);
        assert_eq!(
            parse_web_permission_state("granted"),
            PermissionStatus::Granted
        );
        assert_eq!(
            parse_web_permission_state("denied"),
            PermissionStatus::Denied
        );
        assert_eq!(
            parse_web_permission_state("default"),
            PermissionStatus::NotDetermined
        );

        let services = host.tree.services.clone();
        assert_eq!(
            services.check_permission(&Permission::Camera),
            PermissionStatus::NotDetermined
        );
        assert_eq!(
            services.check_permission(&Permission::SmsSend),
            PermissionStatus::Unsupported
        );
        host.hydrate_permission_status(Permission::Camera, PermissionStatus::Granted);
        assert_eq!(
            services.check_permission(&Permission::Camera),
            PermissionStatus::Granted
        );
        assert!(!services.open_app_settings());

        // Host fallback share writes formatted string into WebClipboard cache
        assert!(!services.share(&SharePayload::new()));
        assert!(
            services.share(
                &SharePayload::url("https://craftui.dev")
                    .with_title("CraftUI")
                    .with_text("Check this out")
            )
        );
        let clip_text = services.read_clipboard().unwrap_or_default();
        assert!(clip_text.contains("https://craftui.dev"));

        // Secure storage + device telemetry + lifecycle tracking
        assert!(services.set_secret("session_jwt", "jwt_abc"));
        assert_eq!(
            services.get_secret("session_jwt").as_deref(),
            Some("jwt_abc")
        );
        assert!(services.delete_secret("session_jwt"));
        assert_eq!(services.get_secret("session_jwt"), None);

        host.bridge().set_network_status(NetworkStatus::Wifi);
        host.bridge().set_battery_state(BatteryState {
            level_pct: Some(74),
            charging: true,
        });
        assert_eq!(services.device_info().os, "Web");
        assert_eq!(services.network_status(), NetworkStatus::Wifi);
        assert_eq!(services.battery_state().level_pct, Some(74));

        host.push_lifecycle_event(AppLifecycleEvent::Paused);
        host.push_lifecycle_event(AppLifecycleEvent::Resumed);
        assert_eq!(
            host.bridge().lifecycle_history(),
            vec![AppLifecycleEvent::Paused, AppLifecycleEvent::Resumed]
        );
        assert_eq!(WebHapticsBackend::duration_ms(HapticStyle::Medium), 25.0);
    }
}
