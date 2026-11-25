use crate::Point;
use serde::{Deserialize, Serialize};

/// Unique pointer/finger identifier across multi-touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerId(pub u64);

/// Mouse / pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

/// Pointer lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Unified mouse/touch/stylus pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub id: PointerId,
    pub phase: PointerPhase,
    pub position: Point,
    pub button: Option<PointerButton>,
    pub modifiers: Modifiers,
    pub is_touch: bool,
    pub timestamp_ms: u64,
}

/// Keyboard modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    #[serde(default)]
    pub alt_gr: bool,
    #[serde(default)]
    pub caps_lock: bool,
}

impl Modifiers {
    /// Primary shortcut modifier (`Cmd` on macOS/iOS/browser-Mac, `Ctrl` everywhere else).
    #[inline]
    pub fn command(self) -> bool {
        if cfg!(any(
            target_os = "macos",
            target_os = "ios",
            target_arch = "wasm32"
        )) {
            self.super_key || self.ctrl
        } else {
            self.ctrl
        }
    }

    /// Word navigation / deletion modifier (`Alt`/`Option` on macOS/iOS, `Ctrl` elsewhere).
    #[inline]
    pub fn word_jump(self) -> bool {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            self.alt
        } else {
            self.ctrl
        }
    }
}

/// Platform-neutral logical and physical ANSI key codes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    Character(String),
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Space,
    CapsLock,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Backquote,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Minus,
    Equal,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Unknown,
}

impl KeyCode {
    /// Returns the W3C DOM `KeyboardEvent.code` string for physical keys when known.
    #[inline]
    pub fn dom_code(&self) -> Option<&'static str> {
        match self {
            Self::Enter => Some("Enter"),
            Self::Tab => Some("Tab"),
            Self::Backspace => Some("Backspace"),
            Self::Delete => Some("Delete"),
            Self::Escape => Some("Escape"),
            Self::Space => Some("Space"),
            Self::CapsLock => Some("CapsLock"),
            Self::ShiftLeft => Some("ShiftLeft"),
            Self::ShiftRight => Some("ShiftRight"),
            Self::ControlLeft => Some("ControlLeft"),
            Self::ControlRight => Some("ControlRight"),
            Self::AltLeft => Some("AltLeft"),
            Self::AltRight => Some("AltRight"),
            Self::ArrowLeft => Some("ArrowLeft"),
            Self::ArrowRight => Some("ArrowRight"),
            Self::ArrowUp => Some("ArrowUp"),
            Self::ArrowDown => Some("ArrowDown"),
            Self::Home => Some("Home"),
            Self::End => Some("End"),
            Self::PageUp => Some("PageUp"),
            Self::PageDown => Some("PageDown"),
            Self::Backquote => Some("Backquote"),
            Self::Digit0 => Some("Digit0"),
            Self::Digit1 => Some("Digit1"),
            Self::Digit2 => Some("Digit2"),
            Self::Digit3 => Some("Digit3"),
            Self::Digit4 => Some("Digit4"),
            Self::Digit5 => Some("Digit5"),
            Self::Digit6 => Some("Digit6"),
            Self::Digit7 => Some("Digit7"),
            Self::Digit8 => Some("Digit8"),
            Self::Digit9 => Some("Digit9"),
            Self::Minus => Some("Minus"),
            Self::Equal => Some("Equal"),
            Self::KeyA => Some("KeyA"),
            Self::KeyB => Some("KeyB"),
            Self::KeyC => Some("KeyC"),
            Self::KeyD => Some("KeyD"),
            Self::KeyE => Some("KeyE"),
            Self::KeyF => Some("KeyF"),
            Self::KeyG => Some("KeyG"),
            Self::KeyH => Some("KeyH"),
            Self::KeyI => Some("KeyI"),
            Self::KeyJ => Some("KeyJ"),
            Self::KeyK => Some("KeyK"),
            Self::KeyL => Some("KeyL"),
            Self::KeyM => Some("KeyM"),
            Self::KeyN => Some("KeyN"),
            Self::KeyO => Some("KeyO"),
            Self::KeyP => Some("KeyP"),
            Self::KeyQ => Some("KeyQ"),
            Self::KeyR => Some("KeyR"),
            Self::KeyS => Some("KeyS"),
            Self::KeyT => Some("KeyT"),
            Self::KeyU => Some("KeyU"),
            Self::KeyV => Some("KeyV"),
            Self::KeyW => Some("KeyW"),
            Self::KeyX => Some("KeyX"),
            Self::KeyY => Some("KeyY"),
            Self::KeyZ => Some("KeyZ"),
            Self::BracketLeft => Some("BracketLeft"),
            Self::BracketRight => Some("BracketRight"),
            Self::Backslash => Some("Backslash"),
            Self::Semicolon => Some("Semicolon"),
            Self::Quote => Some("Quote"),
            Self::Comma => Some("Comma"),
            Self::Period => Some("Period"),
            Self::Slash => Some("Slash"),
            Self::F1 => Some("F1"),
            Self::F2 => Some("F2"),
            Self::F3 => Some("F3"),
            Self::F4 => Some("F4"),
            Self::F5 => Some("F5"),
            Self::F6 => Some("F6"),
            Self::F7 => Some("F7"),
            Self::F8 => Some("F8"),
            Self::F9 => Some("F9"),
            Self::F10 => Some("F10"),
            Self::F11 => Some("F11"),
            Self::F12 => Some("F12"),
            Self::Character(_) | Self::Unknown => None,
        }
    }

    /// Parses a W3C DOM `KeyboardEvent.code` string into a physical [`KeyCode`].
    #[inline]
    pub fn from_dom_code(code: &str) -> Option<Self> {
        Some(match code {
            "Enter" | "NumpadEnter" => Self::Enter,
            "Tab" => Self::Tab,
            "Backspace" => Self::Backspace,
            "Delete" => Self::Delete,
            "Escape" => Self::Escape,
            "Space" => Self::Space,
            "CapsLock" => Self::CapsLock,
            "ShiftLeft" => Self::ShiftLeft,
            "ShiftRight" => Self::ShiftRight,
            "ControlLeft" => Self::ControlLeft,
            "ControlRight" => Self::ControlRight,
            "AltLeft" => Self::AltLeft,
            "AltRight" => Self::AltRight,
            "ArrowLeft" => Self::ArrowLeft,
            "ArrowRight" => Self::ArrowRight,
            "ArrowUp" => Self::ArrowUp,
            "ArrowDown" => Self::ArrowDown,
            "Home" => Self::Home,
            "End" => Self::End,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            "Backquote" => Self::Backquote,
            "Digit0" => Self::Digit0,
            "Digit1" => Self::Digit1,
            "Digit2" => Self::Digit2,
            "Digit3" => Self::Digit3,
            "Digit4" => Self::Digit4,
            "Digit5" => Self::Digit5,
            "Digit6" => Self::Digit6,
            "Digit7" => Self::Digit7,
            "Digit8" => Self::Digit8,
            "Digit9" => Self::Digit9,
            "Minus" => Self::Minus,
            "Equal" => Self::Equal,
            "KeyA" => Self::KeyA,
            "KeyB" => Self::KeyB,
            "KeyC" => Self::KeyC,
            "KeyD" => Self::KeyD,
            "KeyE" => Self::KeyE,
            "KeyF" => Self::KeyF,
            "KeyG" => Self::KeyG,
            "KeyH" => Self::KeyH,
            "KeyI" => Self::KeyI,
            "KeyJ" => Self::KeyJ,
            "KeyK" => Self::KeyK,
            "KeyL" => Self::KeyL,
            "KeyM" => Self::KeyM,
            "KeyN" => Self::KeyN,
            "KeyO" => Self::KeyO,
            "KeyP" => Self::KeyP,
            "KeyQ" => Self::KeyQ,
            "KeyR" => Self::KeyR,
            "KeyS" => Self::KeyS,
            "KeyT" => Self::KeyT,
            "KeyU" => Self::KeyU,
            "KeyV" => Self::KeyV,
            "KeyW" => Self::KeyW,
            "KeyX" => Self::KeyX,
            "KeyY" => Self::KeyY,
            "KeyZ" => Self::KeyZ,
            "BracketLeft" => Self::BracketLeft,
            "BracketRight" => Self::BracketRight,
            "Backslash" => Self::Backslash,
            "Semicolon" => Self::Semicolon,
            "Quote" => Self::Quote,
            "Comma" => Self::Comma,
            "Period" => Self::Period,
            "Slash" => Self::Slash,
            "F1" => Self::F1,
            "F2" => Self::F2,
            "F3" => Self::F3,
            "F4" => Self::F4,
            "F5" => Self::F5,
            "F6" => Self::F6,
            "F7" => Self::F7,
            "F8" => Self::F8,
            "F9" => Self::F9,
            "F10" => Self::F10,
            "F11" => Self::F11,
            "F12" => Self::F12,
            _ => return None,
        })
    }

    /// Returns true if this key corresponds to ASCII letter `ch` (case-insensitive),
    /// matching both named `Key*` variants and [`KeyCode::Character`].
    #[inline]
    pub fn matches_ascii_letter(&self, ch: char) -> bool {
        let lower = ch.to_ascii_lowercase();
        match self {
            Self::KeyA => lower == 'a',
            Self::KeyB => lower == 'b',
            Self::KeyC => lower == 'c',
            Self::KeyD => lower == 'd',
            Self::KeyE => lower == 'e',
            Self::KeyF => lower == 'f',
            Self::KeyG => lower == 'g',
            Self::KeyH => lower == 'h',
            Self::KeyI => lower == 'i',
            Self::KeyJ => lower == 'j',
            Self::KeyK => lower == 'k',
            Self::KeyL => lower == 'l',
            Self::KeyM => lower == 'm',
            Self::KeyN => lower == 'n',
            Self::KeyO => lower == 'o',
            Self::KeyP => lower == 'p',
            Self::KeyQ => lower == 'q',
            Self::KeyR => lower == 'r',
            Self::KeyS => lower == 's',
            Self::KeyT => lower == 't',
            Self::KeyU => lower == 'u',
            Self::KeyV => lower == 'v',
            Self::KeyW => lower == 'w',
            Self::KeyX => lower == 'x',
            Self::KeyY => lower == 'y',
            Self::KeyZ => lower == 'z',
            Self::Character(s) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => c.to_ascii_lowercase() == lower,
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

/// Keyboard press/release event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub text: Option<String>,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: Modifiers,
}

/// Mobile & desktop Input Method Editor (IME) composition event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImeEvent {
    Enabled,
    Preedit(String, Option<(usize, usize)>),
    Commit(String),
    Disabled,
}

/// Mouse wheel or trackpad scroll delta.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ScrollDelta {
    pub x: f32,
    pub y: f32,
    pub position: Point,
}

/// Top-level UI event routed into the widget tree & gesture arena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UiEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
    Ime(ImeEvent),
    Scroll(ScrollDelta),
    Pinch {
        factor: f32,
        center: Point,
    },
    Resize {
        width: f32,
        height: f32,
        scale_factor: f32,
    },
    FocusChanged(bool),
}
