use super::*;
use crate::dev::{CheckStats, check_cui_paths, render_cui_to_png};
use crate::doctor::{collect_doctor_report, format_doctor_json};
use crate::manifest::AppManifest;
use crate::permissions::format_permissions_list_json;
use crate::platform::android::emit_android_template_with_manifest;
use crate::platform::ios::emit_ios_template_with_manifest;
use crate::platform::web::{build_http_response, emit_web_bundle};
use crate::templates::{init_manifest_in_dir, scaffold_project_with_template};
use craftui_reactive::reset_runtime;

#[test]
fn scaffolds_all_templates_init_permissions_and_cross_platform_bundles() {
    reset_runtime();
    let base_tmp = std::env::temp_dir().join(format!(
        "craftui_cli_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    for (tmpl, sub) in [
        (ProjectTemplate::Counter, "counter_demo"),
        (ProjectTemplate::Dashboard, "dash_demo"),
        (ProjectTemplate::Mobile, "mobile_demo"),
    ] {
        let dir = base_tmp.join(sub);
        scaffold_project_with_template(sub, &dir, tmpl).unwrap();
        let cui_path = dir.join("ui/main.cui");
        assert!(cui_path.exists());
        assert!(dir.join("craftui.toml").exists());

        let stats = check_cui_paths(&cui_path, false).unwrap();
        assert_eq!(
            stats,
            CheckStats {
                files: 1,
                warnings: 0,
                errors: 0
            },
            "template {sub} produced .cui diagnostics"
        );

        let loaded_manifest = AppManifest::load_file(&dir.join("craftui.toml")).unwrap();
        assert!(loaded_manifest.validate().iter().all(|d| !d.is_error));
    }

    // Test `init` manifest creation + no-overwrite guard
    let existing_dir = base_tmp.join("existing_crate");
    std::fs::create_dir_all(&existing_dir).unwrap();
    std::fs::write(
        existing_dir.join("Cargo.toml"),
        "[package]\nname = \"acme-wallet\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let inited = init_manifest_in_dir(&existing_dir, None, None, false).unwrap();
    let m_inited = AppManifest::load_file(&inited).unwrap();
    assert_eq!(m_inited.app.name, "acme-wallet");
    assert_eq!(m_inited.app.id, "dev.craftui.acme_wallet");
    assert!(init_manifest_in_dir(&existing_dir, None, None, false).is_err());

    // Test mobile manifest-driven Android + iOS codegen
    let mobile_dir = base_tmp.join("mobile_demo");
    let mobile_manifest = AppManifest::load_file(&mobile_dir.join("craftui.toml")).unwrap();

    let android_out = mobile_dir.join("dist/android");
    emit_android_template_with_manifest(
        &android_out,
        "mobile_demo",
        "mobile_demo",
        None,
        Some(&mobile_manifest),
    )
    .unwrap();
    let manifest_xml =
        std::fs::read_to_string(android_out.join("app/src/main/AndroidManifest.xml")).unwrap();
    assert!(manifest_xml.contains("android.permission.CAMERA"));
    assert!(manifest_xml.contains("android.permission.POST_NOTIFICATIONS"));
    assert!(manifest_xml.contains(
        "<uses-feature android:name=\"android.hardware.camera\" android:required=\"false\" />"
    ));
    let act = std::fs::read_to_string(
        android_out.join("app/src/main/kotlin/dev/craftui/app/MainActivity.kt"),
    )
    .unwrap();
    assert!(act.contains("RequestMultiplePermissions"));
    assert!(act.contains("OpenMultipleDocuments"));
    assert!(act.contains("queryNetworkClassification"));

    let ios_out = mobile_dir.join("dist/ios");
    emit_ios_template_with_manifest(
        &ios_out,
        "mobile_demo",
        "mobile_demo",
        Some(&mobile_manifest),
    )
    .unwrap();
    let hdr = std::fs::read_to_string(ios_out.join("CraftUI.h")).unwrap();
    let vc = std::fs::read_to_string(ios_out.join("CraftUIViewController.swift")).unwrap();
    let perms_swift = std::fs::read_to_string(ios_out.join("CraftUIPermissions.swift")).unwrap();
    let plist = std::fs::read_to_string(ios_out.join("Info.plist")).unwrap();
    assert!(hdr.contains("craftui_ios_app_poll_command_json"));
    assert!(hdr.contains("craftui_ios_app_deliver_permission_result"));
    assert!(vc.contains("NWPathMonitor") && vc.contains("UIImpactFeedbackGenerator"));
    assert!(perms_swift.contains("CLLocationManager") && perms_swift.contains("LAContext"));
    assert!(plist.contains("NSCameraUsageDescription"));
    assert!(plist.contains("NSFaceIDUsageDescription"));
    assert!(plist.contains("remote-notification"));

    // Test Web bundle & static MIME server helper (WASM + traversal protection)
    let web_out = mobile_dir.join("dist/web");
    emit_web_bundle(&web_out, "mobile_demo", None).unwrap();
    std::fs::write(web_out.join("app.wasm"), b"\0asm\x01\x00\x00\x00").unwrap();
    let ok_html = String::from_utf8_lossy(&build_http_response(&web_out, "/")).into_owned();
    assert!(ok_html.starts_with("HTTP/1.1 200 OK") && ok_html.contains("text/html"));
    let ok_wasm =
        String::from_utf8_lossy(&build_http_response(&web_out, "/app.wasm?v=1")).into_owned();
    assert!(ok_wasm.contains("application/wasm"));
    let blocked =
        String::from_utf8_lossy(&build_http_response(&web_out, "/../Cargo.toml")).into_owned();
    assert!(blocked.starts_with("HTTP/1.1 403 Forbidden"));

    // Test permissions JSON lists all 35 permissions cleanly
    let perms_json = format_permissions_list_json();
    assert!(perms_json.contains("\"camera\"") && perms_json.contains("\"clipboard_read\""));

    // Test rendering + doctor on mobile project
    let png_out = mobile_dir.join("preview.png");
    let mobile_src = std::fs::read_to_string(mobile_dir.join("ui/main.cui")).unwrap();
    render_cui_to_png(&mobile_src, Some("MainView"), 360, 640, &png_out).unwrap();
    assert!(std::fs::metadata(&png_out).unwrap().len() > 100);

    let doctor = collect_doctor_report(&mobile_dir).unwrap();
    assert!(doctor.manifest_found);
    assert_eq!(doctor.manifest_permissions, 6);
    assert!(format_doctor_json(&doctor).contains("\"manifest\""));

    let _ = std::fs::remove_dir_all(&base_tmp);
}

#[test]
fn test_detect_cargo_package_name_handles_comments_and_quotes() {
    let tmp = std::env::temp_dir().join(format!(
        "craftui_cargo_name_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let cargo_file = tmp.join("Cargo.toml");
    std::fs::write(
        &cargo_file,
        r#"
[package]
name = "demo-pkg" # this is a comment
version = "0.1.0"
"#,
    )
    .unwrap();
    assert_eq!(
        crate::templates::detect_cargo_package_name(&cargo_file).as_deref(),
        Some("demo-pkg")
    );

    std::fs::write(
        &cargo_file,
        r#"
[package]
name = 'single-quoted-pkg'   # trailing comment
"#,
    )
    .unwrap();
    assert_eq!(
        crate::templates::detect_cargo_package_name(&cargo_file).as_deref(),
        Some("single-quoted-pkg")
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
