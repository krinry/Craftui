use craftui_core::{CursorIcon, KeyCode, KeyEvent, Modifiers, PointerButton};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{CursorIcon as WinitCursorIcon, WindowId};

/// Custom event loop messages delivered from background worker threads, AccessKit, `.cui` watcher, or window opening.
#[derive(Debug)]
pub enum DesktopUserEvent {
    WakeUi,
    AccessKit(accesskit_winit::Event),
    #[cfg(feature = "hot-reload")]
    FileChanged,
    OpenWindow(u64),
    CloseWindow(WindowId),
    SetWindowTitle(WindowId, String),
    SetWindowVisible(WindowId, bool),
}

impl From<accesskit_winit::Event> for DesktopUserEvent {
    fn from(ev: accesskit_winit::Event) -> Self {
        Self::AccessKit(ev)
    }
}

pub fn translate_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
        ..Default::default()
    }
}

/// Update `caps_lock` and `alt_gr` tracking on desktop keyboard press/release events.
pub fn sync_modifiers_from_key(mods: &mut Modifiers, key_ev: &winit::event::KeyEvent) {
    let pressed = key_ev.state == ElementState::Pressed;
    if matches!(
        key_ev.physical_key,
        PhysicalKey::Code(WinitKeyCode::AltRight)
    ) || matches!(&key_ev.logical_key, Key::Named(NamedKey::AltGraph))
    {
        mods.alt_gr = pressed;
    }

    if (matches!(
        key_ev.physical_key,
        PhysicalKey::Code(WinitKeyCode::CapsLock)
    ) || matches!(&key_ev.logical_key, Key::Named(NamedKey::CapsLock)))
        && pressed
        && !key_ev.repeat
    {
        mods.caps_lock = !mods.caps_lock;
    }

    // Re-sync initial CapsLock state when an unshifted ASCII letter arrives.
    if pressed
        && !mods.shift
        && !mods.ctrl
        && !mods.alt
        && let Key::Character(s) = &key_ev.logical_key
    {
        let mut chars = s.chars();
        if let (Some(ch), None) = (chars.next(), chars.next())
            && ch.is_ascii_alphabetic()
        {
            mods.caps_lock = ch.is_ascii_uppercase();
        }
    }
}

pub fn translate_winit_cursor(cursor: CursorIcon) -> WinitCursorIcon {
    match cursor {
        CursorIcon::Default => WinitCursorIcon::Default,
        CursorIcon::Pointer => WinitCursorIcon::Pointer,
        CursorIcon::Text => WinitCursorIcon::Text,
        CursorIcon::Grab => WinitCursorIcon::Grab,
        CursorIcon::Grabbing => WinitCursorIcon::Grabbing,
        CursorIcon::ResizeHorizontal => WinitCursorIcon::EwResize,
        CursorIcon::ResizeVertical => WinitCursorIcon::NsResize,
        CursorIcon::NotAllowed => WinitCursorIcon::NotAllowed,
        CursorIcon::Crosshair => WinitCursorIcon::Crosshair,
    }
}

pub fn translate_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(_) => PointerButton::Primary,
    }
}

pub fn translate_scroll_delta(delta: MouseScrollDelta, scale_factor: f32) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * 36.0, y * 36.0),
        MouseScrollDelta::PixelDelta(pos) => {
            (pos.x as f32 / scale_factor, pos.y as f32 / scale_factor)
        }
    }
}

pub fn translate_winit_key(key_ev: &winit::event::KeyEvent, modifiers: Modifiers) -> KeyEvent {
    let code = match key_ev.physical_key {
        PhysicalKey::Code(WinitKeyCode::Enter) | PhysicalKey::Code(WinitKeyCode::NumpadEnter) => {
            KeyCode::Enter
        }
        PhysicalKey::Code(WinitKeyCode::Tab) => KeyCode::Tab,
        PhysicalKey::Code(WinitKeyCode::Backspace) => KeyCode::Backspace,
        PhysicalKey::Code(WinitKeyCode::Delete) => KeyCode::Delete,
        PhysicalKey::Code(WinitKeyCode::Escape) => KeyCode::Escape,
        PhysicalKey::Code(WinitKeyCode::Space) => KeyCode::Space,
        PhysicalKey::Code(WinitKeyCode::CapsLock) => KeyCode::CapsLock,
        PhysicalKey::Code(WinitKeyCode::ShiftLeft) => KeyCode::ShiftLeft,
        PhysicalKey::Code(WinitKeyCode::ShiftRight) => KeyCode::ShiftRight,
        PhysicalKey::Code(WinitKeyCode::ControlLeft) => KeyCode::ControlLeft,
        PhysicalKey::Code(WinitKeyCode::ControlRight) => KeyCode::ControlRight,
        PhysicalKey::Code(WinitKeyCode::AltLeft) => KeyCode::AltLeft,
        PhysicalKey::Code(WinitKeyCode::AltRight) => KeyCode::AltRight,
        PhysicalKey::Code(WinitKeyCode::ArrowLeft) => KeyCode::ArrowLeft,
        PhysicalKey::Code(WinitKeyCode::ArrowRight) => KeyCode::ArrowRight,
        PhysicalKey::Code(WinitKeyCode::ArrowUp) => KeyCode::ArrowUp,
        PhysicalKey::Code(WinitKeyCode::ArrowDown) => KeyCode::ArrowDown,
        PhysicalKey::Code(WinitKeyCode::Home) => KeyCode::Home,
        PhysicalKey::Code(WinitKeyCode::End) => KeyCode::End,
        PhysicalKey::Code(WinitKeyCode::PageUp) => KeyCode::PageUp,
        PhysicalKey::Code(WinitKeyCode::PageDown) => KeyCode::PageDown,
        PhysicalKey::Code(WinitKeyCode::Backquote) => KeyCode::Backquote,
        PhysicalKey::Code(WinitKeyCode::Digit0) => KeyCode::Digit0,
        PhysicalKey::Code(WinitKeyCode::Digit1) => KeyCode::Digit1,
        PhysicalKey::Code(WinitKeyCode::Digit2) => KeyCode::Digit2,
        PhysicalKey::Code(WinitKeyCode::Digit3) => KeyCode::Digit3,
        PhysicalKey::Code(WinitKeyCode::Digit4) => KeyCode::Digit4,
        PhysicalKey::Code(WinitKeyCode::Digit5) => KeyCode::Digit5,
        PhysicalKey::Code(WinitKeyCode::Digit6) => KeyCode::Digit6,
        PhysicalKey::Code(WinitKeyCode::Digit7) => KeyCode::Digit7,
        PhysicalKey::Code(WinitKeyCode::Digit8) => KeyCode::Digit8,
        PhysicalKey::Code(WinitKeyCode::Digit9) => KeyCode::Digit9,
        PhysicalKey::Code(WinitKeyCode::Minus) => KeyCode::Minus,
        PhysicalKey::Code(WinitKeyCode::Equal) => KeyCode::Equal,
        PhysicalKey::Code(WinitKeyCode::KeyA) => KeyCode::KeyA,
        PhysicalKey::Code(WinitKeyCode::KeyB) => KeyCode::KeyB,
        PhysicalKey::Code(WinitKeyCode::KeyC) => KeyCode::KeyC,
        PhysicalKey::Code(WinitKeyCode::KeyD) => KeyCode::KeyD,
        PhysicalKey::Code(WinitKeyCode::KeyE) => KeyCode::KeyE,
        PhysicalKey::Code(WinitKeyCode::KeyF) => KeyCode::KeyF,
        PhysicalKey::Code(WinitKeyCode::KeyG) => KeyCode::KeyG,
        PhysicalKey::Code(WinitKeyCode::KeyH) => KeyCode::KeyH,
        PhysicalKey::Code(WinitKeyCode::KeyI) => KeyCode::KeyI,
        PhysicalKey::Code(WinitKeyCode::KeyJ) => KeyCode::KeyJ,
        PhysicalKey::Code(WinitKeyCode::KeyK) => KeyCode::KeyK,
        PhysicalKey::Code(WinitKeyCode::KeyL) => KeyCode::KeyL,
        PhysicalKey::Code(WinitKeyCode::KeyM) => KeyCode::KeyM,
        PhysicalKey::Code(WinitKeyCode::KeyN) => KeyCode::KeyN,
        PhysicalKey::Code(WinitKeyCode::KeyO) => KeyCode::KeyO,
        PhysicalKey::Code(WinitKeyCode::KeyP) => KeyCode::KeyP,
        PhysicalKey::Code(WinitKeyCode::KeyQ) => KeyCode::KeyQ,
        PhysicalKey::Code(WinitKeyCode::KeyR) => KeyCode::KeyR,
        PhysicalKey::Code(WinitKeyCode::KeyS) => KeyCode::KeyS,
        PhysicalKey::Code(WinitKeyCode::KeyT) => KeyCode::KeyT,
        PhysicalKey::Code(WinitKeyCode::KeyU) => KeyCode::KeyU,
        PhysicalKey::Code(WinitKeyCode::KeyV) => KeyCode::KeyV,
        PhysicalKey::Code(WinitKeyCode::KeyW) => KeyCode::KeyW,
        PhysicalKey::Code(WinitKeyCode::KeyX) => KeyCode::KeyX,
        PhysicalKey::Code(WinitKeyCode::KeyY) => KeyCode::KeyY,
        PhysicalKey::Code(WinitKeyCode::KeyZ) => KeyCode::KeyZ,
        PhysicalKey::Code(WinitKeyCode::BracketLeft) => KeyCode::BracketLeft,
        PhysicalKey::Code(WinitKeyCode::BracketRight) => KeyCode::BracketRight,
        PhysicalKey::Code(WinitKeyCode::Backslash) => KeyCode::Backslash,
        PhysicalKey::Code(WinitKeyCode::Semicolon) => KeyCode::Semicolon,
        PhysicalKey::Code(WinitKeyCode::Quote) => KeyCode::Quote,
        PhysicalKey::Code(WinitKeyCode::Comma) => KeyCode::Comma,
        PhysicalKey::Code(WinitKeyCode::Period) => KeyCode::Period,
        PhysicalKey::Code(WinitKeyCode::Slash) => KeyCode::Slash,
        PhysicalKey::Code(WinitKeyCode::F1) => KeyCode::F1,
        PhysicalKey::Code(WinitKeyCode::F2) => KeyCode::F2,
        PhysicalKey::Code(WinitKeyCode::F3) => KeyCode::F3,
        PhysicalKey::Code(WinitKeyCode::F4) => KeyCode::F4,
        PhysicalKey::Code(WinitKeyCode::F5) => KeyCode::F5,
        PhysicalKey::Code(WinitKeyCode::F6) => KeyCode::F6,
        PhysicalKey::Code(WinitKeyCode::F7) => KeyCode::F7,
        PhysicalKey::Code(WinitKeyCode::F8) => KeyCode::F8,
        PhysicalKey::Code(WinitKeyCode::F9) => KeyCode::F9,
        PhysicalKey::Code(WinitKeyCode::F10) => KeyCode::F10,
        PhysicalKey::Code(WinitKeyCode::F11) => KeyCode::F11,
        PhysicalKey::Code(WinitKeyCode::F12) => KeyCode::F12,
        _ => match &key_ev.logical_key {
            Key::Named(NamedKey::Enter) => KeyCode::Enter,
            Key::Named(NamedKey::Tab) => KeyCode::Tab,
            Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
            Key::Named(NamedKey::Delete) => KeyCode::Delete,
            Key::Named(NamedKey::Escape) => KeyCode::Escape,
            Key::Named(NamedKey::Space) => KeyCode::Space,
            Key::Named(NamedKey::CapsLock) => KeyCode::CapsLock,
            Key::Named(NamedKey::ArrowLeft) => KeyCode::ArrowLeft,
            Key::Named(NamedKey::ArrowRight) => KeyCode::ArrowRight,
            Key::Named(NamedKey::ArrowUp) => KeyCode::ArrowUp,
            Key::Named(NamedKey::ArrowDown) => KeyCode::ArrowDown,
            Key::Named(NamedKey::Home) => KeyCode::Home,
            Key::Named(NamedKey::End) => KeyCode::End,
            Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
            Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
            Key::Named(NamedKey::F1) => KeyCode::F1,
            Key::Named(NamedKey::F2) => KeyCode::F2,
            Key::Named(NamedKey::F3) => KeyCode::F3,
            Key::Named(NamedKey::F4) => KeyCode::F4,
            Key::Named(NamedKey::F5) => KeyCode::F5,
            Key::Named(NamedKey::F6) => KeyCode::F6,
            Key::Named(NamedKey::F7) => KeyCode::F7,
            Key::Named(NamedKey::F8) => KeyCode::F8,
            Key::Named(NamedKey::F9) => KeyCode::F9,
            Key::Named(NamedKey::F10) => KeyCode::F10,
            Key::Named(NamedKey::F11) => KeyCode::F11,
            Key::Named(NamedKey::F12) => KeyCode::F12,
            Key::Character(s) => KeyCode::Character(s.to_string()),
            _ => KeyCode::Unknown,
        },
    };
    let text = key_ev.text.as_ref().map(|t| t.to_string());
    KeyEvent {
        code,
        text,
        pressed: key_ev.state == ElementState::Pressed,
        repeat: key_ev.repeat,
        modifiers,
    }
}
