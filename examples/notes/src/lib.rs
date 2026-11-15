//! CraftUI Notes: a small notes app that runs on Android, the Web and desktop
//! from one codebase.
//!
//! The crate is split so that each part can be tested on its own:
//!
//! * [`model`] holds the notes and every rule about them. It knows nothing about the UI.
//! * [`storage`] saves one string: a file on desktop and Android, `localStorage` on the Web.
//! * `ui/notes.cui` describes the screen. `build.rs` compiles it to Rust ([`aot_ui`]).
//! * [`NotesController`] (this file) connects the three.
//!
//! Each platform has a tiny entry point that picks a storage backend and calls
//! [`mount_notes_app`]: `src/main.rs` for desktop, [`start`] for the Web and
//! `android_main` for Android.

pub mod model;
pub mod storage;

/// Rust generated from `ui/notes.cui` at build time.
pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/notes.rs"));
}

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use craftui::prelude::*;

use crate::aot_ui::{NoteCard, NotesScreen};
use crate::model::{NoteFilter, Notebook, format_age, truncate_chars};
use crate::storage::NoteStorage;

/// Windows at least this many dp wide show the list and the editor side by side.
pub const WIDE_LAYOUT_MIN_WIDTH: f32 = 720.0;

/// Longest title shown on a card in the list.
const CARD_TITLE_CHARS: usize = 34;
/// Longest body preview shown on a card in the list.
const CARD_PREVIEW_CHARS: usize = 44;

/// Source of the current time, in milliseconds since the Unix epoch.
/// Injected so tests can control the clock.
pub type Clock = Rc<dyn Fn() -> u64>;

/// What the app needs from the platform it runs on.
#[derive(Clone)]
pub struct NotesConfig {
    pub storage: Rc<dyn NoteStorage>,
    pub clock: Clock,
}

impl NotesConfig {
    /// Use `storage` together with the real wall clock.
    pub fn new(storage: impl NoteStorage + 'static) -> Self {
        Self {
            storage: Rc::new(storage),
            clock: Rc::new(wall_clock_ms),
        }
    }

    /// Replace the clock (tests).
    pub fn with_clock(mut self, clock: impl Fn() -> u64 + 'static) -> Self {
        self.clock = Rc::new(clock);
        self
    }
}

/// Milliseconds since the Unix epoch.
#[cfg(not(target_arch = "wasm32"))]
pub fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch. `std::time` is not available in the
/// browser, so the JavaScript clock is used instead.
#[cfg(target_arch = "wasm32")]
pub fn wall_clock_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// Set a signal only when the value differs, without subscribing the caller.
/// Keeps effects from waking each other up for no reason.
fn set_quiet<T: Clone + PartialEq + 'static>(signal: Signal<T>, value: T) {
    if signal.get_untracked() != value {
        signal.set(value);
    }
}

/// Connects the generated screen to the notebook and its storage.
///
/// Cloning is cheap: all clones share the same state.
#[derive(Clone)]
pub struct NotesController {
    /// The generated screen. Its public fields are the signals the UI reads.
    pub screen: NotesScreen,
    book: Rc<RefCell<Notebook>>,
    storage: Rc<dyn NoteStorage>,
    clock: Clock,
    /// Id of the note open in the editor.
    selected: Rc<Cell<Option<u64>>>,
    /// Ids of the cards currently in the list, in display order.
    visible_ids: Rc<RefCell<Vec<u64>>>,
    /// Bumped after every change to the notebook or the selection so the list refreshes.
    revision: Signal<u64>,
    /// False when the saved data could not be read. Nothing is written then,
    /// so the unreadable data is never overwritten.
    can_save: Rc<Cell<bool>>,
    theme: Signal<ThemeTokens>,
}

impl NotesController {
    // ----- Read access (used by tests and by the entry points) -------------

    /// Number of notes in the notebook.
    pub fn note_count(&self) -> usize {
        self.book.borrow().len()
    }

    /// Id of the note open in the editor, if any.
    pub fn selected_id(&self) -> Option<u64> {
        self.selected.get()
    }

    /// A copy of the notebook as it is right now.
    pub fn snapshot(&self) -> Notebook {
        self.book.borrow().clone()
    }

    /// Whether the dark theme is active.
    pub fn is_dark(&self) -> bool {
        self.book.borrow().dark_mode()
    }

    // ----- Actions ----------------------------------------------------------

    /// Create an empty note and open it in the editor.
    pub fn new_note(&self) -> u64 {
        self.discard_blank_selection();
        let now = (self.clock)();
        let id = self.book.borrow_mut().create("", "", now);
        self.select(Some(id));
        id
    }

    /// Open the card at `index` of the visible list.
    pub fn open_visible(&self, index: usize) {
        let id = self.visible_ids.borrow().get(index).copied();
        if let Some(id) = id {
            self.open_note(id);
        }
    }

    /// Open a note by id. Unknown ids are ignored.
    pub fn open_note(&self, id: u64) {
        if self.selected.get() == Some(id) || self.book.borrow().get(id).is_none() {
            return;
        }
        self.discard_blank_selection();
        self.select(Some(id));
    }

    /// Close the editor and go back to the list.
    pub fn close_editor(&self) {
        self.discard_blank_selection();
        self.select(None);
    }

    /// Pin or unpin the open note.
    pub fn toggle_pin(&self) {
        let Some(id) = self.selected.get() else {
            return;
        };
        let pinned = self.book.borrow_mut().toggle_pinned(id);
        if let Some(pinned) = pinned {
            self.screen.pin_label.set(pin_label(pinned).to_string());
            self.persist();
            self.bump();
        }
    }

    /// Delete the open note and close the editor.
    pub fn delete_selected(&self) {
        let Some(id) = self.selected.get() else {
            return;
        };
        let removed = self.book.borrow_mut().delete(id);
        if removed.is_some() {
            self.persist();
        }
        self.select(None);
    }

    /// Switch between the light and the dark theme and remember the choice.
    pub fn toggle_theme(&self) {
        let dark = !self.is_dark();
        self.book.borrow_mut().set_dark_mode(dark);
        self.apply_theme(dark);
        self.persist();
    }

    // ----- Internals --------------------------------------------------------

    fn apply_theme(&self, dark: bool) {
        let mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        self.theme.set(ThemeTokens::from_mode(mode));
        // The button shows what a tap will switch to.
        let icon = if dark { "sun" } else { "moon" };
        self.screen.theme_icon.set(icon.to_string());
    }

    /// Point the editor at `id` (or at nothing) and load its text into the fields.
    fn select(&self, id: Option<u64>) {
        // While the fields are being filled, no note is selected, so the draft
        // effect cannot write a half-loaded title or body into any note.
        self.selected.set(None);

        let loaded = id.and_then(|id| self.book.borrow().get(id).cloned());
        let now = (self.clock)();
        match &loaded {
            Some(note) => {
                self.screen.draft_title.set(note.title.clone());
                self.screen.draft_body.set(note.body.clone());
                self.screen
                    .pin_label
                    .set(pin_label(note.pinned).to_string());
                let status = if note.is_blank() {
                    "New note".to_string()
                } else {
                    format!("Edited {}", format_age(now, note.updated_ms))
                };
                self.screen.editor_status.set(status);
            }
            None => {
                self.screen.draft_title.set(String::new());
                self.screen.draft_body.set(String::new());
                self.screen.editor_status.set(String::new());
            }
        }

        self.selected.set(loaded.as_ref().map(|note| note.id));
        self.screen.confirm_delete.set(false);
        set_quiet(self.screen.has_selection, loaded.is_some());
        self.bump();
    }

    /// Called whenever the title or body field changes.
    fn apply_draft(&self, title: &str, body: &str) {
        let Some(id) = self.selected.get() else {
            return;
        };
        let now = (self.clock)();
        let changed = self.book.borrow_mut().update(id, title, body, now);
        if changed {
            let saved = self.persist();
            let status = if saved { "Saved" } else { "Not saved" };
            self.screen.editor_status.set(status.to_string());
            self.bump();
        }
    }

    /// A note the user opened but never wrote in is removed instead of kept as clutter.
    fn discard_blank_selection(&self) {
        let Some(id) = self.selected.get() else {
            return;
        };
        let blank = self
            .book
            .borrow()
            .get(id)
            .is_some_and(|note| note.is_blank());
        if blank {
            self.book.borrow_mut().delete(id);
            self.persist();
        }
    }

    /// Write the notebook to storage. Returns `true` when it was saved.
    fn persist(&self) -> bool {
        if !self.can_save.get() {
            return false;
        }
        let json = self.book.borrow().to_json();
        match self.storage.save(&json) {
            Ok(()) => {
                self.show_storage_error(None);
                true
            }
            Err(reason) => {
                self.show_storage_error(Some(format!("Could not save your notes: {reason}")));
                false
            }
        }
    }

    fn show_storage_error(&self, message: Option<String>) {
        set_quiet(self.screen.has_storage_error, message.is_some());
        set_quiet(self.screen.storage_error, message.unwrap_or_default());
    }

    fn bump(&self) {
        self.revision.set(self.revision.get_untracked() + 1);
    }

    /// Rebuild the cards shown in the list for the given filter and search text.
    fn refresh_list(&self, filter: NoteFilter, query: &str) {
        let now = (self.clock)();
        let selected = self.selected.get();
        let book = self.book.borrow();
        let visible = book.visible(filter, query);

        let ids: Vec<u64> = visible.iter().map(|note| note.id).collect();
        let cards: Vec<NoteCard> = visible
            .iter()
            .map(|note| {
                let age = format_age(now, note.updated_ms);
                NoteCard {
                    title: truncate_chars(&note.display_title(), CARD_TITLE_CHARS),
                    preview: note.preview(CARD_PREVIEW_CHARS),
                    meta: if note.pinned {
                        format!("Pinned, edited {age}")
                    } else {
                        format!("Edited {age}")
                    },
                    selected: selected == Some(note.id),
                }
            })
            .collect();

        let total = book.len();
        let searching = !query.trim().is_empty();
        let summary = if searching || filter != NoteFilter::All {
            format!("{} of {}", cards.len(), count_label(total))
        } else {
            match book.pinned_count() {
                0 => count_label(total),
                pinned => format!("{}, {pinned} pinned", count_label(total)),
            }
        };
        let (empty_title, empty_hint) = if total == 0 {
            ("No notes yet", "Tap + to write your first note.")
        } else if searching {
            (
                "Nothing found",
                "Try a different word, or clear the search.",
            )
        } else {
            (
                "No pinned notes",
                "Open a note and tap Pin to keep it here.",
            )
        };
        drop(book);

        *self.visible_ids.borrow_mut() = ids;
        self.screen.notes.set(cards);
        set_quiet(self.screen.summary_text, summary);
        set_quiet(self.screen.empty_title, empty_title.to_string());
        set_quiet(self.screen.empty_hint, empty_hint.to_string());
    }
}

fn pin_label(pinned: bool) -> &'static str {
    if pinned { "Unpin" } else { "Pin" }
}

fn count_label(count: usize) -> String {
    if count == 1 {
        "1 note".to_string()
    } else {
        format!("{count} notes")
    }
}

/// Build the notes screen inside `tree` and return its root node together with
/// the controller that drives it.
pub fn mount_notes_app(
    cx: Scope,
    tree: &mut WidgetTree,
    config: NotesConfig,
) -> (NodeId, NotesController) {
    let now = (config.clock)();

    // Load what was saved. A first run starts with a welcome note. Data that
    // cannot be read is left untouched and saving is switched off.
    let (book, load_error) = match config.storage.load() {
        // Nothing saved yet.
        Ok(None) => (Notebook::with_welcome_note(now), None),
        Ok(Some(text)) => match Notebook::from_json(&text) {
            Ok(book) => (book, None),
            Err(err) => (Notebook::new(), Some(err.to_string())),
        },
        // Saved data that cannot be read. Start empty and keep saving off, so
        // the unreadable file is left as it is.
        Err(err) => (Notebook::new(), Some(err)),
    };

    let screen = NotesScreen::new(cx);
    let controller = NotesController {
        screen: screen.clone(),
        book: Rc::new(RefCell::new(book)),
        storage: config.storage,
        clock: config.clock,
        selected: Rc::new(Cell::new(None)),
        visible_ids: Rc::new(RefCell::new(Vec::new())),
        revision: cx.signal(0u64),
        can_save: Rc::new(Cell::new(load_error.is_none())),
        theme: tree.theme_signal,
    };

    if let Some(reason) = load_error {
        controller.show_storage_error(Some(format!(
            "Your saved notes could not be read, so nothing will be saved this session. ({reason})"
        )));
    }
    controller.apply_theme(controller.is_dark());

    // Layout: wide windows show both panes, narrow ones show one at a time.
    let metrics = tree.metrics_signal;
    let wide = screen.wide_layout;
    cx.effect(move || {
        let width = metrics.get().logical_size.width;
        set_quiet(wide, width >= WIDE_LAYOUT_MIN_WIDTH);
    });

    let panes = screen.clone();
    cx.effect(move || {
        let wide = panes.wide_layout.get();
        let editing = panes.has_selection.get();
        set_quiet(panes.show_list, wide || !editing);
        set_quiet(panes.show_editor, wide || editing);
        set_quiet(panes.show_back, !wide && editing);
    });

    // List: refresh when the notebook, the filter tab or the search text changes.
    let list = controller.clone();
    cx.effect(move || {
        list.revision.get();
        let filter = NoteFilter::from_tab(list.screen.filter_tab.get());
        let query = list.screen.search_query.get();
        list.refresh_list(filter, &query);
    });

    // Editor: every keystroke updates the open note and saves it.
    let editor = controller.clone();
    cx.effect(move || {
        let title = editor.screen.draft_title.get();
        let body = editor.screen.draft_body.get();
        editor.apply_draft(&title, &body);
    });

    let c = controller.clone();
    screen.set_on_new_note(move || {
        c.new_note();
    });
    let c = controller.clone();
    screen.set_on_open_note(move |index| {
        if index >= 0 {
            c.open_visible(index as usize);
        }
    });
    let c = controller.clone();
    screen.set_on_close_editor(move || c.close_editor());
    let c = controller.clone();
    screen.set_on_toggle_pin(move || c.toggle_pin());
    let c = controller.clone();
    screen.set_on_delete_note(move || c.delete_selected());
    let c = controller.clone();
    screen.set_on_toggle_theme(move || c.toggle_theme());

    let root = controller.screen.build(cx, tree);
    (root, controller)
}

// ---------------------------------------------------------------------------
// Web entry point
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Key under which the notebook is kept in the browser's `localStorage`.
#[cfg(target_arch = "wasm32")]
pub const WEB_STORAGE_KEY: &str = "craftui-notes/v1";

/// Called by the browser as soon as the WebAssembly module is loaded.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    craftui_platform_web::start_web_app(
        craftui_platform_web::WebBridgeConfig::default(),
        |cx, tree, _textures, _dispatcher| {
            let config = NotesConfig::new(storage::LocalStorage::new(WEB_STORAGE_KEY));
            mount_notes_app(cx, tree, config).0
        },
    )
}

// ---------------------------------------------------------------------------
// Android entry point
// ---------------------------------------------------------------------------

/// Called by `GameActivity` on the app's native thread.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: craftui::platform_android::android_activity::AndroidApp) {
    // The app's private files directory. It needs no permission and is removed
    // when the app is uninstalled.
    let data_dir = app.internal_data_path().unwrap_or_else(std::env::temp_dir);
    let notes_file = data_dir.join("notes.json");

    craftui::platform_android::android_runtime::run_android_app(
        app,
        move |cx, tree, _textures, _dispatcher| {
            let config = NotesConfig::new(storage::FileStorage::new(notes_file.clone()));
            mount_notes_app(cx, tree, config).0
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use craftui::reactive::reset_runtime;

    const PHONE: (f32, f32) = (390.0, 800.0);
    const DESKTOP: (f32, f32) = (1100.0, 720.0);
    const T0: u64 = 1_700_000_000_000;

    struct Harness {
        tree: WidgetTree,
        ctrl: NotesController,
        storage: MemoryStorage,
        clock: Rc<Cell<u64>>,
        metrics: WindowMetrics,
    }

    impl Harness {
        fn start(storage: MemoryStorage, size: (f32, f32)) -> Self {
            Self::mount(storage.clone(), storage, size)
        }

        /// Mount with any backend. `inspect` is the cell the test reads saved
        /// text from, which is the same object for the usual `MemoryStorage`
        /// case and an empty cell for the backends that cannot save.
        fn mount(
            backend: impl NoteStorage + 'static,
            inspect: MemoryStorage,
            size: (f32, f32),
        ) -> Self {
            reset_runtime();
            let cx = Scope::root();
            let mut tree = WidgetTree::new(cx);
            let clock = Rc::new(Cell::new(T0));
            let clock_handle = clock.clone();
            let config = NotesConfig::new(backend).with_clock(move || clock_handle.get());
            let (root, ctrl) = mount_notes_app(cx, &mut tree, config);
            tree.set_root(root);
            let mut harness = Self {
                tree,
                ctrl,
                storage: inspect,
                clock,
                metrics: WindowMetrics::default(),
            };
            harness.resize(size);
            harness
        }

        fn resize(&mut self, size: (f32, f32)) {
            self.metrics = WindowMetrics {
                logical_size: Size::new(size.0, size.1),
                ..Default::default()
            };
            self.layout();
        }

        fn layout(&mut self) {
            // Two passes: the first applies pending changes (which can mount
            // or unmount whole panes), the second positions the result.
            self.tree.layout(self.metrics);
            self.tree.layout(self.metrics);
        }

        fn advance(&self, ms: u64) {
            self.clock.set(self.clock.get() + ms);
        }

        fn saved(&self) -> Notebook {
            let text = self.storage.contents().expect("nothing was saved");
            Notebook::from_json(&text).expect("saved notebook is valid")
        }

        /// Whether the node with this `id` is on screen. An `if` without `else`
        /// keeps its element mounted and hides it, so being mounted is not enough.
        fn has_node(&self, id: &str) -> bool {
            self.tree
                .node_by_id(id)
                .is_some_and(|node| self.tree.is_visible(node))
        }

        fn tap(&mut self, id: &str) {
            let node = self
                .tree
                .node_by_id(id)
                .unwrap_or_else(|| panic!("no node with id `{id}` is mounted"));
            let rect = self.tree.rect(node);
            assert!(
                rect.width > 0.0 && rect.height > 0.0,
                "node `{id}` has no size: {rect:?}"
            );
            let position = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            for (phase, timestamp_ms) in [(PointerPhase::Down, 10), (PointerPhase::Up, 40)] {
                self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase,
                    position,
                    button: Some(PointerButton::Primary),
                    modifiers: Modifiers::default(),
                    is_touch: false,
                    timestamp_ms,
                }));
            }
            self.layout();
        }

        fn render(&mut self) -> Vec<u8> {
            let mut scene = PaintScene::new();
            self.tree.paint(&mut scene);
            let textures = TextureRegistry::new();
            render_scene_to_rgba(
                BackendKind::VelloCpu,
                &scene,
                self.metrics.logical_size.width as u32,
                self.metrics.logical_size.height as u32,
                1.0,
                self.tree.theme().background,
                &textures,
                &mut self.tree.text_engine,
            )
        }

        fn titles(&self) -> Vec<String> {
            self.ctrl
                .screen
                .notes
                .get()
                .into_iter()
                .map(|card| card.title)
                .collect()
        }
    }

    /// Storage whose writes always fail, like a full disk.
    #[derive(Clone, Default)]
    struct BrokenStorage;

    impl NoteStorage for BrokenStorage {
        fn load(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn save(&self, _text: &str) -> Result<(), String> {
            Err("disk full".to_string())
        }
    }

    /// Storage with saved data that cannot be read back: bad UTF-8, or a file
    /// the process is not allowed to open. `save` panics, because nothing may
    /// overwrite data that is still there.
    #[derive(Clone, Default)]
    struct UnreadableStorage;

    impl NoteStorage for UnreadableStorage {
        fn load(&self) -> Result<Option<String>, String> {
            Err("cannot read notes.json: stream did not contain valid UTF-8".to_string())
        }

        fn save(&self, _text: &str) -> Result<(), String> {
            panic!("a session that could not read the notebook must not save over it")
        }
    }

    fn seeded() -> MemoryStorage {
        let mut book = Notebook::new();
        book.create("Groceries", "milk, eggs, bread", T0 - 3 * 3_600_000);
        book.create("Trip ideas", "Jaipur in winter", T0 - 2 * 3_600_000);
        book.create("Plumber", "call tomorrow at 10", T0 - 60_000);
        book.set_pinned(1, true);
        MemoryStorage::with_contents(book.to_json())
    }

    #[test]
    fn first_run_shows_a_pinned_welcome_note_without_saving() {
        let h = Harness::start(MemoryStorage::new(), PHONE);
        assert_eq!(h.ctrl.note_count(), 1);
        assert_eq!(h.titles(), vec!["Welcome to CraftUI Notes"]);
        assert_eq!(h.ctrl.screen.summary_text.get(), "1 note, 1 pinned");
        assert!(h.ctrl.screen.notes.get()[0].meta.starts_with("Pinned"));
        // Nothing is written until the user changes something.
        assert_eq!(h.storage.save_count(), 0);
    }

    #[test]
    fn saved_notes_are_listed_pinned_first_then_newest() {
        let h = Harness::start(seeded(), PHONE);
        assert_eq!(h.titles(), vec!["Groceries", "Plumber", "Trip ideas"]);
        assert_eq!(h.ctrl.screen.summary_text.get(), "3 notes, 1 pinned");
        let cards = h.ctrl.screen.notes.get();
        assert_eq!(cards[0].meta, "Pinned, edited 3 h ago");
        assert_eq!(cards[1].meta, "Edited 1 min ago");
        assert_eq!(cards[1].preview, "call tomorrow at 10");
    }

    #[test]
    fn typing_in_the_editor_updates_the_note_and_saves_it() {
        let mut h = Harness::start(seeded(), PHONE);
        let id = h.ctrl.new_note();
        assert_eq!(h.ctrl.selected_id(), Some(id));
        assert_eq!(h.ctrl.screen.editor_status.get(), "New note");

        h.advance(5_000);
        h.ctrl.screen.draft_title.set("Ideas".to_string());
        h.ctrl.screen.draft_body.set("Write the docs".to_string());
        h.layout();

        assert_eq!(h.ctrl.screen.editor_status.get(), "Saved");
        let saved = h.saved();
        let note = saved.get(id).expect("the new note was saved");
        assert_eq!(note.title, "Ideas");
        assert_eq!(note.body, "Write the docs");
        assert_eq!(note.updated_ms, T0 + 5_000);
        // The freshly edited note moves to the top of the unpinned notes.
        assert_eq!(
            h.titles(),
            vec!["Groceries", "Ideas", "Plumber", "Trip ideas"]
        );
    }

    #[test]
    fn opening_another_note_never_mixes_up_their_text() {
        let h = Harness::start(seeded(), DESKTOP);
        h.ctrl.open_note(2);
        assert_eq!(h.ctrl.screen.draft_title.get(), "Trip ideas");
        h.ctrl
            .screen
            .draft_body
            .set("Jaipur in winter, Goa in March".to_string());

        h.ctrl.open_note(3);
        assert_eq!(h.ctrl.screen.draft_title.get(), "Plumber");
        assert_eq!(h.ctrl.screen.draft_body.get(), "call tomorrow at 10");

        let book = h.ctrl.snapshot();
        assert_eq!(book.get(2).unwrap().body, "Jaipur in winter, Goa in March");
        assert_eq!(book.get(2).unwrap().title, "Trip ideas");
        assert_eq!(book.get(3).unwrap().body, "call tomorrow at 10");
        assert_eq!(book.get(1).unwrap().body, "milk, eggs, bread");
        // Just looking at a note is not an edit.
        assert_eq!(book.get(3).unwrap().updated_ms, T0 - 60_000);
    }

    #[test]
    fn a_note_left_empty_is_dropped_when_the_editor_closes() {
        let h = Harness::start(seeded(), PHONE);
        h.ctrl.new_note();
        assert_eq!(h.ctrl.note_count(), 4);
        h.ctrl.close_editor();
        assert_eq!(h.ctrl.note_count(), 3);
        assert_eq!(h.ctrl.selected_id(), None);
        assert_eq!(h.saved().len(), 3);

        // Opening a different note discards the blank one too.
        h.ctrl.new_note();
        h.ctrl.open_note(1);
        assert_eq!(h.ctrl.note_count(), 3);
        assert_eq!(h.ctrl.selected_id(), Some(1));
    }

    #[test]
    fn search_and_pinned_filter_narrow_the_list() {
        let h = Harness::start(seeded(), PHONE);
        h.ctrl.screen.search_query.set("JAIPUR".to_string());
        assert_eq!(h.titles(), vec!["Trip ideas"]);
        assert_eq!(h.ctrl.screen.summary_text.get(), "1 of 3 notes");

        h.ctrl.screen.search_query.set("zzz".to_string());
        assert!(h.titles().is_empty());
        assert_eq!(h.ctrl.screen.empty_title.get(), "Nothing found");

        h.ctrl.screen.search_query.set(String::new());
        h.ctrl.screen.filter_tab.set(1);
        assert_eq!(h.titles(), vec!["Groceries"]);
        assert_eq!(h.ctrl.screen.summary_text.get(), "1 of 3 notes");
    }

    #[test]
    fn open_visible_follows_the_filtered_order() {
        let h = Harness::start(seeded(), PHONE);
        h.ctrl.screen.search_query.set("tomorrow".to_string());
        h.ctrl.open_visible(0);
        assert_eq!(h.ctrl.selected_id(), Some(3));
        // Out-of-range taps are ignored.
        h.ctrl.open_visible(9);
        assert_eq!(h.ctrl.selected_id(), Some(3));
    }

    #[test]
    fn pin_and_delete_change_the_notebook_and_are_saved() {
        let h = Harness::start(seeded(), PHONE);
        h.ctrl.open_note(3);
        assert_eq!(h.ctrl.screen.pin_label.get(), "Pin");
        h.ctrl.toggle_pin();
        assert_eq!(h.ctrl.screen.pin_label.get(), "Unpin");
        assert!(h.saved().get(3).unwrap().pinned);
        assert_eq!(h.titles(), vec!["Plumber", "Groceries", "Trip ideas"]);

        h.ctrl.screen.confirm_delete.set(true);
        h.ctrl.delete_selected();
        assert_eq!(h.ctrl.selected_id(), None);
        assert!(!h.ctrl.screen.confirm_delete.get());
        assert!(h.saved().get(3).is_none());
        assert_eq!(h.titles(), vec!["Groceries", "Trip ideas"]);

        // With the "Pinned" tab active and nothing pinned, the empty state explains why.
        h.ctrl.open_note(1);
        h.ctrl.toggle_pin();
        h.ctrl.screen.filter_tab.set(1);
        assert_eq!(h.ctrl.screen.empty_title.get(), "No pinned notes");
    }

    #[test]
    fn notes_survive_a_restart() {
        let storage = seeded();
        {
            let h = Harness::start(storage.clone(), PHONE);
            let id = h.ctrl.new_note();
            h.ctrl.screen.draft_title.set("Remember me".to_string());
            assert_eq!(h.ctrl.selected_id(), Some(id));
        }
        let h = Harness::start(storage, PHONE);
        assert_eq!(h.ctrl.note_count(), 4);
        assert!(h.titles().contains(&"Remember me".to_string()));
    }

    #[test]
    fn theme_choice_is_applied_and_remembered() {
        let storage = seeded();
        {
            let h = Harness::start(storage.clone(), PHONE);
            assert_eq!(h.tree.theme().mode, ThemeMode::Light);
            assert_eq!(h.ctrl.screen.theme_icon.get(), "moon");
            h.ctrl.toggle_theme();
            assert_eq!(h.tree.theme().mode, ThemeMode::Dark);
            assert_eq!(h.ctrl.screen.theme_icon.get(), "sun");
        }
        let h = Harness::start(storage, PHONE);
        assert!(h.ctrl.is_dark());
        assert_eq!(h.tree.theme().mode, ThemeMode::Dark);
    }

    #[test]
    fn unreadable_data_is_reported_and_never_overwritten() {
        let storage = MemoryStorage::with_contents("{ this is not a notebook");
        let h = Harness::start(storage.clone(), PHONE);
        assert!(h.ctrl.screen.has_storage_error.get());
        assert!(
            h.ctrl
                .screen
                .storage_error
                .get()
                .contains("could not be read")
        );

        h.ctrl.new_note();
        h.ctrl.screen.draft_title.set("Typed anyway".to_string());
        assert_eq!(h.ctrl.screen.editor_status.get(), "Not saved");
        assert_eq!(h.storage.save_count(), 0);
        assert_eq!(
            storage.contents().as_deref(),
            Some("{ this is not a notebook")
        );
    }

    #[test]
    fn a_read_error_is_reported_and_never_overwritten() {
        let h = Harness::mount(UnreadableStorage, MemoryStorage::new(), PHONE);
        assert_eq!(h.ctrl.note_count(), 0);
        assert!(h.ctrl.screen.has_storage_error.get());
        assert!(
            h.ctrl
                .screen
                .storage_error
                .get()
                .contains("could not be read")
        );

        h.ctrl.new_note();
        h.ctrl.screen.draft_title.set("Typed anyway".to_string());
        assert_eq!(h.ctrl.screen.editor_status.get(), "Not saved");
        // `UnreadableStorage::save` panics, so a write attempt fails this test.
    }

    #[test]
    fn a_failed_save_is_shown_to_the_user() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let config = NotesConfig::new(BrokenStorage).with_clock(|| T0);
        let (root, ctrl) = mount_notes_app(cx, &mut tree, config);
        tree.set_root(root);

        assert!(!ctrl.screen.has_storage_error.get());
        ctrl.new_note();
        ctrl.screen.draft_body.set("important".to_string());
        assert!(ctrl.screen.has_storage_error.get());
        assert_eq!(
            ctrl.screen.storage_error.get(),
            "Could not save your notes: disk full"
        );
        assert_eq!(ctrl.screen.editor_status.get(), "Not saved");
        // The text is still in memory, so the user can keep working.
        assert_eq!(ctrl.snapshot().notes().last().unwrap().body, "important");
    }

    #[test]
    fn phone_layout_shows_one_pane_at_a_time() {
        let mut h = Harness::start(seeded(), PHONE);
        assert!(!h.ctrl.screen.wide_layout.get());
        assert!(h.has_node("list_pane"));
        assert!(!h.has_node("editor_pane"));

        h.ctrl.open_note(2);
        h.layout();
        assert!(!h.has_node("list_pane"));
        assert!(h.has_node("editor_pane"));
        assert!(h.has_node("back_button"));
        assert!(h.has_node("title_input"));

        h.ctrl.close_editor();
        h.layout();
        assert!(h.has_node("list_pane"));
        assert!(!h.has_node("editor_pane"));
    }

    #[test]
    fn wide_layout_shows_list_and_editor_side_by_side() {
        let mut h = Harness::start(seeded(), DESKTOP);
        assert!(h.ctrl.screen.wide_layout.get());
        assert!(h.has_node("list_pane"));
        assert!(h.has_node("editor_pane"));
        assert!(h.has_node("editor_placeholder"));

        h.ctrl.open_note(2);
        h.layout();
        assert!(h.has_node("list_pane"));
        assert!(h.has_node("title_input"));
        assert!(!h.has_node("back_button"));
        assert!(!h.has_node("editor_placeholder"));

        let list = h.tree.rect(h.tree.node_by_id("list_pane").unwrap());
        let editor = h.tree.rect(h.tree.node_by_id("editor_pane").unwrap());
        assert!((list.width - 340.0).abs() < 1.0, "list pane: {list:?}");
        assert!(editor.x >= list.x + list.width, "editor pane: {editor:?}");
        assert!(editor.width > 600.0, "editor pane: {editor:?}");

        // The open note is highlighted in the list.
        let cards = h.ctrl.screen.notes.get();
        let highlighted: Vec<&str> = cards
            .iter()
            .filter(|card| card.selected)
            .map(|card| card.title.as_str())
            .collect();
        assert_eq!(highlighted, vec!["Trip ideas"]);
    }

    #[test]
    fn resizing_switches_between_phone_and_wide_layouts() {
        let mut h = Harness::start(seeded(), DESKTOP);
        h.ctrl.open_note(1);
        h.layout();
        assert!(h.has_node("list_pane") && h.has_node("editor_pane"));

        h.resize(PHONE);
        assert!(!h.ctrl.screen.wide_layout.get());
        assert!(!h.has_node("list_pane"));
        assert!(h.has_node("editor_pane"));
        // The open note stays open across the change.
        assert_eq!(h.ctrl.selected_id(), Some(1));

        h.resize(DESKTOP);
        assert!(h.has_node("list_pane") && h.has_node("editor_pane"));
    }

    #[test]
    fn tapping_the_buttons_drives_the_whole_flow() {
        let mut h = Harness::start(seeded(), PHONE);

        h.tap("new_note_button");
        assert_eq!(h.ctrl.note_count(), 4);
        assert!(h.has_node("editor_pane"));
        h.ctrl.screen.draft_title.set("From a tap".to_string());
        h.layout();

        h.tap("pin_button");
        assert_eq!(h.ctrl.screen.pin_label.get(), "Unpin");

        // Delete asks first, and Cancel keeps the note.
        h.tap("delete_button");
        assert!(h.ctrl.screen.confirm_delete.get());
        assert!(h.has_node("delete_confirm_bar"));
        h.tap("cancel_delete_button");
        assert!(!h.ctrl.screen.confirm_delete.get());
        assert_eq!(h.ctrl.note_count(), 4);

        h.tap("delete_button");
        h.tap("confirm_delete_button");
        assert_eq!(h.ctrl.note_count(), 3);
        assert!(h.has_node("list_pane"));
        assert!(!h.titles().contains(&"From a tap".to_string()));

        h.tap("theme_button");
        assert!(h.ctrl.is_dark());
    }

    #[test]
    fn back_button_returns_to_the_list_on_a_phone() {
        let mut h = Harness::start(seeded(), PHONE);
        h.ctrl.open_note(2);
        h.layout();
        h.tap("back_button");
        assert_eq!(h.ctrl.selected_id(), None);
        assert!(h.has_node("list_pane"));
    }

    #[test]
    fn the_note_body_fills_the_rest_of_the_editor() {
        for size in [PHONE, DESKTOP] {
            let mut h = Harness::start(seeded(), size);
            h.ctrl.open_note(2);
            h.layout();

            let title = h.tree.rect(h.tree.node_by_id("title_input").unwrap());
            let body = h.tree.rect(h.tree.node_by_id("body_input").unwrap());
            let bottom = body.y + body.height;
            assert!(body.y > title.y + title.height - 1.0, "body: {body:?}");
            // 16dp of padding is left under the text area.
            assert!(
                (bottom - (size.1 - 16.0)).abs() < 2.0,
                "the body should reach the bottom of a {size:?} window: {body:?}"
            );
        }
    }

    #[test]
    fn a_long_list_scrolls_inside_the_window() {
        let mut book = Notebook::new();
        for n in 0..60 {
            book.create(&format!("Note {n}"), "some text", T0 - n * 60_000);
        }
        let mut h = Harness::start(MemoryStorage::with_contents(book.to_json()), PHONE);
        h.layout();

        // The list is taller than the phone, so it has to stay inside the
        // window and scroll rather than push its end off the screen.
        let list = h.tree.rect(h.tree.node_by_id("note_list").unwrap());
        assert!(list.height > 200.0, "list: {list:?}");
        assert!(
            list.y + list.height <= PHONE.1 + 0.5,
            "the list must end inside the window: {list:?}"
        );
        assert_eq!(h.titles().len(), 60);

        // The header, search box and tabs above the list keep their size.
        let mut short = Harness::start(seeded(), PHONE);
        short.layout();
        let short_list = short.tree.rect(short.tree.node_by_id("note_list").unwrap());
        assert_eq!(
            list.y, short_list.y,
            "the list must start where it always does"
        );
        assert_eq!(list.height, short_list.height);
    }

    #[test]
    fn text_fields_are_named_for_screen_readers() {
        let mut h = Harness::start(seeded(), DESKTOP);
        h.ctrl.open_note(2);
        h.layout();
        for (id, name) in [
            ("search_input", "Search notes"),
            ("title_input", "Title"),
            ("body_input", "Start writing..."),
        ] {
            let node = h.tree.node_by_id(id).unwrap_or_else(|| panic!("no `{id}`"));
            let label = h.tree.nodes.get(node).and_then(|n| n.a11y.label.clone());
            assert_eq!(label.as_deref(), Some(name), "accessible name of `{id}`");
        }
    }

    /// Renders the main screens. When `CRAFTUI_SNAPSHOT_DIR` is set (CI does
    /// this) the frames are also written there as PNG files for review.
    #[test]
    fn screenshots_of_every_screen() {
        let out_dir = std::env::var_os("CRAFTUI_SNAPSHOT_DIR").map(std::path::PathBuf::from);
        let shoot = |h: &mut Harness, name: &str| {
            h.layout();
            let rgba = h.render();
            let width = h.metrics.logical_size.width as u32;
            let height = h.metrics.logical_size.height as u32;
            assert_eq!(rgba.len(), (width * height * 4) as usize);
            if let Some(dir) = &out_dir {
                craftui::render::write_rgba_png(dir.join(name), width, height, &rgba)
                    .expect("could not write screenshot");
            }
        };

        let mut phone = Harness::start(seeded(), PHONE);
        shoot(&mut phone, "notes-phone-1-list.png");
        phone.ctrl.open_note(2);
        shoot(&mut phone, "notes-phone-2-editor.png");
        phone.ctrl.screen.confirm_delete.set(true);
        shoot(&mut phone, "notes-phone-3-confirm-delete.png");
        phone.ctrl.close_editor();
        phone.ctrl.screen.search_query.set("zzz".to_string());
        shoot(&mut phone, "notes-phone-4-empty-search.png");

        let mut desktop = Harness::start(seeded(), DESKTOP);
        shoot(&mut desktop, "notes-desktop-1-placeholder.png");
        desktop.ctrl.open_note(2);
        shoot(&mut desktop, "notes-desktop-2-editor.png");
        desktop.ctrl.toggle_theme();
        shoot(&mut desktop, "notes-desktop-3-dark.png");
    }

    #[test]
    fn the_screen_renders_and_changes_with_the_content() {
        let mut h = Harness::start(seeded(), PHONE);
        let list = h.render();
        assert_eq!(list.len(), 390 * 800 * 4);
        // Not a blank frame: more than one colour was drawn.
        let first = &list[0..4];
        assert!(list.as_chunks::<4>().0.iter().any(|pixel| pixel != first));

        h.ctrl.open_note(2);
        h.layout();
        let editor = h.render();
        assert_ne!(list, editor);

        h.ctrl.toggle_theme();
        h.layout();
        let dark = h.render();
        assert_ne!(editor, dark);
    }
}
