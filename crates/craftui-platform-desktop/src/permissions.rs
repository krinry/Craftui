use craftui_core::{Permission, PermissionBackend, PermissionStatus};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::file_dialog::has_unix_display;

/// Desktop host permission backend reporting desktop-accessible capabilities as [`PermissionStatus::Granted`],
/// mobile/telephony-only capabilities as [`PermissionStatus::Unsupported`], and honoring explicit test/host overrides.
#[derive(Debug, Default)]
pub struct DesktopPermissionBackend {
    overrides: Mutex<HashMap<Permission, PermissionStatus>>,
    launch_os_settings: bool,
}

impl DesktopPermissionBackend {
    pub fn new() -> Self {
        Self {
            overrides: Mutex::new(HashMap::new()),
            launch_os_settings: true,
        }
    }

    pub fn with_status(self, permission: Permission, status: PermissionStatus) -> Self {
        self.set_status(permission, status);
        self
    }

    pub fn with_settings_launcher(mut self, enabled: bool) -> Self {
        self.launch_os_settings = enabled;
        self
    }

    pub fn set_status(&self, permission: Permission, status: PermissionStatus) {
        if let Ok(mut guard) = self.overrides.lock() {
            guard.insert(permission, status);
        }
    }

    pub fn default_desktop_status(permission: &Permission) -> PermissionStatus {
        match permission {
            Permission::Camera
            | Permission::Microphone
            | Permission::LocationWhenInUse
            | Permission::LocationCoarse
            | Permission::Photos
            | Permission::PhotosAddOnly
            | Permission::Videos
            | Permission::AudioLibrary
            | Permission::Notifications
            | Permission::Bluetooth
            | Permission::BluetoothScan
            | Permission::BluetoothConnect
            | Permission::BluetoothAdvertise
            | Permission::Biometrics
            | Permission::Contacts
            | Permission::CalendarRead
            | Permission::CalendarWrite
            | Permission::StorageRead
            | Permission::StorageWrite
            | Permission::BackgroundRefresh
            | Permission::MediaProjection
            | Permission::ClipboardRead => PermissionStatus::Granted,
            Permission::LocationAlways
            | Permission::Sensors
            | Permission::MotionActivity
            | Permission::ManageExternalStorage
            | Permission::Nfc
            | Permission::PhoneState
            | Permission::CallPhone
            | Permission::SmsRead
            | Permission::SmsSend
            | Permission::SystemAlertWindow
            | Permission::ScheduleExactAlarm
            | Permission::InstallPackages
            | Permission::Vibrate
            | Permission::DeviceAdmin
            | Permission::WriteSettings
            | Permission::PackageUsageStats
            | Permission::IgnoreBatteryOptimizations
            | Permission::NotificationListener
            | Permission::VpnService
            | Permission::Custom(_) => PermissionStatus::Unsupported,
            Permission::AccessibilityService => PermissionStatus::Granted,
            Permission::AdminPrivileges => {
                #[cfg(target_os = "windows")]
                {
                    PermissionStatus::Denied
                }
                #[cfg(unix)]
                {
                    if std::env::var("USER").map(|u| u == "root").unwrap_or(false)
                        || std::env::var("SUDO_USER").is_ok()
                    {
                        PermissionStatus::Granted
                    } else {
                        PermissionStatus::Denied
                    }
                }
                #[cfg(not(any(target_os = "windows", unix)))]
                {
                    PermissionStatus::Denied
                }
            }
        }
    }
}

impl PermissionBackend for DesktopPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        if let Ok(guard) = self.overrides.lock()
            && let Some(status) = guard.get(permission)
        {
            return *status;
        }
        Self::default_desktop_status(permission)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        self.check(permission)
    }

    fn should_show_rationale(&self, _permission: &Permission) -> bool {
        false
    }

    fn open_app_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security")
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:privacy"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open")
                .arg("settings://privacy")
                .spawn()
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn open_accessibility_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:easeofaccess"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("gnome-control-center")
                .arg("universal-access")
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("xdg-open")
                        .arg("settings://accessibility")
                        .spawn()
                })
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn open_admin_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security")
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:privacy"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("gnome-control-center")
                .arg("user-accounts")
                .spawn()
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn is_accessibility_enabled(&self) -> bool {
        self.check(&Permission::AccessibilityService) == PermissionStatus::Granted
    }

    fn is_admin_active(&self) -> bool {
        self.check(&Permission::AdminPrivileges) == PermissionStatus::Granted
    }

    fn request_admin_privileges(&self) -> bool {
        if self.is_admin_active() {
            return true;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("osascript")
                .args([
                    "-e",
                    "do shell script \"echo admin\" with administrator privileges",
                ])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(target_os = "windows")]
        {
            false
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("pkexec")
                .args(["echo", "admin"])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }
}
