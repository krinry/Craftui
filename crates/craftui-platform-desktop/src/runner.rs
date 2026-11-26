use accesskit_winit::Adapter as AccessKitAdapter;
#[cfg(feature = "hot-reload")]
use craftui_core::KeyCode;
use craftui_core::{
    CraftError, ImeEvent, Modifiers, NodeId, PlatformServices, Point, PointerButton, PointerEvent,
    PointerId, PointerPhase, Result, ScrollDelta, Size, UiDispatcher, UiEvent, WindowMetrics,
    monotonic_ms,
};
#[cfg(feature = "hot-reload")]
use craftui_core::{ThemeMode, ThemeTokens};
use craftui_reactive::Scope;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use craftui_render::GpuSurfacePipeline;
use craftui_render::{PaintScene, TextureRegistry};
#[cfg(feature = "hot-reload")]
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;
#[cfg(feature = "hot-reload")]
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
#[cfg(feature = "hot-reload")]
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime as WinitIme, TouchPhase, WindowEvent};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::window::{CursorIcon as WinitCursorIcon, Window, WindowAttributes, WindowId};

use crate::events::{
    DesktopUserEvent, sync_modifiers_from_key, translate_modifiers, translate_mouse_button,
    translate_scroll_delta, translate_winit_cursor, translate_winit_key,
};
#[cfg(feature = "hot-reload")]
use crate::hot_reload::{HotReloadSpec, paint_live_preview_overlays, should_trigger_hot_reload};
use crate::presenter::{DesktopPresenter, SoftbufferPresenter};
use crate::services::default_desktop_services;

/// Desktop window configuration.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub force_cpu: bool,
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "CraftUI Application".to_string(),
            width: 1024.0,
            height: 768.0,
            force_cpu: false,
            resizable: true,
        }
    }
}

pub(crate) type BuildUiFn = Box<dyn FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId>;

pub(crate) struct PendingWindowRequest {
    pub(crate) config: WindowConfig,
    pub(crate) builder: BuildUiFn,
}

static NEXT_WINDOW_REQ_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

thread_local! {
    static PENDING_WINDOW_QUEUE: RefCell<HashMap<u64, PendingWindowRequest>> =
        RefCell::new(HashMap::new());
}

/// Cloneable handle enabling button click handlers and effects to spawn additional desktop windows.
/// Bound to the event-loop thread.
#[derive(Clone)]
pub struct WindowLauncher {
    proxy: EventLoopProxy<DesktopUserEvent>,
    _not_send: PhantomData<*const ()>,
}

impl WindowLauncher {
    /// Request opening a new secondary desktop window with its own independent [`WidgetTree`] & GPU surface.
    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        let id = NEXT_WINDOW_REQ_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        PENDING_WINDOW_QUEUE.with(|cell| {
            cell.borrow_mut().insert(
                id,
                PendingWindowRequest {
                    config,
                    builder: Box::new(build_ui),
                },
            );
        });
        self.proxy
            .send_event(DesktopUserEvent::OpenWindow(id))
            .map_err(|e| CraftError::Platform(format!("failed queuing open_window: {e}")))
    }

    /// Close a specific desktop window by its [`WindowId`].
    pub fn close_window(&self, id: WindowId) -> Result<()> {
        self.proxy
            .send_event(DesktopUserEvent::CloseWindow(id))
            .map_err(|e| CraftError::Platform(format!("failed queuing close_window: {e}")))
    }

    /// Update the OS title bar of an open desktop window.
    pub fn set_title(&self, id: WindowId, title: impl Into<String>) -> Result<()> {
        self.proxy
            .send_event(DesktopUserEvent::SetWindowTitle(id, title.into()))
            .map_err(|e| CraftError::Platform(format!("failed queuing set_title: {e}")))
    }
}

/// Platform services provided during app setup.
pub struct AppContext {
    pub dispatcher: UiDispatcher,
    pub textures: TextureRegistry,
    pub services: PlatformServices,
    pub tray: crate::tray::SystemTrayHandle,
    pub(crate) launcher: WindowLauncher,
}

impl AppContext {
    pub fn window_launcher(&self) -> WindowLauncher {
        self.launcher.clone()
    }

    pub fn system_tray(&self) -> crate::tray::SystemTrayHandle {
        self.tray.clone()
    }

    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        self.launcher.open_window(config, build_ui)
    }
}

pub(crate) struct InitialWindowSetup {
    pub(crate) config: WindowConfig,
    pub(crate) root_scope: Scope,
    pub(crate) tree: WidgetTree,
    pub(crate) builder: Option<BuildUiFn>,
    #[cfg(feature = "hot-reload")]
    pub(crate) hot_reload: Option<HotReloadSpec>,
}

pub(crate) struct DesktopWindowState {
    pub(crate) window: Arc<Window>,
    pub(crate) root_scope: Scope,
    pub(crate) is_primary: bool,
    pub(crate) tree: WidgetTree,
    pub(crate) textures: TextureRegistry,
    pub(crate) scene: PaintScene,
    pub(crate) a11y: Option<AccessKitAdapter>,
    pub(crate) a11y_active: bool,
    pub(crate) presenter: Option<DesktopPresenter>,
    #[cfg(feature = "hot-reload")]
    pub(crate) hot_reload: Option<HotReloadSpec>,
    pub(crate) cursor_pos: Point,
    pub(crate) modifiers: Modifiers,
    pub(crate) mouse_down: bool,
    pub(crate) last_focused_was_input: bool,
    pub(crate) last_cursor: Option<WinitCursorIcon>,
}

pub(crate) fn sync_window_cursor(state: &mut DesktopWindowState) {
    let desired = translate_winit_cursor(state.tree.hovered_cursor());
    if state.last_cursor != Some(desired) {
        state.last_cursor = Some(desired);
        state.window.set_cursor(desired);
    }
}

pub(crate) struct DesktopRunner {
    proxy: EventLoopProxy<DesktopUserEvent>,
    dispatcher: UiDispatcher,
    services: PlatformServices,
    tray: crate::tray::SystemTrayHandle,
    softbuffer_ctx: Option<softbuffer::Context<OwnedDisplayHandle>>,
    pending_primary: Option<InitialWindowSetup>,
    windows: HashMap<WindowId, DesktopWindowState>,
    start_instant: Instant,
    last_frame_instant: Instant,
}

impl DesktopRunner {
    pub(crate) fn new(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let root_scope = Scope::root();
        let mut tree = WidgetTree::new(root_scope);
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        Self::new_with_tree(config, proxy, dispatcher, root_scope, tree, builder)
    }

    pub(crate) fn new_with_tree(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        root_scope: Scope,
        mut tree: WidgetTree,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(config.width, config.height),
            scale_factor: 1.0,
            ..Default::default()
        };
        let tray = crate::tray::SystemTrayHandle::with_proxy(proxy.clone());
        Self {
            proxy,
            dispatcher,
            services,
            tray,
            softbuffer_ctx: None,
            pending_primary: Some(InitialWindowSetup {
                config,
                root_scope,
                tree,
                builder,
                #[cfg(feature = "hot-reload")]
                hot_reload: None,
            }),
            windows: HashMap::new(),
            start_instant: Instant::now(),
            last_frame_instant: Instant::now(),
        }
    }

    #[cfg(feature = "hot-reload")]
    pub(crate) fn set_primary_hot_reload(&mut self, spec: HotReloadSpec) {
        if let Some(init) = self.pending_primary.as_mut() {
            init.hot_reload = Some(spec);
        }
    }

    fn create_softbuffer_presenter(
        &mut self,
        display_handle: OwnedDisplayHandle,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<SoftbufferPresenter> {
        if self.softbuffer_ctx.is_none() {
            let ctx = softbuffer::Context::new(display_handle).map_err(|e| {
                CraftError::Render(format!("softbuffer context creation failed: {e}"))
            })?;
            self.softbuffer_ctx = Some(ctx);
        }
        let ctx = self
            .softbuffer_ctx
            .as_ref()
            .ok_or_else(|| CraftError::Render("softbuffer context missing".into()))?;
        SoftbufferPresenter::new(ctx, window, width, height)
    }

    fn timestamp_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        setup: InitialWindowSetup,
        is_primary: bool,
    ) {
        let InitialWindowSetup {
            config,
            root_scope,
            mut tree,
            builder,
            #[cfg(feature = "hot-reload")]
            hot_reload,
        } = setup;
        let attrs = WindowAttributes::default()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(config.width, config.height))
            .with_resizable(config.resizable)
            .with_visible(false);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("create_window failed: {e}");
                eprintln!("[craftui] create_window failed: {e}");
                if self.windows.is_empty() {
                    event_loop.exit();
                }
                return;
            }
        };

        let a11y = AccessKitAdapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);

        let scale = window.scale_factor().max(1.0) as f32;
        let phys = window.inner_size();
        let init_w = if phys.width > 0 {
            phys.width
        } else {
            (config.width.max(1.0) * scale).round().max(1.0) as u32
        };
        let init_h = if phys.height > 0 {
            phys.height
        } else {
            (config.height.max(1.0) * scale).round().max(1.0) as u32
        };

        tree.set_platform_services(self.services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(init_w as f32 / scale, init_h as f32 / scale),
            scale_factor: scale,
            ..Default::default()
        };

        let force_cpu = config.force_cpu
            || std::env::var("CRAFTUI_FORCE_CPU")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            || !cfg!(any(feature = "gpu-sdf", feature = "gpu-vello"));

        let display_handle = event_loop.owned_display_handle();
        let presenter = if force_cpu {
            match self.create_softbuffer_presenter(
                display_handle.clone(),
                Arc::clone(&window),
                init_w,
                init_h,
            ) {
                Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                Err(sb_err) => {
                    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                    {
                        tracing::warn!(
                            "Softbuffer init failed ({sb_err}); trying wgpu CPU fallback"
                        );
                        match GpuSurfacePipeline::new_with_display(
                            || Arc::clone(&window),
                            || Box::new(display_handle.clone()),
                            init_w,
                            init_h,
                            true,
                        ) {
                            Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                            Err(gpu_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize any CPU presenter (softbuffer: {sb_err}; wgpu: {gpu_err})",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                    #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
                    {
                        eprintln!(
                            "[craftui] window '{}' failed to initialize softbuffer CPU presenter: {sb_err}",
                            config.title
                        );
                        None
                    }
                }
            }
        } else {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            {
                match GpuSurfacePipeline::new_with_display(
                    || Arc::clone(&window),
                    || Box::new(display_handle.clone()),
                    init_w,
                    init_h,
                    false,
                ) {
                    Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                    Err(gpu_err) => {
                        tracing::warn!(
                            "GpuSurfacePipeline initialization failed ({gpu_err}); falling back to softbuffer CPU presenter"
                        );
                        eprintln!(
                            "[craftui] GPU presenter unavailable ({gpu_err}); falling back to CPU softbuffer"
                        );
                        match self.create_softbuffer_presenter(
                            display_handle,
                            Arc::clone(&window),
                            init_w,
                            init_h,
                        ) {
                            Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                            Err(sb_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize softbuffer fallback: {sb_err}",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                }
            }
            #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
            {
                unreachable!("force_cpu is always true when GPU backends are disabled")
            }
        };

        if let Some(p) = presenter.as_ref() {
            tracing::info!(
                "CraftUI Desktop Window '{}' Renderer initialized ({})",
                config.title,
                p.label()
            );
        }

        let mut app_ctx = AppContext {
            dispatcher: self.dispatcher.clone(),
            textures: TextureRegistry::new(),
            services: self.services.clone(),
            tray: self.tray.clone(),
            launcher: WindowLauncher {
                proxy: self.proxy.clone(),
                _not_send: PhantomData,
            },
        };

        let win_id = window.id();
        root_scope.provide_context(app_ctx.launcher.clone());
        root_scope.provide_context(app_ctx.tray.clone());
        root_scope.provide_context(win_id);

        if let Some(build_fn) = builder {
            let root = build_fn(root_scope, &mut tree, &mut app_ctx);
            tree.set_root(root);
        }

        window.request_redraw();
        self.windows.insert(
            win_id,
            DesktopWindowState {
                window,
                root_scope,
                is_primary,
                tree,
                textures: app_ctx.textures,
                scene: PaintScene::new(),
                a11y: Some(a11y),
                a11y_active: false,
                presenter,
                #[cfg(feature = "hot-reload")]
                hot_reload,
                cursor_pos: Point::ZERO,
                modifiers: Modifiers::default(),
                mouse_down: false,
                last_focused_was_input: false,
                last_cursor: None,
            },
        );
    }

    fn render_and_present_window(&mut self, id: WindowId) {
        let now = Instant::now();
        let dt = (now - self.last_frame_instant)
            .as_secs_f32()
            .clamp(0.0001, 0.1);
        self.last_frame_instant = now;

        let _ = craftui_reactive::tick_timers();
        let _ = self.dispatcher.drain();

        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        let frame_start = Instant::now();
        let _ = state.tree.tick_blink_timers(monotonic_ms());
        let anim_tick = state.tree.tick_animations(dt);

        let layout_start = Instant::now();
        let dirty_count = state.tree.layout(state.tree.metrics);
        let layout_ms = layout_start.elapsed().as_secs_f32() * 1000.0;
        let animating = anim_tick || state.tree.has_active_animations();

        state.tree.paint(&mut state.scene);

        #[cfg(feature = "hot-reload")]
        if let Some(spec) = state.hot_reload.as_mut() {
            paint_live_preview_overlays(&mut state.scene, &state.tree, state.cursor_pos, spec);
        }

        let bg = state.tree.theme().background;
        let scale = state.tree.metrics.scale_factor;
        let tex_reg = &state.textures;
        if let Some(presenter) = state.presenter.as_mut()
            && let Err(e) = presenter.render_frame(
                &state.scene,
                scale,
                bg,
                tex_reg,
                &mut state.tree.text_engine,
            )
        {
            tracing::warn!("desktop frame render error: {e}");
            eprintln!("[craftui] frame render error: {e}");
        }

        if state.a11y_active
            && let Some(a11y) = state.a11y.as_mut()
            && let Some(update) = state.tree.build_a11y_update()
        {
            a11y.update_if_active(|| update);
        }

        let focused_is_input = state.tree.focused_is_text_input();
        if focused_is_input != state.last_focused_was_input {
            state.last_focused_was_input = focused_is_input;
            state.window.set_ime_allowed(focused_is_input);
        }
        sync_window_cursor(state);

        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        let node_count = state.tree.nodes.len();
        state
            .tree
            .inspector
            .record_frame(frame_ms, layout_ms, node_count, dirty_count);

        if animating {
            state.window.request_redraw();
        }
    }

    #[cfg(feature = "hot-reload")]
    fn reload_cui_from_disk(&mut self) {
        for state in self.windows.values_mut() {
            let Some(spec) = state.hot_reload.as_mut() else {
                continue;
            };
            match spec
                .session
                .reload_file(&spec.path, spec.component.as_deref(), &mut state.tree)
            {
                Ok(_) => {
                    spec.last_reload_error = None;
                    tracing::info!(
                        "Live-reloaded {} in {:.2}ms",
                        spec.path.display(),
                        spec.session.last_reload_duration.as_secs_f64() * 1000.0
                    );
                    state.window.request_redraw();
                }
                Err(e) => {
                    spec.last_reload_error = Some(e.to_string());
                    tracing::warn!("Live-reload diagnostic: {e}");
                    state.window.request_redraw();
                }
            }
        }
    }
}

impl ApplicationHandler<DesktopUserEvent> for DesktopRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(init) = self.pending_primary.take() {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.spawn_window(event_loop, init, true);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopUserEvent) {
        match event {
            DesktopUserEvent::WakeUi => {
                let _ = self.dispatcher.drain();
                for state in self.windows.values() {
                    state.window.request_redraw();
                }
            }
            DesktopUserEvent::AccessKit(ev) => {
                if let Some(state) = self.windows.get_mut(&ev.window_id) {
                    match &ev.window_event {
                        accesskit_winit::WindowEvent::ActionRequested(req) => {
                            if state.tree.handle_a11y_action(req) {
                                state.window.request_redraw();
                            }
                        }
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            state.a11y_active = true;
                            state.tree.invalidate_a11y();
                            if let Some(a11y) = state.a11y.as_mut()
                                && let Some(update) = state.tree.build_a11y_update()
                            {
                                a11y.update_if_active(|| update);
                            }
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            state.a11y_active = false;
                            state.tree.invalidate_a11y();
                        }
                    }
                }
            }
            #[cfg(feature = "hot-reload")]
            DesktopUserEvent::FileChanged => {
                self.reload_cui_from_disk();
            }
            DesktopUserEvent::OpenWindow(req_id) => {
                let req = PENDING_WINDOW_QUEUE.with(|cell| cell.borrow_mut().remove(&req_id));
                if let Some(req) = req {
                    let child_scope = Scope::root().child_scope();
                    let child_tree = WidgetTree::new(child_scope);
                    self.spawn_window(
                        event_loop,
                        InitialWindowSetup {
                            config: req.config,
                            root_scope: child_scope,
                            tree: child_tree,
                            builder: Some(req.builder),
                            #[cfg(feature = "hot-reload")]
                            hot_reload: None,
                        },
                        false,
                    );
                }
            }
            DesktopUserEvent::CloseWindow(win_id) => {
                if let Some(removed) = self.windows.remove(&win_id)
                    && !removed.is_primary
                {
                    removed.root_scope.dispose();
                }
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            DesktopUserEvent::SetWindowTitle(win_id, title) => {
                if let Some(state) = self.windows.get(&win_id) {
                    state.window.set_title(&title);
                }
            }
            DesktopUserEvent::SetWindowVisible(win_id, visible) => {
                if let Some(state) = self.windows.get(&win_id) {
                    state.window.set_visible(visible);
                    if visible {
                        state.window.focus_window();
                        state.window.request_redraw();
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now_ms = monotonic_ms();
        let timers_ran = craftui_reactive::tick_timers();
        let jobs_ran = self.dispatcher.drain();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        for state in self.windows.values_mut() {
            let blink_due = state.tree.tick_blink_timers(now_ms);
            if timers_ran > 0 || jobs_ran > 0 || blink_due {
                state.window.request_redraw();
            }
            if let Some(win_delay) = state.tree.next_internal_delay_ms(now_ms) {
                min_delay = Some(min_delay.map_or(win_delay, |cur| cur.min(win_delay)));
            }
        }
        if let Some(delay_ms) = min_delay {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(delay_ms.max(1)),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        if let Some(a11y) = state.a11y.as_mut() {
            a11y.process_event(&state.window, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(removed) = self.windows.remove(&id)
                    && !removed.is_primary
                {
                    removed.root_scope.dispose();
                }
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(size.width, size.height);
                    }
                    let sf = state.tree.metrics.scale_factor.max(1.0);
                    state.tree.handle_event(&UiEvent::Resize {
                        width: size.width as f32 / sf,
                        height: size.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let sf = (scale_factor as f32).max(1.0);
                let phys = state.window.inner_size();
                if phys.width > 0 && phys.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(phys.width, phys.height);
                    }
                    state.tree.handle_event(&UiEvent::Resize {
                        width: phys.width as f32 / sf,
                        height: phys.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                state.cursor_pos = Point::new(position.x as f32 / sf, position.y as f32 / sf);
                let ts = self.start_instant.elapsed().as_millis() as u64;
                let handled = state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase: PointerPhase::Move,
                    position: state.cursor_pos,
                    button: if state.mouse_down {
                        Some(PointerButton::Primary)
                    } else {
                        None
                    },
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                }));
                sync_window_cursor(state);
                if handled {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                let phase = match btn_state {
                    ElementState::Pressed => {
                        state.mouse_down = true;
                        PointerPhase::Down
                    }
                    ElementState::Released => {
                        state.mouse_down = false;
                        PointerPhase::Up
                    }
                };
                let ts = self.start_instant.elapsed().as_millis() as u64;
                let handled = state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase,
                    position: state.cursor_pos,
                    button: Some(translate_mouse_button(button)),
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                }));
                sync_window_cursor(state);
                if handled {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let (dx, dy) = translate_scroll_delta(delta, sf);
                if state.tree.handle_event(&UiEvent::Scroll(ScrollDelta {
                    x: dx,
                    y: dy,
                    position: state.cursor_pos,
                })) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let pos = Point::new(touch.location.x as f32 / sf, touch.location.y as f32 / sf);
                let phase = match touch.phase {
                    TouchPhase::Started => PointerPhase::Down,
                    TouchPhase::Moved => PointerPhase::Move,
                    TouchPhase::Ended => PointerPhase::Up,
                    TouchPhase::Cancelled => PointerPhase::Cancel,
                };
                let ts = self.timestamp_ms();
                if let Some(st) = self.windows.get_mut(&id)
                    && st.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                        id: PointerId(touch.id + 100),
                        phase,
                        position: pos,
                        button: Some(PointerButton::Primary),
                        modifiers: st.modifiers,
                        is_touch: true,
                        timestamp_ms: ts,
                    }))
                {
                    st.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                let mut next = translate_modifiers(mods.state());
                next.alt_gr = state.modifiers.alt_gr;
                next.caps_lock = state.modifiers.caps_lock;
                state.modifiers = next;
            }
            WindowEvent::KeyboardInput { event: key_ev, .. } => {
                sync_modifiers_from_key(&mut state.modifiers, &key_ev);
                let ke = translate_winit_key(&key_ev, state.modifiers);
                if self.tray.dispatch_key_event(&ke) {
                    state.window.request_redraw();
                    return;
                }
                #[cfg(feature = "hot-reload")]
                if key_ev.state == ElementState::Pressed
                    && let Some(spec) = state.hot_reload.as_mut()
                {
                    match ke.code {
                        KeyCode::F1 => {
                            spec.show_inspector = !spec.show_inspector;
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F2 => {
                            let cur = state.tree.theme().mode;
                            let next = match cur {
                                ThemeMode::Amoled => ThemeTokens::dark(),
                                ThemeMode::Dark => ThemeTokens::light(),
                                _ => ThemeTokens::amoled(),
                            };
                            state.tree.set_theme(next);
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F3 => {
                            spec.device_mode = !spec.device_mode;
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F5 => {
                            match spec.session.reload_file(
                                &spec.path,
                                spec.component.as_deref(),
                                &mut state.tree,
                            ) {
                                Ok(_) => {
                                    spec.last_reload_error = None;
                                }
                                Err(e) => {
                                    spec.last_reload_error = Some(e.to_string());
                                }
                            }
                            state.window.request_redraw();
                            return;
                        }
                        _ => {}
                    }
                }
                if state.tree.handle_event(&UiEvent::Key(ke)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                let ev = match ime {
                    WinitIme::Enabled => ImeEvent::Enabled,
                    WinitIme::Preedit(s, cur) => ImeEvent::Preedit(s, cur),
                    WinitIme::Commit(s) => ImeEvent::Commit(s),
                    WinitIme::Disabled => ImeEvent::Disabled,
                };
                if state.tree.handle_event(&UiEvent::Ime(ev)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if state.tree.handle_event(&UiEvent::Pinch {
                    factor: 1.0 + delta as f32,
                    center: state.cursor_pos,
                }) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                if state.tree.handle_event(&UiEvent::FocusChanged(focused)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_and_present_window(id);
            }
            _ => {}
        }
    }
}

/// Run a CraftUI application in a desktop `winit` window (supporting multi-window spawning via `AppContext::open_window`).
pub fn run_app<F>(config: WindowConfig, build_ui: F) -> Result<()>
where
    F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
{
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let mut runner = DesktopRunner::new(config, proxy, dispatcher, Some(Box::new(build_ui)));
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

/// Run an interactive live preview canvas for `.cui` files with device framing,
/// live telemetry HUD, syntax error banners, element inspector, and theme switching.
#[cfg(feature = "hot-reload")]
pub fn run_live_preview_cui(
    config: WindowConfig,
    cui_path: PathBuf,
    component: Option<String>,
    device_mode: bool,
    show_inspector: bool,
) -> Result<()> {
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let watch_proxy = proxy.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        if let Ok(ev) = res
            && should_trigger_hot_reload(&ev)
        {
            let _ = watch_proxy.send_event(DesktopUserEvent::FileChanged);
        }
    })
    .map_err(|e| CraftError::Platform(format!("file watcher init failed: {e}")))?;

    let watch_target = cui_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    watcher
        .watch(watch_target, RecursiveMode::Recursive)
        .map_err(|e| CraftError::Platform(format!("watch failed: {e}")))?;

    let root_scope = Scope::root();
    let mut tree = WidgetTree::new(root_scope);
    tree.set_platform_services(default_desktop_services());
    let mut session = HotReloadSession::new(root_scope);
    let mut initial_error = None;
    if let Err(e) = session.reload_file(&cui_path, component.as_deref(), &mut tree) {
        initial_error = Some(e.to_string());
    }

    let spec = HotReloadSpec {
        path: cui_path,
        component,
        session,
        show_hud: true,
        show_inspector,
        device_mode,
        last_reload_error: initial_error,
        frame_count: 0,
        fps_sample_time: Instant::now(),
        current_fps: 60.0,
        _watcher: Some(watcher),
    };

    let mut runner =
        DesktopRunner::new_with_tree(config, proxy, dispatcher, root_scope, tree, None);
    runner.set_primary_hot_reload(spec);
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

/// Run a live hot-reloading `.cui` preview window using `notify` + `craftui-runtime` (with full multi-file import support).
#[cfg(feature = "hot-reload")]
pub fn run_hot_reload_cui(
    config: WindowConfig,
    cui_path: PathBuf,
    component: Option<String>,
) -> Result<()> {
    run_live_preview_cui(config, cui_path, component, false, false)
}
