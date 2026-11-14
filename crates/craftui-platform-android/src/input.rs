use craftui_core::PlatformViewId;

/// Embedding strategy for native Android Views (e.g., CameraX / ExoPlayer / WebView)
/// hosted inside CraftUI [`WidgetKind::PlatformView`](craftui_widgets::WidgetKind::PlatformView).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPlatformViewMode {
    /// Zero-copy Vulkan/GLES `HardwareBuffer` / `SurfaceTexture` external texture composited directly inside Vello.
    ExternalSurfaceTexture { surface_id: u64 },
    /// Transparent `PaintCommand::PlatformViewHole` punch composited over an Android `SurfaceView`.
    HolePunch { view_id: PlatformViewId },
}

/// Bridge trait for importing Android `AHardwareBuffer` / `SurfaceTexture` RGBA frames into CraftUI textures.
pub trait ExternalSurfaceTextureBridge: Send + Sync {
    fn surface_id(&self) -> u64;
    /// Copy or acquire the latest frame dimensions and straight RGBA8 bytes when dirty.
    fn acquire_latest_rgba(&mut self) -> Option<(u32, u32, Vec<u8>)>;
}

/// Platform-neutral Android keycodes translated by [`AndroidHost::handle_android_key_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidKeyCode {
    Back,
    Enter,
    NumpadEnter,
    Tab,
    Del,
    ForwardDel,
    Escape,
    DpadLeft,
    DpadRight,
    DpadUp,
    DpadDown,
    Home,
    MoveEnd,
    PageUp,
    PageDown,
    Space,
    Cut,
    Copy,
    Paste,
    Character(char),
    Other,
}

/// Hardware / virtual punctuation & numpad keys mapped across US QWERTY shift layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPunctuationKey {
    Comma,
    Period,
    Minus,
    Equals,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Slash,
    Grave,
    At,
    Plus,
    Star,
    Pound,
    NumpadDivide,
    NumpadMultiply,
    NumpadSubtract,
    NumpadAdd,
    NumpadDot,
    NumpadComma,
    NumpadEquals,
    NumpadLeftParen,
    NumpadRightParen,
}

/// Translate a punctuation/numpad key with active `shift` modifier into a printable character.
pub fn map_android_punctuation(key: AndroidPunctuationKey, shift: bool) -> char {
    match key {
        AndroidPunctuationKey::Comma => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        AndroidPunctuationKey::Period => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        AndroidPunctuationKey::Minus => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        AndroidPunctuationKey::Equals => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        AndroidPunctuationKey::LeftBracket => {
            if shift {
                '{'
            } else {
                '['
            }
        }
        AndroidPunctuationKey::RightBracket => {
            if shift {
                '}'
            } else {
                ']'
            }
        }
        AndroidPunctuationKey::Backslash => {
            if shift {
                '|'
            } else {
                '\\'
            }
        }
        AndroidPunctuationKey::Semicolon => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        AndroidPunctuationKey::Apostrophe => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        AndroidPunctuationKey::Slash => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        AndroidPunctuationKey::Grave => {
            if shift {
                '~'
            } else {
                '`'
            }
        }
        AndroidPunctuationKey::At => '@',
        AndroidPunctuationKey::Plus => '+',
        AndroidPunctuationKey::Star => '*',
        AndroidPunctuationKey::Pound => '#',
        AndroidPunctuationKey::NumpadDivide => '/',
        AndroidPunctuationKey::NumpadMultiply => '*',
        AndroidPunctuationKey::NumpadSubtract => '-',
        AndroidPunctuationKey::NumpadAdd => '+',
        AndroidPunctuationKey::NumpadDot => '.',
        AndroidPunctuationKey::NumpadComma => ',',
        AndroidPunctuationKey::NumpadEquals => '=',
        AndroidPunctuationKey::NumpadLeftParen => '(',
        AndroidPunctuationKey::NumpadRightParen => ')',
    }
}

/// Translate top-row digits `'0'..='9'` with Shift modifier cleanly.
pub fn map_android_digit(digit: char, shift: bool) -> char {
    if !shift {
        return digit;
    }
    match digit {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        other => other,
    }
}

/// Commands emitted toward Android `InputMethodManager` via `GameActivity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidImeCommand {
    ShowSoftInput,
    HideSoftInput,
    SyncTextState {
        text: String,
        selection_start: usize,
        selection_end: usize,
        compose_region: Option<(usize, usize)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncedImeSnapshot {
    pub(crate) text: String,
    pub(crate) selection_start: usize,
    pub(crate) selection_end: usize,
    pub(crate) compose_region: Option<(usize, usize)>,
}

pub(crate) fn clamp_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
