use crate::doctor::json_escape;
use crate::manifest::{AppManifest, find_project_manifest, web_permission_name};
use craftui_core::{CraftError, Permission, Result};
use std::path::Path;

pub fn format_permissions_list_json() -> String {
    let items: Vec<String> = Permission::all_standard()
        .iter()
        .map(|perm| {
            let key = json_escape(&perm.key());
            let android: Vec<String> = perm
                .android_permissions()
                .into_iter()
                .map(|s| json_escape(&s.name))
                .collect();
            let ios: Vec<String> = perm
                .ios_usage_keys()
                .iter()
                .map(|k| json_escape(k))
                .collect();
            let web = web_permission_name(perm).map_or_else(|| "null".to_string(), json_escape);
            format!(
                "  {{\"key\": {key}, \"android\": [{}], \"ios\": [{}], \"web\": {web}}}",
                android.join(", "),
                ios.join(", ")
            )
        })
        .collect();
    format!("[\n{}\n]", items.join(",\n"))
}

pub fn run_permissions_list(json: bool) -> Result<()> {
    if json {
        println!("{}", format_permissions_list_json());
        return Ok(());
    }
    println!(
        "CraftUI Standard Permissions ({} built-in keys)",
        Permission::all_standard().len()
    );
    println!("-------------------------------------------------");
    for perm in Permission::all_standard() {
        let android: Vec<String> = perm
            .android_permissions()
            .into_iter()
            .map(|s| match (s.min_sdk, s.max_sdk) {
                (Some(min), Some(max)) => format!("{} (SDK {min}..={max})", s.name),
                (Some(min), None) => format!("{} (SDK >= {min})", s.name),
                (None, Some(max)) => format!("{} (SDK <= {max})", s.name),
                (None, None) => s.name.into_owned(),
            })
            .collect();
        let ios = if perm.ios_usage_keys().is_empty() {
            "-".to_string()
        } else {
            perm.ios_usage_keys().join(", ")
        };
        let web = web_permission_name(perm).unwrap_or("-");
        println!(
            "  {:<24} | Android: {:<50} | iOS: {:<38} | Web: {}",
            perm.key(),
            if android.is_empty() {
                "-".to_string()
            } else {
                android.join(", ")
            },
            ios,
            web
        );
    }
    Ok(())
}

pub fn run_permissions_check(path: &Path, json: bool) -> Result<()> {
    let manifest_path = find_project_manifest(path).ok_or_else(|| {
        CraftError::Platform(format!(
            "no craftui.toml found at {} (run `craftui init` to create one)",
            path.display()
        ))
    })?;
    let manifest = AppManifest::load_file(&manifest_path)?;
    let diags = manifest.validate();
    let error_count = diags.iter().filter(|d| d.is_error).count();
    let warning_count = diags.len() - error_count;

    if json {
        let android_list: Vec<String> = manifest
            .android_manifest_permissions()
            .into_iter()
            .map(|s| json_escape(&s.name))
            .collect();
        let ios_list: Vec<String> = manifest
            .ios_info_plist_usage_entries()
            .into_iter()
            .map(|(k, _)| json_escape(&k))
            .collect();
        let diag_json: Vec<String> = diags
            .iter()
            .map(|d| {
                format!(
                    "    {{\"severity\": {}, \"message\": {}}}",
                    if d.is_error {
                        "\"error\""
                    } else {
                        "\"warning\""
                    },
                    json_escape(&d.message)
                )
            })
            .collect();
        println!(
            "{{\n  \"manifest\": {},\n  \"app_id\": {},\n  \"enabled_permissions\": {},\n  \"android_uses_permissions\": [{}],\n  \"ios_usage_keys\": [{}],\n  \"errors\": {},\n  \"warnings\": {},\n  \"diagnostics\": [\n{}\n  ]\n}}",
            json_escape(&manifest_path.display().to_string()),
            json_escape(&manifest.app.id),
            manifest.enabled_permissions().len(),
            android_list.join(", "),
            ios_list.join(", "),
            error_count,
            warning_count,
            diag_json.join(",\n")
        );
    } else {
        println!(
            "CraftUI Permission Audit — {} ({})",
            manifest.app.name, manifest.app.id
        );
        println!("Manifest: {}", manifest_path.display());
        println!(
            "Enabled Permissions: {} | Android entries: {} | iOS Info.plist keys: {}",
            manifest.enabled_permissions().len(),
            manifest.android_manifest_permissions().len(),
            manifest.ios_info_plist_usage_entries().len()
        );
        for d in &diags {
            let level = if d.is_error { "error" } else { "warning" };
            println!("  [{level}] {}", d.message);
        }
        println!("Result: {warning_count} warning(s), {error_count} error(s)");
    }

    if error_count > 0 {
        return Err(CraftError::Platform(format!(
            "{error_count} craftui.toml manifest error(s) found"
        )));
    }
    Ok(())
}

pub fn load_active_manifest(dir: &Path) -> Result<Option<AppManifest>> {
    if let Some(p) = find_project_manifest(dir) {
        Ok(Some(AppManifest::load_file(&p)?))
    } else {
        Ok(None)
    }
}
