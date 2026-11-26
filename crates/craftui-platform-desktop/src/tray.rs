use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use craftui_core::{CraftError, KeyCode, KeyEvent, Modifiers, Result};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use crate::events::DesktopUserEvent;

/// Menu item displayed in a desktop OS system tray context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    pub is_separator: bool,
}

impl TrayMenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            enabled: true,
            checked: false,
            is_separator: false,
        }
    }

    pub fn separator() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            enabled: false,
            checked: false,
            is_separator: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

/// Global keyboard shortcut binding for desktop applications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShortcut {
    pub id: String,
    pub modifiers: Modifiers,
    pub key: KeyCode,
    pub label: String,
}

impl GlobalShortcut {
    pub fn new(
        id: impl Into<String>,
        modifiers: Modifiers,
        key: KeyCode,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            modifiers,
            key,
            label: label.into(),
        }
    }

    /// Returns true if a key press matches this shortcut's modifiers and key (including repeats).
    pub fn matches_chord(&self, ev: &KeyEvent) -> bool {
        ev.pressed
            && ev.code == self.key
            && ev.modifiers.ctrl == self.modifiers.ctrl
            && ev.modifiers.alt == self.modifiers.alt
            && ev.modifiers.shift == self.modifiers.shift
            && ev.modifiers.super_key == self.modifiers.super_key
    }

    /// Returns true if a non-repeat key press matches this shortcut's modifiers and key.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        !ev.repeat && self.matches_chord(ev)
    }
}

type TrayActionCb = Rc<RefCell<Box<dyn FnMut()>>>;

struct ShortcutEntry {
    spec: GlobalShortcut,
    callback: TrayActionCb,
}

#[derive(Default)]
struct TrayState {
    tooltip: String,
    visible: bool,
    menu: Vec<TrayMenuItem>,
    menu_callbacks: HashMap<String, TrayActionCb>,
    shortcuts: Vec<ShortcutEntry>,
    hidden_windows: HashSet<WindowId>,
}

/// Cloneable handle managing the desktop system tray menu, window show/hide toggles, and global shortcuts.
#[derive(Clone, Default)]
pub struct SystemTrayHandle {
    state: Rc<RefCell<TrayState>>,
    proxy: Option<EventLoopProxy<DesktopUserEvent>>,
}

impl SystemTrayHandle {
    pub(crate) fn with_proxy(proxy: EventLoopProxy<DesktopUserEvent>) -> Self {
        Self {
            state: Rc::new(RefCell::new(TrayState::default())),
            proxy: Some(proxy),
        }
    }

    /// Create an in-memory handle for headless tests without a `winit` event loop.
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn set_tooltip(&self, tooltip: impl Into<String>) {
        self.state.borrow_mut().tooltip = tooltip.into();
    }

    pub fn tooltip(&self) -> String {
        self.state.borrow().tooltip.clone()
    }

    pub fn set_visible(&self, visible: bool) {
        self.state.borrow_mut().visible = visible;
    }

    pub fn is_visible(&self) -> bool {
        self.state.borrow().visible
    }

    pub fn set_menu(&self, items: Vec<TrayMenuItem>) {
        self.state.borrow_mut().menu = items;
    }

    pub fn menu(&self) -> Vec<TrayMenuItem> {
        self.state.borrow().menu.clone()
    }

    pub fn on_menu_select(&self, item_id: impl Into<String>, cb: impl FnMut() + 'static) {
        self.state
            .borrow_mut()
            .menu_callbacks
            .insert(item_id.into(), Rc::new(RefCell::new(Box::new(cb))));
    }

    /// Trigger a tray menu item callback by ID (returns false if disabled, separator, or unbound).
    pub fn trigger_menu_item(&self, item_id: &str) -> bool {
        let enabled = self
            .state
            .borrow()
            .menu
            .iter()
            .find(|m| m.id == item_id)
            .is_none_or(|m| m.enabled && !m.is_separator);
        if !enabled {
            return false;
        }
        let cb = self.state.borrow().menu_callbacks.get(item_id).cloned();
        if let Some(cb) = cb {
            (cb.borrow_mut())();
            true
        } else {
            false
        }
    }

    pub fn show_window(&self, id: WindowId) -> Result<()> {
        self.set_window_visible(id, true)
    }

    pub fn hide_window(&self, id: WindowId) -> Result<()> {
        self.set_window_visible(id, false)
    }

    pub fn toggle_window(&self, id: WindowId) -> Result<()> {
        let next_visible = self.state.borrow().hidden_windows.contains(&id);
        self.set_window_visible(id, next_visible)
    }

    pub fn is_window_hidden(&self, id: WindowId) -> bool {
        self.state.borrow().hidden_windows.contains(&id)
    }

    fn set_window_visible(&self, id: WindowId, visible: bool) -> Result<()> {
        {
            let mut st = self.state.borrow_mut();
            if visible {
                st.hidden_windows.remove(&id);
            } else {
                st.hidden_windows.insert(id);
            }
        }
        if let Some(ref proxy) = self.proxy {
            proxy
                .send_event(DesktopUserEvent::SetWindowVisible(id, visible))
                .map_err(|e| {
                    CraftError::Platform(format!("failed queuing SetWindowVisible: {e}"))
                })?;
        }
        Ok(())
    }

    pub fn register_shortcut(&self, shortcut: GlobalShortcut, cb: impl FnMut() + 'static) {
        let mut st = self.state.borrow_mut();
        st.shortcuts.retain(|s| s.spec.id != shortcut.id);
        st.shortcuts.push(ShortcutEntry {
            spec: shortcut,
            callback: Rc::new(RefCell::new(Box::new(cb))),
        });
    }

    pub fn unregister_shortcut(&self, id: &str) -> bool {
        let mut st = self.state.borrow_mut();
        let before = st.shortcuts.len();
        st.shortcuts.retain(|s| s.spec.id != id);
        st.shortcuts.len() != before
    }

    pub fn shortcuts(&self) -> Vec<GlobalShortcut> {
        self.state
            .borrow()
            .shortcuts
            .iter()
            .map(|s| s.spec.clone())
            .collect()
    }

    /// Match a keyboard event against registered shortcuts; consumes matching repeats without re-invoking.
    pub fn dispatch_key_event(&self, ev: &KeyEvent) -> bool {
        let matched = self
            .state
            .borrow()
            .shortcuts
            .iter()
            .find(|entry| entry.spec.matches_chord(ev))
            .map(|entry| entry.callback.clone());
        let Some(cb) = matched else {
            return false;
        };
        if !ev.repeat {
            (cb.borrow_mut())();
        }
        true
    }
}
