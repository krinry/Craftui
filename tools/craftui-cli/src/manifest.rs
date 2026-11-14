use craftui_core::{AndroidPermissionSpec, CraftError, Permission, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Parsed `craftui.toml` manifest controlling cross-platform app metadata & permissions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppManifest {
    pub app: AppSection,
    pub android: AndroidSection,
    pub ios: IosSection,
    pub permissions: BTreeMap<String, PermissionDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSection {
    pub name: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidSection {
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub version_code: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IosSection {
    pub deployment_target: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDeclaration {
    pub key: String,
    pub permission: Permission,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDiagnostic {
    pub is_error: bool,
    pub message: String,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            name: "CraftUI App".to_string(),
            id: "dev.craftui.app".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

impl Default for AndroidSection {
    fn default() -> Self {
        Self {
            min_sdk: 24,
            target_sdk: 35,
            version_code: 1,
            features: Vec::new(),
        }
    }
}

impl Default for IosSection {
    fn default() -> Self {
        Self {
            deployment_target: "15.0".to_string(),
            capabilities: Vec::new(),
        }
    }
}

impl AppManifest {
    pub fn with_app(name: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            app: AppSection {
                name: name.into(),
                id: id.into(),
                version: "0.1.0".to_string(),
            },
            ..Default::default()
        }
    }

    pub fn sanitize_id(name: &str) -> String {
        let mut slug = String::with_capacity(name.len());
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
            } else if (ch == '_' || ch == '-' || ch == ' ') && !slug.ends_with('_') {
                slug.push('_');
            }
        }
        let trimmed = slug.trim_matches('_');
        if trimmed.is_empty() {
            "dev.craftui.app".to_string()
        } else {
            format!("dev.craftui.{trimmed}")
        }
    }

    pub fn load_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            CraftError::Platform(format!("failed reading manifest {}: {e}", path.display()))
        })?;
        Self::parse(&raw)
    }

    pub fn parse(src: &str) -> Result<Self> {
        let mut manifest = Self::default();
        let mut section = String::new();

        for (line_idx, raw_line) in src.lines().enumerate() {
            let line_no = line_idx + 1;
            let stripped = strip_toml_comment(raw_line).trim();
            if stripped.is_empty() {
                continue;
            }

            if stripped.starts_with('[') && stripped.ends_with(']') {
                section = stripped[1..stripped.len() - 1].trim().to_ascii_lowercase();
                continue;
            }

            let Some((raw_key, raw_val)) = stripped.split_once('=') else {
                return Err(CraftError::Platform(format!(
                    "craftui.toml line {line_no}: expected `key = value`, found `{stripped}`"
                )));
            };
            let key = unquote_key(raw_key.trim());
            let val = raw_val.trim();

            match section.as_str() {
                "" | "app" => match key.as_str() {
                    "name" => manifest.app.name = parse_toml_string(val, line_no)?,
                    "id" | "bundle_id" | "application_id" => {
                        manifest.app.id = parse_toml_string(val, line_no)?;
                    }
                    "version" => manifest.app.version = parse_toml_string(val, line_no)?,
                    _ => {}
                },
                "android" => match key.as_str() {
                    "min_sdk" => manifest.android.min_sdk = parse_toml_u32(val, line_no)?,
                    "target_sdk" => manifest.android.target_sdk = parse_toml_u32(val, line_no)?,
                    "version_code" => {
                        manifest.android.version_code = parse_toml_u32(val, line_no)?;
                    }
                    "features" => {
                        manifest.android.features = parse_toml_string_array(val, line_no)?
                    }
                    _ => {}
                },
                "ios" => match key.as_str() {
                    "deployment_target" => {
                        manifest.ios.deployment_target = parse_toml_string(val, line_no)?;
                    }
                    "capabilities" => {
                        manifest.ios.capabilities = parse_toml_string_array(val, line_no)?;
                    }
                    _ => {}
                },
                "permissions" => {
                    let permission = Permission::from_key(&key);
                    let canonical = permission.key().into_owned();
                    let (enabled, reason) = if val == "true" {
                        (true, None)
                    } else if val == "false" {
                        (false, None)
                    } else if val.starts_with('"') || val.starts_with('\'') {
                        let s = parse_toml_string(val, line_no)?;
                        let reason = if s.trim().is_empty() { None } else { Some(s) };
                        (true, reason)
                    } else {
                        return Err(CraftError::Platform(format!(
                            "craftui.toml line {line_no}: expected usage description string or bool for permission `{key}`"
                        )));
                    };
                    manifest.permissions.insert(
                        canonical.clone(),
                        PermissionDeclaration {
                            key: canonical,
                            permission,
                            enabled,
                            reason,
                        },
                    );
                }
                other => {
                    return Err(CraftError::Platform(format!(
                        "craftui.toml line {line_no}: unknown section `[{other}]`"
                    )));
                }
            }
        }

        Ok(manifest)
    }

    pub fn enabled_permissions(&self) -> Vec<&PermissionDeclaration> {
        self.permissions.values().filter(|p| p.enabled).collect()
    }

    /// Resolve deduplicated Android `<uses-permission>` entries matching SDK bounds in `[android]`.
    pub fn android_manifest_permissions(&self) -> Vec<AndroidPermissionSpec> {
        let mut specs: Vec<AndroidPermissionSpec> =
            vec![AndroidPermissionSpec::new("android.permission.INTERNET")];
        for decl in self.enabled_permissions() {
            for spec in decl.permission.android_permissions() {
                if let Some(max_sdk) = spec.max_sdk
                    && self.android.min_sdk > max_sdk
                {
                    // Never emit obsolete maxSdkVersion permissions excluded by app min_sdk.
                    continue;
                }
                if !specs.contains(&spec) {
                    specs.push(spec);
                }
            }
        }
        specs
    }

    /// Resolve optional `<uses-feature android:required="false" />` declarations so Play Store keeps broad device compatibility.
    pub fn android_uses_features(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut push_unique = |name: &str| {
            if !out.iter().any(|existing| existing == name) {
                out.push(name.to_string());
            }
        };

        for decl in self.enabled_permissions() {
            for feat in android_features_for_permission(&decl.permission) {
                push_unique(feat);
            }
        }
        for custom in &self.android.features {
            if !custom.trim().is_empty() {
                push_unique(custom.trim());
            }
        }
        out
    }

    pub fn render_android_manifest_permissions_xml(&self) -> String {
        let mut out = String::new();
        for spec in self.android_manifest_permissions() {
            let escaped_name = xml_escape(&spec.name);
            match (spec.max_sdk, spec.min_sdk) {
                (Some(max_sdk), Some(min_sdk)) => {
                    let _ = writeln!(
                        out,
                        "    <!-- minSdk {min_sdk} -->\n    <uses-permission android:name=\"{escaped_name}\" android:maxSdkVersion=\"{max_sdk}\" />"
                    );
                }
                (Some(max_sdk), None) => {
                    let _ = writeln!(
                        out,
                        "    <uses-permission android:name=\"{escaped_name}\" android:maxSdkVersion=\"{max_sdk}\" />"
                    );
                }
                (None, Some(min_sdk)) => {
                    let _ = writeln!(
                        out,
                        "    <!-- minSdk {min_sdk} -->\n    <uses-permission android:name=\"{escaped_name}\" />"
                    );
                }
                (None, None) => {
                    let _ = writeln!(
                        out,
                        "    <uses-permission android:name=\"{escaped_name}\" />"
                    );
                }
            }
        }
        for feat in self.android_uses_features() {
            let _ = writeln!(
                out,
                "    <uses-feature android:name=\"{}\" android:required=\"false\" />",
                xml_escape(&feat)
            );
        }
        out
    }

    /// Resolve deduplicated iOS `Info.plist` `NS*UsageDescription` key/value pairs.
    pub fn ios_info_plist_usage_entries(&self) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for decl in self.enabled_permissions() {
            let reason = decl
                .reason
                .as_deref()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| default_usage_description(&decl.permission));
            for plist_key in decl.permission.ios_usage_keys() {
                if !entries.iter().any(|(k, _)| k == plist_key) {
                    entries.push(((*plist_key).to_string(), reason.to_string()));
                }
            }
        }
        entries
    }

    /// Resolve recommended `UIBackgroundModes` entries from declared permissions and `[ios].capabilities`.
    pub fn ios_background_modes(&self) -> Vec<String> {
        let mut modes = Vec::new();
        let mut push_mode = |m: &str| {
            if !modes.iter().any(|x| x == m) {
                modes.push(m.to_string());
            }
        };
        for decl in self.enabled_permissions() {
            match decl.permission {
                Permission::LocationAlways => push_mode("location"),
                Permission::BackgroundRefresh => push_mode("fetch"),
                Permission::Bluetooth
                | Permission::BluetoothScan
                | Permission::BluetoothConnect => push_mode("bluetooth-central"),
                Permission::BluetoothAdvertise => push_mode("bluetooth-peripheral"),
                Permission::Notifications => push_mode("remote-notification"),
                _ => {}
            }
        }
        for cap in &self.ios.capabilities {
            let c = cap.trim();
            if !c.is_empty() {
                push_mode(c);
            }
        }
        modes
    }

    /// Validate manifest completeness across Android, iOS, and Web targets.
    pub fn validate(&self) -> Vec<ManifestDiagnostic> {
        let mut diags = Vec::new();
        if self.app.name.trim().is_empty() {
            diags.push(ManifestDiagnostic {
                is_error: true,
                message: "[app].name cannot be empty".to_string(),
            });
        }
        if !self.app.id.contains('.') {
            diags.push(ManifestDiagnostic {
                is_error: true,
                message: format!(
                    "[app].id `{}` must use reverse-DNS format (e.g., `dev.craftui.myapp`)",
                    self.app.id
                ),
            });
        }
        if self.android.min_sdk < 21 || self.android.min_sdk > self.android.target_sdk {
            diags.push(ManifestDiagnostic {
                is_error: true,
                message: format!(
                    "[android] min_sdk ({}) must be >= 21 and <= target_sdk ({})",
                    self.android.min_sdk, self.android.target_sdk
                ),
            });
        }
        for decl in self.enabled_permissions() {
            if matches!(decl.permission, Permission::Custom(_)) {
                diags.push(ManifestDiagnostic {
                    is_error: false,
                    message: format!(
                        "permission `{}` is custom (passed verbatim to Android manifest; verify manual iOS/Web handling)",
                        decl.key
                    ),
                });
            }
            if !decl.permission.ios_usage_keys().is_empty()
                && decl.reason.as_deref().is_none_or(|r| r.trim().is_empty())
            {
                diags.push(ManifestDiagnostic {
                    is_error: false,
                    message: format!(
                        "permission `{}` uses auto-generated default iOS usage string; provide an explicit description in [permissions] for App Store review",
                        decl.key
                    ),
                });
            }
            if decl.permission == Permission::LocationAlways
                && !self.permissions.contains_key("location_when_in_use")
            {
                diags.push(ManifestDiagnostic {
                    is_error: false,
                    message: "permission `location_always`: Android 11+ & iOS require requesting `location_when_in_use` before upgrading to background location".to_string(),
                });
            }
        }
        diags
    }

    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "[app]");
        let _ = writeln!(out, "name = {}", toml_quote(&self.app.name));
        let _ = writeln!(out, "id = {}", toml_quote(&self.app.id));
        let _ = writeln!(out, "version = {}", toml_quote(&self.app.version));
        let _ = writeln!(out);
        let _ = writeln!(out, "[android]");
        let _ = writeln!(out, "min_sdk = {}", self.android.min_sdk);
        let _ = writeln!(out, "target_sdk = {}", self.android.target_sdk);
        let _ = writeln!(out, "version_code = {}", self.android.version_code);
        if !self.android.features.is_empty() {
            let quoted: Vec<String> = self
                .android
                .features
                .iter()
                .map(|s| toml_quote(s))
                .collect();
            let _ = writeln!(out, "features = [{}]", quoted.join(", "));
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "[ios]");
        let _ = writeln!(
            out,
            "deployment_target = {}",
            toml_quote(&self.ios.deployment_target)
        );
        if !self.ios.capabilities.is_empty() {
            let quoted: Vec<String> = self
                .ios
                .capabilities
                .iter()
                .map(|s| toml_quote(s))
                .collect();
            let _ = writeln!(out, "capabilities = [{}]", quoted.join(", "));
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "[permissions]");
        if self.permissions.is_empty() {
            let _ = writeln!(
                out,
                "# Declare native mobile/web permissions and usage prompts here:"
            );
            let _ = writeln!(out, "# camera = \"Capture photos and scan QR codes.\"");
            let _ = writeln!(
                out,
                "# notifications = \"Deliver live alerts and reminders.\""
            );
        } else {
            for decl in self.permissions.values() {
                if !decl.enabled {
                    let _ = writeln!(out, "{} = false", decl.key);
                } else if let Some(ref reason) = decl.reason {
                    let _ = writeln!(out, "{} = {}", decl.key, toml_quote(reason));
                } else {
                    let _ = writeln!(out, "{} = true", decl.key);
                }
            }
        }
        out
    }
}

pub fn find_project_manifest(start: &Path) -> Option<PathBuf> {
    let candidate = if start.is_file() {
        if start.file_name().and_then(|n| n.to_str()) == Some("craftui.toml") {
            return Some(start.to_path_buf());
        }
        start.parent().map(|p| p.join("craftui.toml"))
    } else {
        Some(start.join("craftui.toml"))
    };
    candidate.filter(|p| p.is_file())
}

pub fn android_features_for_permission(perm: &Permission) -> &'static [&'static str] {
    match perm {
        Permission::Camera => &[
            "android.hardware.camera",
            "android.hardware.camera.autofocus",
        ],
        Permission::Microphone => &["android.hardware.microphone"],
        Permission::LocationWhenInUse | Permission::LocationAlways | Permission::LocationCoarse => {
            &["android.hardware.location.gps"]
        }
        Permission::Bluetooth
        | Permission::BluetoothScan
        | Permission::BluetoothConnect
        | Permission::BluetoothAdvertise => &[
            "android.hardware.bluetooth",
            "android.hardware.bluetooth_le",
        ],
        Permission::Biometrics => &["android.hardware.fingerprint"],
        Permission::Nfc => &["android.hardware.nfc"],
        Permission::PhoneState
        | Permission::CallPhone
        | Permission::SmsRead
        | Permission::SmsSend => &["android.hardware.telephony"],
        _ => &[],
    }
}

pub fn web_permission_name(perm: &Permission) -> Option<&'static str> {
    match perm {
        Permission::Camera => Some("camera"),
        Permission::Microphone => Some("microphone"),
        Permission::LocationWhenInUse | Permission::LocationCoarse | Permission::LocationAlways => {
            Some("geolocation")
        }
        Permission::Notifications => Some("notifications"),
        Permission::ClipboardRead => Some("clipboard-read"),
        Permission::Bluetooth | Permission::BluetoothScan | Permission::BluetoothConnect => {
            Some("bluetooth")
        }
        Permission::Nfc => Some("nfc"),
        Permission::Sensors | Permission::MotionActivity => Some("accelerometer"),
        Permission::MediaProjection => Some("display-capture"),
        Permission::BackgroundRefresh => Some("periodic-background-sync"),
        _ => None,
    }
}

pub fn default_usage_description(perm: &Permission) -> &'static str {
    match perm {
        Permission::Camera => "Camera access is used to capture photos and scan visual codes.",
        Permission::Microphone => "Microphone access is used to record audio and voice input.",
        Permission::LocationWhenInUse | Permission::LocationCoarse => {
            "Location access is used while the app is open to power nearby features."
        }
        Permission::LocationAlways => {
            "Location access enables foreground and background location updates."
        }
        Permission::Photos => "Photo library access allows selecting and editing saved images.",
        Permission::PhotosAddOnly => {
            "Photo library write access saves captured media to your device."
        }
        Permission::Videos => "Video library access allows choosing and importing video clips.",
        Permission::AudioLibrary => {
            "Audio library access allows picking local music or recordings."
        }
        Permission::Notifications => "Notifications deliver timely updates, reminders, and alerts.",
        Permission::Bluetooth
        | Permission::BluetoothScan
        | Permission::BluetoothConnect
        | Permission::BluetoothAdvertise => {
            "Bluetooth access pairs and communicates with nearby accessories."
        }
        Permission::Biometrics => {
            "Biometrics protect sensitive actions with Face ID or fingerprint."
        }
        Permission::Contacts => {
            "Contacts access lets you pick people to share with or collaborate."
        }
        Permission::CalendarRead | Permission::CalendarWrite => {
            "Calendar access displays and schedules upcoming events."
        }
        Permission::Sensors | Permission::MotionActivity => {
            "Motion and sensor access tracks device orientation and activity state."
        }
        Permission::StorageRead | Permission::StorageWrite | Permission::ManageExternalStorage => {
            "Storage access reads and exports user-selected documents."
        }
        Permission::Nfc => "NFC access reads and writes nearby contactless tags.",
        Permission::PhoneState
        | Permission::CallPhone
        | Permission::SmsRead
        | Permission::SmsSend => "Telephony permissions integrate calling and verification flows.",
        Permission::SystemAlertWindow => "Overlay permission renders floating status controls.",
        Permission::ScheduleExactAlarm => {
            "Exact alarm permission triggers scheduled local reminders."
        }
        Permission::InstallPackages => "Package installation permission applies in-app updates.",
        Permission::Vibrate => "Haptic vibration provides subtle tactile interaction feedback.",
        Permission::BackgroundRefresh => "Background refresh keeps application state synchronized.",
        Permission::MediaProjection => "Screen capture records or shares visible app content.",
        Permission::ClipboardRead => "Clipboard access pastes copied text or snippets on demand.",
        Permission::DeviceAdmin => {
            "Device administrator permission enforces enterprise policies and device management."
        }
        Permission::AccessibilityService => {
            "Accessibility service assists users with screen reading, automation, and UI navigation."
        }
        Permission::WriteSettings => {
            "System settings permission modifies system preferences and display attributes."
        }
        Permission::PackageUsageStats => {
            "Package usage statistics track application usage and active intervals."
        }
        Permission::IgnoreBatteryOptimizations => {
            "Battery optimization exemption allows uninterrupted background processing."
        }
        Permission::NotificationListener => {
            "Notification listener service reads and routes system notifications."
        }
        Permission::VpnService => "VPN service establishes secure encrypted network tunnels.",
        Permission::AdminPrivileges => {
            "Administrator privileges execute elevated system management tasks."
        }
        Permission::Custom(_) => "Requested to enable a custom application capability.",
    }
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn toml_quote(s: &str) -> String {
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

fn strip_toml_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '#' && !in_double && !in_single {
            return &line[..idx];
        }
    }
    line
}

fn unquote_key(k: &str) -> String {
    if (k.starts_with('"') && k.ends_with('"')) || (k.starts_with('\'') && k.ends_with('\'')) {
        k[1..k.len().saturating_sub(1)].to_string()
    } else {
        k.to_string()
    }
}

fn parse_toml_string(val: &str, line_no: usize) -> Result<String> {
    if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
        let inner = &val[1..val.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {
                        return Err(CraftError::Platform(format!(
                            "craftui.toml line {line_no}: unterminated escape"
                        )));
                    }
                }
            } else {
                out.push(ch);
            }
        }
        Ok(out)
    } else if val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2 {
        Ok(val[1..val.len() - 1].to_string())
    } else {
        Err(CraftError::Platform(format!(
            "craftui.toml line {line_no}: expected quoted string, found `{val}`"
        )))
    }
}

fn parse_toml_u32(val: &str, line_no: usize) -> Result<u32> {
    val.parse::<u32>().map_err(|e| {
        CraftError::Platform(format!(
            "craftui.toml line {line_no}: invalid integer `{val}`: {e}"
        ))
    })
}

fn parse_toml_string_array(val: &str, line_no: usize) -> Result<Vec<String>> {
    let trimmed = val.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Err(CraftError::Platform(format!(
            "craftui.toml line {line_no}: expected array `[...]`, found `{val}`"
        )));
    }
    let inner = trimmed[1..trimmed.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            cur.push(ch);
            escaped = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            cur.push(ch);
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
            cur.push(ch);
        } else if ch == ',' && !in_double && !in_single {
            let item = cur.trim();
            if !item.is_empty() {
                out.push(parse_toml_string(item, line_no)?);
            }
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    let item = cur.trim();
    if !item.is_empty() {
        out.push(parse_toml_string(item, line_no)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roundtrips_and_validates_craftui_toml_across_all_35_permissions() {
        let mut toml = String::from(
            r#"[app]
name = "CraftPay & Studio"
id = "dev.craftui.pay"
version = "1.2.3"

[android]
min_sdk = 24
target_sdk = 35
version_code = 7

[ios]
deployment_target = "16.0"

[permissions]
"#,
        );
        for perm in Permission::all_standard() {
            let _ = writeln!(
                toml,
                "{} = {}",
                perm.key(),
                toml_quote(&format!("Use {} safely & securely", perm.key()))
            );
        }
        toml.push_str("vibrate = true # inline comment ignored\n");

        let manifest = AppManifest::parse(&toml).expect("valid craftui.toml");
        assert_eq!(manifest.app.name, "CraftPay & Studio");
        assert_eq!(manifest.app.id, "dev.craftui.pay");
        assert_eq!(manifest.android.min_sdk, 24);
        assert_eq!(manifest.ios.deployment_target, "16.0");
        assert_eq!(
            manifest.enabled_permissions().len(),
            Permission::all_standard().len()
        );

        let xml = manifest.render_android_manifest_permissions_xml();
        assert!(xml.contains("android.permission.CAMERA"));
        assert!(xml.contains("android.permission.POST_NOTIFICATIONS"));
        assert!(xml.contains("android:maxSdkVersion=\"32\""));
        assert!(xml.contains("android:maxSdkVersion=\"30\""));
        assert!(xml.contains(
            "<uses-feature android:name=\"android.hardware.camera\" android:required=\"false\" />"
        ));

        let plist = manifest.ios_info_plist_usage_entries();
        assert!(
            plist
                .iter()
                .any(|(k, v)| k == "NSCameraUsageDescription" && v.contains("camera"))
        );
        assert!(plist.iter().any(|(k, _)| k == "NSFaceIDUsageDescription"));
        assert!(
            plist
                .iter()
                .any(|(k, _)| k == "NSLocationAlwaysAndWhenInUseUsageDescription")
        );

        let bg = manifest.ios_background_modes();
        assert!(bg.contains(&"location".to_string()));
        assert!(bg.contains(&"fetch".to_string()));
        assert!(bg.contains(&"bluetooth-central".to_string()));

        let roundtrip = AppManifest::parse(&manifest.to_toml()).expect("roundtrip toml");
        assert_eq!(roundtrip.app, manifest.app);
        assert_eq!(roundtrip.permissions.len(), manifest.permissions.len());
    }

    #[test]
    fn xml_escapes_and_checks_min_sdk_constraints() {
        let mut m = AppManifest::parse(
            r#"[app]
name = "A & B <Demo>"
id = "dev.craftui.ab"

[android]
min_sdk = 33
target_sdk = 35

[permissions]
photos = "Pick images"
camera = true
location_always = "Track runs"
"#,
        )
        .unwrap();

        let xml = m.render_android_manifest_permissions_xml();
        assert!(xml.contains("android.permission.READ_MEDIA_IMAGES"));
        // Because min_sdk = 33 > max_sdk = 32, legacy READ_EXTERNAL_STORAGE is omitted automatically!
        assert!(!xml.contains("READ_EXTERNAL_STORAGE"));

        let diags = m.validate();
        assert!(diags.iter().all(|d| !d.is_error));
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("camera") && d.message.contains("default iOS"))
        );
        assert!(diags.iter().any(|d| d.message.contains("location_always")));

        assert_eq!(xml_escape(&m.app.name), "A &amp; B &lt;Demo&gt;");
        m.app.id = "invalid_id".to_string();
        assert!(m.validate().iter().any(|d| d.is_error));
    }
}
