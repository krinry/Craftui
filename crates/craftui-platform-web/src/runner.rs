#![cfg(target_arch = "wasm32")]

#[cfg(target_arch = "wasm32")]
pub mod wasm_runner {
    use crate::dom::*;
    use crate::host::*;
    use crate::route::*;
    use craftui_a11y::A11yRole;
    use craftui_core::*;
    use craftui_core::{NodeId, set_monotonic_ms_provider};
    use craftui_reactive::Scope;
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    use craftui_render::GpuSurfacePipeline;
    use craftui_render::TextureRegistry;
    use craftui_widgets::WidgetTree;
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::rc::{Rc, Weak};
    use std::result::Result;
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
        host: Rc<RefCell<WebAppHost>>,
        scheduler: Rc<WebScheduler>,
        listeners: Vec<DomListener>,
        pointer_listeners: Rc<RefCell<Vec<DomListener>>>,
        hidden_input: HtmlInputElement,
        a11y_root: Element,
    }

    impl WebRunnerHandle {
        /// Update the mounted widget tree and schedule a new frame.
        /// The callback must not re-enter this handle while the tree is borrowed.
        pub fn with_tree<R>(&self, update: impl FnOnce(&mut WidgetTree) -> R) -> R {
            let result = {
                let mut host = self.host.borrow_mut();
                let result = update(&mut host.tree);
                host.request_redraw();
                result
            };
            self.scheduler.request_frame();
            result
        }

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

        // Delegated click listener on a11y_root to route screen-reader / accessibility clicks to host
        {
            let h_a11y = host_rc.clone();
            let sched_a11y = scheduler.clone();
            let a11y_click_cb = Closure::wrap(Box::new(move |e: web_sys::MouseEvent| {
                if let Some(target) = e.target()
                    && let Ok(el) = target.dyn_into::<Element>()
                    && let Some(closest) = el.closest("[data-craftui-node]").ok().flatten()
                    && let Some(node_str) = closest.get_attribute("data-craftui-node")
                    && let Ok(node_id) = node_str.parse::<u64>()
                {
                    if h_a11y.borrow_mut().handle_a11y_click(node_id) {
                        sched_a11y.request_frame();
                    }
                }
            })
                as Box<dyn FnMut(web_sys::MouseEvent)>);
            let a11y_target: &EventTarget = a11y_root.as_ref();
            listeners.push(DomListener::attach(a11y_target, "click", a11y_click_cb)?);
        }

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
            } else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "Unable to initialize the 2D canvas renderer",
                ));
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
        let host_raf = host_rc.clone();
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

            let mut h = host_raf.borrow_mut();
            let dpr_now = window_raf.device_pixel_ratio() as f32;
            let cur_canvas = canvas_raf.borrow().clone();
            let bcr = cur_canvas.get_bounding_client_rect();
            let html: &web_sys::HtmlElement = cur_canvas.unchecked_ref();
            let layout_w = html.offset_width() as f32;
            let layout_h = html.offset_height() as f32;
            let (pw, ph) = h.resize_css(layout_w, layout_h, dpr_now);
            if cur_canvas.width() != pw || cur_canvas.height() != ph {
                let css_w = layout_w;
                let css_h = layout_h;
                let style = cur_canvas.style();
                let _ = style.set_property("width", &format!("{css_w}px"));
                let _ = style.set_property("height", &format!("{css_h}px"));
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
            host: host_rc,
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
                let html: &HtmlElement = c_el.unchecked_ref();
                let layout_w = html.offset_width() as f32;
                let layout_h = html.offset_height() as f32;
                let bcr_w = bcr.width() as f32;
                let bcr_h = bcr.height() as f32;
                let scale_x = if bcr_w > 0.0 && layout_w > 0.0 {
                    layout_w / bcr_w
                } else {
                    1.0
                };
                let scale_y = if bcr_h > 0.0 && layout_h > 0.0 {
                    layout_h / bcr_h
                } else {
                    1.0
                };
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                    ..Default::default()
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
                    scale_x,
                    scale_y,
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
            let html: &HtmlElement = c_w.unchecked_ref();
            let layout_w = html.offset_width() as f32;
            let layout_h = html.offset_height() as f32;
            let bcr_w = bcr.width() as f32;
            let bcr_h = bcr.height() as f32;
            let scale_x = if bcr_w > 0.0 && layout_w > 0.0 {
                layout_w / bcr_w
            } else {
                1.0
            };
            let scale_y = if bcr_h > 0.0 && layout_h > 0.0 {
                layout_h / bcr_h
            } else {
                1.0
            };
            let ev = dom_wheel_event(
                e.client_x() as f32,
                e.client_y() as f32,
                bcr.left() as f32,
                bcr.top() as f32,
                scale_x,
                scale_y,
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
            let hidden_clone = hidden_input.clone();
            let win_clone = window.clone();
            let cb = Closure::wrap(Box::new(move |e: DomKeyboardEvent| {
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                    alt_gr: e.get_modifier_state("AltGraph"),
                    caps_lock: e.get_modifier_state("CapsLock"),
                };
                if comp_k.get() || e.is_composing() {
                    return;
                }
                // Do not intercept or cancel composition-initiating keys (e.g. dead keys)
                let is_composition_start = e.key() == "Dead" || e.key().starts_with("Dead");
                if is_composition_start {
                    return;
                }

                // Verify keyboard event belongs to this CraftUI application.
                // If focus is currently in an external form input (e.g. <input>, <textarea>, <select>, contenteditable)
                // that is not our hidden input, do not capture or prevent default.
                if let Some(target) = e.target()
                    && let Ok(el) = target.dyn_into::<HtmlElement>()
                {
                    let tag = el.tag_name().to_ascii_uppercase();
                    let is_external = (matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                        || el.is_content_editable())
                        && !el.is_same_node(Some(&hidden_clone));
                    if is_external {
                        return;
                    }
                }
                if let Some(doc) = win_clone.document()
                    && let Some(active) = doc.active_element()
                    && let Ok(el) = active.dyn_into::<HtmlElement>()
                {
                    let tag = el.tag_name().to_ascii_uppercase();
                    let is_external = (matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                        || el.is_content_editable())
                        && !el.is_same_node(Some(&hidden_clone));
                    if is_external {
                        return;
                    }
                }

                // Defer Cmd/Ctrl+V keydown to the trusted DOM `paste` event so live OS clipboard
                // contents from other tabs/apps arrive synchronously without double-pasting.
                if pressed && mods.command() && !mods.alt && matches!(e.key().as_str(), "v" | "V") {
                    return;
                }
                let ke =
                    dom_keyboard_event_to_craft(&e.code(), &e.key(), pressed, e.repeat(), mods);
                if h_c.borrow_mut().handle_ui_event(&UiEvent::Key(ke)) {
                    e.prevent_default();
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
