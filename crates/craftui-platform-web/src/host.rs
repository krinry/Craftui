use craftui_a11y::A11yRole;
use craftui_core::{
    AppLifecycleEvent, NodeId, Permission, PermissionStatus,
    PlatformServices, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba};
use craftui_widgets::{Router, WidgetKind, WidgetTree};
use slotmap::Key;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::dom::*;
use crate::route::*;
use crate::services::*;

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

