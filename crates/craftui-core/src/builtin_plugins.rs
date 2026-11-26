//! Built-in first-party `MethodChannel` plugins and typed client facades for CraftUI.
//!
//! Covers the 4 core mobile/desktop capabilities (`camera`, `biometrics`,
//! `notifications`, and `url_launcher`) using uniform channel names and JSON
//! schemas shared across `craftui-core`, Android `MainActivity.kt`, and iOS
//! `CraftUIViewController.swift`.

use crate::error::{CraftError, Result};
use crate::plugin::{CraftPlugin, MethodCall, PluginRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const CHANNEL_CAMERA: &str = "dev.craftui.camera";
pub const CHANNEL_BIOMETRICS: &str = "dev.craftui.biometrics";
pub const CHANNEL_NOTIFICATIONS: &str = "dev.craftui.notifications";
pub const CHANNEL_URL_LAUNCHER: &str = "dev.craftui.url_launcher";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraResult {
    pub uri: Option<String>,
    pub cancelled: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiometricsAvailability {
    pub available: bool,
    pub biometry_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BiometricsAuthResult {
    pub authenticated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSpec {
    pub id: String,
    pub title: String,
    pub body: String,
    pub delay_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationScheduleResult {
    pub scheduled: bool,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlCanLaunchResult {
    pub can_launch: bool,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlLaunchResult {
    pub launched: bool,
    pub url: String,
}

/// Default/in-memory [`CraftPlugin`] implementation for `dev.craftui.camera`.
#[derive(Debug)]
pub struct CameraPlugin {
    captured_uri: Mutex<String>,
    gallery_uri: Mutex<String>,
}

impl Default for CameraPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraPlugin {
    pub fn new() -> Self {
        Self {
            captured_uri: Mutex::new("craftui://camera/mock_capture.jpg".to_string()),
            gallery_uri: Mutex::new("craftui://gallery/mock_photo.jpg".to_string()),
        }
    }

    pub fn set_mock_uris(&self, capture_uri: impl Into<String>, gallery_uri: impl Into<String>) {
        if let Ok(mut g) = self.captured_uri.lock() {
            *g = capture_uri.into();
        }
        if let Ok(mut g) = self.gallery_uri.lock() {
            *g = gallery_uri.into();
        }
    }
}

impl CraftPlugin for CameraPlugin {
    fn name(&self) -> &str {
        CHANNEL_CAMERA
    }

    fn handle(&self, call: &MethodCall) -> Result<Value> {
        match call.method.as_str() {
            "capturePhoto" => {
                let uri = self
                    .captured_uri
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                serde_json::to_value(CameraResult {
                    uri: (!uri.is_empty()).then_some(uri),
                    cancelled: false,
                    source: "camera".to_string(),
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            "pickImage" => {
                let uri = self
                    .gallery_uri
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                serde_json::to_value(CameraResult {
                    uri: (!uri.is_empty()).then_some(uri),
                    cancelled: false,
                    source: "gallery".to_string(),
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            other => Err(CraftError::Platform(format!(
                "unknown camera method `{other}`"
            ))),
        }
    }
}

/// Default/in-memory [`CraftPlugin`] implementation for `dev.craftui.biometrics`.
#[derive(Debug)]
pub struct BiometricsPlugin {
    available: Mutex<bool>,
    biometry_type: Mutex<String>,
    should_authenticate: Mutex<bool>,
}

impl Default for BiometricsPlugin {
    fn default() -> Self {
        Self::new(true, "biometric")
    }
}

impl BiometricsPlugin {
    pub fn new(available: bool, biometry_type: impl Into<String>) -> Self {
        Self {
            available: Mutex::new(available),
            biometry_type: Mutex::new(biometry_type.into()),
            should_authenticate: Mutex::new(available),
        }
    }

    pub fn set_state(
        &self,
        available: bool,
        biometry_type: impl Into<String>,
        should_authenticate: bool,
    ) {
        if let Ok(mut g) = self.available.lock() {
            *g = available;
        }
        if let Ok(mut g) = self.biometry_type.lock() {
            *g = biometry_type.into();
        }
        if let Ok(mut g) = self.should_authenticate.lock() {
            *g = should_authenticate;
        }
    }
}

impl CraftPlugin for BiometricsPlugin {
    fn name(&self) -> &str {
        CHANNEL_BIOMETRICS
    }

    fn handle(&self, call: &MethodCall) -> Result<Value> {
        match call.method.as_str() {
            "isAvailable" => {
                let available = self.available.lock().map(|g| *g).unwrap_or(false);
                let biometry_type = self
                    .biometry_type
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_else(|_| "none".to_string());
                serde_json::to_value(BiometricsAvailability {
                    available,
                    biometry_type,
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            "authenticate" => {
                let available = self.available.lock().map(|g| *g).unwrap_or(false);
                let ok = available && self.should_authenticate.lock().map(|g| *g).unwrap_or(false);
                let error = (!ok).then(|| {
                    if available {
                        "authentication declined".to_string()
                    } else {
                        "biometrics unavailable".to_string()
                    }
                });
                serde_json::to_value(BiometricsAuthResult {
                    authenticated: ok,
                    error,
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            other => Err(CraftError::Platform(format!(
                "unknown biometrics method `{other}`"
            ))),
        }
    }
}

/// Default/in-memory [`CraftPlugin`] implementation for `dev.craftui.notifications`.
#[derive(Debug, Default)]
pub struct NotificationsPlugin {
    scheduled: Mutex<HashMap<String, NotificationSpec>>,
}

impl NotificationsPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scheduled_notifications(&self) -> Vec<NotificationSpec> {
        self.scheduled
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }
}

impl CraftPlugin for NotificationsPlugin {
    fn name(&self) -> &str {
        CHANNEL_NOTIFICATIONS
    }

    fn handle(&self, call: &MethodCall) -> Result<Value> {
        match call.method.as_str() {
            "schedule" => {
                let id = extract_notification_id(&call.args).ok_or_else(|| {
                    CraftError::Platform("notifications.schedule requires `id`".to_string())
                })?;
                let title = call
                    .args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CraftUI")
                    .to_string();
                let body = call
                    .args
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let delay_seconds = call
                    .args
                    .get("delaySeconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if let Ok(mut map) = self.scheduled.lock() {
                    map.insert(
                        id.clone(),
                        NotificationSpec {
                            id: id.clone(),
                            title,
                            body,
                            delay_seconds,
                        },
                    );
                }
                serde_json::to_value(NotificationScheduleResult {
                    scheduled: true,
                    id,
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            "cancel" => {
                let id = extract_notification_id(&call.args).ok_or_else(|| {
                    CraftError::Platform("notifications.cancel requires `id`".to_string())
                })?;
                let removed = self
                    .scheduled
                    .lock()
                    .map(|mut map| map.remove(&id).is_some())
                    .unwrap_or(false);
                Ok(json!({ "cancelled": removed, "id": id }))
            }
            "cancelAll" => {
                let count = self
                    .scheduled
                    .lock()
                    .map(|mut map| {
                        let len = map.len();
                        map.clear();
                        len
                    })
                    .unwrap_or(0);
                Ok(json!({ "cancelledAll": true, "cleared": count }))
            }
            other => Err(CraftError::Platform(format!(
                "unknown notifications method `{other}`"
            ))),
        }
    }
}

fn extract_notification_id(args: &Value) -> Option<String> {
    args.get("id").and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            v.as_u64().map(|n| n.to_string())
        }
    })
}

/// Default/in-memory [`CraftPlugin`] implementation for `dev.craftui.url_launcher`.
#[derive(Debug, Default)]
pub struct UrlLauncherPlugin {
    launched_urls: Mutex<Vec<String>>,
}

impl UrlLauncherPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn launched_urls(&self) -> Vec<String> {
        self.launched_urls
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn is_supported_scheme(url: &str) -> bool {
        let lower = url.trim().to_ascii_lowercase();
        lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:")
            || lower.starts_with("tel:")
    }
}

impl CraftPlugin for UrlLauncherPlugin {
    fn name(&self) -> &str {
        CHANNEL_URL_LAUNCHER
    }

    fn handle(&self, call: &MethodCall) -> Result<Value> {
        let url = call
            .args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CraftError::Platform("url_launcher requires `url` string".to_string()))?
            .to_string();
        let allowed = Self::is_supported_scheme(&url);
        match call.method.as_str() {
            "canLaunch" => serde_json::to_value(UrlCanLaunchResult {
                can_launch: allowed,
                url,
            })
            .map_err(|e| CraftError::Platform(e.to_string())),
            "launch" => {
                if allowed && let Ok(mut list) = self.launched_urls.lock() {
                    list.push(url.clone());
                }
                serde_json::to_value(UrlLaunchResult {
                    launched: allowed,
                    url,
                })
                .map_err(|e| CraftError::Platform(e.to_string()))
            }
            other => Err(CraftError::Platform(format!(
                "unknown url_launcher method `{other}`"
            ))),
        }
    }
}

/// Typed Rust client wrapper around `dev.craftui.camera` on a [`PluginRegistry`].
pub struct CameraClient<'a>(pub &'a PluginRegistry);

impl<'a> CameraClient<'a> {
    pub fn capture_photo(&self) -> Result<CameraResult> {
        let val = self.0.invoke(
            CHANNEL_CAMERA,
            "capturePhoto",
            json!({ "source": "camera" }),
        )?;
        serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))
    }

    pub fn pick_image(&self) -> Result<CameraResult> {
        let val = self
            .0
            .invoke(CHANNEL_CAMERA, "pickImage", json!({ "source": "gallery" }))?;
        serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))
    }
}

/// Typed Rust client wrapper around `dev.craftui.biometrics` on a [`PluginRegistry`].
pub struct BiometricsClient<'a>(pub &'a PluginRegistry);

impl<'a> BiometricsClient<'a> {
    pub fn is_available(&self) -> Result<BiometricsAvailability> {
        let val = self
            .0
            .invoke(CHANNEL_BIOMETRICS, "isAvailable", json!({}))?;
        serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))
    }

    pub fn authenticate(&self, reason: &str) -> Result<BiometricsAuthResult> {
        let val = self.0.invoke(
            CHANNEL_BIOMETRICS,
            "authenticate",
            json!({ "reason": reason }),
        )?;
        serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))
    }
}

/// Typed Rust client wrapper around `dev.craftui.notifications` on a [`PluginRegistry`].
pub struct NotificationsClient<'a>(pub &'a PluginRegistry);

impl<'a> NotificationsClient<'a> {
    pub fn schedule(&self, spec: &NotificationSpec) -> Result<NotificationScheduleResult> {
        let val = self.0.invoke(
            CHANNEL_NOTIFICATIONS,
            "schedule",
            serde_json::to_value(spec).map_err(|e| CraftError::Platform(e.to_string()))?,
        )?;
        serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))
    }

    pub fn cancel(&self, id: &str) -> Result<bool> {
        let val = self
            .0
            .invoke(CHANNEL_NOTIFICATIONS, "cancel", json!({ "id": id }))?;
        Ok(val
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub fn cancel_all(&self) -> Result<()> {
        let _ = self
            .0
            .invoke(CHANNEL_NOTIFICATIONS, "cancelAll", json!({}))?;
        Ok(())
    }
}

/// Typed Rust client wrapper around `dev.craftui.url_launcher` on a [`PluginRegistry`].
pub struct UrlLauncherClient<'a>(pub &'a PluginRegistry);

impl<'a> UrlLauncherClient<'a> {
    pub fn can_launch(&self, url: &str) -> Result<bool> {
        let val = self
            .0
            .invoke(CHANNEL_URL_LAUNCHER, "canLaunch", json!({ "url": url }))?;
        let parsed: UrlCanLaunchResult =
            serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))?;
        Ok(parsed.can_launch)
    }

    pub fn launch(&self, url: &str) -> Result<bool> {
        let val = self
            .0
            .invoke(CHANNEL_URL_LAUNCHER, "launch", json!({ "url": url }))?;
        let parsed: UrlLaunchResult =
            serde_json::from_value(val).map_err(|e| CraftError::Platform(e.to_string()))?;
        Ok(parsed.launched)
    }
}

impl PluginRegistry {
    /// Register the 4 built-in mock/in-memory plugins (`camera`, `biometrics`,
    /// `notifications`, `url_launcher`) onto this registry.
    pub fn register_builtin_plugins(&self) {
        self.register_arc(Arc::new(CameraPlugin::new()));
        self.register_arc(Arc::new(BiometricsPlugin::default()));
        self.register_arc(Arc::new(NotificationsPlugin::new()));
        self.register_arc(Arc::new(UrlLauncherPlugin::new()));
    }

    /// Construct a [`PluginRegistry`] pre-populated with all 4 built-in plugins.
    pub fn with_builtin_plugins() -> Self {
        let reg = Self::new();
        reg.register_builtin_plugins();
        reg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_plugins_roundtrip_all_four_channels() {
        let registry = PluginRegistry::with_builtin_plugins();
        assert!(registry.has_channel(CHANNEL_CAMERA));
        assert!(registry.has_channel(CHANNEL_BIOMETRICS));
        assert!(registry.has_channel(CHANNEL_NOTIFICATIONS));
        assert!(registry.has_channel(CHANNEL_URL_LAUNCHER));

        let cam = CameraClient(&registry);
        let photo = cam.capture_photo().expect("capture_photo succeeds");
        assert!(!photo.cancelled);
        assert!(photo.uri.as_deref().unwrap().contains("mock_capture"));
        let picked = cam.pick_image().expect("pick_image succeeds");
        assert_eq!(picked.source, "gallery");

        let bio = BiometricsClient(&registry);
        let avail = bio.is_available().expect("is_available succeeds");
        assert!(avail.available);
        let auth = bio
            .authenticate("Confirm action")
            .expect("authenticate succeeds");
        assert!(auth.authenticated);

        let notif = NotificationsClient(&registry);
        let scheduled = notif
            .schedule(&NotificationSpec {
                id: "n-1".to_string(),
                title: "Reminder".to_string(),
                body: "Deploy build".to_string(),
                delay_seconds: 5,
            })
            .expect("schedule succeeds");
        assert!(scheduled.scheduled);
        assert!(notif.cancel("n-1").expect("cancel succeeds"));
        assert!(!notif.cancel("n-1").expect("second cancel returns false"));
        notif.cancel_all().expect("cancel_all succeeds");

        let launcher = UrlLauncherClient(&registry);
        assert!(launcher.can_launch("https://craftui.dev").unwrap());
        assert!(!launcher.can_launch("javascript:alert(1)").unwrap());
        assert!(launcher.launch("https://craftui.dev").unwrap());
        assert!(!launcher.launch("file:///etc/passwd").unwrap());
    }
}
