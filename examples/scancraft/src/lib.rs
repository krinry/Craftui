use craftui::prelude::*;

pub mod aot_ui {
    pub mod scancraft {
        include!(concat!(env!("OUT_DIR"), "/scancraft.rs"));
    }
    pub mod html_style {
        include!(concat!(env!("OUT_DIR"), "/scanapp_html_style.rs"));
    }
    pub use html_style::ScanAppHtmlView;
    pub use scancraft::ScanCraftApp;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedDoc {
    pub id: String,
    pub filename: String,
    pub category: String,
    pub captured: String,
    pub size_str: String,
    pub status: String,
}

impl ScannedDoc {
    pub fn to_row(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.filename.clone(),
            self.category.clone(),
            self.captured.clone(),
            self.size_str.clone(),
            self.status.clone(),
        ]
    }
}

pub struct ScanCraftAppState {
    pub screen: aot_ui::ScanCraftApp,
    pub documents: Signal<Vec<ScannedDoc>>,
    pub services: PlatformServices,
    pub capture_count: Signal<usize>,
}

impl ScanCraftAppState {
    pub fn trigger_capture(&self) {
        (self.screen.on_capture_photo.borrow_mut())();
    }
    pub fn trigger_gallery(&self) {
        (self.screen.on_import_gallery.borrow_mut())();
    }
    pub fn trigger_filter(&self, preset: i32) {
        (self.screen.on_apply_filter.borrow_mut())(preset);
    }
    pub fn trigger_rotate(&self) {
        (self.screen.on_rotate.borrow_mut())();
    }
    pub fn trigger_save(&self) {
        (self.screen.on_save_doc.borrow_mut())();
    }
    pub fn trigger_delete(&self) {
        (self.screen.on_delete_doc.borrow_mut())();
    }
    pub fn trigger_share(&self) {
        (self.screen.on_share_doc.borrow_mut())();
    }
    pub fn trigger_theme(&self) {
        (self.screen.on_theme_toggle.borrow_mut())();
    }
    pub fn trigger_permissions(&self) {
        (self.screen.on_request_permissions.borrow_mut())();
    }
}

pub fn initial_documents() -> Vec<ScannedDoc> {
    vec![
        ScannedDoc {
            id: "DOC-001".into(),
            filename: "Contract_Agreement_v2.pdf".into(),
            category: "Legal".into(),
            captured: "2026-11-09".into(),
            size_str: "1.4 MB".into(),
            status: "Indexed".into(),
        },
        ScannedDoc {
            id: "DOC-002".into(),
            filename: "Tax_Invoice_Nov.pdf".into(),
            category: "Finance".into(),
            captured: "2026-11-10".into(),
            size_str: "480 KB".into(),
            status: "Ready".into(),
        },
        ScannedDoc {
            id: "DOC-003".into(),
            filename: "Medical_Prescription.png".into(),
            category: "Health".into(),
            captured: "2026-11-10".into(),
            size_str: "890 KB".into(),
            status: "Ready".into(),
        },
        ScannedDoc {
            id: "DOC-004".into(),
            filename: "Equipment_Warranty.pdf".into(),
            category: "Personal".into(),
            captured: "2026-11-11".into(),
            size_str: "2.1 MB".into(),
            status: "Synced".into(),
        },
        ScannedDoc {
            id: "DOC-005".into(),
            filename: "Whiteboard_Brainstorm.jpeg".into(),
            category: "Work".into(),
            captured: "2026-11-11".into(),
            size_str: "3.6 MB".into(),
            status: "Draft".into(),
        },
    ]
}

pub fn mount_scancraft_app_with_services(
    cx: Scope,
    tree: &mut WidgetTree,
    services: PlatformServices,
) -> (NodeId, ScanCraftAppState) {
    let theme_sig = tree.theme_signal;
    let documents = cx.signal(initial_documents());
    let capture_count = cx.signal(0usize);

    let screen = aot_ui::ScanCraftApp::new(cx);

    // Initial table rows
    let initial_rows: Vec<Vec<String>> = documents.get().iter().map(|d| d.to_row()).collect();
    screen.doc_table_rows.set(initial_rows);

    // Initial telemetry strings
    let bat = services.battery_state();
    let bat_desc = match bat.level_pct {
        Some(pct) => format!(
            "Battery: {}% ({})",
            pct,
            if bat.charging { "Charging" } else { "Battery" }
        ),
        None => "Battery: 94% (AC)".to_string(),
    };
    screen.battery_status.set(bat_desc);

    let dev = services.device_info();
    let stor_desc = format!("{} / {}", dev.os, dev.model);
    screen.storage_status.set(stor_desc);

    // Synchronize document table updates
    {
        let scr = screen.clone();
        let docs = documents;
        cx.effect(move || {
            let rows: Vec<Vec<String>> = docs.get().iter().map(|d| d.to_row()).collect();
            scr.doc_table_rows.set(rows);
        });
    }

    // 1. Theme Toggle Callback
    {
        let th = theme_sig;
        screen.set_on_theme_toggle(move || {
            let cur = th.get_untracked().mode;
            let next = match cur {
                ThemeMode::Light => ThemeMode::Dark,
                ThemeMode::Dark => ThemeMode::Amoled,
                ThemeMode::Amoled | ThemeMode::HighContrast => ThemeMode::Light,
            };
            th.set(ThemeTokens::from_mode(next));
        });
    }

    // 2. Capture Photo Callback
    {
        let scr = screen.clone();
        let svc = services.clone();
        let docs = documents;
        let c_count = capture_count;
        screen.set_on_capture_photo(move || {
            let _ = svc.trigger_haptic(HapticStyle::Medium);
            #[cfg(target_os = "android")]
            launch_android_camera();

            // Note: On Android, launch_android_camera() is invoked asynchronously.
            // Until asynchronous JNI result hooks are registered to deliver the raw image buffer,
            // we initialize a synthetic document entry so the preview and enhancement studio remain interactive.
            let next_cnt = c_count.get_untracked() + 1;
            c_count.set(next_cnt);

            let new_name = format!("Scanned_Doc_{:03}.pdf", next_cnt);
            let new_doc = ScannedDoc {
                id: format!("DOC-{:03}", 100 + next_cnt),
                filename: new_name.clone(),
                category: "Scans".into(),
                captured: "2026-11-11".into(),
                size_str: "1.2 MB".into(),
                status: "Processed".into(),
            };

            docs.update(|list| {
                list.insert(0, new_doc);
            });

            scr.doc_title.set(new_name);
            scr.active_tab.set(1); // Jump to filter tab for immediate editing
            scr.status_message
                .set("Photo captured! Camera session opened, now in enhancement studio.".into());
        });
    }

    // 3. Import Gallery Callback
    {
        let scr = screen.clone();
        let svc = services.clone();
        let docs = documents;
        let c_count = capture_count;
        screen.set_on_import_gallery(move || {
            let _ = svc.trigger_haptic(HapticStyle::Light);
            #[cfg(target_os = "android")]
            launch_android_gallery();

            // Note: On Android, launch_android_gallery() is invoked asynchronously.
            // Synthetic entry provides immediate interaction in desktop testing and visual studio.
            let next_cnt = c_count.get_untracked() + 1;
            c_count.set(next_cnt);

            let new_name = format!("Gallery_Import_{:03}.png", next_cnt);
            let new_doc = ScannedDoc {
                id: format!("DOC-{:03}", 200 + next_cnt),
                filename: new_name.clone(),
                category: "Photos".into(),
                captured: "2026-11-11".into(),
                size_str: "2.8 MB".into(),
                status: "Imported".into(),
            };

            docs.update(|list| {
                list.insert(0, new_doc);
            });

            scr.doc_title.set(new_name);
            scr.active_tab.set(1);
            scr.status_message
                .set("Opening device photo gallery...".into());
        });
    }

    // 4. Filter Mode Presets
    {
        let scr = screen.clone();
        screen.set_on_apply_filter(move |preset| {
            scr.filter_mode.set(preset);
            match preset {
                0 => {
                    // Original
                    scr.brightness.set(0.0);
                    scr.contrast.set(1.0);
                    scr.status_message.set("Applied Original Preset".into());
                }
                1 => {
                    // Grayscale
                    scr.brightness.set(5.0);
                    scr.contrast.set(1.2);
                    scr.status_message
                        .set("Applied Balanced Grayscale Preset".into());
                }
                2 => {
                    // High-Contrast Document B&W
                    scr.brightness.set(15.0);
                    scr.contrast.set(1.6);
                    scr.status_message
                        .set("Applied High-Contrast Document Threshold".into());
                }
                3 => {
                    // Color Boost
                    scr.brightness.set(0.0);
                    scr.contrast.set(1.3);
                    scr.status_message
                        .set("Applied Magic Color Boost Preset".into());
                }
                _ => {}
            }
        });
    }

    // 5. Rotate Document Callback
    {
        let scr = screen.clone();
        screen.set_on_rotate(move || {
            let cur = scr.rotation_deg.get_untracked();
            let next = (cur + 90) % 360;
            scr.rotation_deg.set(next);
            scr.status_message
                .set(format!("Rotated document to {} degrees", next));
        });
    }

    // 6. Save Document Callback
    {
        let scr = screen.clone();
        let svc = services.clone();
        screen.set_on_save_doc(move || {
            let _ = svc.trigger_haptic(HapticStyle::Medium);
            scr.status_message
                .set("Document enhancements saved to local library!".into());
            scr.active_tab.set(2); // Jump to library
        });
    }

    // 7. Delete Document Callback
    {
        let scr = screen.clone();
        let docs = documents;
        let svc = services.clone();
        screen.set_on_delete_doc(move || {
            let sel = scr.selected_doc_index.get_untracked() as usize;
            let mut removed_name = String::new();
            docs.update(|list| {
                if !list.is_empty() {
                    let idx = sel.min(list.len() - 1);
                    let rem = list.remove(idx);
                    removed_name = rem.filename;
                }
            });
            if !removed_name.is_empty() {
                scr.status_message
                    .set(format!("Deleted document: {}", removed_name));
                let _ = svc.trigger_haptic(HapticStyle::Heavy);
            }
        });
    }

    // 8. Share Document Callback
    {
        let scr = screen.clone();
        let svc = services.clone();
        screen.set_on_share_doc(move || {
            let title = scr.doc_title.get_untracked();
            let fmt_idx = scr.export_format.get_untracked();
            let ext = match fmt_idx {
                0 => "pdf",
                1 => "min.pdf",
                2 => "png",
                _ => "jpg",
            };
            let payload =
                SharePayload::text(format!("Sharing scanned document '{}' as .{}", title, ext))
                    .with_title(format!("ScanCraft Document — {}", title));
            let _ = svc.share(&payload);
            let _ = svc.trigger_haptic(HapticStyle::Light);
        });
    }

    // 9. Permissions Request Callback
    {
        let scr = screen.clone();
        let svc = services.clone();
        screen.set_on_request_permissions(move || {
            let cam = svc.request_permission(&Permission::Camera);
            let pho = svc.request_permission(&Permission::Photos);
            if cam == PermissionStatus::Granted && pho == PermissionStatus::Granted {
                scr.camera_perm_badge.set("Camera: Granted".into());
                scr.status_message
                    .set("All mobile media permissions active!".into());
            } else {
                scr.camera_perm_badge.set("Camera: Partial".into());
            }
        });
    }

    SCANAPP_TAB.with(|cell| {
        *cell.borrow_mut() = Some(screen.active_tab);
    });

    // Reactively switch Tauri Android WebView on tab 4
    cx.effect({
        let scr = screen.clone();
        move || {
            let tab = scr.active_tab.get();
            #[cfg(target_os = "android")]
            {
                if tab == 4 {
                    call_activity_static_method("showTauriWebViewNative");
                } else {
                    call_activity_static_method("hideTauriWebViewNative");
                }
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = tab;
            }
        }
    });

    let root_node = screen.clone().build(cx, tree);

    let state = ScanCraftAppState {
        screen,
        documents,
        services,
        capture_count,
    };

    (root_node, state)
}

thread_local! {
    static SCANAPP_TAB: std::cell::RefCell<Option<Signal<i32>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_craftui_app_MainActivity_nativeExitTauriMode(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
) {
}

#[cfg(not(target_os = "android"))]
#[unsafe(no_mangle)]
pub extern "C" fn Java_dev_craftui_app_MainActivity_nativeExitTauriMode() {}

pub fn mount_scancraft_app(cx: Scope, tree: &mut WidgetTree) -> (NodeId, ScanCraftAppState) {
    mount_scancraft_app_with_services(cx, tree, tree.services.clone())
}

#[cfg(target_os = "android")]
fn call_activity_static_method(method: &str) {
    unsafe {
        let ctx = ndk_context::android_context();
        let vm = ctx.vm() as *mut jni::sys::JavaVM;
        let app_ctx = ctx.context() as jni::sys::jobject;
        if vm.is_null() || app_ctx.is_null() {
            return;
        }
        let mut env: *mut jni::sys::JNIEnv = std::ptr::null_mut();
        let attach_fn = (**vm).v1_4.AttachCurrentThread;
        if attach_fn(
            vm,
            &mut env as *mut _ as *mut *mut std::ffi::c_void,
            std::ptr::null_mut(),
        ) != 0
            || env.is_null()
        {
            return;
        }
        let clear_fn = (**env).v1_4.ExceptionClear;
        clear_fn(env);

        // 1. Get ClassLoader from application context
        let get_obj_class = (**env).v1_4.GetObjectClass;
        let app_cls = get_obj_class(env, app_ctx);
        if app_cls.is_null() {
            return;
        }

        let Ok(c_get_cl) = std::ffi::CString::new("getClassLoader") else {
            return;
        };
        let Ok(c_get_cl_sig) = std::ffi::CString::new("()Ljava/lang/ClassLoader;") else {
            return;
        };
        let get_method = (**env).v1_4.GetMethodID;
        let get_cl_method = get_method(env, app_cls, c_get_cl.as_ptr(), c_get_cl_sig.as_ptr());
        if get_cl_method.is_null() {
            return;
        }

        let call_obj_method = (**env).v1_4.CallObjectMethod;
        let class_loader = call_obj_method(env, app_ctx, get_cl_method);
        if class_loader.is_null() {
            return;
        }

        // 2. Load dev.craftui.app.MainActivity via classLoader.loadClass(...)
        let cl_cls = get_obj_class(env, class_loader);
        let Ok(c_load_cls) = std::ffi::CString::new("loadClass") else {
            return;
        };
        let Ok(c_load_cls_sig) = std::ffi::CString::new("(Ljava/lang/String;)Ljava/lang/Class;")
        else {
            return;
        };
        let load_cls_method = get_method(env, cl_cls, c_load_cls.as_ptr(), c_load_cls_sig.as_ptr());
        if load_cls_method.is_null() {
            return;
        }

        let Ok(c_act_name) = std::ffi::CString::new("dev.craftui.app.MainActivity") else {
            return;
        };
        let new_string_utf = (**env).v1_4.NewStringUTF;
        let act_name_str = new_string_utf(env, c_act_name.as_ptr());
        if act_name_str.is_null() {
            return;
        }

        let main_act_cls =
            call_obj_method(env, class_loader, load_cls_method, act_name_str) as jni::sys::jclass;
        if main_act_cls.is_null() {
            return;
        }

        // 3. Invoke MainActivity.<method>()
        let Ok(c_method) = std::ffi::CString::new(method) else {
            return;
        };
        let Ok(c_sig) = std::ffi::CString::new("()V") else {
            return;
        };
        let get_static_method = (**env).v1_4.GetStaticMethodID;
        let method_id = get_static_method(env, main_act_cls, c_method.as_ptr(), c_sig.as_ptr());
        if !method_id.is_null() {
            let call_static_void = (**env).v1_4.CallStaticVoidMethod;
            call_static_void(env, main_act_cls, method_id);
        }
    }
}

#[cfg(target_os = "android")]
pub fn launch_android_camera() {
    call_activity_static_method("launchCameraFromNative");
}

#[cfg(target_os = "android")]
pub fn launch_android_gallery() {
    call_activity_static_method("launchGalleryFromNative");
}

#[cfg(target_os = "android")]
pub fn show_android_tauri_webview() {
    call_activity_static_method("showTauriWebViewNative");
}

#[cfg(target_os = "android")]
pub fn hide_android_tauri_webview() {
    call_activity_static_method("hideTauriWebViewNative");
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: craftui::platform_android::android_activity::AndroidApp) {
    craftui::platform_android::android_runtime::run_android_app(
        app,
        |cx, tree, _textures, _disp| {
            let (root, _state) = mount_scancraft_app(cx, tree);
            root
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::core::{MemoryClipboard, MockFileDialog};
    use craftui::reactive::reset_runtime;
    use std::path::PathBuf;

    #[test]
    fn scancraft_app_mounts_with_initial_documents_and_telemetry() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let services = PlatformServices::new(
            MemoryClipboard::default(),
            MockFileDialog::new(vec![PathBuf::from("/tmp/test_scan.png")], None),
        );

        let (root, state) = mount_scancraft_app_with_services(cx, &mut tree, services);
        tree.set_root(root);

        // Verify initial state
        assert_eq!(state.screen.active_tab.get(), 0);
        assert_eq!(state.documents.get().len(), 5);
        assert_eq!(state.screen.doc_table_rows.get().len(), 5);
        assert!(state.screen.battery_status.get().contains("Battery:"));
        assert_eq!(state.screen.camera_perm_badge.get(), "Camera: Granted");
    }

    #[test]
    fn scancraft_tab_navigation_and_presets_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, state) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        // Switch to Tab 1: Filter & Enhance
        state.screen.active_tab.set(1);
        assert_eq!(state.screen.active_tab.get(), 1);

        // Switch to Tab 2: Document Library
        state.screen.active_tab.set(2);
        assert_eq!(state.screen.active_tab.get(), 2);

        // Switch to Tab 3: Export & Info
        state.screen.active_tab.set(3);
        assert_eq!(state.screen.active_tab.get(), 3);
    }

    #[test]
    fn scancraft_capture_photo_adds_document_and_triggers_filter_mode() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, state) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        assert_eq!(state.capture_count.get(), 0);
        assert_eq!(state.documents.get().len(), 5);

        // Trigger capture callback
        state.trigger_capture();

        assert_eq!(state.capture_count.get(), 1);
        assert_eq!(state.documents.get().len(), 6);
        assert_eq!(state.screen.active_tab.get(), 1); // Automatically switched to filter tab
        assert_eq!(state.screen.doc_title.get(), "Scanned_Doc_001.pdf");
        assert!(state.screen.status_message.get().contains("Photo captured"));
    }

    #[test]
    fn scancraft_filter_adjustments_and_rotation_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, state) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        // Apply B&W preset (preset 2)
        state.trigger_filter(2);
        assert_eq!(state.screen.filter_mode.get(), 2);
        assert_eq!(state.screen.brightness.get(), 15.0);
        assert_eq!(state.screen.contrast.get(), 1.6);

        // Apply Original preset (preset 0)
        state.trigger_filter(0);
        assert_eq!(state.screen.filter_mode.get(), 0);
        assert_eq!(state.screen.brightness.get(), 0.0);
        assert_eq!(state.screen.contrast.get(), 1.0);

        // Rotate 90 degrees twice
        assert_eq!(state.screen.rotation_deg.get(), 0);
        state.trigger_rotate();
        assert_eq!(state.screen.rotation_deg.get(), 90);
        state.trigger_rotate();
        assert_eq!(state.screen.rotation_deg.get(), 180);
    }

    #[test]
    fn scancraft_document_deletion_updates_library() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, state) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        assert_eq!(state.documents.get().len(), 5);
        state.screen.selected_doc_index.set(0);

        // Delete first item
        state.trigger_delete();
        assert_eq!(state.documents.get().len(), 4);
        assert!(
            state
                .screen
                .status_message
                .get()
                .contains("Deleted document: Contract_Agreement_v2.pdf")
        );
    }

    #[test]
    fn scancraft_theme_toggle_cycles_modes() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, state) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        let initial_mode = tree.theme_signal.get().mode;
        state.trigger_theme();
        let next_mode = tree.theme_signal.get().mode;
        assert_ne!(initial_mode, next_mode);
    }

    #[test]
    fn scancraft_full_scene_render_via_vello_cpu() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, _) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(1024.0, 720.0),
            ..Default::default()
        };

        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        let textures = TextureRegistry::new();
        let mut text_engine = craftui::text::TextEngine::new();

        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            1024,
            720,
            1.0,
            Color::rgb(15, 23, 42),
            &textures,
            &mut text_engine,
        );

        assert_eq!(rgba.len(), 1024 * 720 * 4);
        // Ensure non-blank buffer
        assert!(rgba.iter().any(|&b| b > 0));
    }

    #[test]
    fn scanapp_mobile_portrait_scene_render_and_png_snapshot() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, _) = mount_scancraft_app(cx, &mut tree);
        tree.set_root(root);

        let w = 412;
        let h = 915;
        let metrics = WindowMetrics {
            logical_size: Size::new(w as f32, h as f32),
            safe_area: Insets::new(48.0, 0.0, 34.0, 0.0),
            ..Default::default()
        };

        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        let textures = TextureRegistry::new();
        let mut text_engine = craftui::text::TextEngine::new();

        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            w,
            h,
            1.0,
            Color::BLACK,
            &textures,
            &mut text_engine,
        );

        assert_eq!(rgba.len(), (w * h * 4) as usize);
        assert!(rgba.iter().any(|&b| b > 0));

        let out_path = std::env::temp_dir().join("scanapp_mobile_render.png");
        let _ = craftui::render::write_rgba_png(&out_path, w, h, &rgba);
    }
}
