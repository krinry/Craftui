use craftui_a11y::A11yRole;
use craftui_core::{
    ClipboardBackend, CursorIcon, ImeEvent, KeyCode, KeyEvent, Modifiers, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, Rect, ScrollDelta, UiEvent,
};
use std::sync::{Arc, Mutex};

/// Map a CraftUI [`CursorIcon`] hint to the standard CSS `cursor` keyword.
pub fn craft_cursor_to_css(cursor: CursorIcon) -> &'static str {
    match cursor {
        CursorIcon::Default => "default",
        CursorIcon::Pointer => "pointer",
        CursorIcon::Text => "text",
        CursorIcon::Grab => "grab",
        CursorIcon::Grabbing => "grabbing",
        CursorIcon::ResizeHorizontal => "ew-resize",
        CursorIcon::ResizeVertical => "ns-resize",
        CursorIcon::NotAllowed => "not-allowed",
        CursorIcon::Crosshair => "crosshair",
    }
}

/// Map a CraftUI semantic [`A11yRole`] to a WAI-ARIA `role` attribute string.
pub fn craft_role_to_aria(role: A11yRole) -> &'static str {
    match role {
        A11yRole::Window => "application",
        A11yRole::Container => "group",
        A11yRole::Label => "note",
        A11yRole::Button => "button",
        A11yRole::TextInput | A11yRole::MultilineTextInput => "textbox",
        A11yRole::CheckBox => "checkbox",
        A11yRole::RadioButton => "radio",
        A11yRole::Switch => "switch",
        A11yRole::Slider => "slider",
        A11yRole::ComboBox => "combobox",
        A11yRole::TabList => "tablist",
        A11yRole::Tab => "tab",
        A11yRole::Menu => "menu",
        A11yRole::MenuItem => "menuitem",
        A11yRole::Tooltip => "tooltip",
        A11yRole::Divider => "separator",
        A11yRole::ProgressIndicator => "progressbar",
        A11yRole::Image => "img",
        A11yRole::ScrollView => "region",
        A11yRole::Canvas => "img",
        A11yRole::Dialog => "dialog",
        A11yRole::Alert => "alert",
        A11yRole::List => "list",
        A11yRole::ListItem => "listitem",
    }
}

/// Snapshot node used to synchronize an invisible DOM ARIA mirror under `#craftui-a11y-root`.
#[derive(Debug, Clone, PartialEq)]
pub struct WebAriaNode {
    pub id: u64,
    pub role: &'static str,
    pub raw_role: A11yRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub numeric_value: Option<f64>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub focusable: bool,
    pub focused: bool,
    pub rect: Rect,
}

/// Browser clipboard bridge backed by `navigator.clipboard` + trusted DOM `paste` events (on `wasm32`)
/// and a synchronous shared cache.
#[derive(Clone, Default)]
pub struct WebClipboard {
    cache: Arc<Mutex<String>>,
}

impl WebClipboard {
    pub fn new(cache: Arc<Mutex<String>>) -> Self {
        Self { cache }
    }

    /// Update the synchronous clipboard cache (e.g. from a DOM `ClipboardEvent` or async `readText()`).
    pub fn hydrate(&self, text: &str) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
    }

    /// Populate clipboard cache asynchronously via `navigator.clipboard.readText()` on `wasm32`.
    #[cfg(target_arch = "wasm32")]
    pub fn read_browser_clipboard_async<F>(cache: Arc<Mutex<String>>, on_ready: F)
    where
        F: FnOnce(String) + 'static,
    {
        if let Some(window) = web_sys::window() {
            let promise = window.navigator().clipboard().read_text();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(val) = wasm_bindgen_futures::JsFuture::from(promise).await
                    && let Some(text) = val.as_string()
                {
                    if let Ok(mut guard) = cache.lock() {
                        guard.clear();
                        guard.push_str(&text);
                    }
                    on_ready(text);
                }
            });
        }
    }
}

impl ClipboardBackend for WebClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.cache.lock().ok().map(|g| g.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(text);
        }
        true
    }
}

/// Convert browser `KeyboardEvent.code` + `KeyboardEvent.key` into a CraftUI [`KeyEvent`].
/// Physical scancodes (`KeyF`, `BracketLeft`, `Digit1`, etc.) take priority over layout-mapped `key`.
pub fn dom_keyboard_event_to_craft(
    dom_code: &str,
    key: &str,
    pressed: bool,
    repeat: bool,
    modifiers: Modifiers,
) -> KeyEvent {
    let code = if let Some(phys) = KeyCode::from_dom_code(dom_code) {
        phys
    } else {
        return dom_key_to_craft(key, pressed, repeat, modifiers);
    };
    let text = if key.chars().count() == 1 {
        Some(key.to_string())
    } else {
        None
    };
    KeyEvent {
        code,
        text,
        pressed,
        repeat,
        modifiers,
    }
}

/// Convert browser `KeyboardEvent.key` string into a CraftUI [`KeyEvent`].
pub fn dom_key_to_craft(key: &str, pressed: bool, repeat: bool, modifiers: Modifiers) -> KeyEvent {
    let cmd = modifiers.command();
    let code = match key {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Escape" => KeyCode::Escape,
        " " => KeyCode::Space,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "i" | "I" if cmd && modifiers.shift => KeyCode::KeyI,
        "a" | "A" if cmd => KeyCode::KeyA,
        "c" | "C" if cmd => KeyCode::KeyC,
        "v" | "V" if cmd => KeyCode::KeyV,
        "x" | "X" if cmd => KeyCode::KeyX,
        "y" | "Y" if modifiers.ctrl => KeyCode::KeyY,
        "z" | "Z" if cmd => KeyCode::KeyZ,
        s if s.chars().count() == 1 => KeyCode::Character(s.to_string()),
        _ => KeyCode::Unknown,
    };
    let text = if key.chars().count() == 1 {
        Some(key.to_string())
    } else {
        None
    };
    KeyEvent {
        code,
        text,
        pressed,
        repeat,
        modifiers,
    }
}

/// Convert mobile/virtual-keyboard DOM `InputEvent` updates on `<input>` into CraftUI key/IME events.
pub fn dom_input_to_events(
    prev: &str,
    current: &str,
    input_type: &str,
    data: Option<&str>,
) -> Vec<UiEvent> {
    let mut events = Vec::new();
    match input_type {
        "deleteContentBackward" | "deleteWordBackward" | "deleteSoftLineBackward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Backspace,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "deleteContentForward" | "deleteWordForward" | "deleteSoftLineForward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Delete,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "insertLineBreak" | "insertParagraph" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                text: Some("\n".to_string()),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        _ => {}
    }

    if let Some(text) = data
        && !text.is_empty()
        && matches!(
            input_type,
            "insertText" | "insertFromPaste" | "insertCompositionText" | ""
        )
    {
        events.push(UiEvent::Ime(ImeEvent::Commit(text.to_string())));
        return events;
    }

    if prev == current {
        return events;
    }

    // Fallback diff between `prev` and `current` for Android Chrome Gboard batch mutations
    let prev_chars: Vec<char> = prev.chars().collect();
    let curr_chars: Vec<char> = current.chars().collect();
    let common_prefix = prev_chars
        .iter()
        .zip(curr_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = prev_chars.len().saturating_sub(common_prefix);
    for _ in 0..removed {
        events.push(UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
    }
    if curr_chars.len() > common_prefix {
        let inserted: String = curr_chars[common_prefix..].iter().collect();
        events.push(UiEvent::Ime(ImeEvent::Commit(inserted)));
    }
    events
}

pub fn dom_button_to_craft(button: i16) -> Option<PointerButton> {
    match button {
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Secondary),
        3 => Some(PointerButton::Back),
        4 => Some(PointerButton::Forward),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn dom_pointer_event(
    pointer_id: i32,
    phase: PointerPhase,
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    scale_x: f32,
    scale_y: f32,
    button: i16,
    is_touch: bool,
    modifiers: Modifiers,
    timestamp_ms: u64,
) -> UiEvent {
    let sx = if scale_x > 0.0 { scale_x } else { 1.0 };
    let sy = if scale_y > 0.0 { scale_y } else { 1.0 };
    UiEvent::Pointer(PointerEvent {
        id: PointerId(pointer_id as u64),
        phase,
        position: Point::new((client_x - rect_left) * sx, (client_y - rect_top) * sy),
        button: dom_button_to_craft(button),
        modifiers,
        is_touch,
        timestamp_ms,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn dom_wheel_event(
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    scale_x: f32,
    scale_y: f32,
    delta_x: f32,
    delta_y: f32,
) -> UiEvent {
    let sx = if scale_x > 0.0 { scale_x } else { 1.0 };
    let sy = if scale_y > 0.0 { scale_y } else { 1.0 };
    UiEvent::Scroll(ScrollDelta {
        x: -delta_x,
        y: -delta_y,
        position: Point::new((client_x - rect_left) * sx, (client_y - rect_top) * sy),
    })
}

pub fn dom_composition_to_ime(phase_end: bool, data: String) -> UiEvent {
    if phase_end {
        UiEvent::Ime(ImeEvent::Commit(data))
    } else {
        let len = data.len();
        UiEvent::Ime(ImeEvent::Preedit(data, Some((len, len))))
    }
}
