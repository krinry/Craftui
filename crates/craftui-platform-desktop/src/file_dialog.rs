use craftui_core::{FileDialogBackend, FileDialogRequest};
use std::path::PathBuf;

/// Pure permissive native OS file open/save picker powered in-process by `rfd` (`xdg-portal` + `wayland`
/// on Linux/BSD, AppKit `NSOpenPanel`/`NSSavePanel` on macOS, Win32 `IFileDialog` on Windows).
///
/// Use [`PlatformServices::pick_open_file_async`], [`PlatformServices::pick_open_files_async`], or
/// [`PlatformServices::pick_save_file_async`] from UI handlers so dialogs run off the UI thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopFileDialog;

/// Build an `rfd::FileDialog` configured from a platform-neutral [`FileDialogRequest`].
pub fn build_rfd_dialog(request: &FileDialogRequest) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new();
    if !request.title.is_empty() {
        dialog = dialog.set_title(&request.title);
    }
    if let Some(dir) = &request.default_directory {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = &request.default_name {
        dialog = dialog.set_file_name(name);
    }
    for filter in &request.filters {
        if !filter.extensions.is_empty() {
            let exts: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&filter.name, &exts);
        }
    }
    dialog
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn has_unix_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub(crate) fn has_unix_display() -> bool {
    true
}

impl FileDialogBackend for DesktopFileDialog {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        if !has_unix_display() {
            return Vec::new();
        }
        let dialog = build_rfd_dialog(request);
        match (request.directory, request.multiple) {
            (true, true) => dialog.pick_folders().unwrap_or_default(),
            (true, false) => dialog.pick_folder().into_iter().collect(),
            (false, true) => dialog.pick_files().unwrap_or_default(),
            (false, false) => dialog.pick_file().into_iter().collect(),
        }
    }

    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        if !has_unix_display() {
            return None;
        }
        build_rfd_dialog(request).save_file()
    }
}
