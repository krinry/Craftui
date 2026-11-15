use crate::doctor::find_in_path;
use crate::manifest::{AppManifest, xml_escape};
use craftui_core::{CraftError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn is_valid_ndk_root(dir: &Path) -> bool {
    dir.join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .is_dir()
}

pub fn discover_android_ndk() -> Option<PathBuf> {
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            let p = PathBuf::from(val);
            if is_valid_ndk_root(&p) {
                return Some(p);
            }
        }
    }

    let mut sdk_roots = Vec::new();
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            sdk_roots.push(PathBuf::from(val));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        sdk_roots.push(h.join("Android").join("Sdk"));
        sdk_roots.push(h.join("Library").join("Android").join("sdk"));
    }

    let mut ndk_dirs = Vec::new();
    for sdk in sdk_roots {
        let ndk_parent = sdk.join("ndk");
        if let Ok(entries) = std::fs::read_dir(&ndk_parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if is_valid_ndk_root(&p) {
                    ndk_dirs.push(p);
                }
            }
        }
    }
    ndk_dirs.sort();
    ndk_dirs.pop()
}

pub fn discover_adb() -> Option<PathBuf> {
    if let Some(p) = find_in_path("adb") {
        return Some(p);
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        for cand in [
            h.join("Android/Sdk/platform-tools/adb"),
            h.join("Library/Android/sdk/platform-tools/adb"),
        ] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn ndk_toolchain_bin(ndk_root: &Path) -> Option<PathBuf> {
    let prebuilt = ndk_root.join("toolchains").join("llvm").join("prebuilt");
    for host in [
        "linux-x86_64",
        "darwin-arm64",
        "darwin-x86_64",
        "windows-x86_64",
    ] {
        let bin = prebuilt.join(host).join("bin");
        if bin.is_dir() {
            return Some(bin);
        }
    }
    None
}

pub fn configure_android_ndk_env(cmd: &mut Command, ndk_root: &Path) -> Option<(PathBuf, PathBuf)> {
    let bin = ndk_toolchain_bin(ndk_root)?;
    let mut best_api: Option<(u32, PathBuf, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(rest) = fname.strip_prefix("aarch64-linux-android") else {
                continue;
            };
            let Some(api_str) = rest.strip_suffix("-clang++") else {
                continue;
            };
            if let Ok(api) = api_str.parse::<u32>() {
                let cc = bin.join(format!("aarch64-linux-android{api}-clang"));
                if cc.is_file()
                    && best_api.as_ref().is_none_or(|(b, _, _)| {
                        api > *b && *b < 24 || (api >= 24 && (*b < 24 || api < *b))
                    })
                {
                    best_api = Some((api, cc, p));
                }
            }
        }
    }
    let (_, cc, cxx) = best_api?;
    let ar = bin.join("llvm-ar");
    if std::env::var_os("CC_aarch64_linux_android").is_none() {
        cmd.env("CC_aarch64_linux_android", &cc);
        cmd.env("CC_aarch64-linux-android", &cc);
    }
    if std::env::var_os("CXX_aarch64_linux_android").is_none() {
        cmd.env("CXX_aarch64_linux_android", &cxx);
        cmd.env("CXX_aarch64-linux-android", &cxx);
    }
    if ar.is_file() && std::env::var_os("AR_aarch64_linux_android").is_none() {
        cmd.env("AR_aarch64_linux_android", &ar);
        cmd.env("AR_aarch64-linux-android", &ar);
    }
    if std::env::var_os("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER").is_none() {
        cmd.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", &cc);
    }
    Some((cc, cxx))
}

#[allow(dead_code)]
pub fn emit_android_template(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    built_so: Option<&Path>,
) -> Result<()> {
    emit_android_template_with_manifest(out_dir, app_name, rust_lib_name, built_so, None)
}

pub fn emit_android_template_with_manifest(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    built_so: Option<&Path>,
    manifest_opt: Option<&AppManifest>,
) -> Result<()> {
    let effective_manifest = manifest_opt
        .cloned()
        .unwrap_or_else(|| AppManifest::with_app(app_name, AppManifest::sanitize_id(app_name)));

    let app_display_name = xml_escape(&effective_manifest.app.name);
    let app_id = effective_manifest.app.id.clone();
    let app_version = effective_manifest.app.version.clone();
    let min_sdk = effective_manifest.android.min_sdk;
    let target_sdk = effective_manifest.android.target_sdk;
    let version_code = effective_manifest.android.version_code;
    let permission_xml = effective_manifest.render_android_manifest_permissions_xml();

    let java_dir = out_dir.join("app/src/main/kotlin/dev/craftui/app");
    let jni_dir = out_dir.join("app/src/main/jniLibs/arm64-v8a");
    let res_values_dir = out_dir.join("app/src/main/res/values");
    std::fs::create_dir_all(&java_dir)
        .and_then(|_| std::fs::create_dir_all(&jni_dir))
        .and_then(|_| std::fs::create_dir_all(&res_values_dir))
        .map_err(|e| CraftError::Platform(format!("failed creating android dirs: {e}")))?;

    let settings_gradle = format!(
        r#"pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}
dependencyResolutionManagement {{
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {{
        google()
        mavenCentral()
    }}
}}
rootProject.name = "{}"
include(":app")
"#,
        effective_manifest.app.name.replace('"', "\\\"")
    );
    let root_gradle = r#"plugins {
    id("com.android.application") version "8.6.1" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
"#;
    let app_gradle = format!(
        r#"plugins {{
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}}

android {{
    namespace = "dev.craftui.app"
    compileSdk = {target_sdk}

    defaultConfig {{
        applicationId = "{app_id}"
        minSdk = {min_sdk}
        targetSdk = {target_sdk}
        versionCode = {version_code}
        versionName = "{app_version}"
        ndk {{
            abiFilters += listOf("arm64-v8a")
        }}
    }}

    buildFeatures {{
        prefab = true
    }}

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}
    kotlinOptions {{
        jvmTarget = "17"
    }}
}}

dependencies {{
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.biometric:biometric:1.1.0")
    implementation("androidx.games:games-activity:3.0.5")
}}
"#
    );

    let android_manifest = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
{permission_xml}    <application
        android:label="{app_display_name}"
        android:allowBackup="true"
        android:supportsRtl="true"
        android:theme="@style/Theme.CraftUI">
        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:launchMode="singleTask"
            android:windowSoftInputMode="adjustResize"
            android:configChanges="orientation|keyboardHidden|keyboard|screenSize|smallestScreenSize|locale|layoutDirection|fontScale|screenLayout|density|uiMode">
            <meta-data
                android:name="android.app.lib_name"
                android:value="{rust_lib_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    );

    let styles_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="Theme.CraftUI" parent="Theme.AppCompat.DayNight.NoActionBar" />
</resources>
"#;

    let main_activity = format!(
        r#"package dev.craftui.app

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.BatteryManager
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.Settings
import android.view.HapticFeedbackConstants
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import com.google.androidgamesdk.GameActivity

class MainActivity : GameActivity() {{
    companion object {{
        init {{
            System.loadLibrary("{rust_lib_name}")
        }}
        var instance: MainActivity? = null

        @JvmStatic
        fun launchCameraFromNative() {{
            instance?.launchCameraWithPermission()
        }}

        @JvmStatic
        fun launchGalleryFromNative() {{
            instance?.launchGalleryWithPermission()
        }}

        @JvmStatic
        fun showTauriWebViewNative() {{
            instance?.showWebView()
        }}

        @JvmStatic
        fun hideTauriWebViewNative() {{
            instance?.hideWebView()
        }}
    }}

    private var pendingPermRequestId: Long = 0L
    private var pendingFileRequestId: Long = 0L
    private var lastReportedInsetsKey: String = ""

    private val cameraPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {{ isGranted ->
        if (isGranted) {{
            openCameraIntent()
        }}
    }}

    private val galleryPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {{ isGranted ->
        if (isGranted) {{
            openGalleryIntent()
        }}
    }}

    private val takePictureLauncher = registerForActivityResult(
        ActivityResultContracts.TakePicturePreview()
    ) {{ _ ->
    }}

    private val pickImageLauncher = registerForActivityResult(
        ActivityResultContracts.GetContent()
    ) {{ _ ->
    }}

    fun launchCameraWithPermission() {{
        runOnUiThread {{
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {{
                openCameraIntent()
            }} else {{
                cameraPermissionLauncher.launch(android.Manifest.permission.CAMERA)
            }}
        }}
    }}

    fun launchGalleryWithPermission() {{
        runOnUiThread {{
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {{
                openGalleryIntent()
            }} else if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED) {{
                openGalleryIntent()
            }} else {{
                galleryPermissionLauncher.launch(android.Manifest.permission.READ_EXTERNAL_STORAGE)
            }}
        }}
    }}

    fun showWebView() {{
        runOnUiThread {{
            // Activate overlay WebView on demand
        }}
    }}

    fun hideWebView() {{
        runOnUiThread {{
            // Deactivate overlay WebView
        }}
    }}

    private fun openCameraIntent() {{
        try {{
            takePictureLauncher.launch(null)
        }} catch (_: Exception) {{
            val intent = Intent(android.provider.MediaStore.ACTION_IMAGE_CAPTURE)
            if (intent.resolveActivity(packageManager) != null) {{
                startActivity(intent)
            }}
        }}
    }}

    private fun openGalleryIntent() {{
        try {{
            pickImageLauncher.launch("image/*")
        }} catch (_: Exception) {{
            val intent = Intent(Intent.ACTION_PICK, android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI)
            startActivity(intent)
        }}
    }}

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) {{ grants ->
        val rationales = grants.keys.associateWith {{ perm ->
            ActivityCompat.shouldShowRequestPermissionRationale(this, perm)
        }}
        onNativePermissionResults(pendingPermRequestId, grants, rationales)
    }}

    private val openFilesLauncher = registerForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments()
    ) {{ uris: List<Uri> ->
        onNativeFileDialogResults(pendingFileRequestId, uris.map {{ it.toString() }})
    }}

    private val saveFileLauncher = registerForActivityResult(
        ActivityResultContracts.CreateDocument("*/*")
    ) {{ uri: Uri? ->
        onNativeFileDialogResults(pendingFileRequestId, uri?.let {{ listOf(it.toString()) }} ?: emptyList())
    }}

    override fun onCreate(savedInstanceState: Bundle?) {{
        super.onCreate(savedInstanceState)
        instance = this
        WindowCompat.setDecorFitsSystemWindows(window, false)
        ViewCompat.setOnApplyWindowInsetsListener(window.decorView) {{ _, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val key = "${{safe.top}},${{safe.right}},${{safe.bottom}},${{safe.left}},${{ime.bottom}}"
            if (key != lastReportedInsetsKey) {{
                lastReportedInsetsKey = key
            }}
            insets
        }}
    }}

    fun requestCraftUiPermissions(requestId: Long, manifestPermissions: Array<String>) {{
        pendingPermRequestId = requestId
        permissionLauncher.launch(manifestPermissions)
    }}

    fun checkCraftUiPermissionGranted(manifestPermission: String): Boolean {{
        return ContextCompat.checkSelfPermission(this, manifestPermission) == PackageManager.PERMISSION_GRANTED
    }}

    fun openCraftUiAppSettings() {{
        val intent = Intent(
            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.fromParts("package", packageName, null)
        )
        startActivity(intent)
    }}

    fun openCraftUiDocuments(requestId: Long, mimeTypes: Array<String>) {{
        pendingFileRequestId = requestId
        openFilesLauncher.launch(if (mimeTypes.isEmpty()) arrayOf("*/*") else mimeTypes)
    }}

    fun saveCraftUiDocument(requestId: Long, suggestedName: String) {{
        pendingFileRequestId = requestId
        saveFileLauncher.launch(suggestedName)
    }}

    fun triggerCraftUiHaptic(style: String) {{
        val view = window.decorView
        when (style) {{
            "selection" -> view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
            "heavy" -> vibrateOneShot(35L, 220)
            "medium" -> vibrateOneShot(20L, 150)
            "success", "warning", "error" -> vibrateOneShot(28L, 180)
            else -> vibrateOneShot(12L, 90)
        }}
    }}

    private fun vibrateOneShot(durationMs: Long, amplitude: Int) {{
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {{
            val vm = getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
            vm?.defaultVibrator?.vibrate(VibrationEffect.createOneShot(durationMs, amplitude))
        }} else {{
            @Suppress("DEPRECATION")
            val v = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {{
                v?.vibrate(VibrationEffect.createOneShot(durationMs, amplitude))
            }}
        }}
    }}

    fun shareCraftUiContent(title: String?, text: String?) {{
        val send = Intent(Intent.ACTION_SEND).apply {{
            type = "text/plain"
            text?.let {{ putExtra(Intent.EXTRA_TEXT, it) }}
            title?.let {{ putExtra(Intent.EXTRA_SUBJECT, it) }}
        }}
        startActivity(Intent.createChooser(send, title ?: "Share"))
    }}

    fun queryNetworkClassification(): Int {{
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return 0
        val network = cm.activeNetwork ?: return 1
        val caps = cm.getNetworkCapabilities(network) ?: return 1
        return when {{
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> 2
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> 3
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> 4
            else -> 0
        }}
    }}

    fun queryBatteryPercentAndCharging(): Pair<Int, Boolean> {{
        val bm = getSystemService(Context.BATTERY_SERVICE) as? BatteryManager ?: return Pair(-1, false)
        val pct = bm.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
        val charging = bm.isCharging
        return Pair(pct, charging)
    }}

    fun writeAppSecret(key: String, value: String) {{
        getSharedPreferences("craftui_secure_cache", Context.MODE_PRIVATE)
            .edit()
            .putString(key, value)
            .apply()
    }}

    fun deleteAppSecret(key: String) {{
        getSharedPreferences("craftui_secure_cache", Context.MODE_PRIVATE)
            .edit()
            .remove(key)
            .apply()
    }}

    private fun onNativePermissionResults(
        requestId: Long,
        grants: Map<String, Boolean>,
        rationales: Map<String, Boolean>
    ) {{
        val names = grants.keys.toTypedArray()
        val results = grants.values.map {{ if (it) 0 else -1 }}.toIntArray()
        val ratList = rationales.values.map {{ it }}.toBooleanArray()
        try {{
            nativeOnPermissionResults(requestId, names, results, ratList)
        }} catch (_: UnsatisfiedLinkError) {{
            // Native library not yet initialized or bridge unavailable
        }}
    }}

    private fun onNativeFileDialogResults(requestId: Long, uris: List<String>) {{
        try {{
            nativeOnFileDialogResults(requestId, uris.toTypedArray())
        }} catch (_: UnsatisfiedLinkError) {{
            // Native library not yet initialized or bridge unavailable
        }}
    }}

    private external fun nativeOnPermissionResults(
        requestId: Long,
        permissions: Array<String>,
        grantResults: IntArray,
        rationales: BooleanArray
    )

    private external fun nativeOnFileDialogResults(
        requestId: Long,
        uris: Array<String>
    )
}}
"#
    );

    if let Some(so_path) = built_so
        && so_path.is_file()
    {
        if let Ok(entries) = std::fs::read_dir(&jni_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "so") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let target_so = jni_dir.join(format!("lib{rust_lib_name}.so"));
        std::fs::copy(so_path, &target_so).map_err(|e| {
            CraftError::Platform(format!("failed copying .so {}: {e}", so_path.display()))
        })?;
    }

    std::fs::write(out_dir.join("settings.gradle.kts"), settings_gradle)
        .and_then(|_| std::fs::write(out_dir.join("build.gradle.kts"), root_gradle))
        .and_then(|_| std::fs::write(out_dir.join("app/build.gradle.kts"), app_gradle))
        .and_then(|_| {
            std::fs::write(
                out_dir.join("app/src/main/AndroidManifest.xml"),
                android_manifest,
            )
        })
        .and_then(|_| std::fs::write(res_values_dir.join("styles.xml"), styles_xml))
        .and_then(|_| std::fs::write(java_dir.join("MainActivity.kt"), main_activity))
        .map_err(|e| CraftError::Platform(format!("failed writing android template: {e}")))?;

    // `local.properties` tells Gradle where the Android SDK is. When it cannot
    // be worked out, the file is left out and Gradle falls back to ANDROID_HOME
    // or reports a clear "SDK location not found" error.
    let sdk_dir = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .or_else(|_| std::env::var("HOME").map(|home| format!("{home}/Android/Sdk")));
    if let Ok(sdk_dir) = sdk_dir {
        let _ = std::fs::write(
            out_dir.join("local.properties"),
            format!("sdk.dir={}\n", sdk_dir.replace('\\', "/")),
        );
    }
    let _ = std::fs::write(
        out_dir.join("gradle.properties"),
        "android.useAndroidX=true\n",
    );

    Ok(())
}
