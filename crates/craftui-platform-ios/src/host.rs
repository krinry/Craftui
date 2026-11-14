use craftui_core::{
    ImeEvent, Insets, KeyCode, KeyEvent, Modifiers, NodeId, PlatformServices, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, Size, TextureId, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{WidgetKind, WidgetTree};
use std::ffi::{CString, c_void};
use std::sync::Arc;
use std::time::Duration;

use crate::services::*;
use crate::types::*;

/// iOS host controller bridging `CADisplayLink` ticks, UIKit touches, SafeArea, and Metal rendering.
pub struct IosAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    pub bridge: Arc<IosBridgeState>,
    last_first_responder: bool,
    last_synced_text: Option<String>,
    pub(crate) needs_redraw: bool,
    pub last_ime_commands: Vec<IosImeCommand>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_error_cstr: Option<CString>,
    #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
    gpu: Option<craftui_render::GpuSurfacePipeline>,
}

impl IosAppHost {
    pub fn new(dispatcher: UiDispatcher, width_pt: f32, height_pt: f32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let bridge = Arc::new(IosBridgeState::new());
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_permissions(IosPermissionBackend::new(bridge.clone()))
                .with_haptics(IosHapticsBackend::new(bridge.clone()))
                .with_share(IosShareBackend::new(bridge.clone()))
                .with_secure_storage(IosSecureStorage::new(bridge.clone()))
                .with_device_capabilities(IosDeviceCapabilities::new(bridge.clone())),
        );
        tree.metrics = WindowMetrics {
            logical_size: Size::new(width_pt.max(1.0), height_pt.max(1.0)),
            scale_factor: scale_factor.max(1.0),
            safe_area: Insets::ZERO,
            keyboard_inset: 0.0,
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            bridge,
            last_first_responder: false,
            last_synced_text: None,
            needs_redraw: true,
            last_ime_commands: Vec::new(),
            last_error: None,
            last_error_cstr: None,
            #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
            gpu: None,
        }
    }

    pub fn bridge(&self) -> Arc<IosBridgeState> {
        self.bridge.clone()
    }

    pub fn set_permission_wait_timeout(&self, timeout: Duration) {
        self.bridge.set_request_timeout(timeout);
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
        self.last_error_cstr = None;
    }

    pub fn record_error(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        let sanitized = text.replace('\0', "?");
        self.last_error_cstr = CString::new(sanitized).ok();
        self.last_error = Some(text);
    }

    /// Attach or replace the UIKit `CAMetalLayer` target, recording errors in [`Self::last_error`].
    ///
    /// # Safety
    /// On iOS, if `layer.ca_metal_layer` is non-null it must point to a valid retained `CAMetalLayer`
    /// that outlives the attached GPU pipeline.
    pub unsafe fn attach_metal(
        &mut self,
        layer: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<()> {
        if layer.ca_metal_layer.is_null() {
            let err =
                craftui_core::CraftError::Platform("iOS CAMetalLayer pointer is null".to_string());
            self.record_error(err.to_string());
            return Err(err);
        }
        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        {
            // SAFETY: forwarded from caller invariant that `layer.ca_metal_layer` outlives `self.gpu`.
            match unsafe { ios_metal::attach_metal_pipeline(self, layer) } {
                Ok(()) => {
                    self.clear_error();
                    Ok(())
                }
                Err(err) => {
                    let msg = format!("iOS Metal attach failed: {err}");
                    tracing::warn!("{msg}");
                    self.record_error(msg);
                    Err(err)
                }
            }
        }
        #[cfg(not(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello"))))]
        {
            let _ = layer;
            let err = craftui_core::CraftError::Platform(
                "iOS Metal pipeline requires target_os = \"ios\" with gpu-sdf or gpu-vello enabled"
                    .to_string(),
            );
            self.record_error(err.to_string());
            Err(err)
        }
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    /// Returns whether `CADisplayLink` should remain active for ongoing animations, pending redraws, or timers.
    pub fn needs_display_link(&self, animating: bool) -> bool {
        if animating || self.needs_redraw {
            return true;
        }
        let now_ms = craftui_core::monotonic_ms();
        craftui_reactive::next_timer_delay_ms(now_ms).is_some()
            || self.tree.next_internal_delay_ms(now_ms).is_some()
    }

    /// Build a CraftUI root view inside Rust before handing control to the UIKit CADisplayLink loop.
    pub fn mount_root<F>(&mut self, build_ui: F)
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId,
    {
        let root = build_ui(
            self.scope,
            &mut self.tree,
            &mut self.textures,
            &self.dispatcher,
        );
        self.tree.set_root(root);
        self.needs_redraw = true;
    }

    pub fn set_safe_area(&mut self, safe_area: IosSafeAreaInsets) {
        let next: Insets = safe_area.into();
        if self.tree.metrics.safe_area != next {
            self.tree.metrics.safe_area = next;
            self.needs_redraw = true;
        }
    }

    pub fn set_keyboard_frame(&mut self, frame: IosKeyboardFrame) {
        let next = if frame.visible {
            frame.overlap_height_pt.max(0.0)
        } else {
            0.0
        };
        if (self.tree.metrics.keyboard_inset - next).abs() > 0.01 {
            self.tree.metrics.keyboard_inset = next;
            self.needs_redraw = true;
        }
    }

    pub fn resize_pt(&mut self, width_pt: f32, height_pt: f32, scale_factor: f32) {
        let w = width_pt.max(1.0);
        let h = height_pt.max(1.0);
        let s = scale_factor.max(1.0);
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
        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        if let Some(gpu) = self.gpu.as_mut() {
            let (pw, ph) = self.tree.metrics.physical_size();
            gpu.resize(pw, ph);
        }
    }

    pub fn push_ui_touch(
        &mut self,
        touch_id: u64,
        phase: PointerPhase,
        x_pt: f32,
        y_pt: f32,
        timestamp_ms: u64,
    ) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(touch_id),
            phase,
            position: Point::new(x_pt, y_pt),
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

    pub fn push_marked_text(&mut self, marked: &str, cursor_end: usize) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Ime(ImeEvent::Preedit(
            marked.to_string(),
            Some((cursor_end, cursor_end)),
        )));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_insert_text(&mut self, committed: &str) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(committed.to_string())));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_delete_backward(&mut self) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Pull the latest frame from a [`CvPixelBufferBridge`] into a CraftUI [`TextureHandle`].
    pub fn sync_cv_pixel_buffer(
        &mut self,
        bridge: &mut dyn CvPixelBufferBridge,
        slot: &mut Option<TextureHandle>,
    ) -> Option<TextureId> {
        let frame = bridge.acquire_latest_frame()?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        let rgba = bgra_frame_to_packed_rgba(&frame);
        self.needs_redraw = true;
        if let Some(existing) = slot.as_ref() {
            existing
                .buffer
                .replace_rgba(frame.width, frame.height, rgba);
            Some(existing.id)
        } else {
            let handle = self
                .textures
                .create_from_rgba(frame.width, frame.height, rgba);
            let id = handle.id;
            *slot = Some(handle);
            Some(id)
        }
    }

    pub fn display_link_tick_with_redraw(
        &mut self,
        dt_sec: f32,
        now_ms: u64,
    ) -> (bool, bool, Vec<IosImeCommand>) {
        let timers = craftui_reactive::tick_timers_at(now_ms);
        let jobs = self.dispatcher.drain();
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
        let focused_text = self.tree.focused.and_then(|id| {
            self.tree.nodes.get(id).and_then(|n| match &n.kind {
                WidgetKind::TextInput { editor, .. } => Some(editor.text.clone()),
                _ => None,
            })
        });
        let needs_kb = focused_text.is_some();
        if needs_kb != self.last_first_responder {
            self.last_first_responder = needs_kb;
            if needs_kb {
                cmds.push(IosImeCommand::BecomeFirstResponder);
            } else {
                self.last_synced_text = None;
                cmds.push(IosImeCommand::ResignFirstResponder);
            }
        }
        if let Some(txt) = focused_text
            && self.last_synced_text.as_ref() != Some(&txt)
        {
            self.last_synced_text = Some(txt.clone());
            cmds.push(IosImeCommand::SyncText(txt));
        }

        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        if should_paint && let Some(gpu) = self.gpu.as_mut() {
            let bg = self.tree.theme().background;
            let sf = self.tree.metrics.scale_factor;
            if let Err(err) = gpu.render_frame(
                &self.scene,
                sf,
                bg,
                &self.textures,
                &mut self.tree.text_engine,
            ) {
                let msg = format!("iOS Metal render_frame failed: {err}");
                tracing::warn!("{msg}");
                self.record_error(msg);
            }
        }

        self.last_ime_commands = cmds.clone();
        (animating, should_paint, cmds)
    }

    pub fn display_link_tick(&mut self, dt_sec: f32) -> (bool, Vec<IosImeCommand>) {
        let (animating, _did_paint, cmds) =
            self.display_link_tick_with_redraw(dt_sec, craftui_core::monotonic_ms());
        (animating, cmds)
    }
}

/// FFI-safe `CAMetalLayer` descriptor passed from Swift/ObjC `UIViewController`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IosMetalLayerDescriptor {
    pub ca_metal_layer: *mut c_void,
    pub width_pt: f32,
    pub height_pt: f32,
    pub contents_scale: f32,
}

#[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
pub mod ios_metal {
    use super::*;

    /// Initialize a `wgpu::Surface` targeting a `CAMetalLayer` pointer from UIKit.
    ///
    /// # Safety
    /// `desc.ca_metal_layer` must point to a valid retained `CALayer` / `CAMetalLayer`.
    pub unsafe fn create_metal_surface(
        instance: &wgpu::Instance,
        desc: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<wgpu::Surface<'static>> {
        let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(desc.ca_metal_layer);
        // SAFETY: forwarded from caller invariant that `desc.ca_metal_layer` is a live `CAMetalLayer`.
        unsafe { instance.create_surface_unsafe(target) }.map_err(|e| {
            craftui_core::CraftError::Platform(format!("iOS CAMetalLayer surface error: {e}"))
        })
    }

    /// Attach a live Metal [`craftui_render::GpuSurfacePipeline`] to an [`IosAppHost`].
    ///
    /// # Safety
    /// `desc.ca_metal_layer` must remain valid until the host is destroyed or replaced.
    pub unsafe fn attach_metal_pipeline(
        host: &mut IosAppHost,
        desc: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<()> {
        host.resize_pt(desc.width_pt, desc.height_pt, desc.contents_scale);
        let (pw, ph) = host.tree.metrics.physical_size();
        let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(desc.ca_metal_layer);
        // SAFETY: forwarded from caller invariant that `desc.ca_metal_layer` outlives `host.gpu`.
        let pipeline =
            unsafe { craftui_render::GpuSurfacePipeline::new_unsafe(target, pw, ph, false) }?;
        host.gpu = Some(pipeline);
        host.needs_redraw = true;
        Ok(())
    }
}
