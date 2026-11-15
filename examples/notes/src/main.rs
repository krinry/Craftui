//! Desktop entry point. Android and the Web start from `src/lib.rs`.

use craftui::prelude::*;
use craftui_example_notes::storage::{FileStorage, default_data_file};
use craftui_example_notes::{NotesConfig, mount_notes_app};

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Notes".to_string(),
            width: 1000.0,
            height: 700.0,
            ..Default::default()
        },
        |cx, tree, _app_context| {
            let config = NotesConfig::new(FileStorage::new(default_data_file()));
            mount_notes_app(cx, tree, config).0
        },
    )
}
