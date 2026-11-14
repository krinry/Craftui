//! Android platform adapter (`android-activity` `GameActivity` + Vulkan/GLES `wgpu` + JNI/IME/Surface bridges).

#[cfg(target_os = "android")]
pub use android_activity;

pub mod host;
pub mod input;
pub mod remote_preview;
#[cfg(target_os = "android")]
pub mod runtime;
#[cfg(target_os = "android")]
pub use runtime as android_runtime;
pub mod services;

#[cfg(test)]
mod tests;

pub use host::AndroidHost;
pub use input::{
    map_android_digit, map_android_punctuation, AndroidImeCommand, AndroidKeyCode,
    AndroidPlatformViewMode, AndroidPunctuationKey, ExternalSurfaceTextureBridge,
};
pub use services::{
    AndroidBridgeState, AndroidDeviceCapabilities, AndroidFileDialogBridge, AndroidHapticsBackend,
    AndroidPermissionBackend, AndroidPlatformCommand, AndroidSecureStorage, AndroidShareBackend,
    ANDROID_PERMISSION_GRANTED,
};

#[cfg(all(target_os = "android", feature = "builtin-main"))]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    android_runtime::run_android_app(app, |cx, tree, _textures, _disp| {
        use craftui_core::{pct, Insets};
        use craftui_layout::LayoutStyle;
        use craftui_text::TextStyle;
        use craftui_widgets::BoxStyle;

        let root = tree.container(
            cx,
            LayoutStyle::column()
                .with_size(pct(100.0), pct(100.0))
                .with_padding(Insets::all(16.0))
                .with_gap(12.0),
            BoxStyle::new(),
        );

        let safe = tree.safe_area(cx);

        let header = tree.text(
            cx,
            "CraftUI Native Android",
            TextStyle::default()
                .with_size(22.0)
                .with_weight(700.0)
                .with_color(tree.theme().text_primary),
        );

        let desc = tree.text(
            cx,
            "Running high-performance GPU SDF / Vulkan on Android!",
            TextStyle::default()
                .with_size(14.0)
                .with_color(tree.theme().text_secondary),
        );

        let btn = tree.button(cx, "Interactive Native Button", move || {
            tracing::info!("Android native button tapped!");
        });

        tree.append_child(safe, header);
        tree.append_child(safe, desc);
        tree.append_child(safe, btn);
        tree.append_child(root, safe);

        root
    });
}
