use craftui_reactive::Scope;
use craftui_widgets::Router;
use std::rc::Rc;

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

