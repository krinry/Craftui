use super::*;
use craftui_core::*;
use craftui_layout::LayoutStyle;
use craftui_reactive::reset_runtime;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct DummyCvPixelBuffer {
    frame: Option<CvPixelBufferFrame>,
}

impl CvPixelBufferBridge for DummyCvPixelBuffer {
    fn acquire_latest_frame(&mut self) -> Option<CvPixelBufferFrame> {
        self.frame.take()
    }
}

extern "C" fn build_sample_ios_scene(host: *mut IosAppHost, user_data: *mut c_void) {
    // SAFETY: invoked synchronously inside launch test with valid `host` and `*mut bool` signal handle.
    let host = unsafe { &mut *host };
    let clicked_ptr = user_data as *mut std::sync::atomic::AtomicBool;
    host.mount_root(|cx, tree, _, _| {
        let root = tree.column(cx);
        let txt = tree.text_input(cx, "", "Type here", None);
        let btn = tree.button(cx, "Submit", move || {
            if !clicked_ptr.is_null() {
                // SAFETY: test atomic bool outlives C-ABI invocation.
                unsafe { (*clicked_ptr).store(true, std::sync::atomic::Ordering::SeqCst) };
            }
        });
        tree.set_layout(
            txt,
            LayoutStyle::row().with_size(Length::Dp(200.0), Length::Dp(40.0)),
        );
        tree.set_layout(
            btn,
            LayoutStyle::row().with_size(Length::Dp(120.0), Length::Dp(44.0)),
        );
        tree.append_child(root, txt);
        tree.append_child(root, btn);
        root
    });
}

#[test]
fn ios_safe_area_keyboard_and_touch_flow() {
    reset_runtime();
    let mut host = IosAppHost::new(UiDispatcher::new(), 393.0, 852.0, 3.0);
    // Dynamic Island + Home indicator safe area
    host.set_safe_area(IosSafeAreaInsets {
        top: 59.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    });
    host.set_keyboard_frame(IosKeyboardFrame {
        overlap_height_pt: 312.0,
        visible: true,
    });
    let eff = host.tree.metrics.effective_safe_area();
    assert_eq!(eff.top, 59.0);
    assert_eq!(eff.bottom, 312.0);

    let clicked = host.scope.signal(false);
    let btn = host
        .tree
        .button(host.scope, "Confirm", move || clicked.set(true));
    let mut ls = LayoutStyle::row().with_size(Length::Dp(200.0), Length::Dp(48.0));
    ls.margin = Insets::new(80.0, 0.0, 0.0, 20.0);
    host.tree.set_layout(btn, ls);
    host.tree.set_root(btn);
    let _ = host.display_link_tick(0.016);

    let c = host.tree.rect(btn).center();
    host.push_ui_touch(1, PointerPhase::Down, c.x, c.y, 10);
    host.push_ui_touch(1, PointerPhase::Up, c.x, c.y, 35);
    assert!(clicked.get());

    // CVPixelBuffer BGRA -> RGBA external texture sync
    let mut bridge = DummyCvPixelBuffer {
        frame: Some(CvPixelBufferFrame {
            width: 2,
            height: 1,
            bytes_per_row: 8,
            format: CvPixelBufferFormat::Bgra8,
            bytes: vec![10, 20, 200, 255, 0, 180, 0, 255], // BGRx -> RGBx: (200, 20, 10)
        }),
    };
    let mut tex_slot = None;
    let tid = host
        .sync_cv_pixel_buffer(&mut bridge, &mut tex_slot)
        .expect("CVPixelBuffer converted and registered");
    let handle = host.textures.get(tid).unwrap();
    let (_, _, _, rgba) = handle.snapshot();
    assert_eq!(&rgba[0..4], &[200, 20, 10, 255]);
}

#[test]
fn ios_c_abi_lifecycle_entrypoints_work_end_to_end() {
    reset_runtime();
    let clicked = std::sync::atomic::AtomicBool::new(false);
    let layer = IosMetalLayerDescriptor {
        ca_metal_layer: std::ptr::null_mut(),
        width_pt: 390.0,
        height_pt: 844.0,
        contents_scale: 3.0,
    };
    // SAFETY: Passing valid atomic pointer and C builder function within test lifetime.
    let app = unsafe {
        craftui_ios_app_launch(
            layer,
            Some(build_sample_ios_scene),
            (&clicked as *const std::sync::atomic::AtomicBool) as *mut c_void,
        )
    };
    assert!(!app.is_null());
    unsafe {
        craftui_ios_app_set_safe_area(app, 47.0, 0.0, 34.0, 0.0);
        craftui_ios_app_set_keyboard(app, 0.0, false);
        craftui_ios_app_resize(app, 400.0, 860.0, 3.0);
        let _ = craftui_ios_app_tick(app, 0.016);
        // Tap first TextInput (centered around y=20)
        let _ = craftui_ios_app_touch(app, 1, 0, 50.0, 20.0, 10);
        assert!(craftui_ios_app_touch(app, 1, 2, 50.0, 20.0, 30));
        let flags = craftui_ios_app_tick(app, 0.016);
        assert_ne!(flags & 0b10, 0, "expected keyboard requested");
        // Subsequent idle ticks neither force Metal redraw nor re-emit duplicate SyncText
        let host_ref = &mut *app;
        let now_ms = craftui_core::monotonic_ms();
        host_ref.tree.reset_caret_blink(now_ms);
        let (anim_idle, painted_idle, cmds_idle) =
            host_ref.display_link_tick_with_redraw(0.016, now_ms + 50);
        assert!(!anim_idle && !painted_idle && cmds_idle.is_empty());
        assert!(craftui_ios_app_last_error(app).is_null());
        let marked = b"nihon";
        assert!(craftui_ios_app_set_marked_text(
            app,
            marked.as_ptr(),
            marked.len(),
            5
        ));
        let msg = b"hello";
        assert!(craftui_ios_app_insert_text(app, msg.as_ptr(), msg.len()));
        // Attaching a null Metal layer fails gracefully and exposes last_error + tick bit 2
        assert_eq!(craftui_ios_app_attach_metal(app, layer), -1);
        let err_ptr = craftui_ios_app_last_error(app);
        assert!(!err_ptr.is_null());
        let err_str = std::ffi::CStr::from_ptr(err_ptr).to_str().unwrap();
        assert!(err_str.contains("null"), "unexpected error: {err_str}");
        let err_flags = craftui_ios_app_tick(app, 0.016);
        assert_ne!(err_flags & 0b100, 0, "expected error bit in tick flags");
        craftui_ios_app_clear_error(app);
        assert!(craftui_ios_app_last_error(app).is_null());
        assert_eq!(craftui_ios_app_tick(app, 0.016) & 0b100, 0);
        // Tap Button below TextInput (y around 60)
        assert!(craftui_ios_app_touch(app, 2, 0, 40.0, 60.0, 50));
        assert!(craftui_ios_app_touch(app, 2, 2, 40.0, 60.0, 70));
        assert!(clicked.load(std::sync::atomic::Ordering::SeqCst));
        craftui_ios_app_destroy(app);
    }
}

#[test]
fn ios_c_abi_permissions_commands_haptics_share_and_telemetry_work() {
    reset_runtime();
    let layer = IosMetalLayerDescriptor {
        ca_metal_layer: std::ptr::null_mut(),
        width_pt: 393.0,
        height_pt: 852.0,
        contents_scale: 3.0,
    };
    let app = unsafe { craftui_ios_app_launch(layer, None, std::ptr::null_mut()) };
    assert!(!app.is_null());

    unsafe {
        let cam_key = b"camera";
        assert_eq!(
            craftui_ios_app_permission_status_from_key(app, cam_key.as_ptr(), cam_key.len()),
            0
        );
        craftui_ios_app_set_permission_status(app, cam_key.as_ptr(), cam_key.len(), 1);
        assert_eq!(
            craftui_ios_app_permission_status_from_key(app, cam_key.as_ptr(), cam_key.len()),
            1
        );

        craftui_ios_app_push_lifecycle(app, 0);
        craftui_ios_app_push_lifecycle(app, 4);
        craftui_ios_app_set_network_status(app, 2);
        craftui_ios_app_set_battery(app, 91, true);

        let host = &mut *app;
        assert_eq!(host.tree.services.network_status(), NetworkStatus::Wifi);
        assert_eq!(
            host.tree.services.battery_state(),
            BatteryState {
                level_pct: Some(91),
                charging: true,
            }
        );
        assert_eq!(
            host.bridge.lifecycle_history(),
            vec![AppLifecycleEvent::Resumed, AppLifecycleEvent::MemoryWarning]
        );

        assert!(host.tree.services.trigger_haptic(HapticStyle::Success));
        assert!(
            host.tree
                .services
                .share(&SharePayload::url("https://craftui.dev").with_title("CraftUI"))
        );
        assert!(host.tree.services.open_app_settings());
        assert!(host.tree.services.set_secret("api_key", "ios-token"));
        assert_eq!(
            host.tree.services.get_secret("api_key").as_deref(),
            Some("ios-token")
        );
        assert!(host.tree.services.delete_secret("api_key"));

        // Poll first command with undersized buffer -> negative required size, not popped
        let needed = craftui_ios_app_poll_command_json(app, std::ptr::null_mut(), 0);
        assert!(needed < 0);
        let mut buf = vec![0u8; (-needed) as usize];
        let written = craftui_ios_app_poll_command_json(app, buf.as_mut_ptr(), buf.len());
        assert_eq!(written, -needed);
        let cmd0 = std::str::from_utf8(&buf).unwrap();
        assert!(cmd0.contains("\"haptic\"") && cmd0.contains("success"));

        // Drain remaining queued commands
        let mut cmds = Vec::new();
        let mut large_buf = vec![0u8; 512];
        loop {
            let n = craftui_ios_app_poll_command_json(app, large_buf.as_mut_ptr(), large_buf.len());
            if n == 0 {
                break;
            }
            assert!(n > 0);
            cmds.push(String::from_utf8_lossy(&large_buf[..n as usize]).into_owned());
        }
        assert!(cmds.iter().any(|s| s.contains("\"share\"")));
        assert!(cmds.iter().any(|s| s.contains("open_app_settings")));
        assert!(cmds.iter().any(|s| s.contains("sync_secret")));
        assert!(cmds.iter().any(|s| s.contains("delete_secret")));

        // Async permission request unblocked via C-ABI deliver call
        let disp = host.dispatcher.clone();
        let result_slot = Arc::new(Mutex::new(None));
        let rs_c = result_slot.clone();
        host.tree.services.request_permission_async(
            Permission::Biometrics,
            disp.clone(),
            move |st| {
                *rs_c.lock().unwrap() = Some(st);
            },
        );

        let mut bio_req_id = None;
        for _ in 0..100 {
            let n = craftui_ios_app_poll_command_json(app, large_buf.as_mut_ptr(), large_buf.len());
            if n > 0 {
                let s = std::str::from_utf8(&large_buf[..n as usize]).unwrap();
                if s.contains("biometrics")
                    && let Some(pos) = s.find("\"request_id\":")
                {
                    let rest = &s[pos + "\"request_id\":".len()..];
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    bio_req_id = digits.parse::<u64>().ok();
                }
            }
            if bio_req_id.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let rid = bio_req_id.expect("request_permission biometrics emitted");
        let bio_key = b"biometrics";
        craftui_ios_app_deliver_permission_result(
            app,
            rid,
            bio_key.as_ptr(),
            bio_key.len(),
            1,
            false,
        );
        for _ in 0..100 {
            let _ = disp.drain();
            if result_slot.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            *result_slot.lock().unwrap(),
            Some(PermissionStatus::Granted)
        );
        craftui_ios_app_destroy(app);
    }
}

#[test]
fn test_ios_method_channel_plugin_dispatch() {
    struct UrlLauncherStub;
    impl craftui_core::CraftPlugin for UrlLauncherStub {
        fn name(&self) -> &str {
            "plugins.flutter.io/url_launcher"
        }

        fn handle(
            &self,
            call: &craftui_core::MethodCall,
        ) -> craftui_core::Result<serde_json::Value> {
            match call.method.as_str() {
                "canLaunch" => Ok(serde_json::json!({
                    "canLaunch": call.args.get("url").and_then(|v| v.as_str()).is_some()
                })),
                other => Err(craftui_core::CraftError::Platform(format!(
                    "unsupported method `{other}`"
                ))),
            }
        }
    }

    let host = IosAppHost::new(UiDispatcher::new(), 390.0, 844.0, 3.0);
    host.register_plugin(UrlLauncherStub);
    let out = host
        .invoke_plugin(
            "plugins.flutter.io/url_launcher",
            "canLaunch",
            serde_json::json!({ "url": "https://craftui.dev" }),
        )
        .expect("registered ios plugin should respond");
    assert_eq!(out, serde_json::json!({ "canLaunch": true }));

    // Async Swift C-FFI MethodChannel roundtrip
    let app = craftui_ios_app_create(390.0, 844.0, 3.0);
    unsafe {
        let host_ref = &*app;
        let bridge = host_ref.bridge();
        let handle = std::thread::spawn(move || {
            bridge.invoke_method_channel(
                "dev.craftui.biometrics",
                "authenticate",
                serde_json::json!({ "reason": "Unlock vault" }),
            )
        });

        let mut req_id = None;
        let mut buf = [0u8; 512];
        for _ in 0..50 {
            let n = craftui_ios_app_poll_command_json(app, buf.as_mut_ptr(), buf.len());
            if n > 0 {
                let s = std::str::from_utf8(&buf[..n as usize]).unwrap();
                if s.contains("\"method_channel\"")
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
                {
                    assert_eq!(v["channel"], "dev.craftui.biometrics");
                    assert_eq!(v["method"], "authenticate");
                    req_id = v["request_id"].as_u64();
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let rid = req_id.expect("iOS method_channel command must be polled as JSON");
        let payload = br#"{"verified":true}"#;
        craftui_ios_app_deliver_method_channel_result(
            app,
            rid,
            true,
            payload.as_ptr(),
            payload.len(),
        );
        let native_res = handle
            .join()
            .unwrap()
            .expect("async iOS method channel result");
        assert_eq!(native_res, serde_json::json!({ "verified": true }));
        craftui_ios_app_destroy(app);
    }
}
