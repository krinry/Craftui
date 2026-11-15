# Building the Notes app

`examples/notes` is a complete, tested app that runs on Android, in the browser
and on the desktop. This guide walks through how it is built, so you can use the
same structure for your own app.

The finished code is short: about 340 lines of model, 170 of storage, 370 of
`.cui` and 520 of controller and entry points, plus tests.

## The structure

```text
examples/notes/
├── Cargo.toml       one library (used by Android and the Web) plus a desktop binary
├── craftui.toml     app name, id, Android SDK levels
├── build.rs         compiles ui/*.cui to Rust
├── ui/notes.cui     the screen
└── src/
    ├── model.rs     notes and their rules
    ├── storage.rs   saving and loading one string
    ├── lib.rs       controller, Web and Android entry points, UI tests
    └── main.rs      desktop entry point
```

Three decisions shape everything else:

1. **The model knows nothing about the UI.** `Notebook` is ordinary Rust with
   ordinary unit tests.
2. **Storage is a trait with two methods**, `load` and `save`, so the platform
   difference is confined to one small file and tests can use memory.
3. **The screen is passive.** `notes.cui` shows the properties it is given and
   reports what the user did. It decides nothing.

## Step 1: the package

Android and the Web load a library, the desktop runs a binary, so the package
has both:

```toml
[lib]
name = "craftui_example_notes"
crate-type = ["lib", "cdylib"]

[[bin]]
name = "craftui-notes"
path = "src/main.rs"
required-features = ["desktop"]

[features]
default = ["desktop"]
desktop = ["craftui/desktop"]
```

The CLI builds Android and Web with `--no-default-features`, which leaves the
desktop windowing code out. Keep the library name equal to the package name with
underscores: the CLI looks for it under that name.

`build.rs` is the same in every CraftUI app:

```rust
fn main() {
    println!("cargo:rerun-if-changed=ui");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    craftui_dsl::compile_cui_dir("ui", &out_dir).expect("failed compiling .cui files");
}
```

## Step 2: the model

`src/model.rs` defines `Note` and `Notebook`. The notebook can create, update,
pin and delete notes, return them filtered and ordered (`visible`), and convert
itself to and from JSON. Time is always passed in as a number, never read from
the system clock, which keeps every rule testable:

```rust
let mut book = Notebook::new();
let id = book.create("Groceries", "milk, eggs", now_ms);
book.set_pinned(id, true);
let shown = book.visible(NoteFilter::All, "milk");
```

## Step 3: storage

```rust
pub trait NoteStorage {
    fn load(&self) -> Option<String>;
    fn save(&self, text: &str) -> Result<(), String>;
}
```

There are three implementations: `FileStorage` (desktop and Android; writes to a
temporary file and renames it, so a crash cannot leave half a file),
`LocalStorage` (browser) and `MemoryStorage` (tests).

## Step 4: the screen

`ui/notes.cui` declares one struct for a list row and one component:

```cui
export struct NoteCard {
    title: string = "",
    preview: string = "",
    meta: string = "",
    selected: bool = false,
}

export component NotesList {
    in-out property <[NoteCard]> notes: [];
    in-out property <string> search_query: "";
    callback on_open_note(int);

    Column {
        width: 100%;
        height: 100%;
        padding: 16dp;
        gap: 10dp;

        TextInput {
            width: 100%;
            placeholder: "Search notes";
            value <=> search_query;
        }

        ScrollView {
            width: 100%;
            flex_grow: 1.0;

            Column {
                width: 100%;
                gap: 8dp;

                for note, idx in notes : Column {
                    width: 100%;
                    padding: 12dp;
                    border_radius: 12dp;
                    background: note.selected ? Theme.primary_container : Theme.surface_variant;
                    on_tap => { on_open_note(idx); }

                    Text { text: note.title; font_weight: SemiBold; color: Theme.text_primary; }
                    Text { text: note.preview; color: Theme.text_secondary; }
                    Text { text: note.meta; font_size: 11; color: Theme.text_secondary; }
                }
            }
        }
    }
}
```

That is a trimmed version of the list pane. Points worth noticing:

- The list rows are **already formatted text** (`title`, `preview`, `meta`).
  Truncating, "3 h ago" and "Pinned" are decided in Rust, where they are tested.
- A tap reports the **row index**; Rust maps it back to a note id.
- `value <=> search_query` means Rust can simply watch `search_query` to react
  to typing.

### One layout for phone and desktop

The real screen holds both panes in a `Row` and shows them according to three
flags:

```text
Row {
    if show_list : Column { width: wide_layout ? 340dp : 100%; ... }
    if wide_layout : Divider { ... }
    if show_editor : Column { flex_grow: 1.0; ... }
}
```

Rust sets the flags from the window width and from whether a note is open:

```rust
let metrics = tree.metrics_signal;
cx.effect(move || {
    let wide = metrics.get().logical_size.width >= 720.0;
    screen.wide_layout.set_if_changed(wide);
});
```

On a phone, opening a note hides the list and shows the editor with a back
button. On a wide window both are visible. Resizing switches between the two
and keeps the open note open.

## Step 5: the controller

`NotesController` in `src/lib.rs` owns the notebook and the storage, and holds
the generated `NotesScreen`. `mount_notes_app` wires it up in three kinds of
connection:

```rust
// 1. Callbacks: the user did something.
let c = controller.clone();
screen.set_on_new_note(move || c.new_note());

// 2. Effects on UI state: the user typed or switched tab.
let c = controller.clone();
cx.effect(move || {
    let filter = NoteFilter::from_tab(c.screen.filter_tab.get());
    let query = c.screen.search_query.get();
    c.refresh_list(filter, &query);
});

// 3. Publishing: after any change, the controller writes the new list of cards
//    and labels into the screen's properties.
```

Every change to the notebook goes through one `persist` method that saves and,
on failure, shows the reason in a banner while keeping the text in memory.

## Step 6: one entry point per platform

Each one picks a storage backend and calls `mount_notes_app`.

Desktop (`src/main.rs`):

```rust
fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig { title: "CraftUI Notes".to_string(), ..Default::default() },
        |cx, tree, _app_context| {
            let config = NotesConfig::new(FileStorage::new(default_data_file()));
            mount_notes_app(cx, tree, config).0
        },
    )
}
```

Web (`src/lib.rs`): the browser calls `start` when the module has loaded.

```rust
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    craftui_platform_web::start_web_app(
        craftui_platform_web::WebBridgeConfig::default(),
        |cx, tree, _textures, _dispatcher| {
            let config = NotesConfig::new(storage::LocalStorage::new("craftui-notes/v1"));
            mount_notes_app(cx, tree, config).0
        },
    )
}
```

Android (`src/lib.rs`): the system calls `android_main`. Notes go into the app's
private files directory, which needs no permission.

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: craftui::platform_android::android_activity::AndroidApp) {
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
```

## Step 7: tests

Because the screen is plain Rust, the tests mount the real UI without a window.
A small harness provides `tap("id")`, `resize(...)` and `render()`:

```rust
#[test]
fn phone_layout_shows_one_pane_at_a_time() {
    let mut h = Harness::start(seeded(), PHONE);
    assert!(h.has_node("list_pane"));
    assert!(!h.has_node("editor_pane"));

    h.ctrl.open_note(2);
    h.layout();
    assert!(!h.has_node("list_pane"));
    assert!(h.has_node("title_input"));
}
```

Widgets are found by the `id` set in the `.cui` file, so give an `id` to
everything a test needs to reach. The harness uses an in-memory storage and a
clock it controls, which makes "edited 3 h ago" and "what was saved" exact.

## Step 8: build and ship

```bash
# Web: a folder of static files
cargo run -p craftui-cli -- build --platform web --release \
    -p craftui-example-notes -o dist/notes-web

# Android: a Gradle project, then an APK
cargo run -p craftui-cli -- build --platform android --release \
    -p craftui-example-notes -o dist/notes-android
gradle -p dist/notes-android assembleDebug
```

`examples/notes/README.md` lists the prerequisites. The repository's CI
(`.github/workflows/ci.yml`, jobs `notes-web` and `notes-android`) runs exactly
these commands, tests the web build in headless Chrome, and publishes the APK
and the web bundle as artifacts, so it doubles as a working recipe for your own
pipeline.
