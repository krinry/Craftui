#![cfg(target_os = "android")]

use crate::host::AndroidHost;
use crate::input::*;
use crate::services::*;
use android_activity::input::{
    InputEvent, KeyAction, Keycode, MotionAction, TextInputState, TextSpan,
};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use craftui_core::{AppLifecycleEvent, Modifiers, NodeId, PointerPhase, UiDispatcher, UiEvent};
use craftui_reactive::Scope;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use craftui_render::GpuSurfacePipeline;
use craftui_render::TextureRegistry;
use craftui_widgets::WidgetTree;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use std::ptr::NonNull;
use std::time::Instant;

#[cfg(target_os = "android")]
pub fn android_log(msg: &str) {
    unsafe extern "C" {
        fn __android_log_write(
            prio: std::ffi::c_int,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> std::ffi::c_int;
    }
    use std::ffi::CString;
    if let (Ok(tag), Ok(text)) = (CString::new("CraftUI_Native"), CString::new(msg)) {
        unsafe {
            __android_log_write(4, tag.as_ptr(), text.as_ptr());
        }
    }
}

#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
struct AndroidSurfaceTarget {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
impl AndroidSurfaceTarget {
    fn new(ptr: NonNull<std::ffi::c_void>) -> Self {
        Self {
            window: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(ptr)),
            display: RawDisplayHandle::Android(AndroidDisplayHandle::new()),
        }
    }
}

fn map_activity_keycode(kc: Keycode, meta: android_activity::input::MetaState) -> AndroidKeyCode {
    let shift = meta.shift_on();
    let letter_upper = shift ^ meta.caps_lock_on();
    let ch = |lower: char, upper: char| {
        AndroidKeyCode::Character(if letter_upper { upper } else { lower })
    };
    let punct =
        |p: AndroidPunctuationKey| AndroidKeyCode::Character(map_android_punctuation(p, shift));
    let digit = |d: char| AndroidKeyCode::Character(map_android_digit(d, shift));

    match kc {
        Keycode::Back => AndroidKeyCode::Back,
        Keycode::Enter | Keycode::DpadCenter => AndroidKeyCode::Enter,
        Keycode::NumpadEnter => AndroidKeyCode::NumpadEnter,
        Keycode::Tab => AndroidKeyCode::Tab,
        Keycode::Del => AndroidKeyCode::Del,
        Keycode::ForwardDel => AndroidKeyCode::ForwardDel,
        Keycode::Escape => AndroidKeyCode::Escape,
        Keycode::DpadLeft => AndroidKeyCode::DpadLeft,
        Keycode::DpadRight => AndroidKeyCode::DpadRight,
        Keycode::DpadUp => AndroidKeyCode::DpadUp,
        Keycode::DpadDown => AndroidKeyCode::DpadDown,
        Keycode::MoveHome | Keycode::Home => AndroidKeyCode::Home,
        Keycode::MoveEnd => AndroidKeyCode::MoveEnd,
        Keycode::PageUp => AndroidKeyCode::PageUp,
        Keycode::PageDown => AndroidKeyCode::PageDown,
        Keycode::Space => AndroidKeyCode::Space,
        Keycode::Cut => AndroidKeyCode::Cut,
        Keycode::Copy => AndroidKeyCode::Copy,
        Keycode::Paste => AndroidKeyCode::Paste,
        Keycode::A => ch('a', 'A'),
        Keycode::B => ch('b', 'B'),
        Keycode::C => ch('c', 'C'),
        Keycode::D => ch('d', 'D'),
        Keycode::E => ch('e', 'E'),
        Keycode::F => ch('f', 'F'),
        Keycode::G => ch('g', 'G'),
        Keycode::H => ch('h', 'H'),
        Keycode::I => ch('i', 'I'),
        Keycode::J => ch('j', 'J'),
        Keycode::K => ch('k', 'K'),
        Keycode::L => ch('l', 'L'),
        Keycode::M => ch('m', 'M'),
        Keycode::N => ch('n', 'N'),
        Keycode::O => ch('o', 'O'),
        Keycode::P => ch('p', 'P'),
        Keycode::Q => ch('q', 'Q'),
        Keycode::R => ch('r', 'R'),
        Keycode::S => ch('s', 'S'),
        Keycode::T => ch('t', 'T'),
        Keycode::U => ch('u', 'U'),
        Keycode::V => ch('v', 'V'),
        Keycode::W => ch('w', 'W'),
        Keycode::X => ch('x', 'X'),
        Keycode::Y => ch('y', 'Y'),
        Keycode::Z => ch('z', 'Z'),
        Keycode::Keycode0 => digit('0'),
        Keycode::Keycode1 => digit('1'),
        Keycode::Keycode2 => digit('2'),
        Keycode::Keycode3 => digit('3'),
        Keycode::Keycode4 => digit('4'),
        Keycode::Keycode5 => digit('5'),
        Keycode::Keycode6 => digit('6'),
        Keycode::Keycode7 => digit('7'),
        Keycode::Keycode8 => digit('8'),
        Keycode::Keycode9 => digit('9'),
        Keycode::Numpad0 => AndroidKeyCode::Character('0'),
        Keycode::Numpad1 => AndroidKeyCode::Character('1'),
        Keycode::Numpad2 => AndroidKeyCode::Character('2'),
        Keycode::Numpad3 => AndroidKeyCode::Character('3'),
        Keycode::Numpad4 => AndroidKeyCode::Character('4'),
        Keycode::Numpad5 => AndroidKeyCode::Character('5'),
        Keycode::Numpad6 => AndroidKeyCode::Character('6'),
        Keycode::Numpad7 => AndroidKeyCode::Character('7'),
        Keycode::Numpad8 => AndroidKeyCode::Character('8'),
        Keycode::Numpad9 => AndroidKeyCode::Character('9'),
        Keycode::Comma => punct(AndroidPunctuationKey::Comma),
        Keycode::Period => punct(AndroidPunctuationKey::Period),
        Keycode::Minus => punct(AndroidPunctuationKey::Minus),
        Keycode::Equals => punct(AndroidPunctuationKey::Equals),
        Keycode::LeftBracket => punct(AndroidPunctuationKey::LeftBracket),
        Keycode::RightBracket => punct(AndroidPunctuationKey::RightBracket),
        Keycode::Backslash => punct(AndroidPunctuationKey::Backslash),
        Keycode::Semicolon => punct(AndroidPunctuationKey::Semicolon),
        Keycode::Apostrophe => punct(AndroidPunctuationKey::Apostrophe),
        Keycode::Slash => punct(AndroidPunctuationKey::Slash),
        Keycode::Grave => punct(AndroidPunctuationKey::Grave),
        Keycode::At => punct(AndroidPunctuationKey::At),
        Keycode::Plus => punct(AndroidPunctuationKey::Plus),
        Keycode::Star => punct(AndroidPunctuationKey::Star),
        Keycode::Pound => punct(AndroidPunctuationKey::Pound),
        Keycode::NumpadDivide => punct(AndroidPunctuationKey::NumpadDivide),
        Keycode::NumpadMultiply => punct(AndroidPunctuationKey::NumpadMultiply),
        Keycode::NumpadSubtract => punct(AndroidPunctuationKey::NumpadSubtract),
        Keycode::NumpadAdd => punct(AndroidPunctuationKey::NumpadAdd),
        Keycode::NumpadDot => punct(AndroidPunctuationKey::NumpadDot),
        Keycode::NumpadComma => punct(AndroidPunctuationKey::NumpadComma),
        Keycode::NumpadEquals => punct(AndroidPunctuationKey::NumpadEquals),
        Keycode::NumpadLeftParen => punct(AndroidPunctuationKey::NumpadLeftParen),
        Keycode::NumpadRightParen => punct(AndroidPunctuationKey::NumpadRightParen),
        _ => AndroidKeyCode::Other,
    }
}

pub fn run_android_app<F>(app: AndroidApp, mut build_ui: F)
where
    F: FnMut(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId,
{
    let waker = app.create_waker();
    let dispatcher = UiDispatcher::with_waker(move || {
        waker.wake();
    });

    let density = app
        .config()
        .density()
        .map(|d| (d as f32 / 160.0).max(1.0))
        .unwrap_or(2.0);

    let mut host = AndroidHost::new(dispatcher.clone(), 1080, 1920, density);
    #[cfg(feature = "hot-reload")]
    host.enable_remote_preview(craftui_core::DEFAULT_REMOTE_PREVIEW_PORT);
    let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
    host.tree.set_root(root);

    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    let mut gpu: Option<GpuSurfacePipeline> = None;
    let mut last_tick = Instant::now();
    let start = Instant::now();
    let mut running = true;
    let mut animating = false;

    while running {
        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
        let has_gpu = gpu.is_some();
        #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
        let has_gpu = false;
        let timeout = host.next_poll_timeout(animating, has_gpu);
        app.poll_events(timeout, |event| match event {
            PollEvent::Wake | PollEvent::Timeout => {
                host.request_redraw();
            }
            PollEvent::Main(main_ev) => match main_ev {
                MainEvent::InitWindow { .. } => {
                    android_log("run_android_app: MainEvent::InitWindow received");
                    if let Some(nw) = app.native_window() {
                        let w = nw.width().max(1) as u32;
                        let h = nw.height().max(1) as u32;
                        android_log(&format!(
                            "run_android_app: native_window dimensions: {w}x{h}"
                        ));
                        host.tree.handle_event(&UiEvent::Resize {
                            width: w as f32 / density,
                            height: h as f32 / density,
                            scale_factor: density,
                        });
                        host.request_redraw();
                        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                        if let Some(nn) = NonNull::new(nw.ptr().as_ptr().cast()) {
                            let tgt = AndroidSurfaceTarget::new(nn);
                            let raw_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                raw_display_handle: Some(tgt.display),
                                raw_window_handle: tgt.window,
                            };
                            android_log(
                                "run_android_app: calling GpuSurfacePipeline::new_unsafe...",
                            );
                            // SAFETY: `nw` is a live ANativeWindow until `TerminateWindow` drops `gpu`.
                            match unsafe { GpuSurfacePipeline::new_unsafe(raw_target, w, h, false) }
                            {
                                Ok(pipeline) => {
                                    android_log(
                                        "run_android_app: GpuSurfacePipeline created successfully!",
                                    );
                                    host.clear_error();
                                    gpu = Some(pipeline);
                                }
                                Err(err) => {
                                    let msg =
                                        format!("Android Vulkan/GLES surface init failed: {err}");
                                    android_log(&format!("run_android_app ERROR: {msg}"));
                                    tracing::warn!("{msg}");
                                    host.record_error(msg);
                                    gpu = None;
                                }
                            }
                        }
                    }
                }
                MainEvent::TerminateWindow { .. } => {
                    android_log("run_android_app: MainEvent::TerminateWindow received");
                    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                    {
                        gpu = None;
                    }
                }
                MainEvent::WindowResized { .. } | MainEvent::ContentRectChanged { .. } => {
                    if let Some(nw) = app.native_window() {
                        let w = nw.width().max(1) as u32;
                        let h = nw.height().max(1) as u32;
                        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                        if let Some(g) = gpu.as_mut() {
                            g.resize(w, h);
                        }
                        let cr = app.content_rect();
                        host.update_safe_area_and_ime(
                            w as i32, h as i32, cr.left, cr.top, cr.right, cr.bottom,
                        );
                        host.tree.handle_event(&UiEvent::Resize {
                            width: w as f32 / density,
                            height: h as f32 / density,
                            scale_factor: density,
                        });
                        host.request_redraw();
                    }
                }
                MainEvent::InputAvailable => {
                    if let Ok(mut iter) = app.input_events_iter() {
                        while iter.next(|ev| match ev {
                            InputEvent::MotionEvent(me) => {
                                let action = me.action();
                                let ts = start.elapsed().as_millis() as u64;
                                let ptr_idx = me.pointer_index();
                                for p in me.pointers() {
                                    let phase = match action {
                                        MotionAction::Down => PointerPhase::Down,
                                        MotionAction::PointerDown
                                            if p.pointer_index() == ptr_idx =>
                                        {
                                            PointerPhase::Down
                                        }
                                        MotionAction::Up => PointerPhase::Up,
                                        MotionAction::PointerUp if p.pointer_index() == ptr_idx => {
                                            PointerPhase::Up
                                        }
                                        MotionAction::Cancel => PointerPhase::Cancel,
                                        _ => PointerPhase::Move,
                                    };
                                    host.push_touch(p.pointer_id(), phase, p.x(), p.y(), ts);
                                }
                                InputStatus::Handled
                            }
                            InputEvent::KeyEvent(ke) => {
                                let pressed = ke.action() == KeyAction::Down;
                                let repeat = ke.repeat_count() > 0;
                                let meta = ke.meta_state();
                                let mods = Modifiers {
                                    shift: meta.shift_on(),
                                    ctrl: meta.ctrl_on(),
                                    alt: meta.alt_on(),
                                    super_key: meta.meta_on(),
                                };
                                let code = map_activity_keycode(ke.key_code(), meta);
                                if host.handle_android_key_event(code, pressed, repeat, mods) {
                                    InputStatus::Handled
                                } else if code == AndroidKeyCode::Back {
                                    InputStatus::Unhandled
                                } else {
                                    InputStatus::Handled
                                }
                            }
                            InputEvent::TextEvent(ts) => {
                                let state: TextInputState = ts.clone();
                                let compose = state.compose_region.map(|r| (r.start, r.end));
                                if host.push_ime_text_state(
                                    &state.text,
                                    state.selection.start,
                                    state.selection.end,
                                    compose,
                                ) {
                                    host.request_redraw();
                                }
                                InputStatus::Handled
                            }
                            _ => InputStatus::Unhandled,
                        }) {}
                    }
                }
                MainEvent::Resume { .. } => {
                    host.push_lifecycle_event(AppLifecycleEvent::Resumed);
                    host.request_redraw();
                }
                MainEvent::Pause => {
                    host.push_lifecycle_event(AppLifecycleEvent::Paused);
                }
                MainEvent::LowMemory => {
                    host.push_lifecycle_event(AppLifecycleEvent::MemoryWarning);
                }
                MainEvent::Destroy => {
                    android_log("run_android_app: MainEvent::Destroy received!");
                    host.bridge.terminate();
                    host.push_lifecycle_event(AppLifecycleEvent::Detached);
                    running = false;
                }
                _ => {}
            },
            _ => {}
        });

        if !running {
            break;
        }
        let now = Instant::now();
        let dt = (now - last_tick).as_secs_f32().clamp(0.001, 0.05);
        last_tick = now;

        #[cfg_attr(
            not(any(feature = "gpu-sdf", feature = "gpu-vello")),
            allow(unused_variables)
        )]
        let (still_anim, did_paint, cmds) = host.step_frame_with_redraw(dt);
        animating = still_anim;
        let platform_cmds = host.bridge.drain_commands();
        for pcmd in platform_cmds {
            match pcmd {
                AndroidPlatformCommand::RequestPermissions { request_id, .. } => {
                    tracing::debug!("Android platform command: RequestPermissions {request_id}");
                }
                AndroidPlatformCommand::OpenFileDialog { request_id, .. } => {
                    tracing::debug!("Android platform command: OpenFileDialog {request_id}");
                }
                AndroidPlatformCommand::SaveFileDialog { request_id, .. } => {
                    tracing::debug!("Android platform command: SaveFileDialog {request_id}");
                }
                _ => {}
            }
        }
        for cmd in cmds {
            match cmd {
                AndroidImeCommand::ShowSoftInput => app.show_soft_input(true),
                AndroidImeCommand::HideSoftInput => app.hide_soft_input(false),
                AndroidImeCommand::SyncTextState {
                    text,
                    selection_start,
                    selection_end,
                    compose_region,
                } => {
                    app.set_text_input_state(TextInputState {
                        text,
                        selection: TextSpan {
                            start: selection_start,
                            end: selection_end,
                        },
                        compose_region: compose_region.map(|(start, end)| TextSpan { start, end }),
                    });
                }
            }
        }
        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
        if did_paint && let Some(g) = gpu.as_mut() {
            let bg = host.tree.theme().background;
            if let Err(err) = g.render_frame(
                &host.scene,
                density,
                bg,
                &host.textures,
                &mut host.tree.text_engine,
            ) {
                let msg = format!("Android GPU render_frame failed: {err}");
                android_log(&format!("run_android_app: {msg}"));
                tracing::warn!("{msg}");
                host.record_error(msg);
            } else {
                static ONCE: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    android_log(
                        "run_android_app: first frame rendered successfully to ANativeWindow!",
                    );
                }
            }
        }
    }
    host.bridge.terminate();
}
