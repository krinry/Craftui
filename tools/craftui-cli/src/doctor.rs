use crate::dev::{CheckStats, check_cui_paths};
use crate::manifest::{AppManifest, ManifestDiagnostic, find_project_manifest};
use crate::platform::android::{discover_adb, discover_android_ndk};
use craftui_core::{CraftError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub rustc_version: Option<String>,
    pub cargo_version: Option<String>,
    pub installed_targets: Vec<String>,
    pub wasm_target: bool,
    pub android_target: bool,
    pub ios_target: bool,
    pub wasm_bindgen: bool,
    pub wasm_opt: bool,
    pub trunk: bool,
    pub adb: Option<PathBuf>,
    pub android_ndk: Option<PathBuf>,
    pub clang: Option<PathBuf>,
    pub xcodebuild: bool,
    pub cui_stats: CheckStats,
    pub manifest_found: bool,
    pub manifest_permissions: usize,
    pub manifest_diagnostics: Vec<ManifestDiagnostic>,
}

pub fn command_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let candidates = [
        format!("{name}.exe"),
        format!("{name}.cmd"),
        name.to_string(),
    ];
    #[cfg(not(windows))]
    let candidates = [name.to_string()];

    for dir in std::env::split_paths(&path_var) {
        for cand in &candidates {
            let full = dir.join(cand);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

pub fn run_setup(check_only: bool, skip_ios: bool) -> Result<()> {
    let report = collect_doctor_report(Path::new("."))?;
    println!("CraftUI Toolchain Setup");
    println!("-----------------------");
    let mut needed: Vec<&str> = Vec::new();
    if !report.wasm_target {
        needed.push("wasm32-unknown-unknown");
    }
    if !report.android_target {
        needed.push("aarch64-linux-android");
    }
    if cfg!(target_os = "macos") && !skip_ios && !report.ios_target {
        needed.push("aarch64-apple-ios");
    }

    if needed.is_empty() {
        println!("[ok] Required Rust compilation targets are already installed.");
    } else if check_only {
        println!(
            "[info] Missing Rust targets (--check-only): {}",
            needed.join(", ")
        );
        println!(
            "       Run `craftui setup` or `rustup target add {}`",
            needed.join(" ")
        );
    } else if find_in_path("rustup").is_some() {
        println!("Installing missing Rust targets: {}", needed.join(", "));
        let mut cmd = Command::new("rustup");
        cmd.arg("target").arg("add");
        for target in &needed {
            cmd.arg(target);
        }
        let status = cmd
            .status()
            .map_err(|e| CraftError::Platform(format!("failed running rustup: {e}")))?;
        if !status.success() {
            return Err(CraftError::Platform(format!(
                "rustup target add exited with {status}"
            )));
        }
        println!("[ok] Installed Rust targets successfully.");
    } else {
        println!("[warn] `rustup` was not found on PATH; install rustup from https://rustup.rs/");
    }

    println!();
    println!("Native SDK Hints:");
    println!(
        "  Web:     {} (`cargo install wasm-bindgen-cli` for JS glue)",
        if report.wasm_bindgen {
            "wasm-bindgen detected"
        } else {
            "wasm-bindgen not installed"
        }
    );
    println!(
        "  Android: {} (set ANDROID_NDK_HOME or install via Android Studio SDK Manager)",
        report.android_ndk.as_ref().map_or_else(
            || "NDK not detected".to_string(),
            |p| p.display().to_string()
        )
    );
    println!(
        "  ADB:     {}",
        report
            .adb
            .as_ref()
            .map_or_else(|| "not found".to_string(), |p| p.display().to_string())
    );
    if cfg!(target_os = "macos") {
        println!(
            "  iOS:     xcodebuild = {}",
            if report.xcodebuild {
                "ready"
            } else {
                "missing (install Xcode Command Line Tools)"
            }
        );
    } else {
        println!(
            "  iOS:     macOS + Xcode required for final .ipa linking (`--platform ios --skip-cargo` emits Swift/Metal template anywhere)"
        );
    }
    Ok(())
}

pub fn collect_doctor_report(project_dir: &Path) -> Result<DoctorReport> {
    let rustc_version = command_stdout("rustc", &["--version"]);
    let cargo_version = command_stdout("cargo", &["--version"]);
    let installed_targets: Vec<String> =
        command_stdout("rustup", &["target", "list", "--installed"])
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();

    let has_target = |t: &str| installed_targets.iter().any(|x| x == t);
    let cui_stats = if project_dir.exists() {
        check_cui_paths(project_dir, false)?
    } else {
        CheckStats::default()
    };

    let (manifest_found, manifest_permissions, manifest_diagnostics) =
        if let Some(mpath) = find_project_manifest(project_dir) {
            match AppManifest::load_file(&mpath) {
                Ok(m) => {
                    let perms = m.enabled_permissions().len();
                    let diags = m.validate();
                    (true, perms, diags)
                }
                Err(err) => (
                    true,
                    0,
                    vec![ManifestDiagnostic {
                        is_error: true,
                        message: err.to_string(),
                    }],
                ),
            }
        } else {
            (false, 0, Vec::new())
        };

    Ok(DoctorReport {
        rustc_version,
        cargo_version,
        wasm_target: has_target("wasm32-unknown-unknown"),
        android_target: has_target("aarch64-linux-android"),
        ios_target: has_target("aarch64-apple-ios"),
        installed_targets,
        wasm_bindgen: find_in_path("wasm-bindgen").is_some(),
        wasm_opt: find_in_path("wasm-opt").is_some(),
        trunk: find_in_path("trunk").is_some(),
        adb: discover_adb(),
        android_ndk: discover_android_ndk(),
        clang: find_in_path("clang"),
        xcodebuild: cfg!(target_os = "macos") && find_in_path("xcodebuild").is_some(),
        cui_stats,
        manifest_found,
        manifest_permissions,
        manifest_diagnostics,
    })
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn format_doctor_json(r: &DoctorReport) -> String {
    let opt_str = |o: &Option<String>| {
        o.as_ref()
            .map_or_else(|| "null".to_string(), |s| json_escape(s))
    };
    let opt_path = |o: &Option<PathBuf>| {
        o.as_ref().map_or_else(
            || "null".to_string(),
            |p| json_escape(&p.display().to_string()),
        )
    };
    let manifest_errors = r.manifest_diagnostics.iter().filter(|d| d.is_error).count();
    format!(
        "{{\n  \"rustc\": {},\n  \"cargo\": {},\n  \"targets\": {{\n    \"wasm32_unknown_unknown\": {},\n    \"aarch64_linux_android\": {},\n    \"aarch64_apple_ios\": {}\n  }},\n  \"tools\": {{\n    \"wasm_bindgen\": {},\n    \"wasm_opt\": {},\n    \"trunk\": {},\n    \"adb\": {},\n    \"android_ndk\": {},\n    \"clang\": {},\n    \"xcodebuild\": {}\n  }},\n  \"cui\": {{\n    \"files\": {},\n    \"warnings\": {},\n    \"errors\": {}\n  }},\n  \"manifest\": {{\n    \"found\": {},\n    \"permissions\": {},\n    \"errors\": {}\n  }}\n}}",
        opt_str(&r.rustc_version),
        opt_str(&r.cargo_version),
        r.wasm_target,
        r.android_target,
        r.ios_target,
        r.wasm_bindgen,
        r.wasm_opt,
        r.trunk,
        opt_path(&r.adb),
        opt_path(&r.android_ndk),
        opt_path(&r.clang),
        r.xcodebuild,
        r.cui_stats.files,
        r.cui_stats.warnings,
        r.cui_stats.errors,
        r.manifest_found,
        r.manifest_permissions,
        manifest_errors,
    )
}

pub fn run_doctor(path: &Path, json: bool) -> Result<()> {
    let report = collect_doctor_report(path)?;
    if json {
        println!("{}", format_doctor_json(&report));
    } else {
        println!("CraftUI Environment Doctor");
        println!("--------------------------");
        println!(
            "[{}] rustc: {}",
            if report.rustc_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            report.rustc_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] cargo: {}",
            if report.cargo_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            report.cargo_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] Desktop target (host): ready{}",
            if report.rustc_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            if report.clang.is_some() {
                ""
            } else {
                " (clang missing)"
            }
        );
        println!(
            "[{}] Web target (wasm32-unknown-unknown): {} (wasm-bindgen: {}, wasm-opt: {}, trunk: {})",
            if report.wasm_target { "ok" } else { "missing" },
            if report.wasm_target {
                "installed"
            } else {
                "run `craftui setup`"
            },
            if report.wasm_bindgen { "yes" } else { "no" },
            if report.wasm_opt { "yes" } else { "no" },
            if report.trunk { "yes" } else { "no" },
        );
        println!(
            "[{}] Android target (aarch64-linux-android): {} (NDK: {}, adb: {})",
            if report.android_target && report.android_ndk.is_some() {
                "ok"
            } else {
                "warn"
            },
            if report.android_target {
                "installed"
            } else {
                "run `craftui setup`"
            },
            report
                .android_ndk
                .as_ref()
                .map_or_else(|| "not detected".into(), |p| p.display().to_string()),
            report
                .adb
                .as_ref()
                .map_or_else(|| "not found".into(), |p| p.display().to_string()),
        );
        println!(
            "[{}] iOS target (aarch64-apple-ios): {} (xcodebuild: {})",
            if report.ios_target && report.xcodebuild {
                "ok"
            } else {
                "info"
            },
            if report.ios_target {
                "installed"
            } else if cfg!(target_os = "macos") {
                "run `craftui setup`"
            } else {
                "macOS/Xcode required for linking (`craftui build --platform ios --skip-cargo` emits Swift/Metal template)"
            },
            if report.xcodebuild { "yes" } else { "no" },
        );
        println!(
            "[{}] Templates (.cui): {} file(s), {} warning(s), {} error(s)",
            if report.cui_stats.errors == 0 {
                "ok"
            } else {
                "error"
            },
            report.cui_stats.files,
            report.cui_stats.warnings,
            report.cui_stats.errors,
        );
        let m_errors = report
            .manifest_diagnostics
            .iter()
            .filter(|d| d.is_error)
            .count();
        println!(
            "[{}] Manifest (craftui.toml): {}",
            if !report.manifest_found {
                "info"
            } else if m_errors == 0 {
                "ok"
            } else {
                "error"
            },
            if report.manifest_found {
                format!(
                    "{} permission(s) declared, {} diagnostic(s)",
                    report.manifest_permissions,
                    report.manifest_diagnostics.len()
                )
            } else {
                "not present (`craftui init` to add)".to_string()
            }
        );
    }

    if report.rustc_version.is_none() || report.cargo_version.is_none() {
        return Err(CraftError::Platform(
            "Rust toolchain (rustc/cargo) is required".to_string(),
        ));
    }
    if report.cui_stats.errors > 0 {
        return Err(CraftError::Dsl(format!(
            "{} .cui validation error(s) found",
            report.cui_stats.errors
        )));
    }
    let manifest_errors = report
        .manifest_diagnostics
        .iter()
        .filter(|d| d.is_error)
        .count();
    if manifest_errors > 0 {
        return Err(CraftError::Platform(format!(
            "{manifest_errors} craftui.toml manifest error(s) found"
        )));
    }
    Ok(())
}
