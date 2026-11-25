//! Core geometry, color, style, event, theme, platform services, and dispatcher primitives for CraftUI.

pub mod builtin_plugins;
pub mod color;
pub mod dispatch;
pub mod error;
pub mod events;
pub mod geometry;
pub mod hybrid;
pub mod i18n;
pub mod ids;
pub mod platform;
pub mod plugin;
pub mod remote;
pub mod style;
pub mod theme;

pub use builtin_plugins::{
    BiometricsAuthResult, BiometricsAvailability, BiometricsClient, BiometricsPlugin,
    CHANNEL_BIOMETRICS, CHANNEL_CAMERA, CHANNEL_NOTIFICATIONS, CHANNEL_URL_LAUNCHER, CameraClient,
    CameraPlugin, CameraResult, NotificationScheduleResult, NotificationSpec, NotificationsClient,
    NotificationsPlugin, UrlCanLaunchResult, UrlLaunchResult, UrlLauncherClient, UrlLauncherPlugin,
};
pub use color::Color;
pub use dispatch::{TimerHandle, UiDispatcher, monotonic_ms, set_monotonic_ms_provider};
pub use error::{CraftError, Result};
pub use events::{
    ImeEvent, KeyCode, KeyEvent, Modifiers, PointerButton, PointerEvent, PointerId, PointerPhase,
    ScrollDelta, UiEvent,
};
pub use geometry::{
    Breakpoint, Constraints, CornerRadii, Insets, Point, Rect, Size, Transform2D, WindowMetrics,
};
pub use hybrid::{
    CRAFTUI_TAURI_IPC_SCRIPT, IpcMessage, IpcResponse, IpcRouter, ScreenKind, TailwindTokens,
    WebViewSource,
};
pub use i18n::{LocalizationCatalog, is_rtl_locale, locale_direction};
pub use ids::{DirtyFlags, NodeId, PlatformViewId, ScopeId, TextureId};
pub use platform::{
    AndroidPermissionSpec, AppLifecycleEvent, BatteryState, ClipboardBackend,
    DeviceCapabilitiesBackend, DeviceInfo, FileDialogBackend, FileDialogRequest, FileFilter,
    HapticStyle, HapticsBackend, MemoryClipboard, MemorySecureStorage, MockDeviceCapabilities,
    MockFileDialog, MockHaptics, MockPermissionBackend, MockShare, NetworkStatus, NoopFileDialog,
    NoopHaptics, NoopPermissionBackend, NoopShare, Permission, PermissionBackend,
    PermissionOutcome, PermissionStatus, PlatformServices, SecureStorageBackend, ShareBackend,
    SharePayload,
};
pub use plugin::{CraftPlugin, MethodCall, PluginRegistry};
pub use remote::{DEFAULT_REMOTE_PREVIEW_PORT, RemotePreviewMessage};
pub use style::{
    Alignment, Axis, BlendMode, Border, BoxShadow, Brush, ColorStop, CursorIcon, Direction, Fit,
    Justify, Length, auto, dp, pct, px,
};
pub use theme::{
    DesignSystem, M3Elevation, Material3Tokens, ThemeMode, ThemeOverride, ThemeTokens,
};
