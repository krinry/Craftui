//! Pure data model for the Notes app.
//!
//! Nothing in this module depends on the UI framework, so every rule (sorting,
//! searching, persistence format) is covered by plain unit tests.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Version written into every saved notebook. Bump it when the JSON layout
/// changes in a way older builds cannot read.
pub const SCHEMA_VERSION: u32 = 1;

/// Title shown for a note that has neither a title nor any body text.
pub const UNTITLED: &str = "Untitled note";

/// One note. `created_ms` and `updated_ms` are milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub updated_ms: u64,
}

impl Note {
    /// The title to show in lists: the explicit title, else the first
    /// non-empty line of the body, else [`UNTITLED`].
    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        if !title.is_empty() {
            return title.to_string();
        }
        self.body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| truncate_chars(line, 60))
            .unwrap_or_else(|| UNTITLED.to_string())
    }

    /// A one-line summary of the body with whitespace collapsed, at most
    /// `max_chars` characters long.
    pub fn preview(&self, max_chars: usize) -> String {
        let collapsed = self.body.split_whitespace().collect::<Vec<_>>().join(" ");
        truncate_chars(&collapsed, max_chars)
    }

    /// True when the note has no title and no body text.
    pub fn is_blank(&self) -> bool {
        self.title.trim().is_empty() && self.body.trim().is_empty()
    }

    /// Plain-text form used for sharing and copying.
    pub fn to_share_text(&self) -> String {
        let title = self.display_title();
        let body = self.body.trim();
        if body.is_empty() || self.title.trim().is_empty() {
            // Either there is no body, or the title was derived from it.
            if body.is_empty() {
                title
            } else {
                body.to_string()
            }
        } else {
            format!("{title}\n\n{body}")
        }
    }

    fn matches(&self, terms: &[String]) -> bool {
        if terms.is_empty() {
            return true;
        }
        let haystack = format!("{}\n{}", self.title, self.body).to_lowercase();
        terms.iter().all(|term| haystack.contains(term.as_str()))
    }
}

/// Which notes the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteFilter {
    #[default]
    All,
    Pinned,
}

impl NoteFilter {
    /// Map the index of the filter tab to a filter. Unknown indexes show everything.
    pub fn from_tab(index: i32) -> Self {
        match index {
            1 => Self::Pinned,
            _ => Self::All,
        }
    }
}

/// Why a saved notebook could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotebookError {
    /// The text is not valid notebook JSON.
    Parse(String),
    /// The notebook was written by a newer version of the app.
    UnsupportedVersion(u32),
}

impl fmt::Display for NotebookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "saved notes are not valid JSON: {msg}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "saved notes use format version {v}, but this build only understands up to {SCHEMA_VERSION}"
            ),
        }
    }
}

impl std::error::Error for NotebookError {}

/// Everything the app saves: the notes, the counter used to hand out ids, and
/// the one user preference (light or dark theme).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notebook {
    version: u32,
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    dark_mode: bool,
    #[serde(default)]
    notes: Vec<Note>,
}

impl Default for Notebook {
    fn default() -> Self {
        Self::new()
    }
}

impl Notebook {
    /// An empty notebook.
    pub fn new() -> Self {
        Self {
            version: SCHEMA_VERSION,
            next_id: 1,
            dark_mode: false,
            notes: Vec::new(),
        }
    }

    /// Whether the user chose the dark theme.
    pub fn dark_mode(&self) -> bool {
        self.dark_mode
    }

    /// Remember the theme choice. Returns `true` when it changed.
    pub fn set_dark_mode(&mut self, dark: bool) -> bool {
        let changed = self.dark_mode != dark;
        self.dark_mode = dark;
        changed
    }

    /// The notebook a first-time user sees: one pinned welcome note.
    pub fn with_welcome_note(now_ms: u64) -> Self {
        let mut book = Self::new();
        let id = book.create(
            "Welcome to CraftUI Notes",
            "Tap + to write a new note.\n\nNotes save automatically while you type. Pin the ones you want to keep at the top, and use the search box to find anything by title or text.",
            now_ms,
        );
        book.set_pinned(id, true);
        book
    }

    /// All notes in insertion order.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Number of pinned notes.
    pub fn pinned_count(&self) -> usize {
        self.notes.iter().filter(|n| n.pinned).count()
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Add a note and return its id.
    pub fn create(&mut self, title: &str, body: &str, now_ms: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.notes.push(Note {
            id,
            title: title.to_string(),
            body: body.to_string(),
            pinned: false,
            created_ms: now_ms,
            updated_ms: now_ms,
        });
        id
    }

    /// Replace the text of a note. Returns `true` only when something changed,
    /// so callers can skip saving and the "last edited" time stays honest.
    pub fn update(&mut self, id: u64, title: &str, body: &str, now_ms: u64) -> bool {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        if note.title == title && note.body == body {
            return false;
        }
        note.title = title.to_string();
        note.body = body.to_string();
        note.updated_ms = now_ms;
        true
    }

    /// Pin or unpin a note. Returns `true` when the pin state changed.
    /// Pinning does not count as an edit, so `updated_ms` is left alone.
    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        match self.notes.iter_mut().find(|n| n.id == id) {
            Some(note) if note.pinned != pinned => {
                note.pinned = pinned;
                true
            }
            _ => false,
        }
    }

    /// Flip the pin state and return the new value, or `None` for an unknown id.
    pub fn toggle_pinned(&mut self, id: u64) -> Option<bool> {
        let note = self.notes.iter_mut().find(|n| n.id == id)?;
        note.pinned = !note.pinned;
        Some(note.pinned)
    }

    /// Remove a note and return it.
    pub fn delete(&mut self, id: u64) -> Option<Note> {
        let index = self.notes.iter().position(|n| n.id == id)?;
        Some(self.notes.remove(index))
    }

    /// Notes to show for a filter and search query: pinned first, then most
    /// recently edited first. The query is case-insensitive and every word in
    /// it must appear in the title or the body.
    pub fn visible(&self, filter: NoteFilter, query: &str) -> Vec<&Note> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut out: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| match filter {
                NoteFilter::All => true,
                NoteFilter::Pinned => n.pinned,
            })
            .filter(|n| n.matches(&terms))
            .collect();
        out.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.updated_ms.cmp(&a.updated_ms))
                .then(b.id.cmp(&a.id))
        });
        out
    }

    /// Serialise for storage.
    pub fn to_json(&self) -> String {
        // Serialising plain strings, bools and integers cannot fail.
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// Load a notebook saved by [`Notebook::to_json`].
    ///
    /// The result is repaired where that is safe: duplicate ids are dropped
    /// (first one wins) and the id counter is moved past every existing id so
    /// new notes can never collide with old ones.
    pub fn from_json(text: &str) -> Result<Self, NotebookError> {
        let mut book: Notebook =
            serde_json::from_str(text).map_err(|e| NotebookError::Parse(e.to_string()))?;
        if book.version > SCHEMA_VERSION {
            return Err(NotebookError::UnsupportedVersion(book.version));
        }
        book.version = SCHEMA_VERSION;

        let mut seen = std::collections::HashSet::new();
        book.notes.retain(|n| seen.insert(n.id));

        let max_id = book.notes.iter().map(|n| n.id).max().unwrap_or(0);
        book.next_id = book.next_id.max(max_id + 1).max(1);
        Ok(book)
    }
}

/// Describe how long ago `then_ms` was, relative to `now_ms`.
pub fn format_age(now_ms: u64, then_ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let elapsed = now_ms.saturating_sub(then_ms);
    if elapsed < MINUTE {
        "just now".to_string()
    } else if elapsed < HOUR {
        format!("{} min ago", elapsed / MINUTE)
    } else if elapsed < DAY {
        format!("{} h ago", elapsed / HOUR)
    } else if elapsed < 30 * DAY {
        format!("{} d ago", elapsed / DAY)
    } else if elapsed < 365 * DAY {
        format!("{} mo ago", elapsed / (30 * DAY))
    } else {
        format!("{} y ago", elapsed / (365 * DAY))
    }
}

/// Cut `text` to at most `max_chars` characters, ending with an ellipsis when
/// something was removed. Works on characters, never splitting a code point.
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Notebook {
        let mut book = Notebook::new();
        book.create("Groceries", "milk, eggs, bread", 1_000);
        book.create("Trip ideas", "Jaipur in winter\nGoa in March", 2_000);
        book.create("", "call the plumber tomorrow", 3_000);
        book
    }

    #[test]
    fn create_assigns_increasing_ids() {
        let book = sample();
        let ids: Vec<u64> = book.notes().iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(book.len(), 3);
        assert!(!book.is_empty());
    }

    #[test]
    fn ids_are_not_reused_after_delete() {
        let mut book = sample();
        let removed = book.delete(3).expect("note 3 exists");
        assert_eq!(removed.body, "call the plumber tomorrow");
        assert_eq!(book.create("New", "", 4_000), 4);
        assert!(book.delete(99).is_none());
    }

    #[test]
    fn update_reports_real_changes_only() {
        let mut book = sample();
        assert!(!book.update(1, "Groceries", "milk, eggs, bread", 9_000));
        assert_eq!(book.get(1).unwrap().updated_ms, 1_000);

        assert!(book.update(1, "Groceries", "milk, eggs, bread, tea", 9_000));
        let note = book.get(1).unwrap();
        assert_eq!(note.body, "milk, eggs, bread, tea");
        assert_eq!(note.updated_ms, 9_000);
        assert_eq!(note.created_ms, 1_000);

        assert!(!book.update(42, "x", "y", 9_000));
    }

    #[test]
    fn pinning_does_not_change_edit_time() {
        let mut book = sample();
        assert!(book.set_pinned(1, true));
        assert!(!book.set_pinned(1, true));
        assert_eq!(book.get(1).unwrap().updated_ms, 1_000);
        assert_eq!(book.pinned_count(), 1);
        assert_eq!(book.toggle_pinned(1), Some(false));
        assert_eq!(book.toggle_pinned(404), None);
        assert_eq!(book.pinned_count(), 0);
    }

    #[test]
    fn visible_puts_pinned_first_then_newest() {
        let mut book = sample();
        book.set_pinned(1, true);
        let order: Vec<u64> = book
            .visible(NoteFilter::All, "")
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(order, vec![1, 3, 2]);

        let pinned: Vec<u64> = book
            .visible(NoteFilter::Pinned, "")
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(pinned, vec![1]);
    }

    #[test]
    fn equal_timestamps_fall_back_to_newest_id() {
        let mut book = Notebook::new();
        book.create("a", "", 5);
        book.create("b", "", 5);
        let order: Vec<u64> = book
            .visible(NoteFilter::All, "")
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(order, vec![2, 1]);
    }

    #[test]
    fn search_is_case_insensitive_and_needs_every_word() {
        let book = sample();
        let hits = |q: &str| -> Vec<u64> {
            book.visible(NoteFilter::All, q)
                .iter()
                .map(|n| n.id)
                .collect()
        };
        assert_eq!(hits("GOA"), vec![2]);
        assert_eq!(hits("trip winter"), vec![2]);
        assert_eq!(hits("trip plumber"), Vec::<u64>::new());
        assert_eq!(hits("   "), vec![3, 2, 1]);
        assert_eq!(hits("eggs"), vec![1]);
    }

    #[test]
    fn display_title_falls_back_to_body_then_placeholder() {
        let book = sample();
        assert_eq!(book.get(1).unwrap().display_title(), "Groceries");
        assert_eq!(
            book.get(3).unwrap().display_title(),
            "call the plumber tomorrow"
        );
        let blank = Note {
            id: 9,
            title: "  ".into(),
            body: "\n\n".into(),
            pinned: false,
            created_ms: 0,
            updated_ms: 0,
        };
        assert_eq!(blank.display_title(), UNTITLED);
        assert!(blank.is_blank());
    }

    #[test]
    fn preview_collapses_whitespace_and_truncates() {
        let book = sample();
        assert_eq!(
            book.get(2).unwrap().preview(100),
            "Jaipur in winter Goa in March"
        );
        assert_eq!(book.get(2).unwrap().preview(7), "Jaipur…");
    }

    #[test]
    fn truncate_respects_multibyte_characters() {
        assert_eq!(truncate_chars("नमस्ते दुनिया", 4), "नमस…");
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("abc", 0), "");
        assert_eq!(truncate_chars("abc", 3), "abc");
    }

    #[test]
    fn share_text_does_not_repeat_a_derived_title() {
        let book = sample();
        assert_eq!(
            book.get(1).unwrap().to_share_text(),
            "Groceries\n\nmilk, eggs, bread"
        );
        assert_eq!(
            book.get(3).unwrap().to_share_text(),
            "call the plumber tomorrow"
        );
    }

    #[test]
    fn theme_choice_is_saved_and_defaults_to_light() {
        let mut book = Notebook::new();
        assert!(!book.dark_mode());
        assert!(book.set_dark_mode(true));
        assert!(!book.set_dark_mode(true));
        let restored = Notebook::from_json(&book.to_json()).expect("round trip");
        assert!(restored.dark_mode());

        let old_file = r#"{"version":1,"next_id":1,"notes":[]}"#;
        assert!(!Notebook::from_json(old_file).unwrap().dark_mode());
    }

    #[test]
    fn json_round_trip_preserves_everything() {
        let mut book = sample();
        book.set_pinned(2, true);
        book.set_dark_mode(true);
        let restored = Notebook::from_json(&book.to_json()).expect("round trip");
        assert_eq!(restored, book);
    }

    #[test]
    fn loading_repairs_id_counter_and_duplicates() {
        let text = r#"{"version":1,"next_id":1,"notes":[
            {"id":7,"title":"first","body":""},
            {"id":7,"title":"duplicate","body":""},
            {"id":3,"title":"other","body":"x","pinned":true}
        ]}"#;
        let mut book = Notebook::from_json(text).expect("loads");
        assert_eq!(book.len(), 2);
        assert_eq!(book.get(7).unwrap().title, "first");
        assert!(book.get(3).unwrap().pinned);
        assert_eq!(book.create("new", "", 0), 8);
    }

    #[test]
    fn loading_rejects_garbage_and_future_versions() {
        assert!(matches!(
            Notebook::from_json("not json"),
            Err(NotebookError::Parse(_))
        ));
        let future = r#"{"version":99,"next_id":1,"notes":[]}"#;
        assert_eq!(
            Notebook::from_json(future),
            Err(NotebookError::UnsupportedVersion(99))
        );
        let message = NotebookError::UnsupportedVersion(99).to_string();
        assert!(message.contains("99"));
    }

    #[test]
    fn welcome_notebook_has_one_pinned_note() {
        let book = Notebook::with_welcome_note(123);
        assert_eq!(book.len(), 1);
        assert_eq!(book.pinned_count(), 1);
        assert_eq!(book.notes()[0].created_ms, 123);
    }

    #[test]
    fn filter_tabs_map_to_filters() {
        assert_eq!(NoteFilter::from_tab(0), NoteFilter::All);
        assert_eq!(NoteFilter::from_tab(1), NoteFilter::Pinned);
        assert_eq!(NoteFilter::from_tab(7), NoteFilter::All);
    }

    #[test]
    fn ages_read_naturally() {
        let min = 60_000;
        let hour = 60 * min;
        let day = 24 * hour;
        assert_eq!(format_age(10_000, 9_000), "just now");
        assert_eq!(format_age(5 * min, 0), "5 min ago");
        assert_eq!(format_age(3 * hour, 0), "3 h ago");
        assert_eq!(format_age(2 * day, 0), "2 d ago");
        assert_eq!(format_age(65 * day, 0), "2 mo ago");
        assert_eq!(format_age(800 * day, 0), "2 y ago");
        // A clock that went backwards must not underflow.
        assert_eq!(format_age(0, 5 * day), "just now");
    }
}
