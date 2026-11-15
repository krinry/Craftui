use craftui_core::ClipboardBackend;
use std::sync::{Arc, Mutex};
#[cfg(all(unix, not(target_os = "macos")))]
use std::time::Duration;

#[derive(Default)]
pub(crate) struct DesktopClipboardState {
    pub(crate) fallback: String,
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(crate) x11: Option<x11_clipboard::Clipboard>,
}

/// In-process permissive OS clipboard bridge (`wl-clipboard-rs` + `x11-clipboard` on Linux/BSD,
/// `objc2-app-kit` `NSPasteboard` on macOS, `windows-sys` Win32 Clipboard on Windows) with an
/// in-memory fallback cache for headless CI environments without a display server.
#[derive(Clone, Default)]
pub struct DesktopClipboard {
    state: Arc<Mutex<DesktopClipboardState>>,
}

impl ClipboardBackend for DesktopClipboard {
    fn get_text(&mut self) -> Option<String> {
        let Ok(mut guard) = self.state.lock() else {
            return None;
        };
        if let Some(sys) = read_os_clipboard(&mut guard) {
            guard.fallback = sys.clone();
            return Some(sys);
        }
        Some(guard.fallback.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        let Ok(mut guard) = self.state.lock() else {
            return false;
        };
        guard.fallback.clear();
        guard.fallback.push_str(text);
        let _ = write_os_clipboard(&mut guard, text);
        true
    }
}

fn read_os_clipboard(
    #[allow(unused_variables)] state: &mut DesktopClipboardState,
) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        let pb = NSPasteboard::generalPasteboard();
        // SAFETY: `NSPasteboardTypeString` is a static immutable AppKit UTI constant.
        let pb_type = unsafe { NSPasteboardTypeString };
        pb.stringForType(pb_type).map(|ns| ns.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

        const CF_UNICODETEXT: u32 = 13;
        struct ClipboardCloseGuard;
        impl Drop for ClipboardCloseGuard {
            fn drop(&mut self) {
                // SAFETY: Paired with a successful `OpenClipboard` before guard construction.
                unsafe {
                    CloseClipboard();
                }
            }
        }

        // SAFETY: Opening and reading global clipboard memory handle within balanced Open/Close.
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return None;
            }
            let _guard = ClipboardCloseGuard;
            let handle: HANDLE = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let byte_len = GlobalSize(handle);
            let max_u16 = byte_len / std::mem::size_of::<u16>();
            let slice = std::slice::from_raw_parts(ptr, max_u16);
            let nul_pos = slice.iter().position(|&u| u == 0).unwrap_or(slice.len());
            let decoded = String::from_utf16_lossy(&slice[..nul_pos]);
            GlobalUnlock(handle);
            Some(decoded)
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && let Ok((mut pipe, _mime)) = wl_clipboard_rs::paste::get_contents(
                wl_clipboard_rs::paste::ClipboardType::Regular,
                wl_clipboard_rs::paste::Seat::Unspecified,
                wl_clipboard_rs::paste::MimeType::Text,
            )
        {
            let mut buf = String::new();
            if std::io::Read::read_to_string(&mut pipe, &mut buf).is_ok() {
                return Some(buf);
            }
        }
        if std::env::var_os("DISPLAY").is_some() {
            if state.x11.is_none() {
                state.x11 = x11_clipboard::Clipboard::new().ok();
            }
            if let Some(x11) = state.x11.as_ref()
                && let Ok(bytes) = x11.load(
                    x11.getter.atoms.clipboard,
                    x11.getter.atoms.utf8_string,
                    x11.getter.atoms.property,
                    Some(Duration::from_millis(250)),
                )
                && let Ok(text) = String::from_utf8(bytes)
            {
                return Some(text);
            }
        }
        None
    }
}

fn write_os_clipboard(
    #[allow(unused_variables)] state: &mut DesktopClipboardState,
    text: &str,
) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let ns_text = NSString::from_str(text);
        // SAFETY: `NSPasteboardTypeString` is a static immutable AppKit UTI constant.
        let pb_type = unsafe { NSPasteboardTypeString };
        pb.setString_forType(&ns_text, pb_type)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows_sys::Win32::System::Memory::{
            GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

        const CF_UNICODETEXT: u32 = 13;
        struct ClipboardCloseGuard;
        impl Drop for ClipboardCloseGuard {
            fn drop(&mut self) {
                // SAFETY: Paired with a successful `OpenClipboard` before guard construction.
                unsafe {
                    CloseClipboard();
                }
            }
        }

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * std::mem::size_of::<u16>();
        // SAFETY: Allocates movable HGLOBAL, fills UTF-16 payload, transfers ownership to clipboard on success.
        unsafe {
            let owner = GetDesktopWindow();
            if OpenClipboard(owner) == 0 {
                return false;
            }
            let _guard = ClipboardCloseGuard;
            if EmptyClipboard() == 0 {
                return false;
            }
            let hmem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, byte_len);
            if hmem.is_null() {
                return false;
            }
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                GlobalFree(hmem);
                return false;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            GlobalUnlock(hmem);
            if SetClipboardData(CF_UNICODETEXT, hmem as HANDLE).is_null() {
                GlobalFree(hmem);
                return false;
            }
            true
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && wl_clipboard_rs::copy::Options::new()
                .copy(
                    wl_clipboard_rs::copy::Source::Bytes(text.as_bytes().into()),
                    wl_clipboard_rs::copy::MimeType::Text,
                )
                .is_ok()
        {
            return true;
        }
        if std::env::var_os("DISPLAY").is_some() {
            if state.x11.is_none() {
                state.x11 = x11_clipboard::Clipboard::new().ok();
            }
            if let Some(x11) = state.x11.as_ref()
                && x11
                    .store(
                        x11.setter.atoms.clipboard,
                        x11.setter.atoms.utf8_string,
                        text.as_bytes().to_vec(),
                    )
                    .is_ok()
            {
                return true;
            }
        }
        false
    }
}
