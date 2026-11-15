use crate::doctor::find_in_path;
use crate::manifest::{AppManifest, default_usage_description};
use crate::permissions::load_active_manifest;
use crate::platform::android::{
    configure_android_ndk_env, discover_adb, discover_android_ndk,
    emit_android_template_with_manifest,
};
use crate::platform::ios::emit_ios_template_with_manifest;
use crate::platform::web::{emit_web_bundle, serve_web_directory};
use crate::{PlatformTarget, RunPlatform};
use craftui_core::{CraftError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run_app_target(
    platform: RunPlatform,
    release: bool,
    package: Option<&str>,
    port: u16,
    skip_cargo: bool,
) -> Result<()> {
    match platform {
        RunPlatform::Desktop => {
            let mut cmd = Command::new("cargo");
            cmd.arg("run");
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            }
            if release {
                cmd.arg("--release");
            }
            let status = cmd
                .status()
                .map_err(|e| CraftError::Platform(format!("failed spawning cargo run: {e}")))?;
            if !status.success() {
                return Err(CraftError::Platform(format!(
                    "cargo run exited with status {status}"
                )));
            }
            Ok(())
        }
        RunPlatform::Web => {
            let out_dir = PathBuf::from("dist/web");
            run_build(
                PlatformTarget::Web,
                release,
                package,
                Some(&out_dir),
                skip_cargo,
            )?;
            serve_web_directory(&out_dir, port)
        }
        RunPlatform::Android => {
            let out_dir = PathBuf::from("dist/android");
            run_build(
                PlatformTarget::Android,
                release,
                package,
                Some(&out_dir),
                skip_cargo,
            )?;
            let abs_out_dir = std::fs::canonicalize(&out_dir).unwrap_or_else(|_| out_dir.clone());
            let gradlew = abs_out_dir.join(if cfg!(windows) {
                "gradlew.bat"
            } else {
                "gradlew"
            });
            let gradle_bin = if gradlew.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&gradlew) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&gradlew, perms);
                    }
                }
                Some(gradlew)
            } else if let Some(sys_gradle) = find_in_path("gradle") {
                // Generate gradle wrapper if not present
                let _ = Command::new(&sys_gradle)
                    .arg("wrapper")
                    .current_dir(&abs_out_dir)
                    .status();
                if gradlew.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = std::fs::metadata(&gradlew) {
                            let mut perms = meta.permissions();
                            perms.set_mode(0o755);
                            let _ = std::fs::set_permissions(&gradlew, perms);
                        }
                    }
                    Some(gradlew)
                } else {
                    Some(sys_gradle)
                }
            } else {
                None
            };

            if let Some(gbin) = gradle_bin {
                let task = if release {
                    "assembleRelease"
                } else {
                    "assembleDebug"
                };
                println!("Running gradle {task} in {}...", out_dir.display());
                let status = Command::new(&gbin)
                    .arg(task)
                    .current_dir(&out_dir)
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning gradle: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "gradle {task} exited with {status}"
                    )));
                }

                // Locate generated APK
                let default_apk = out_dir.join(if release {
                    "app/build/outputs/apk/release/app-release-unsigned.apk"
                } else {
                    "app/build/outputs/apk/debug/app-debug.apk"
                });
                let apk = if default_apk.is_file() {
                    Some(default_apk)
                } else {
                    let apk_search_dir = out_dir.join("app/build/outputs/apk");
                    let mut found: Option<PathBuf> = None;
                    if let Ok(entries) = std::fs::read_dir(&apk_search_dir) {
                        for sub in entries.flatten() {
                            if sub.path().is_dir()
                                && let Ok(files) = std::fs::read_dir(sub.path())
                            {
                                for f in files.flatten() {
                                    if f.path().extension().and_then(|e| e.to_str()) == Some("apk")
                                    {
                                        found = Some(f.path());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    found
                };

                if let Some(apk_path) = apk {
                    println!("Built Android APK at: {}", apk_path.display());
                    if let Some(adb) = discover_adb() {
                        let has_devices = Command::new(&adb)
                            .arg("devices")
                            .output()
                            .ok()
                            .map(|out| {
                                let stdout = String::from_utf8_lossy(&out.stdout);
                                stdout.lines().skip(1).any(|l| l.contains("\tdevice"))
                            })
                            .unwrap_or(false);

                        if has_devices {
                            println!("Installing APK on device via {}...", adb.display());
                            let install = Command::new(&adb)
                                .args(["install", "-r"])
                                .arg(&apk_path)
                                .status()
                                .map_err(|e| {
                                    CraftError::Platform(format!("adb install failed: {e}"))
                                })?;
                            if !install.success() {
                                return Err(CraftError::Platform(format!(
                                    "adb install exited with {install}"
                                )));
                            }
                            let app_id =
                                std::fs::read_to_string(out_dir.join("app/build.gradle.kts"))
                                    .ok()
                                    .and_then(|content| {
                                        content.lines().find_map(|line| {
                                            let trimmed = line.trim();
                                            if trimmed.starts_with("applicationId =") {
                                                trimmed.split('"').nth(1).map(str::to_string)
                                            } else {
                                                None
                                            }
                                        })
                                    })
                                    .unwrap_or_else(|| {
                                        load_active_manifest(Path::new("."))
                                            .ok()
                                            .flatten()
                                            .map_or_else(
                                                || "dev.craftui.app".to_string(),
                                                |m| m.app.id,
                                            )
                                    });

                            println!(
                                "Starting Android app: {app_id}/dev.craftui.app.MainActivity..."
                            );
                            let _ = Command::new(&adb)
                                .args([
                                    "shell",
                                    "am",
                                    "start",
                                    "-n",
                                    &format!("{app_id}/dev.craftui.app.MainActivity"),
                                ])
                                .status();
                            println!(
                                "\x1b[32m[craftui]\x1b[0m App successfully deployed and launched on Android!"
                            );
                        } else {
                            println!(
                                "\x1b[33m[craftui]\x1b[0m No Android device or emulator currently connected. Connect a device with USB debugging or launch an emulator, then run `adb install -r {}`.",
                                apk_path.display()
                            );
                        }
                    } else {
                        println!(
                            "`adb` not found on PATH. Install Android platform-tools or run `adb install -r {}` manually.",
                            apk_path.display()
                        );
                    }
                }
            } else {
                println!(
                    "Android Gradle project updated at {}.\n`gradle`/`gradlew` was not found on PATH; open {} in Android Studio or run `./gradlew installDebug` with a connected device.",
                    out_dir.display(),
                    out_dir.display()
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn find_built_artifact(
    target_triple: &str,
    release: bool,
    pkg: &str,
    ext: &str,
) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let stem = pkg.replace('-', "_");
    let direct = PathBuf::from("target")
        .join(target_triple)
        .join(profile)
        .join(if ext == "so" {
            format!("lib{stem}.so")
        } else {
            format!("{stem}.{ext}")
        });
    if direct.is_file() {
        return Some(direct);
    }
    None
}

pub fn run_build(
    platform: PlatformTarget,
    release: bool,
    package: Option<&str>,
    out_override: Option<&Path>,
    skip_cargo: bool,
) -> Result<()> {
    let profile_dir = |default_sub: &str| {
        out_override
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist").join(default_sub))
    };
    let mut manifest_opt = None;
    if let Some(pkg) = package {
        let candidate1 = Path::new("examples").join(pkg).join("craftui.toml");
        let candidate2 = Path::new("examples")
            .join(pkg.trim_start_matches("craftui-example-"))
            .join("craftui.toml");
        let candidate3 = Path::new("packages").join(pkg).join("craftui.toml");
        for cand in [candidate1, candidate2, candidate3] {
            if cand.is_file() {
                manifest_opt = Some(AppManifest::load_file(&cand)?);
                break;
            }
        }
    }
    if manifest_opt.is_none() {
        manifest_opt = load_active_manifest(Path::new("."))?;
    }
    let app_package = package
        .map(str::to_string)
        .or_else(|| crate::templates::detect_cargo_package_name(Path::new("Cargo.toml")))
        .or_else(|| manifest_opt.as_ref().map(|m| m.app.id.clone()));
    let app_display = manifest_opt
        .as_ref()
        .map(|m| m.app.name.clone())
        .or_else(|| package.map(str::to_string))
        .or_else(|| app_package.clone())
        .unwrap_or_else(|| "craftui_app".to_string());

    match platform {
        PlatformTarget::Desktop => {
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.arg("build");
                if let Some(pkg) = package.or(app_package.as_deref()) {
                    cmd.args(["-p", pkg]);
                }
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo build exited with status {status}"
                    )));
                }
            }
            println!("Desktop build finished.");
        }
        PlatformTarget::Web => {
            let pkg = package
                .or(app_package.as_deref())
                .unwrap_or("craftui-platform-web");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "wasm32-unknown-unknown", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                if pkg != "craftui-platform-web" {
                    cmd.arg("--no-default-features");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo wasm32 build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("web");
            let wasm = find_built_artifact("wasm32-unknown-unknown", release, pkg, "wasm");
            emit_web_bundle(&out_dir, &app_display, wasm.as_deref())?;
            println!("Packaged CraftUI Web bundle at {}", out_dir.display());
        }
        PlatformTarget::Android => {
            let pkg = package
                .or(app_package.as_deref())
                .unwrap_or("craftui-platform-android");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-linux-android", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                if pkg != "craftui-platform-android" {
                    cmd.arg("--no-default-features");
                } else {
                    cmd.args(["--features", "builtin-main"]);
                }
                if let Some(ndk) = discover_android_ndk() {
                    let _ = configure_android_ndk_env(&mut cmd, &ndk);
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo android build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("android");
            let lib_name = pkg.replace('-', "_");
            let so = find_built_artifact("aarch64-linux-android", release, pkg, "so");
            emit_android_template_with_manifest(
                &out_dir,
                &app_display,
                &lib_name,
                so.as_deref(),
                manifest_opt.as_ref(),
            )?;
            println!(
                "Packaged CraftUI Android Gradle project at {}",
                out_dir.display()
            );
        }
        PlatformTarget::Ios => {
            let pkg = package
                .or(app_package.as_deref())
                .unwrap_or("craftui-platform-ios");
            if !skip_cargo {
                if !cfg!(target_os = "macos") {
                    return Err(CraftError::Platform(
                        "iOS native compilation (`aarch64-apple-ios`) requires macOS + Xcode; pass `--skip-cargo` to emit the Swift/UIKit/Metal template bundle on Linux/Windows".to_string(),
                    ));
                }
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-apple-ios", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo iOS build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("ios");
            let lib_name = pkg.replace('-', "_");
            emit_ios_template_with_manifest(
                &out_dir,
                &app_display,
                &lib_name,
                manifest_opt.as_ref(),
            )?;
            println!(
                "Packaged CraftUI iOS Xcode bridge bundle at {}",
                out_dir.display()
            );
        }
    }
    let _ = default_usage_description;
    Ok(())
}
