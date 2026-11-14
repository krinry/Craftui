use crate::UiDispatcher;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// File extension filter for native file open/save dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl FileFilter {
    pub fn new(
        name: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.into_iter().map(Into::into).collect(),
        }
    }
}

/// Request parameters for native file open / save / folder selection dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileDialogRequest {
    pub title: String,
    pub default_directory: Option<PathBuf>,
    pub default_name: Option<String>,
    pub filters: Vec<FileFilter>,
    pub multiple: bool,
    pub directory: bool,
}

impl FileDialogRequest {
    pub fn open(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn save(title: impl Into<String>, default_name: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            default_name: Some(default_name.into()),
            ..Default::default()
        }
    }

    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    pub fn with_directory(mut self, directory: bool) -> Self {
        self.directory = directory;
        self
    }
}

/// Cross-platform OS clipboard abstraction implemented per platform shell.
pub trait ClipboardBackend: Send {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str) -> bool;
}

/// Default in-memory clipboard fallback used in headless tests or unsupported hosts.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    text: String,
}

impl MemoryClipboard {
    pub fn new(initial: impl Into<String>) -> Self {
        Self {
            text: initial.into(),
        }
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn get_text(&mut self) -> Option<String> {
        Some(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        self.text.clear();
        self.text.push_str(text);
        true
    }
}

/// Cross-platform native file picker abstraction implemented per platform shell.
pub trait FileDialogBackend: Send + Sync {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf>;
    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf>;
}

/// Headless fallback file dialog backend returning no selection unless overridden.
#[derive(Debug, Default)]
pub struct NoopFileDialog;

impl FileDialogBackend for NoopFileDialog {
    fn open_files(&self, _request: &FileDialogRequest) -> Vec<PathBuf> {
        Vec::new()
    }

    fn save_file(&self, _request: &FileDialogRequest) -> Option<PathBuf> {
        None
    }
}

/// Deterministic mock file dialog backend useful in unit/integration tests.
#[derive(Debug, Default)]
pub struct MockFileDialog {
    pub open_result: Mutex<Vec<PathBuf>>,
    pub save_result: Mutex<Option<PathBuf>>,
}

impl MockFileDialog {
    pub fn new(open_files: Vec<PathBuf>, save_file: Option<PathBuf>) -> Self {
        Self {
            open_result: Mutex::new(open_files),
            save_result: Mutex::new(save_file),
        }
    }
}

impl FileDialogBackend for MockFileDialog {
    fn open_files(&self, _request: &FileDialogRequest) -> Vec<PathBuf> {
        self.open_result
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn save_file(&self, _request: &FileDialogRequest) -> Option<PathBuf> {
        self.save_result.lock().ok().and_then(|g| g.clone())
    }
}

/// Platform-neutral OS permission identifiers covering mobile, web, and desktop capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    Camera,
    Microphone,
    LocationWhenInUse,
    LocationAlways,
    LocationCoarse,
    Photos,
    PhotosAddOnly,
    Videos,
    AudioLibrary,
    Notifications,
    Bluetooth,
    BluetoothScan,
    BluetoothConnect,
    BluetoothAdvertise,
    Biometrics,
    Contacts,
    CalendarRead,
    CalendarWrite,
    Sensors,
    MotionActivity,
    StorageRead,
    StorageWrite,
    ManageExternalStorage,
    Nfc,
    PhoneState,
    CallPhone,
    SmsRead,
    SmsSend,
    SystemAlertWindow,
    ScheduleExactAlarm,
    InstallPackages,
    Vibrate,
    BackgroundRefresh,
    MediaProjection,
    ClipboardRead,
    DeviceAdmin,
    AccessibilityService,
    WriteSettings,
    PackageUsageStats,
    IgnoreBatteryOptimizations,
    NotificationListener,
    VpnService,
    AdminPrivileges,
    Custom(String),
}

const STANDARD_PERMISSIONS: &[Permission] = &[
    Permission::Camera,
    Permission::Microphone,
    Permission::LocationWhenInUse,
    Permission::LocationAlways,
    Permission::LocationCoarse,
    Permission::Photos,
    Permission::PhotosAddOnly,
    Permission::Videos,
    Permission::AudioLibrary,
    Permission::Notifications,
    Permission::Bluetooth,
    Permission::BluetoothScan,
    Permission::BluetoothConnect,
    Permission::BluetoothAdvertise,
    Permission::Biometrics,
    Permission::Contacts,
    Permission::CalendarRead,
    Permission::CalendarWrite,
    Permission::Sensors,
    Permission::MotionActivity,
    Permission::StorageRead,
    Permission::StorageWrite,
    Permission::ManageExternalStorage,
    Permission::Nfc,
    Permission::PhoneState,
    Permission::CallPhone,
    Permission::SmsRead,
    Permission::SmsSend,
    Permission::SystemAlertWindow,
    Permission::ScheduleExactAlarm,
    Permission::InstallPackages,
    Permission::Vibrate,
    Permission::BackgroundRefresh,
    Permission::MediaProjection,
    Permission::ClipboardRead,
    Permission::DeviceAdmin,
    Permission::AccessibilityService,
    Permission::WriteSettings,
    Permission::PackageUsageStats,
    Permission::IgnoreBatteryOptimizations,
    Permission::NotificationListener,
    Permission::VpnService,
    Permission::AdminPrivileges,
];

/// Android `<uses-permission>` manifest declaration specification with optional SDK version bounds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AndroidPermissionSpec {
    pub name: Cow<'static, str>,
    pub max_sdk: Option<u32>,
    pub min_sdk: Option<u32>,
}

impl AndroidPermissionSpec {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            max_sdk: None,
            min_sdk: None,
        }
    }

    pub const fn with_min_sdk(mut self, min_sdk: u32) -> Self {
        self.min_sdk = Some(min_sdk);
        self
    }

    pub const fn with_max_sdk(mut self, max_sdk: u32) -> Self {
        self.max_sdk = Some(max_sdk);
        self
    }
}

impl Permission {
    /// All built-in cross-platform permission variants (excluding [`Permission::Custom`]).
    pub fn all_standard() -> &'static [Permission] {
        STANDARD_PERMISSIONS
    }

    /// Canonical `snake_case` identifier for this permission (or verbatim custom key).
    pub fn key(&self) -> Cow<'_, str> {
        match self {
            Self::Camera => Cow::Borrowed("camera"),
            Self::Microphone => Cow::Borrowed("microphone"),
            Self::LocationWhenInUse => Cow::Borrowed("location_when_in_use"),
            Self::LocationAlways => Cow::Borrowed("location_always"),
            Self::LocationCoarse => Cow::Borrowed("location_coarse"),
            Self::Photos => Cow::Borrowed("photos"),
            Self::PhotosAddOnly => Cow::Borrowed("photos_add_only"),
            Self::Videos => Cow::Borrowed("videos"),
            Self::AudioLibrary => Cow::Borrowed("audio_library"),
            Self::Notifications => Cow::Borrowed("notifications"),
            Self::Bluetooth => Cow::Borrowed("bluetooth"),
            Self::BluetoothScan => Cow::Borrowed("bluetooth_scan"),
            Self::BluetoothConnect => Cow::Borrowed("bluetooth_connect"),
            Self::BluetoothAdvertise => Cow::Borrowed("bluetooth_advertise"),
            Self::Biometrics => Cow::Borrowed("biometrics"),
            Self::Contacts => Cow::Borrowed("contacts"),
            Self::CalendarRead => Cow::Borrowed("calendar_read"),
            Self::CalendarWrite => Cow::Borrowed("calendar_write"),
            Self::Sensors => Cow::Borrowed("sensors"),
            Self::MotionActivity => Cow::Borrowed("motion_activity"),
            Self::StorageRead => Cow::Borrowed("storage_read"),
            Self::StorageWrite => Cow::Borrowed("storage_write"),
            Self::ManageExternalStorage => Cow::Borrowed("manage_external_storage"),
            Self::Nfc => Cow::Borrowed("nfc"),
            Self::PhoneState => Cow::Borrowed("phone_state"),
            Self::CallPhone => Cow::Borrowed("call_phone"),
            Self::SmsRead => Cow::Borrowed("sms_read"),
            Self::SmsSend => Cow::Borrowed("sms_send"),
            Self::SystemAlertWindow => Cow::Borrowed("system_alert_window"),
            Self::ScheduleExactAlarm => Cow::Borrowed("schedule_exact_alarm"),
            Self::InstallPackages => Cow::Borrowed("install_packages"),
            Self::Vibrate => Cow::Borrowed("vibrate"),
            Self::BackgroundRefresh => Cow::Borrowed("background_refresh"),
            Self::MediaProjection => Cow::Borrowed("media_projection"),
            Self::ClipboardRead => Cow::Borrowed("clipboard_read"),
            Self::DeviceAdmin => Cow::Borrowed("device_admin"),
            Self::AccessibilityService => Cow::Borrowed("accessibility_service"),
            Self::WriteSettings => Cow::Borrowed("write_settings"),
            Self::PackageUsageStats => Cow::Borrowed("package_usage_stats"),
            Self::IgnoreBatteryOptimizations => Cow::Borrowed("ignore_battery_optimizations"),
            Self::NotificationListener => Cow::Borrowed("notification_listener"),
            Self::VpnService => Cow::Borrowed("vpn_service"),
            Self::AdminPrivileges => Cow::Borrowed("admin_privileges"),
            Self::Custom(name) => Cow::Borrowed(name.as_str()),
        }
    }

    /// Parse a canonical `snake_case` key or common alias, falling back to [`Permission::Custom`].
    pub fn from_key(s: &str) -> Self {
        let trimmed = s.trim();
        let normalized = trimmed.to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "camera" | "cam" => Self::Camera,
            "microphone" | "mic" | "record_audio" | "audio_record" => Self::Microphone,
            "location_when_in_use" | "location" | "location_fine" | "fine_location" | "gps" => {
                Self::LocationWhenInUse
            }
            "location_always" | "location_background" | "background_location" => {
                Self::LocationAlways
            }
            "location_coarse" | "coarse_location" | "approximate_location" => Self::LocationCoarse,
            "photos" | "photo_library" | "gallery" | "media_images" => Self::Photos,
            "photos_add_only" | "photos_add" | "save_photos" => Self::PhotosAddOnly,
            "videos" | "video_library" | "media_video" => Self::Videos,
            "audio_library" | "music" | "media_audio" => Self::AudioLibrary,
            "notifications" | "notification" | "push" | "push_notifications"
            | "post_notifications" => Self::Notifications,
            "bluetooth" | "ble" => Self::Bluetooth,
            "bluetooth_scan" | "ble_scan" => Self::BluetoothScan,
            "bluetooth_connect" | "ble_connect" => Self::BluetoothConnect,
            "bluetooth_advertise" | "ble_advertise" => Self::BluetoothAdvertise,
            "biometrics" | "biometric" | "face_id" | "touch_id" | "fingerprint" => Self::Biometrics,
            "contacts" | "address_book" => Self::Contacts,
            "calendar_read" | "calendar" | "calendars" => Self::CalendarRead,
            "calendar_write" => Self::CalendarWrite,
            "sensors" | "body_sensors" => Self::Sensors,
            "motion_activity" | "motion" | "activity_recognition" | "fitness" => {
                Self::MotionActivity
            }
            "storage_read" | "storage" | "read_storage" | "external_storage" => Self::StorageRead,
            "storage_write" | "write_storage" => Self::StorageWrite,
            "manage_external_storage" | "all_files_access" => Self::ManageExternalStorage,
            "nfc" => Self::Nfc,
            "phone_state" | "read_phone_state" => Self::PhoneState,
            "call_phone" | "phone_call" | "call" => Self::CallPhone,
            "sms_read" | "read_sms" => Self::SmsRead,
            "sms_send" | "send_sms" | "sms" => Self::SmsSend,
            "system_alert_window" | "overlay" | "draw_over_other_apps" => Self::SystemAlertWindow,
            "schedule_exact_alarm" | "exact_alarm" => Self::ScheduleExactAlarm,
            "install_packages" | "request_install_packages" => Self::InstallPackages,
            "vibrate" | "haptics" => Self::Vibrate,
            "background_refresh" | "background_fetch" => Self::BackgroundRefresh,
            "media_projection" | "screen_capture" | "screen_record" => Self::MediaProjection,
            "clipboard_read" | "clipboard" | "pasteboard" => Self::ClipboardRead,
            "device_admin" | "admin" | "device_policy" | "device_policy_manager" => {
                Self::DeviceAdmin
            }
            "accessibility_service" | "accessibility" | "a11y_service" | "a11y" => {
                Self::AccessibilityService
            }
            "write_settings" | "modify_system_settings" => Self::WriteSettings,
            "package_usage_stats" | "usage_stats" | "app_usage" => Self::PackageUsageStats,
            "ignore_battery_optimizations" | "battery_optimization" | "battery_saver" => {
                Self::IgnoreBatteryOptimizations
            }
            "notification_listener" | "notification_access" => Self::NotificationListener,
            "vpn_service" | "vpn" => Self::VpnService,
            "admin_privileges" | "root" | "sudo" | "uac" | "elevated" => Self::AdminPrivileges,
            _ => Self::Custom(trimmed.to_string()),
        }
    }

    /// Android manifest permission entries and API-level bounds corresponding to this permission.
    pub fn android_permissions(&self) -> Vec<AndroidPermissionSpec> {
        match self {
            Self::Camera => vec![AndroidPermissionSpec::new("android.permission.CAMERA")],
            Self::Microphone => vec![AndroidPermissionSpec::new(
                "android.permission.RECORD_AUDIO",
            )],
            Self::LocationWhenInUse => vec![
                AndroidPermissionSpec::new("android.permission.ACCESS_FINE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_COARSE_LOCATION"),
            ],
            Self::LocationAlways => vec![
                AndroidPermissionSpec::new("android.permission.ACCESS_FINE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_COARSE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_BACKGROUND_LOCATION")
                    .with_min_sdk(29),
            ],
            Self::LocationCoarse => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.ACCESS_COARSE_LOCATION",
                )]
            }
            Self::Photos => vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_IMAGES").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ],
            Self::PhotosAddOnly => vec![
                AndroidPermissionSpec::new("android.permission.WRITE_EXTERNAL_STORAGE")
                    .with_max_sdk(29),
            ],
            Self::Videos => vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_VIDEO").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ],
            Self::AudioLibrary => vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_AUDIO").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ],
            Self::Notifications => vec![
                AndroidPermissionSpec::new("android.permission.POST_NOTIFICATIONS")
                    .with_min_sdk(33),
            ],
            Self::Bluetooth => vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_SCAN").with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_CONNECT").with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADVERTISE")
                    .with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH").with_max_sdk(30),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADMIN").with_max_sdk(30),
            ],
            Self::BluetoothScan => vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_SCAN").with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH").with_max_sdk(30),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADMIN").with_max_sdk(30),
            ],
            Self::BluetoothConnect => vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_CONNECT").with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH").with_max_sdk(30),
            ],
            Self::BluetoothAdvertise => vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADVERTISE")
                    .with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADMIN").with_max_sdk(30),
            ],
            Self::Biometrics => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.USE_BIOMETRIC",
                )]
            }
            Self::Contacts => vec![
                AndroidPermissionSpec::new("android.permission.READ_CONTACTS"),
                AndroidPermissionSpec::new("android.permission.WRITE_CONTACTS"),
            ],
            Self::CalendarRead => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.READ_CALENDAR",
                )]
            }
            Self::CalendarWrite => vec![
                AndroidPermissionSpec::new("android.permission.WRITE_CALENDAR"),
                AndroidPermissionSpec::new("android.permission.READ_CALENDAR"),
            ],
            Self::Sensors => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.BODY_SENSORS",
                )]
            }
            Self::MotionActivity => vec![
                AndroidPermissionSpec::new("android.permission.ACTIVITY_RECOGNITION")
                    .with_min_sdk(29),
            ],
            Self::StorageRead => vec![
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ],
            Self::StorageWrite => vec![
                AndroidPermissionSpec::new("android.permission.WRITE_EXTERNAL_STORAGE")
                    .with_max_sdk(29),
            ],
            Self::ManageExternalStorage => vec![
                AndroidPermissionSpec::new("android.permission.MANAGE_EXTERNAL_STORAGE")
                    .with_min_sdk(30),
            ],
            Self::Nfc => vec![AndroidPermissionSpec::new("android.permission.NFC")],
            Self::PhoneState => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.READ_PHONE_STATE",
                )]
            }
            Self::CallPhone => vec![AndroidPermissionSpec::new("android.permission.CALL_PHONE")],
            Self::SmsRead => vec![AndroidPermissionSpec::new("android.permission.READ_SMS")],
            Self::SmsSend => vec![AndroidPermissionSpec::new("android.permission.SEND_SMS")],
            Self::SystemAlertWindow => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.SYSTEM_ALERT_WINDOW",
                )]
            }
            Self::ScheduleExactAlarm => vec![
                AndroidPermissionSpec::new("android.permission.SCHEDULE_EXACT_ALARM")
                    .with_min_sdk(31),
            ],
            Self::InstallPackages => {
                vec![AndroidPermissionSpec::new(
                    "android.permission.REQUEST_INSTALL_PACKAGES",
                )]
            }
            Self::Vibrate => vec![AndroidPermissionSpec::new("android.permission.VIBRATE")],
            Self::BackgroundRefresh => Vec::new(),
            Self::MediaProjection => vec![
                AndroidPermissionSpec::new(
                    "android.permission.FOREGROUND_SERVICE_MEDIA_PROJECTION",
                )
                .with_min_sdk(34),
            ],
            Self::ClipboardRead => Vec::new(),
            Self::DeviceAdmin => vec![AndroidPermissionSpec::new(
                "android.permission.BIND_DEVICE_ADMIN",
            )],
            Self::AccessibilityService => vec![AndroidPermissionSpec::new(
                "android.permission.BIND_ACCESSIBILITY_SERVICE",
            )],
            Self::WriteSettings => vec![AndroidPermissionSpec::new(
                "android.permission.WRITE_SETTINGS",
            )],
            Self::PackageUsageStats => vec![AndroidPermissionSpec::new(
                "android.permission.PACKAGE_USAGE_STATS",
            )],
            Self::IgnoreBatteryOptimizations => vec![AndroidPermissionSpec::new(
                "android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS",
            )],
            Self::NotificationListener => vec![AndroidPermissionSpec::new(
                "android.permission.BIND_NOTIFICATION_LISTENER_SERVICE",
            )],
            Self::VpnService => vec![AndroidPermissionSpec::new(
                "android.permission.BIND_VPN_SERVICE",
            )],
            Self::AdminPrivileges => Vec::new(),
            Self::Custom(name) => vec![AndroidPermissionSpec {
                name: Cow::Owned(name.clone()),
                max_sdk: None,
                min_sdk: None,
            }],
        }
    }

    /// Apple `Info.plist` privacy usage description keys required for this permission.
    pub fn ios_usage_keys(&self) -> &'static [&'static str] {
        match self {
            Self::Camera => &["NSCameraUsageDescription"],
            Self::Microphone => &["NSMicrophoneUsageDescription"],
            Self::LocationWhenInUse | Self::LocationCoarse => {
                &["NSLocationWhenInUseUsageDescription"]
            }
            Self::LocationAlways => &[
                "NSLocationAlwaysAndWhenInUseUsageDescription",
                "NSLocationWhenInUseUsageDescription",
            ],
            Self::Photos | Self::Videos => &["NSPhotoLibraryUsageDescription"],
            Self::PhotosAddOnly => &["NSPhotoLibraryAddUsageDescription"],
            Self::AudioLibrary => &["NSAppleMusicUsageDescription"],
            Self::Bluetooth
            | Self::BluetoothScan
            | Self::BluetoothConnect
            | Self::BluetoothAdvertise => &["NSBluetoothAlwaysUsageDescription"],
            Self::Biometrics => &["NSFaceIDUsageDescription"],
            Self::Contacts => &["NSContactsUsageDescription"],
            Self::CalendarRead | Self::CalendarWrite => &[
                "NSCalendarsFullAccessUsageDescription",
                "NSCalendarsUsageDescription",
            ],
            Self::Sensors | Self::MotionActivity => &["NSMotionUsageDescription"],
            Self::Nfc => &["NFCReaderUsageDescription"],
            Self::Notifications
            | Self::StorageRead
            | Self::StorageWrite
            | Self::ManageExternalStorage
            | Self::PhoneState
            | Self::CallPhone
            | Self::SmsRead
            | Self::SmsSend
            | Self::SystemAlertWindow
            | Self::ScheduleExactAlarm
            | Self::InstallPackages
            | Self::Vibrate
            | Self::BackgroundRefresh
            | Self::MediaProjection
            | Self::ClipboardRead
            | Self::DeviceAdmin
            | Self::AccessibilityService
            | Self::WriteSettings
            | Self::PackageUsageStats
            | Self::IgnoreBatteryOptimizations
            | Self::NotificationListener
            | Self::VpnService
            | Self::AdminPrivileges
            | Self::Custom(_) => &[],
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

/// Authorization state of an OS permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PermissionStatus {
    #[default]
    NotDetermined,
    Granted,
    Denied,
    PermanentlyDenied,
    Restricted,
    Limited,
    Provisional,
    Unsupported,
}

impl PermissionStatus {
    /// Returns `true` when the application may proceed with the guarded capability
    /// (`Granted`, `Limited`, or `Provisional`).
    #[inline]
    pub fn is_granted(self) -> bool {
        matches!(self, Self::Granted | Self::Limited | Self::Provisional)
    }
}

/// Result of requesting an individual permission within a batch permission request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PermissionOutcome {
    pub permission: Permission,
    pub status: PermissionStatus,
    pub should_show_rationale: bool,
}

impl PermissionOutcome {
    #[inline]
    pub fn is_granted(&self) -> bool {
        self.status.is_granted()
    }
}

/// Cross-platform OS runtime permission check and request abstraction.
pub trait PermissionBackend: Send + Sync {
    fn check(&self, permission: &Permission) -> PermissionStatus;
    fn request(&self, permission: &Permission) -> PermissionStatus;

    fn request_many(&self, permissions: &[Permission]) -> Vec<PermissionOutcome> {
        permissions
            .iter()
            .map(|permission| {
                let status = self.request(permission);
                let should_show_rationale = self.should_show_rationale(permission);
                PermissionOutcome {
                    permission: permission.clone(),
                    status,
                    should_show_rationale,
                }
            })
            .collect()
    }

    fn should_show_rationale(&self, _permission: &Permission) -> bool {
        false
    }

    fn open_app_settings(&self) -> bool {
        false
    }

    /// Open OS Accessibility Settings screen (e.g. `Settings.ACTION_ACCESSIBILITY_SETTINGS` on Android, TCC prompt on macOS).
    fn open_accessibility_settings(&self) -> bool {
        false
    }

    /// Open OS Device Administrator Settings (e.g. `DevicePolicyManager.ACTION_ADD_DEVICE_ADMIN` on Android).
    fn open_admin_settings(&self) -> bool {
        false
    }

    /// Query whether an Accessibility Service is actively enabled by the user in system settings.
    fn is_accessibility_enabled(&self) -> bool {
        false
    }

    /// Query whether Device Admin or elevated administrator/root privileges are active.
    fn is_admin_active(&self) -> bool {
        false
    }

    /// Request elevated administrator / root execution privileges (e.g. UAC `runas`, Linux `pkexec`/Polkit, macOS Authorization).
    fn request_admin_privileges(&self) -> bool {
        false
    }
}

/// Headless/fallback permission backend reporting `Unsupported` and declining settings prompts.
#[derive(Debug, Default)]
pub struct NoopPermissionBackend;

impl PermissionBackend for NoopPermissionBackend {
    fn check(&self, _permission: &Permission) -> PermissionStatus {
        PermissionStatus::Unsupported
    }

    fn request(&self, _permission: &Permission) -> PermissionStatus {
        PermissionStatus::Unsupported
    }
}

/// Deterministic thread-safe permission backend for tests, simulators, and previews.
#[derive(Debug)]
pub struct MockPermissionBackend {
    pub statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    pub rationales: Mutex<HashSet<Permission>>,
    pub auto_grant_on_request: bool,
    pub settings_opened: AtomicUsize,
    pub accessibility_settings_opened: AtomicUsize,
    pub admin_settings_opened: AtomicUsize,
    pub accessibility_enabled: std::sync::atomic::AtomicBool,
    pub admin_active: std::sync::atomic::AtomicBool,
    request_log: Mutex<Vec<Permission>>,
}

impl Default for MockPermissionBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockPermissionBackend {
    pub fn new() -> Self {
        Self {
            statuses: Mutex::new(HashMap::new()),
            rationales: Mutex::new(HashSet::new()),
            auto_grant_on_request: true,
            settings_opened: AtomicUsize::new(0),
            accessibility_settings_opened: AtomicUsize::new(0),
            admin_settings_opened: AtomicUsize::new(0),
            accessibility_enabled: std::sync::atomic::AtomicBool::new(false),
            admin_active: std::sync::atomic::AtomicBool::new(false),
            request_log: Mutex::new(Vec::new()),
        }
    }

    pub fn with_auto_grant(mut self, auto_grant: bool) -> Self {
        self.auto_grant_on_request = auto_grant;
        self
    }

    pub fn with_status(self, permission: Permission, status: PermissionStatus) -> Self {
        self.set_status(permission, status);
        self
    }

    pub fn with_rationale(self, permission: Permission, show: bool) -> Self {
        self.set_rationale(permission, show);
        self
    }

    pub fn set_status(&self, permission: Permission, status: PermissionStatus) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(permission, status);
        }
    }

    pub fn set_rationale(&self, permission: Permission, show: bool) {
        if let Ok(mut guard) = self.rationales.lock() {
            if show {
                guard.insert(permission);
            } else {
                guard.remove(&permission);
            }
        }
    }

    pub fn set_accessibility_enabled(&self, enabled: bool) {
        self.accessibility_enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn set_admin_active(&self, active: bool) {
        self.admin_active.store(active, Ordering::SeqCst);
    }

    pub fn settings_open_count(&self) -> usize {
        self.settings_opened.load(Ordering::SeqCst)
    }

    pub fn accessibility_settings_open_count(&self) -> usize {
        self.accessibility_settings_opened.load(Ordering::SeqCst)
    }

    pub fn admin_settings_open_count(&self) -> usize {
        self.admin_settings_opened.load(Ordering::SeqCst)
    }

    pub fn requested_permissions(&self) -> Vec<Permission> {
        self.request_log
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn clear_requested(&self) {
        if let Ok(mut guard) = self.request_log.lock() {
            guard.clear();
        }
    }
}

impl PermissionBackend for MockPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        self.statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or(PermissionStatus::NotDetermined)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        if let Ok(mut log) = self.request_log.lock() {
            log.push(permission.clone());
        }
        let Ok(mut statuses) = self.statuses.lock() else {
            return PermissionStatus::Unsupported;
        };
        match statuses.get(permission).copied() {
            Some(status) if status != PermissionStatus::NotDetermined => status,
            _ => {
                let next = if self.auto_grant_on_request {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::Denied
                };
                statuses.insert(permission.clone(), next);
                next
            }
        }
    }

    fn should_show_rationale(&self, permission: &Permission) -> bool {
        self.rationales
            .lock()
            .map(|g| g.contains(permission))
            .unwrap_or(false)
    }

    fn open_app_settings(&self) -> bool {
        self.settings_opened.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn open_accessibility_settings(&self) -> bool {
        self.accessibility_settings_opened
            .fetch_add(1, Ordering::SeqCst);
        true
    }

    fn open_admin_settings(&self) -> bool {
        self.admin_settings_opened.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn is_accessibility_enabled(&self) -> bool {
        self.accessibility_enabled.load(Ordering::SeqCst)
    }

    fn is_admin_active(&self) -> bool {
        self.admin_active.load(Ordering::SeqCst)
    }

    fn request_admin_privileges(&self) -> bool {
        self.admin_active.store(true, Ordering::SeqCst);
        true
    }
}

/// Tactile feedback pattern triggered through native mobile/trackpad haptics engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HapticStyle {
    Light,
    Medium,
    Heavy,
    Selection,
    Success,
    Warning,
    Error,
}

/// Cross-platform tactile haptic feedback abstraction.
pub trait HapticsBackend: Send + Sync {
    fn trigger(&self, style: HapticStyle) -> bool;
}

/// Headless no-op haptics fallback.
#[derive(Debug, Default)]
pub struct NoopHaptics;

impl HapticsBackend for NoopHaptics {
    fn trigger(&self, _style: HapticStyle) -> bool {
        false
    }
}

/// Recording mock haptics backend for unit tests and previews.
#[derive(Debug, Default)]
pub struct MockHaptics {
    pub triggered: Mutex<Vec<HapticStyle>>,
}

impl MockHaptics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn history(&self) -> Vec<HapticStyle> {
        self.triggered.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.triggered.lock() {
            guard.clear();
        }
    }
}

impl HapticsBackend for MockHaptics {
    fn trigger(&self, style: HapticStyle) -> bool {
        if let Ok(mut guard) = self.triggered.lock() {
            guard.push(style);
            true
        } else {
            false
        }
    }
}

/// Content payload passed to the native OS share sheet (`UIActivityViewController`, Android `Intent.ACTION_SEND`, Web Share API).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SharePayload {
    pub title: Option<String>,
    pub text: Option<String>,
    pub url: Option<String>,
    pub files: Vec<PathBuf>,
}

impl SharePayload {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            ..Default::default()
        }
    }

    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            files: vec![path.into()],
            ..Default::default()
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push(path.into());
        self
    }

    pub fn with_files(mut self, files: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.files.extend(files.into_iter().map(Into::into));
        self
    }
}

/// Cross-platform native share sheet abstraction.
pub trait ShareBackend: Send + Sync {
    fn share(&self, payload: &SharePayload) -> bool;
}

/// Headless no-op share fallback.
#[derive(Debug, Default)]
pub struct NoopShare;

impl ShareBackend for NoopShare {
    fn share(&self, _payload: &SharePayload) -> bool {
        false
    }
}

/// Recording mock share sheet backend for tests.
#[derive(Debug, Default)]
pub struct MockShare {
    pub shared: Mutex<Vec<SharePayload>>,
}

impl MockShare {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn history(&self) -> Vec<SharePayload> {
        self.shared.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.clear();
        }
    }
}

impl ShareBackend for MockShare {
    fn share(&self, payload: &SharePayload) -> bool {
        if let Ok(mut guard) = self.shared.lock() {
            guard.push(payload.clone());
            true
        } else {
            false
        }
    }
}

/// Cross-platform secure key-value credential store (iOS Keychain, Android Keystore, OS Secret Service).
pub trait SecureStorageBackend: Send + Sync {
    fn get_secret(&self, key: &str) -> Option<String>;
    fn set_secret(&self, key: &str, value: &str) -> bool;
    fn delete_secret(&self, key: &str) -> bool;
}

/// Thread-safe in-memory secret store used in headless tests and non-persistent hosts.
#[derive(Debug, Default)]
pub struct MemorySecureStorage {
    entries: Mutex<HashMap<String, String>>,
}

impl MemorySecureStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecureStorageBackend for MemorySecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        self.entries.lock().ok().and_then(|g| g.get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        if let Ok(mut guard) = self.entries.lock() {
            guard.insert(key.to_string(), value.to_string());
            true
        } else {
            false
        }
    }

    fn delete_secret(&self, key: &str) -> bool {
        self.entries
            .lock()
            .map(|mut guard| guard.remove(key).is_some())
            .unwrap_or(false)
    }
}

/// Current host network connectivity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum NetworkStatus {
    #[default]
    Unknown,
    Offline,
    Wifi,
    Cellular,
    Ethernet,
}

/// Host battery charge level and charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct BatteryState {
    pub level_pct: Option<u8>,
    pub charging: bool,
}

/// Host operating system, hardware model, and application version metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub os: String,
    pub model: String,
    pub os_version: String,
    pub sdk_int: Option<u32>,
    pub app_version: String,
}

/// Application foreground/background/memory lifecycle events emitted by mobile & desktop shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppLifecycleEvent {
    Resumed,
    Paused,
    Inactive,
    Detached,
    MemoryWarning,
}

/// Read-only device telemetry abstraction implemented per platform shell.
pub trait DeviceCapabilitiesBackend: Send + Sync {
    fn device_info(&self) -> DeviceInfo;
    fn network_status(&self) -> NetworkStatus;
    fn battery_state(&self) -> BatteryState;
}

/// Deterministic mock device telemetry backend for tests and previews.
#[derive(Debug)]
pub struct MockDeviceCapabilities {
    pub info: Mutex<DeviceInfo>,
    pub network: Mutex<NetworkStatus>,
    pub battery: Mutex<BatteryState>,
}

impl Default for MockDeviceCapabilities {
    fn default() -> Self {
        Self {
            info: Mutex::new(DeviceInfo {
                os: std::env::consts::OS.to_string(),
                model: String::new(),
                os_version: String::new(),
                sdk_int: None,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            network: Mutex::new(NetworkStatus::Unknown),
            battery: Mutex::new(BatteryState::default()),
        }
    }
}

impl MockDeviceCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_device_info(self, info: DeviceInfo) -> Self {
        self.set_device_info(info);
        self
    }

    pub fn with_network_status(self, status: NetworkStatus) -> Self {
        self.set_network_status(status);
        self
    }

    pub fn with_battery_state(self, state: BatteryState) -> Self {
        self.set_battery_state(state);
        self
    }

    pub fn set_device_info(&self, info: DeviceInfo) {
        if let Ok(mut guard) = self.info.lock() {
            *guard = info;
        }
    }

    pub fn set_network_status(&self, status: NetworkStatus) {
        if let Ok(mut guard) = self.network.lock() {
            *guard = status;
        }
    }

    pub fn set_battery_state(&self, state: BatteryState) {
        if let Ok(mut guard) = self.battery.lock() {
            *guard = state;
        }
    }
}

impl DeviceCapabilitiesBackend for MockDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        self.info.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn network_status(&self) -> NetworkStatus {
        self.network.lock().map(|g| *g).unwrap_or_default()
    }

    fn battery_state(&self) -> BatteryState {
        self.battery.lock().map(|g| *g).unwrap_or_default()
    }
}

impl<T: PermissionBackend + ?Sized> PermissionBackend for Arc<T> {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        (**self).check(permission)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        (**self).request(permission)
    }

    fn request_many(&self, permissions: &[Permission]) -> Vec<PermissionOutcome> {
        (**self).request_many(permissions)
    }

    fn should_show_rationale(&self, permission: &Permission) -> bool {
        (**self).should_show_rationale(permission)
    }

    fn open_app_settings(&self) -> bool {
        (**self).open_app_settings()
    }

    fn open_accessibility_settings(&self) -> bool {
        (**self).open_accessibility_settings()
    }

    fn open_admin_settings(&self) -> bool {
        (**self).open_admin_settings()
    }

    fn is_accessibility_enabled(&self) -> bool {
        (**self).is_accessibility_enabled()
    }

    fn is_admin_active(&self) -> bool {
        (**self).is_admin_active()
    }

    fn request_admin_privileges(&self) -> bool {
        (**self).request_admin_privileges()
    }
}

impl<T: HapticsBackend + ?Sized> HapticsBackend for Arc<T> {
    fn trigger(&self, style: HapticStyle) -> bool {
        (**self).trigger(style)
    }
}

impl<T: ShareBackend + ?Sized> ShareBackend for Arc<T> {
    fn share(&self, payload: &SharePayload) -> bool {
        (**self).share(payload)
    }
}

impl<T: SecureStorageBackend + ?Sized> SecureStorageBackend for Arc<T> {
    fn get_secret(&self, key: &str) -> Option<String> {
        (**self).get_secret(key)
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        (**self).set_secret(key, value)
    }

    fn delete_secret(&self, key: &str) -> bool {
        (**self).delete_secret(key)
    }
}

impl<T: DeviceCapabilitiesBackend + ?Sized> DeviceCapabilitiesBackend for Arc<T> {
    fn device_info(&self) -> DeviceInfo {
        (**self).device_info()
    }

    fn network_status(&self) -> NetworkStatus {
        (**self).network_status()
    }

    fn battery_state(&self) -> BatteryState {
        (**self).battery_state()
    }
}

/// Bundle of OS/platform service backends shared via reactive context or `WidgetTree`.
#[derive(Clone)]
pub struct PlatformServices {
    clipboard: Arc<Mutex<Box<dyn ClipboardBackend>>>,
    file_dialog: Arc<dyn FileDialogBackend>,
    permissions: Arc<dyn PermissionBackend>,
    haptics: Arc<dyn HapticsBackend>,
    share: Arc<dyn ShareBackend>,
    secure_storage: Arc<dyn SecureStorageBackend>,
    device_capabilities: Arc<dyn DeviceCapabilitiesBackend>,
}

impl fmt::Debug for PlatformServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformServices").finish_non_exhaustive()
    }
}

impl Default for PlatformServices {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl PlatformServices {
    pub fn in_memory() -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(Box::new(MemoryClipboard::default()))),
            file_dialog: Arc::new(NoopFileDialog),
            permissions: Arc::new(NoopPermissionBackend),
            haptics: Arc::new(NoopHaptics),
            share: Arc::new(NoopShare),
            secure_storage: Arc::new(MemorySecureStorage::default()),
            device_capabilities: Arc::new(MockDeviceCapabilities::default()),
        }
    }

    pub fn new(
        clipboard: impl ClipboardBackend + 'static,
        file_dialog: impl FileDialogBackend + 'static,
    ) -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(Box::new(clipboard))),
            file_dialog: Arc::new(file_dialog),
            permissions: Arc::new(NoopPermissionBackend),
            haptics: Arc::new(NoopHaptics),
            share: Arc::new(NoopShare),
            secure_storage: Arc::new(MemorySecureStorage::default()),
            device_capabilities: Arc::new(MockDeviceCapabilities::default()),
        }
    }

    pub fn with_clipboard(mut self, clipboard: impl ClipboardBackend + 'static) -> Self {
        self.clipboard = Arc::new(Mutex::new(Box::new(clipboard)));
        self
    }

    pub fn with_file_dialog(mut self, file_dialog: impl FileDialogBackend + 'static) -> Self {
        self.file_dialog = Arc::new(file_dialog);
        self
    }

    pub fn with_permissions(mut self, permissions: impl PermissionBackend + 'static) -> Self {
        self.permissions = Arc::new(permissions);
        self
    }

    pub fn with_haptics(mut self, haptics: impl HapticsBackend + 'static) -> Self {
        self.haptics = Arc::new(haptics);
        self
    }

    pub fn with_share(mut self, share: impl ShareBackend + 'static) -> Self {
        self.share = Arc::new(share);
        self
    }

    pub fn with_secure_storage(
        mut self,
        secure_storage: impl SecureStorageBackend + 'static,
    ) -> Self {
        self.secure_storage = Arc::new(secure_storage);
        self
    }

    pub fn with_device_capabilities(
        mut self,
        device_capabilities: impl DeviceCapabilitiesBackend + 'static,
    ) -> Self {
        self.device_capabilities = Arc::new(device_capabilities);
        self
    }

    pub fn read_clipboard(&self) -> Option<String> {
        self.clipboard.lock().ok().and_then(|mut cb| cb.get_text())
    }

    pub fn write_clipboard(&self, text: &str) -> bool {
        self.clipboard
            .lock()
            .map(|mut cb| cb.set_text(text))
            .unwrap_or(false)
    }

    pub fn pick_open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        self.file_dialog.open_files(request)
    }

    pub fn pick_open_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        self.file_dialog.open_files(request).into_iter().next()
    }

    pub fn pick_save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        self.file_dialog.save_file(request)
    }

    /// Non-blocking file open dialog executed on a worker thread (native) and delivered onto `dispatcher`.
    pub fn pick_open_files_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Vec<PathBuf>) + Send + 'static,
    {
        let backend = self.file_dialog.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let paths = backend.open_files(&request);
                dispatcher.post(move || on_done(paths));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let paths = backend.open_files(&request);
            dispatcher.post(move || on_done(paths));
        }
    }

    /// Non-blocking single-file open dialog executed on a worker thread, delivering the result onto `dispatcher`.
    pub fn pick_open_file_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Option<PathBuf>) + Send + 'static,
    {
        self.pick_open_files_async(request, dispatcher, move |paths| {
            on_done(paths.into_iter().next());
        });
    }

    /// Non-blocking save-file dialog executed on a worker thread (native) and delivered onto `dispatcher`.
    pub fn pick_save_file_async<F>(
        &self,
        request: FileDialogRequest,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Option<PathBuf>) + Send + 'static,
    {
        let backend = self.file_dialog.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let path = backend.save_file(&request);
                dispatcher.post(move || on_done(path));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let path = backend.save_file(&request);
            dispatcher.post(move || on_done(path));
        }
    }

    pub fn check_permission(&self, permission: &Permission) -> PermissionStatus {
        self.permissions.check(permission)
    }

    pub fn request_permission(&self, permission: &Permission) -> PermissionStatus {
        self.permissions.request(permission)
    }

    pub fn request_permissions(&self, permissions: &[Permission]) -> Vec<PermissionOutcome> {
        self.permissions.request_many(permissions)
    }

    pub fn should_show_permission_rationale(&self, permission: &Permission) -> bool {
        self.permissions.should_show_rationale(permission)
    }

    pub fn open_app_settings(&self) -> bool {
        self.permissions.open_app_settings()
    }

    /// Open OS Accessibility Settings screen (e.g. `Settings.ACTION_ACCESSIBILITY_SETTINGS` on Android).
    pub fn open_accessibility_settings(&self) -> bool {
        self.permissions.open_accessibility_settings()
    }

    /// Open OS Device Administrator Settings (e.g. `DevicePolicyManager.ACTION_ADD_DEVICE_ADMIN` on Android).
    pub fn open_admin_settings(&self) -> bool {
        self.permissions.open_admin_settings()
    }

    /// Query whether an Accessibility Service is actively enabled by the user in system settings.
    pub fn is_accessibility_enabled(&self) -> bool {
        self.permissions.is_accessibility_enabled()
    }

    /// Query whether Device Admin or elevated administrator/root privileges are active.
    pub fn is_admin_active(&self) -> bool {
        self.permissions.is_admin_active()
    }

    /// Request elevated administrator / root execution privileges (e.g. UAC `runas`, Linux `pkexec`/Polkit, macOS Authorization).
    pub fn request_admin_privileges(&self) -> bool {
        self.permissions.request_admin_privileges()
    }

    /// Non-blocking single-permission request executed off the UI thread (native) and delivered onto `dispatcher`.
    pub fn request_permission_async<F>(
        &self,
        permission: Permission,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(PermissionStatus) + Send + 'static,
    {
        let backend = self.permissions.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let status = backend.request(&permission);
                dispatcher.post(move || on_done(status));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let status = backend.request(&permission);
            dispatcher.post(move || on_done(status));
        }
    }

    /// Non-blocking batch permission request executed off the UI thread (native) and delivered onto `dispatcher`.
    pub fn request_permissions_async<F>(
        &self,
        permissions: Vec<Permission>,
        dispatcher: UiDispatcher,
        on_done: F,
    ) where
        F: FnOnce(Vec<PermissionOutcome>) + Send + 'static,
    {
        let backend = self.permissions.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                let outcomes = backend.request_many(&permissions);
                dispatcher.post(move || on_done(outcomes));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let outcomes = backend.request_many(&permissions);
            dispatcher.post(move || on_done(outcomes));
        }
    }

    pub fn trigger_haptic(&self, style: HapticStyle) -> bool {
        self.haptics.trigger(style)
    }

    pub fn share(&self, payload: &SharePayload) -> bool {
        self.share.share(payload)
    }

    pub fn get_secret(&self, key: &str) -> Option<String> {
        self.secure_storage.get_secret(key)
    }

    pub fn set_secret(&self, key: &str, value: &str) -> bool {
        self.secure_storage.set_secret(key, value)
    }

    pub fn delete_secret(&self, key: &str) -> bool {
        self.secure_storage.delete_secret(key)
    }

    pub fn device_info(&self) -> DeviceInfo {
        self.device_capabilities.device_info()
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.device_capabilities.network_status()
    }

    pub fn battery_state(&self) -> BatteryState {
        self.device_capabilities.battery_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_services_clipboard_and_mock_dialog_work() {
        let services = PlatformServices::in_memory().with_file_dialog(MockFileDialog::new(
            vec![PathBuf::from("/tmp/example.txt")],
            Some(PathBuf::from("/tmp/out.png")),
        ));

        assert!(services.write_clipboard("hello CraftUI"));
        assert_eq!(services.read_clipboard().as_deref(), Some("hello CraftUI"));

        let opened = services.pick_open_file(&FileDialogRequest::open("Open File"));
        assert_eq!(opened, Some(PathBuf::from("/tmp/example.txt")));

        let saved = services.pick_save_file(&FileDialogRequest::save("Save Image", "out.png"));
        assert_eq!(saved, Some(PathBuf::from("/tmp/out.png")));

        let disp = UiDispatcher::new();
        let got_open = Arc::new(Mutex::new(None));
        let go_c = got_open.clone();
        services.pick_open_file_async(
            FileDialogRequest::open("Async Open"),
            disp.clone(),
            move |p| {
                *go_c.lock().unwrap() = Some(p);
            },
        );
        for _ in 0..50 {
            let _ = disp.drain();
            if got_open.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            *got_open.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/example.txt")))
        );

        let got_save = Arc::new(Mutex::new(None));
        let gs_c = got_save.clone();
        services.pick_save_file_async(
            FileDialogRequest::save("Async Save", "out.png"),
            disp.clone(),
            move |p| {
                *gs_c.lock().unwrap() = Some(p);
            },
        );
        for _ in 0..50 {
            let _ = disp.drain();
            if got_save.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            *got_save.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/out.png")))
        );
    }

    #[test]
    fn permission_keys_aliases_and_roundtrips() {
        let standard = Permission::all_standard();
        assert_eq!(standard.len(), 43);

        let mut seen_keys = HashSet::new();
        for perm in standard {
            let key = perm.key();
            assert!(!key.is_empty());
            assert!(
                seen_keys.insert(key.to_string()),
                "duplicate permission key {key}"
            );
            assert_eq!(Permission::from_key(&key), *perm);
            assert_eq!(perm.to_string(), key);
        }

        // Common aliases
        assert_eq!(
            Permission::from_key("location"),
            Permission::LocationWhenInUse
        );
        assert_eq!(Permission::from_key("mic"), Permission::Microphone);
        assert_eq!(Permission::from_key("storage"), Permission::StorageRead);
        assert_eq!(Permission::from_key("push"), Permission::Notifications);
        assert_eq!(Permission::from_key("face_id"), Permission::Biometrics);
        assert_eq!(Permission::from_key("biometric"), Permission::Biometrics);
        assert_eq!(Permission::from_key("admin"), Permission::DeviceAdmin);
        assert_eq!(
            Permission::from_key("a11y"),
            Permission::AccessibilityService
        );
        assert_eq!(
            Permission::from_key("accessibility"),
            Permission::AccessibilityService
        );
        assert_eq!(Permission::from_key("root"), Permission::AdminPrivileges);
        assert_eq!(Permission::from_key("sudo"), Permission::AdminPrivileges);
        assert_eq!(Permission::from_key("uac"), Permission::AdminPrivileges);
        assert_eq!(
            Permission::from_key("  Location-Always "),
            Permission::LocationAlways
        );

        // Custom fallback preserves trimmed string verbatim
        let custom = Permission::from_key("  com.example.READ_SPECIAL_HW  ");
        assert_eq!(
            custom,
            Permission::Custom("com.example.READ_SPECIAL_HW".to_string())
        );
        assert_eq!(custom.key(), "com.example.READ_SPECIAL_HW");
    }

    #[test]
    fn permission_android_and_ios_mappings_are_accurate() {
        // Photos: API 33+ READ_MEDIA_IMAGES + legacy READ_EXTERNAL_STORAGE max_sdk 32
        let photos = Permission::Photos.android_permissions();
        assert_eq!(
            photos,
            vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_IMAGES").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ]
        );
        assert_eq!(
            Permission::Photos.ios_usage_keys(),
            &["NSPhotoLibraryUsageDescription"]
        );
        assert_eq!(
            Permission::PhotosAddOnly.ios_usage_keys(),
            &["NSPhotoLibraryAddUsageDescription"]
        );

        // Videos & AudioLibrary SDK 33 split
        assert_eq!(
            Permission::Videos.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_VIDEO").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ]
        );
        assert_eq!(
            Permission::AudioLibrary.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.READ_MEDIA_AUDIO").with_min_sdk(33),
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32),
            ]
        );

        // Bluetooth SDK 31+ vs <=30 split
        let bt = Permission::Bluetooth.android_permissions();
        assert!(bt.contains(
            &AndroidPermissionSpec::new("android.permission.BLUETOOTH_SCAN").with_min_sdk(31)
        ));
        assert!(bt.contains(
            &AndroidPermissionSpec::new("android.permission.BLUETOOTH_CONNECT").with_min_sdk(31)
        ));
        assert!(bt.contains(
            &AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADVERTISE").with_min_sdk(31)
        ));
        assert!(bt.contains(
            &AndroidPermissionSpec::new("android.permission.BLUETOOTH").with_max_sdk(30)
        ));
        assert!(bt.contains(
            &AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADMIN").with_max_sdk(30)
        ));
        assert_eq!(
            Permission::BluetoothScan.ios_usage_keys(),
            &["NSBluetoothAlwaysUsageDescription"]
        );

        // Notifications API 33+
        assert_eq!(
            Permission::Notifications.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.POST_NOTIFICATIONS")
                    .with_min_sdk(33)
            ]
        );

        // Location variants
        assert_eq!(
            Permission::LocationWhenInUse.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.ACCESS_FINE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_COARSE_LOCATION"),
            ]
        );
        assert_eq!(
            Permission::LocationWhenInUse.ios_usage_keys(),
            &["NSLocationWhenInUseUsageDescription"]
        );
        assert_eq!(
            Permission::LocationAlways.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.ACCESS_FINE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_COARSE_LOCATION"),
                AndroidPermissionSpec::new("android.permission.ACCESS_BACKGROUND_LOCATION")
                    .with_min_sdk(29),
            ]
        );
        assert_eq!(
            Permission::LocationAlways.ios_usage_keys(),
            &[
                "NSLocationAlwaysAndWhenInUseUsageDescription",
                "NSLocationWhenInUseUsageDescription"
            ]
        );

        // Camera, Mic, Biometrics, NFC, Contacts, Calendar, Motion
        assert_eq!(
            Permission::Camera.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.CAMERA")]
        );
        assert_eq!(
            Permission::Camera.ios_usage_keys(),
            &["NSCameraUsageDescription"]
        );
        assert_eq!(
            Permission::Microphone.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.RECORD_AUDIO"
            )]
        );
        assert_eq!(
            Permission::Microphone.ios_usage_keys(),
            &["NSMicrophoneUsageDescription"]
        );
        assert_eq!(
            Permission::Biometrics.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.USE_BIOMETRIC"
            )]
        );
        assert_eq!(
            Permission::Biometrics.ios_usage_keys(),
            &["NSFaceIDUsageDescription"]
        );
        assert_eq!(
            Permission::Nfc.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.NFC")]
        );
        assert_eq!(
            Permission::Nfc.ios_usage_keys(),
            &["NFCReaderUsageDescription"]
        );
        assert_eq!(
            Permission::Contacts.ios_usage_keys(),
            &["NSContactsUsageDescription"]
        );
        assert_eq!(
            Permission::CalendarRead.ios_usage_keys(),
            &[
                "NSCalendarsFullAccessUsageDescription",
                "NSCalendarsUsageDescription"
            ]
        );
        assert_eq!(
            Permission::MotionActivity.ios_usage_keys(),
            &["NSMotionUsageDescription"]
        );
        assert_eq!(
            Permission::Sensors.ios_usage_keys(),
            &["NSMotionUsageDescription"]
        );
        assert_eq!(
            Permission::Sensors.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.BODY_SENSORS"
            )]
        );
        assert_eq!(
            Permission::MotionActivity.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.ACTIVITY_RECOGNITION")
                    .with_min_sdk(29)
            ]
        );
        assert_eq!(
            Permission::LocationCoarse.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.ACCESS_COARSE_LOCATION"
            )]
        );
        assert_eq!(
            Permission::StorageRead.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.READ_EXTERNAL_STORAGE")
                    .with_max_sdk(32)
            ]
        );
        assert_eq!(
            Permission::StorageWrite.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.WRITE_EXTERNAL_STORAGE")
                    .with_max_sdk(29)
            ]
        );
        assert_eq!(
            Permission::ManageExternalStorage.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.MANAGE_EXTERNAL_STORAGE")
                    .with_min_sdk(30)
            ]
        );
        assert_eq!(
            Permission::ScheduleExactAlarm.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.SCHEDULE_EXACT_ALARM")
                    .with_min_sdk(31)
            ]
        );
        assert_eq!(
            Permission::SystemAlertWindow.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.SYSTEM_ALERT_WINDOW"
            )]
        );
        assert_eq!(
            Permission::InstallPackages.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.REQUEST_INSTALL_PACKAGES"
            )]
        );
        assert_eq!(
            Permission::PhoneState.android_permissions(),
            vec![AndroidPermissionSpec::new(
                "android.permission.READ_PHONE_STATE"
            )]
        );
        assert_eq!(
            Permission::CallPhone.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.CALL_PHONE")]
        );
        assert_eq!(
            Permission::SmsRead.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.READ_SMS")]
        );
        assert_eq!(
            Permission::SmsSend.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.SEND_SMS")]
        );
        assert_eq!(
            Permission::Vibrate.android_permissions(),
            vec![AndroidPermissionSpec::new("android.permission.VIBRATE")]
        );
        assert_eq!(
            Permission::MediaProjection.android_permissions(),
            vec![
                AndroidPermissionSpec::new(
                    "android.permission.FOREGROUND_SERVICE_MEDIA_PROJECTION"
                )
                .with_min_sdk(34)
            ]
        );
        assert!(
            Permission::BackgroundRefresh
                .android_permissions()
                .is_empty()
        );
        assert!(Permission::ClipboardRead.android_permissions().is_empty());
        assert_eq!(
            Permission::BluetoothConnect.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_CONNECT").with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH").with_max_sdk(30),
            ]
        );
        assert_eq!(
            Permission::BluetoothAdvertise.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADVERTISE")
                    .with_min_sdk(31),
                AndroidPermissionSpec::new("android.permission.BLUETOOTH_ADMIN").with_max_sdk(30),
            ]
        );
        assert_eq!(
            Permission::CalendarWrite.android_permissions(),
            vec![
                AndroidPermissionSpec::new("android.permission.WRITE_CALENDAR"),
                AndroidPermissionSpec::new("android.permission.READ_CALENDAR"),
            ]
        );
        assert_eq!(
            Permission::CalendarWrite.ios_usage_keys(),
            &[
                "NSCalendarsFullAccessUsageDescription",
                "NSCalendarsUsageDescription"
            ]
        );

        // Custom permission passes through verbatim on Android
        let custom = Permission::Custom("com.acme.permission.CUSTOM_SENSOR".to_string());
        assert_eq!(
            custom.android_permissions(),
            vec![AndroidPermissionSpec {
                name: Cow::Borrowed("com.acme.permission.CUSTOM_SENSOR"),
                max_sdk: None,
                min_sdk: None,
            }]
        );
        assert!(custom.ios_usage_keys().is_empty());

        // Every standard permission has valid non-empty keys and runs android/ios helpers cleanly
        for perm in Permission::all_standard() {
            let _ = perm.android_permissions();
            let _ = perm.ios_usage_keys();
        }
    }

    #[test]
    fn permission_status_and_backends_work_sync_and_async() {
        assert!(PermissionStatus::Granted.is_granted());
        assert!(PermissionStatus::Limited.is_granted());
        assert!(PermissionStatus::Provisional.is_granted());
        assert!(!PermissionStatus::NotDetermined.is_granted());
        assert!(!PermissionStatus::Denied.is_granted());
        assert!(!PermissionStatus::PermanentlyDenied.is_granted());
        assert!(!PermissionStatus::Restricted.is_granted());
        assert!(!PermissionStatus::Unsupported.is_granted());

        // Default in_memory uses NoopPermissionBackend -> Unsupported
        let noop_services = PlatformServices::in_memory();
        assert_eq!(
            noop_services.check_permission(&Permission::Camera),
            PermissionStatus::Unsupported
        );
        assert_eq!(
            noop_services.request_permission(&Permission::Camera),
            PermissionStatus::Unsupported
        );
        assert!(!noop_services.should_show_permission_rationale(&Permission::Camera));
        assert!(!noop_services.open_app_settings());

        // Rich MockPermissionBackend via Arc so tests can inspect & mutate live
        let mock_perms = Arc::new(
            MockPermissionBackend::new()
                .with_status(Permission::Microphone, PermissionStatus::Denied)
                .with_rationale(Permission::Microphone, true)
                .with_status(Permission::Photos, PermissionStatus::Limited)
                .with_status(
                    Permission::LocationAlways,
                    PermissionStatus::PermanentlyDenied,
                ),
        );
        let services = PlatformServices::in_memory().with_permissions(mock_perms.clone());

        assert_eq!(
            services.check_permission(&Permission::Camera),
            PermissionStatus::NotDetermined
        );
        assert_eq!(
            services.check_permission(&Permission::Microphone),
            PermissionStatus::Denied
        );
        assert!(services.should_show_permission_rationale(&Permission::Microphone));
        assert!(!services.should_show_permission_rationale(&Permission::Camera));

        let outcomes = services.request_permissions(&[
            Permission::Camera,
            Permission::Microphone,
            Permission::Photos,
            Permission::LocationAlways,
        ]);
        assert_eq!(outcomes.len(), 4);
        assert_eq!(outcomes[0].permission, Permission::Camera);
        assert_eq!(outcomes[0].status, PermissionStatus::Granted);
        assert!(!outcomes[0].should_show_rationale);
        assert!(outcomes[0].is_granted());

        assert_eq!(outcomes[1].permission, Permission::Microphone);
        assert_eq!(outcomes[1].status, PermissionStatus::Denied);
        assert!(outcomes[1].should_show_rationale);
        assert!(!outcomes[1].is_granted());

        assert_eq!(outcomes[2].status, PermissionStatus::Limited);
        assert!(outcomes[2].is_granted());

        assert_eq!(outcomes[3].status, PermissionStatus::PermanentlyDenied);
        assert!(!outcomes[3].is_granted());

        assert!(services.open_app_settings());
        assert_eq!(mock_perms.settings_open_count(), 1);
        assert_eq!(mock_perms.requested_permissions().len(), 4);

        // Mutate live + async requests delivered on UiDispatcher
        mock_perms.set_status(Permission::Notifications, PermissionStatus::Provisional);
        let disp = UiDispatcher::new();
        let single_res = Arc::new(Mutex::new(None));
        let sr_c = single_res.clone();
        services.request_permission_async(Permission::Notifications, disp.clone(), move |st| {
            *sr_c.lock().unwrap() = Some(st);
        });
        for _ in 0..50 {
            let _ = disp.drain();
            if single_res.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            *single_res.lock().unwrap(),
            Some(PermissionStatus::Provisional)
        );

        let deny_mock = MockPermissionBackend::new().with_auto_grant(false);
        let deny_services = PlatformServices::in_memory().with_permissions(deny_mock);
        let batch_res = Arc::new(Mutex::new(None));
        let br_c = batch_res.clone();
        deny_services.request_permissions_async(
            vec![Permission::Biometrics, Permission::Nfc],
            disp.clone(),
            move |res| {
                *br_c.lock().unwrap() = Some(res);
            },
        );
        for _ in 0..50 {
            let _ = disp.drain();
            if batch_res.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let batch = batch_res.lock().unwrap().clone().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].status, PermissionStatus::Denied);
        assert_eq!(batch[1].status, PermissionStatus::Denied);
        assert_eq!(
            deny_services.check_permission(&Permission::Biometrics),
            PermissionStatus::Denied
        );
    }

    #[test]
    fn mobile_capabilities_haptics_share_secure_storage_and_device_info() {
        let default_services = PlatformServices::in_memory();
        assert!(!default_services.trigger_haptic(HapticStyle::Light));
        assert!(!default_services.share(&SharePayload::text("hello")));

        // Built-in in-memory secure storage works out of the box
        assert!(default_services.set_secret("auth_token", "tok_12345"));
        assert_eq!(
            default_services.get_secret("auth_token").as_deref(),
            Some("tok_12345")
        );
        assert!(default_services.delete_secret("auth_token"));
        assert_eq!(default_services.get_secret("auth_token"), None);
        assert!(!default_services.delete_secret("auth_token"));

        let haptics = Arc::new(MockHaptics::new());
        let share = Arc::new(MockShare::new());
        let secure = Arc::new(MemorySecureStorage::new());
        let device = Arc::new(
            MockDeviceCapabilities::new()
                .with_device_info(DeviceInfo {
                    os: "android".to_string(),
                    model: "Pixel 9".to_string(),
                    os_version: "15".to_string(),
                    sdk_int: Some(35),
                    app_version: "0.1.0".to_string(),
                })
                .with_network_status(NetworkStatus::Wifi)
                .with_battery_state(BatteryState {
                    level_pct: Some(88),
                    charging: true,
                }),
        );

        let services = PlatformServices::new(MemoryClipboard::default(), NoopFileDialog)
            .with_haptics(haptics.clone())
            .with_share(share.clone())
            .with_secure_storage(secure.clone())
            .with_device_capabilities(device.clone());

        assert!(services.trigger_haptic(HapticStyle::Selection));
        assert!(services.trigger_haptic(HapticStyle::Success));
        assert_eq!(
            haptics.history(),
            vec![HapticStyle::Selection, HapticStyle::Success]
        );

        let payload = SharePayload::url("https://craftui.dev")
            .with_title("CraftUI")
            .with_text("Check out CraftUI")
            .with_file("/tmp/demo.png");
        assert!(services.share(&payload));
        assert_eq!(share.history(), vec![payload]);

        assert!(services.set_secret("refresh_key", "rk_999"));
        assert_eq!(secure.len(), 1);
        assert_eq!(
            services.get_secret("refresh_key").as_deref(),
            Some("rk_999")
        );
        assert!(services.delete_secret("refresh_key"));
        assert!(secure.is_empty());

        let info = services.device_info();
        assert_eq!(info.os, "android");
        assert_eq!(info.model, "Pixel 9");
        assert_eq!(info.sdk_int, Some(35));
        assert_eq!(services.network_status(), NetworkStatus::Wifi);
        assert_eq!(
            services.battery_state(),
            BatteryState {
                level_pct: Some(88),
                charging: true
            }
        );

        device.set_network_status(NetworkStatus::Cellular);
        device.set_battery_state(BatteryState {
            level_pct: Some(42),
            charging: false,
        });
        assert_eq!(services.network_status(), NetworkStatus::Cellular);
        assert_eq!(services.battery_state().level_pct, Some(42));
        assert!(!services.battery_state().charging);

        // Lifecycle enum values are distinct and copy/eq
        assert_ne!(AppLifecycleEvent::Resumed, AppLifecycleEvent::Paused);
        assert_ne!(
            AppLifecycleEvent::Inactive,
            AppLifecycleEvent::MemoryWarning
        );
        assert_ne!(AppLifecycleEvent::Detached, AppLifecycleEvent::Resumed);
    }
}
