use craftui_core::{
    AppLifecycleEvent, BatteryState, DeviceInfo, DirtyFlags, ImeEvent, Insets, KeyCode, KeyEvent,
    Modifiers, NetworkStatus, Permission, PermissionStatus, PlatformServices, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, Size, TextureId, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{Router, WidgetKind, WidgetTree};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::input::*;
use crate::remote_preview;
use crate::services::*;

/// Platform-neutral Android lifecycle, safe-area/IME inset, back navigation, and event translator host.
///
/// Used both on-device inside [`android_runtime::run_android_app`] and in deterministic desktop/CI integration tests.
pub struct AndroidHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    pub bridge: Arc<AndroidBridgeState>,
    back_handler: Option<Box<dyn FnMut() -> bool>>,
    last_ime_visible: bool,
    last_synced_ime_state: Option<SyncedImeSnapshot>,
    pub needs_redraw: bool,
    last_error: Option<String>,
    pub remote_preview: Option<remote_preview::RemotePreviewClient>,
    #[cfg(feature = "hot-reload")]
    pub hot_reload_session: Option<craftui_runtime::HotReloadSession>,
}

impl AndroidHost {
    pub fn new(dispatcher: UiDispatcher, width_px: u32, height_px: u32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let bridge = Arc::new(AndroidBridgeState::new());
        crate::services::set_active_android_bridge(bridge.clone());
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_permissions(AndroidPermissionBackend::new(bridge.clone()))
                .with_haptics(AndroidHapticsBackend::new(bridge.clone()))
                .with_share(AndroidShareBackend::new(bridge.clone()))
                .with_secure_storage(AndroidSecureStorage::new(bridge.clone()))
                .with_device_capabilities(AndroidDeviceCapabilities::new(bridge.clone()))
                .with_file_dialog(AndroidFileDialogBridge::new(bridge.clone())),
        );
        let sf = scale_factor.max(1.0);
        tree.metrics = WindowMetrics {
            logical_size: Size::new(width_px as f32 / sf, height_px as f32 / sf),
            scale_factor: sf,
            safe_area: Insets::new(44.0, 0.0, 40.0, 0.0),
            keyboard_inset: 0.0,
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            bridge,
            back_handler: None,
            last_ime_visible: false,
            last_synced_ime_state: None,
            needs_redraw: true,
            last_error: None,
            remote_preview: None,
            #[cfg(feature = "hot-reload")]
            hot_reload_session: None,
        }
    }

    pub fn bridge(&self) -> Arc<AndroidBridgeState> {
        self.bridge.clone()
    }

    /// Register a `MethodChannel` plugin handler on this Android host's [`PluginRegistry`].
    pub fn register_plugin<P: craftui_core::CraftPlugin + 'static>(&self, plugin: P) {
        self.tree.services.plugins().register(plugin);
    }

    /// Invoke a registered `MethodChannel` plugin method from Rust or an Android bridge handler.
    pub fn invoke_plugin(
        &self,
        channel: &str,
        method: &str,
        args: serde_json::Value,
    ) -> craftui_core::Result<serde_json::Value> {
        self.tree.services.plugins().invoke(channel, method, args)
    }

    /// Queue a native Kotlin `MethodChannel` invocation via [`AndroidBridgeState`] and await its result.
    pub fn invoke_native_channel(
        &self,
        channel: &str,
        method: &str,
        args: serde_json::Value,
    ) -> craftui_core::Result<serde_json::Value> {
        self.bridge.invoke_method_channel(channel, method, args)
    }

    /// Deliver an asynchronous Kotlin `MethodChannel` JSON response for `request_id`.
    pub fn push_method_channel_result(&mut self, request_id: u64, ok: bool, payload_json: &str) {
        self.bridge
            .push_method_channel_result(request_id, ok, payload_json);
        self.needs_redraw = true;
    }

    /// Enable background TCP remote live preview client connecting to host via ADB reverse tunnel.
    pub fn enable_remote_preview(&mut self, port: u16) {
        self.remote_preview = Some(remote_preview::RemotePreviewClient::start(
            "127.0.0.1",
            port,
            self.dispatcher.clone(),
        ));
    }

    /// Apply an incoming in-memory `.cui` source payload directly into the live widget tree.
    pub fn apply_hot_reload_source(
        &mut self,
        source: &str,
        component: Option<&str>,
    ) -> craftui_core::Result<craftui_core::NodeId> {
        #[cfg(feature = "hot-reload")]
        {
            let session = self
                .hot_reload_session
                .get_or_insert_with(|| craftui_runtime::HotReloadSession::new(self.scope));
            let root = session.reload(source, component, &mut self.tree)?;
            self.tree.set_root(root);
            self.needs_redraw = true;
            Ok(root)
        }
        #[cfg(not(feature = "hot-reload"))]
        {
            let _ = (source, component);
            Err(craftui_core::CraftError::Runtime(
                "craftui-platform-android compiled without hot-reload feature".into(),
            ))
        }
    }

    pub fn set_permission_wait_timeout(&self, timeout: Duration) {
        self.bridge.set_request_timeout(timeout);
    }

    pub fn drain_platform_commands(&self) -> Vec<AndroidPlatformCommand> {
        self.bridge.drain_commands()
    }

    pub fn sync_permission_statuses(&mut self, entries: Vec<(Permission, PermissionStatus, bool)>) {
        self.bridge.sync_permission_statuses(entries);
        self.needs_redraw = true;
    }

    pub fn push_permission_results(&mut self, request_id: u64, results: &[(String, i32, bool)]) {
        self.bridge.push_permission_results(request_id, results);
        self.needs_redraw = true;
    }

    pub fn push_file_dialog_result(&mut self, request_id: u64, paths: Vec<PathBuf>) {
        self.bridge.push_file_dialog_result(request_id, paths);
        self.needs_redraw = true;
    }

    pub fn push_lifecycle_event(&mut self, ev: AppLifecycleEvent) {
        self.bridge.push_lifecycle_event(ev);
    }

    pub fn set_device_info(&mut self, info: DeviceInfo) {
        if let Ok(mut guard) = self.bridge.device_info.lock() {
            *guard = info;
        }
    }

    pub fn set_network_status(&mut self, status: NetworkStatus) {
        if let Ok(mut guard) = self.bridge.network_status.lock() {
            *guard = status;
        }
    }

    pub fn set_battery_state(&mut self, state: BatteryState) {
        if let Ok(mut guard) = self.bridge.battery_state.lock() {
            *guard = state;
        }
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn record_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    /// Suggested poll timeout for demand-driven Android event loops.
    pub fn next_poll_timeout(&self, animating: bool, has_gpu: bool) -> Option<std::time::Duration> {
        if animating || self.needs_redraw {
            return Some(if has_gpu {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_millis(16)
            });
        }
        let now_ms = craftui_core::monotonic_ms();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        if let Some(caret_delay) = self.tree.next_internal_delay_ms(now_ms) {
            min_delay = Some(min_delay.map_or(caret_delay, |cur| cur.min(caret_delay)));
        }
        min_delay.map(|ms| std::time::Duration::from_millis(ms.max(1)))
    }

    /// Register a hardware Back button handler (e.g. `move || router.pop()`).
    pub fn set_back_handler(&mut self, handler: impl FnMut() -> bool + 'static) {
        self.back_handler = Some(Box::new(handler));
    }

    /// Convenience helper binding a [`Router<R>`] to Android's hardware/gesture Back key.
    pub fn bind_back_router<R: Clone + PartialEq + 'static>(&mut self, router: Router<R>) {
        self.set_back_handler(move || router.pop());
    }

    pub fn update_safe_area_and_ime(
        &mut self,
        window_width_px: i32,
        window_height_px: i32,
        content_left_px: i32,
        content_top_px: i32,
        content_right_px: i32,
        content_bottom_px: i32,
    ) {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let top = if content_top_px > 0 {
            (content_top_px as f32) / sf
        } else {
            44.0
        };
        let left = (content_left_px.max(0) as f32) / sf;
        let right = ((window_width_px - content_right_px).max(0) as f32) / sf;
        let bottom_px = (window_height_px - content_bottom_px).max(0) as f32;
        let bottom = bottom_px / sf;
        let eff_bottom = if bottom > 0.0 { bottom } else { 40.0 };
        self.tree.metrics.safe_area = Insets::new(top, right, eff_bottom, left);
        self.tree.metrics.keyboard_inset = bottom;
        self.needs_redraw = true;
    }

    pub fn push_touch(
        &mut self,
        pointer_id: i32,
        phase: PointerPhase,
        raw_x_px: f32,
        raw_y_px: f32,
        timestamp_ms: u64,
    ) -> bool {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let pos = Point::new(raw_x_px / sf, raw_y_px / sf);
        let handled = self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(pointer_id as u64),
            phase,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Translate and dispatch Android key events, including `Back`, `Enter`, directional D-pad keys, and text input keys.
    pub fn handle_android_key_event(
        &mut self,
        code: AndroidKeyCode,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    ) -> bool {
        if code == AndroidKeyCode::Back {
            if !pressed {
                return false;
            }
            // 1. Dismiss open overlay / modal / popup menu if active.
            let esc = KeyEvent {
                code: KeyCode::Escape,
                text: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            if self.tree.handle_event(&UiEvent::Key(esc)) {
                self.needs_redraw = true;
                return true;
            }
            // 2. Pop custom registered Back handler / Router.
            if let Some(ref mut cb) = self.back_handler
                && cb()
            {
                self.needs_redraw = true;
                return true;
            }
            // 3. Fallback to Router<String> in root context if present.
            if let Some(router) = self.scope.use_context::<Router<String>>()
                && router.pop()
            {
                self.needs_redraw = true;
                return true;
            }
            return false;
        }

        let mut dispatch_mods = modifiers;
        let (craft_code, text) = match code {
            AndroidKeyCode::Enter | AndroidKeyCode::NumpadEnter => (
                KeyCode::Enter,
                (!modifiers.command()).then(|| "\n".to_string()),
            ),
            AndroidKeyCode::Tab => (KeyCode::Tab, None),
            AndroidKeyCode::Del => (KeyCode::Backspace, None),
            AndroidKeyCode::ForwardDel => (KeyCode::Delete, None),
            AndroidKeyCode::Escape => (KeyCode::Escape, None),
            AndroidKeyCode::DpadLeft => (KeyCode::ArrowLeft, None),
            AndroidKeyCode::DpadRight => (KeyCode::ArrowRight, None),
            AndroidKeyCode::DpadUp => (KeyCode::ArrowUp, None),
            AndroidKeyCode::DpadDown => (KeyCode::ArrowDown, None),
            AndroidKeyCode::Home => (KeyCode::Home, None),
            AndroidKeyCode::MoveEnd => (KeyCode::End, None),
            AndroidKeyCode::PageUp => (KeyCode::PageUp, None),
            AndroidKeyCode::PageDown => (KeyCode::PageDown, None),
            AndroidKeyCode::Space => (
                KeyCode::Space,
                (!modifiers.command()).then(|| " ".to_string()),
            ),
            AndroidKeyCode::Cut => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyX, None)
            }
            AndroidKeyCode::Copy => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyC, None)
            }
            AndroidKeyCode::Paste => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyV, None)
            }
            AndroidKeyCode::Character(ch) => {
                let s = ch.to_string();
                let txt = (!modifiers.command() && !ch.is_control()).then(|| s.clone());
                (KeyCode::Character(s), txt)
            }
            AndroidKeyCode::Back | AndroidKeyCode::Other => (KeyCode::Unknown, None),
        };

        let handled = self.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: craft_code,
            text,
            pressed,
            repeat,
            modifiers: dispatch_mods,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_ime_commit(&mut self, text: impl Into<String>) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(text.into())));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_ime_preedit(
        &mut self,
        text: impl Into<String>,
        cursor: Option<(usize, usize)>,
    ) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Preedit(text.into(), cursor)));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Apply a full Android `GameTextInput` buffer update (`TextEvent`) to the focused text input,
    /// separating active `compose_region` preedit from committed text and avoiding JNI echo loops.
    pub fn push_ime_text_state(
        &mut self,
        text: &str,
        selection_start: usize,
        selection_end: usize,
        compose_region: Option<(usize, usize)>,
    ) -> bool {
        let Some(focused) = self.tree.focused else {
            return false;
        };
        let sel_s = clamp_char_boundary(text, selection_start.min(selection_end));
        let sel_e = clamp_char_boundary(text, selection_start.max(selection_end));
        let compose = compose_region.and_then(|(s, e)| {
            let cs = clamp_char_boundary(text, s.min(e));
            let ce = clamp_char_boundary(text, s.max(e));
            (cs < ce).then_some((cs, ce))
        });

        let mut changed_cb: Option<(craftui_widgets::TextCallback, String)> = None;
        {
            let Some(node) = self.tree.nodes.get_mut(focused) else {
                return false;
            };
            let WidgetKind::TextInput {
                editor,
                bound_signal,
                on_change,
                ..
            } = &mut node.kind
            else {
                return false;
            };

            let (committed, next_cursor, next_anchor, next_preedit, next_pe_cur) =
                if let Some((cs, ce)) = compose {
                    let pre = text[cs..ce].to_string();
                    let pe_len = pre.len();
                    let pe_cur = (
                        sel_s.saturating_sub(cs).min(pe_len),
                        sel_e.saturating_sub(cs).min(pe_len),
                    );
                    (
                        format!("{}{}", &text[..cs], &text[ce..]),
                        cs,
                        cs,
                        Some(pre),
                        Some(pe_cur),
                    )
                } else {
                    (text.to_string(), sel_e, sel_s, None, None)
                };

            let text_changed = editor.text != committed;
            editor.text = committed;
            editor.cursor = next_cursor;
            editor.anchor = next_anchor;
            editor.preedit = next_preedit;
            editor.preedit_cursor = next_pe_cur;

            if text_changed {
                node.a11y.value = Some(editor.text.clone());
                if let Some(sig) = bound_signal
                    && sig.get_untracked() != editor.text
                {
                    sig.set(editor.text.clone());
                }
                if let Some(cb) = on_change.clone() {
                    changed_cb = Some((cb, editor.text.clone()));
                }
            }
            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }

        if let Some((cb, txt)) = changed_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(txt);
        }
        self.tree.reset_caret_blink(craftui_core::monotonic_ms());
        self.last_synced_ime_state = Some(SyncedImeSnapshot {
            text: text.to_string(),
            selection_start: sel_s,
            selection_end: sel_e,
            compose_region: compose,
        });
        self.needs_redraw = true;
        true
    }

    /// Update or allocate a CraftUI [`TextureHandle`] from an external Android [`ExternalSurfaceTextureBridge`].
    pub fn sync_external_texture(
        &mut self,
        bridge: &mut dyn ExternalSurfaceTextureBridge,
        slot: &mut Option<TextureHandle>,
    ) -> Option<TextureId> {
        let (w, h, rgba) = bridge.acquire_latest_rgba()?;
        if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 {
            return None;
        }
        self.needs_redraw = true;
        if let Some(existing) = slot.as_ref() {
            existing.buffer.replace_rgba(w, h, rgba);
            Some(existing.id)
        } else {
            let handle = self.textures.create_from_rgba(w, h, rgba);
            let id = handle.id;
            *slot = Some(handle);
            Some(id)
        }
    }

    /// Advance reactive timers, animations, layout, and paint when dirty or animating.
    /// Returns `(animating, did_paint, cmds)`.
    pub fn step_frame_with_redraw(&mut self, dt_sec: f32) -> (bool, bool, Vec<AndroidImeCommand>) {
        let now_ms = craftui_core::monotonic_ms();
        let timers = craftui_reactive::tick_timers();
        let jobs = self.dispatcher.drain();
        let pending_reload = self
            .remote_preview
            .as_ref()
            .and_then(|c| c.take_pending_reload());
        if let Some(reload) = pending_reload {
            let t0 = Instant::now();
            let res = self.apply_hot_reload_source(&reload.source, reload.component.as_deref());
            if let Some(client) = self.remote_preview.as_ref() {
                match res {
                    Ok(_) => {
                        let dur = t0.elapsed().as_secs_f64() * 1000.0;
                        tracing::info!(
                            "Remote live preview reloaded {} in {:.2}ms",
                            reload.path,
                            dur
                        );
                        client.send_ack(reload.path, dur);
                    }
                    Err(err) => {
                        tracing::warn!("Remote live preview reload error: {err}");
                        client.send_error(reload.path, err.to_string());
                    }
                }
            }
        }
        let blink = self.tree.tick_blink_timers(now_ms);
        let anim_tick = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let animating = anim_tick || self.tree.has_active_animations();
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || anim_tick || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }

        let mut cmds = Vec::new();
        let focused_input = self.tree.focused.and_then(|id| {
            self.tree.nodes.get(id).and_then(|n| match &n.kind {
                WidgetKind::TextInput { editor, .. } => {
                    if let Some(pre) = editor.preedit.as_deref().filter(|p| !p.is_empty()) {
                        let cur = editor.cursor.min(editor.text.len());
                        let full = editor.display_text().into_owned();
                        let compose = Some((cur, cur + pre.len()));
                        let sel = match editor.preedit_cursor {
                            Some((ps, pe)) => {
                                ((cur + ps).min(full.len()), (cur + pe).min(full.len()))
                            }
                            None => (cur + pre.len(), cur + pre.len()),
                        };
                        Some(SyncedImeSnapshot {
                            text: full,
                            selection_start: sel.0,
                            selection_end: sel.1,
                            compose_region: compose,
                        })
                    } else {
                        let (s, e) = editor
                            .selection_range()
                            .unwrap_or((editor.cursor, editor.cursor));
                        Some(SyncedImeSnapshot {
                            text: editor.text.clone(),
                            selection_start: s,
                            selection_end: e,
                            compose_region: None,
                        })
                    }
                }
                _ => None,
            })
        });

        let ime_needed = focused_input.is_some();
        if ime_needed != self.last_ime_visible {
            self.last_ime_visible = ime_needed;
            if ime_needed {
                cmds.push(AndroidImeCommand::ShowSoftInput);
            } else {
                self.last_synced_ime_state = None;
                cmds.push(AndroidImeCommand::HideSoftInput);
            }
        }
        if let Some(next_state) = focused_input
            && self.last_synced_ime_state.as_ref() != Some(&next_state)
        {
            self.last_synced_ime_state = Some(next_state.clone());
            cmds.push(AndroidImeCommand::SyncTextState {
                text: next_state.text,
                selection_start: next_state.selection_start,
                selection_end: next_state.selection_end,
                compose_region: next_state.compose_region,
            });
        }

        (animating, should_paint, cmds)
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> (bool, Vec<AndroidImeCommand>) {
        let (animating, _painted, cmds) = self.step_frame_with_redraw(dt_sec);
        (animating, cmds)
    }
}
