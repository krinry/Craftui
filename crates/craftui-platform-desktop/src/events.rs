use craftui_core::{CursorIcon, KeyCode, KeyEvent, Modifiers, PointerButton};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::CursorIcon as WinitCursorIcon;

/// Custom event loop messages delivered from background worker threads, AccessKit, `.cui` watcher, or window opening.
#[derive(Debug)]
pub enum DesktopUserEvent {
    WakeUi,
    AccessKit(accesskit_winit::Event),
    #[cfg(feature = "hot-reload")]
    FileChanged,
    OpenWindow(u64),
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
        PhysicalKey::Code(WinitKeyCode::KeyI) => KeyCode::KeyI,
        PhysicalKey::Code(WinitKeyCode::KeyA) => KeyCode::KeyA,
        PhysicalKey::Code(WinitKeyCode::KeyC) => KeyCode::KeyC,
        PhysicalKey::Code(WinitKeyCode::KeyV) => KeyCode::KeyV,
        PhysicalKey::Code(WinitKeyCode::KeyX) => KeyCode::KeyX,
        PhysicalKey::Code(WinitKeyCode::KeyY) => KeyCode::KeyY,
        PhysicalKey::Code(WinitKeyCode::KeyZ) => KeyCode::KeyZ,
        _ => match &key_ev.logical_key {
            Key::Named(NamedKey::Enter) => KeyCode::Enter,
            Key::Named(NamedKey::Tab) => KeyCode::Tab,
            Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
            Key::Named(NamedKey::Delete) => KeyCode::Delete,
            Key::Named(NamedKey::Escape) => KeyCode::Escape,
            Key::Named(NamedKey::Space) => KeyCode::Space,
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
