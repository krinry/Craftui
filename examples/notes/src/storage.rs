//! Where the notebook JSON is kept between runs.
//!
//! * Desktop and Android: a file, written atomically.
//! * Web: the browser's `localStorage`.
//! * Tests: an in-memory cell.

use std::cell::RefCell;
use std::rc::Rc;

/// A place to keep one string between runs of the app.
pub trait NoteStorage {
    /// The saved text.
    ///
    /// `Ok(None)` means nothing has been saved yet, so this is a first run.
    /// `Err` means something *is* saved but cannot be read: bad UTF-8, a
    /// permission problem, or a browser that refuses access. The app must not
    /// mistake that for a first run, because the next save would replace data
    /// the user still has.
    fn load(&self) -> Result<Option<String>, String>;
    /// Replace the saved text.
    fn save(&self, text: &str) -> Result<(), String>;
}

/// Storage that lives only as long as the process. Clones share the same cell,
/// which lets a test keep a handle and inspect what the app saved.
#[derive(Debug, Clone, Default)]
pub struct MemoryStorage {
    cell: Rc<RefCell<Option<String>>>,
    saves: Rc<RefCell<usize>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start with `text` already saved.
    pub fn with_contents(text: impl Into<String>) -> Self {
        let storage = Self::default();
        *storage.cell.borrow_mut() = Some(text.into());
        storage
    }

    /// What is currently saved.
    pub fn contents(&self) -> Option<String> {
        self.cell.borrow().clone()
    }

    /// How many times `save` has been called.
    pub fn save_count(&self) -> usize {
        *self.saves.borrow()
    }
}

impl NoteStorage for MemoryStorage {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.cell.borrow().clone())
    }

    fn save(&self, text: &str) -> Result<(), String> {
        *self.cell.borrow_mut() = Some(text.to_string());
        *self.saves.borrow_mut() += 1;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::{FileStorage, default_data_file};

#[cfg(not(target_arch = "wasm32"))]
mod file {
    use super::NoteStorage;
    use std::path::{Path, PathBuf};

    /// Stores the notebook in a single file.
    #[derive(Debug, Clone)]
    pub struct FileStorage {
        path: PathBuf,
    }

    impl FileStorage {
        pub fn new(path: impl Into<PathBuf>) -> Self {
            Self { path: path.into() }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl NoteStorage for FileStorage {
        fn load(&self) -> Result<Option<String>, String> {
            match std::fs::read_to_string(&self.path) {
                Ok(text) => Ok(Some(text)),
                // Nothing saved yet: a normal first run.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                // The file is there but unreadable. Reporting `None` here would
                // start an empty notebook and overwrite it on the next edit.
                Err(err) => Err(format!("cannot read {}: {err}", self.path.display())),
            }
        }

        /// Write to a temporary file next to the target and rename it into
        /// place, so a crash mid-write cannot leave a half-written notebook.
        fn save(&self, text: &str) -> Result<(), String> {
            if let Some(dir) = self.path.parent()
                && !dir.as_os_str().is_empty()
            {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            }
            let mut tmp = self.path.clone().into_os_string();
            tmp.push(".tmp");
            let tmp = PathBuf::from(tmp);
            std::fs::write(&tmp, text)
                .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
            std::fs::rename(&tmp, &self.path)
                .map_err(|e| format!("cannot replace {}: {e}", self.path.display()))
        }
    }

    /// The notes file for a desktop build, following each platform's convention
    /// for per-user application data.
    pub fn default_data_file() -> PathBuf {
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
        };
        base.unwrap_or_else(std::env::temp_dir)
            .join("craftui-notes")
            .join("notes.json")
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::LocalStorage;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::NoteStorage;

    /// Stores the notebook under one key in the browser's `localStorage`.
    #[derive(Debug, Clone)]
    pub struct LocalStorage {
        key: String,
    }

    impl LocalStorage {
        pub fn new(key: impl Into<String>) -> Self {
            Self { key: key.into() }
        }

        fn storage() -> Option<web_sys::Storage> {
            web_sys::window()?.local_storage().ok().flatten()
        }
    }

    impl NoteStorage for LocalStorage {
        fn load(&self) -> Result<Option<String>, String> {
            let storage = Self::storage()
                .ok_or_else(|| "localStorage is not available in this browser".to_string())?;
            storage
                .get_item(&self.key)
                .map_err(|_| "the browser refused to read the stored notes".to_string())
        }

        fn save(&self, text: &str) -> Result<(), String> {
            let storage = Self::storage()
                .ok_or_else(|| "localStorage is not available in this browser".to_string())?;
            storage
                .set_item(&self.key, text)
                .map_err(|_| "the browser refused to store the notes (storage full?)".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_storage_shares_state_between_clones() {
        let storage = MemoryStorage::new();
        let handle = storage.clone();
        assert_eq!(storage.load(), Ok(None));
        storage.save("hello").unwrap();
        assert_eq!(handle.contents().as_deref(), Some("hello"));
        assert_eq!(handle.save_count(), 1);
        let loaded = MemoryStorage::with_contents("x").load().unwrap();
        assert_eq!(loaded.as_deref(), Some("x"));
    }

    #[test]
    fn file_storage_round_trips_and_creates_directories() {
        let dir = std::env::temp_dir().join(format!(
            "craftui-notes-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let storage = FileStorage::new(dir.join("nested").join("notes.json"));
        // A missing file is a first run, not a failure.
        assert_eq!(storage.load(), Ok(None));

        storage.save("first").unwrap();
        assert_eq!(storage.load().unwrap().as_deref(), Some("first"));
        storage.save("second").unwrap();
        assert_eq!(storage.load().unwrap().as_deref(), Some("second"));

        // The temporary file must not be left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.join("nested"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("notes.json")]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_file_is_an_error_not_a_first_run() {
        let dir = std::env::temp_dir().join(format!(
            "craftui-notes-test-unreadable-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.json");
        // Invalid UTF-8: the file exists, so the notes must not be replaced by
        // a fresh notebook on the next save.
        std::fs::write(&path, [0x7b, 0xff, 0xfe, 0x7d]).unwrap();

        let storage = FileStorage::new(&path);
        let loaded = storage.load();
        assert!(loaded.is_err(), "invalid UTF-8 must be an error");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_data_file_ends_with_app_folder() {
        let path = default_data_file();
        assert!(path.ends_with("craftui-notes/notes.json"));
    }
}
