use craftui_core::Color;

use crate::editor::TextEditorState;
use crate::engine::TextEngine;
use crate::style::TextStyle;

#[test]
fn shape_latin_and_unicode_scripts_with_cache_hits() {
    let mut engine = TextEngine::new();
    let style = TextStyle::default().with_size(18.0);
    let size = engine.measure("Hello CraftUI • नमस्ते • مرحبا", &style, Some(300.0));
    assert!(size.width > 0.0);
    assert!(size.height >= 18.0);

    // Same text with different color hits the layout cache!
    let style2 = style.clone().with_color(Color::rgb(255, 0, 128));
    let size2 = engine.measure("Hello CraftUI • नमस्ते • مرحبا", &style2, Some(300.0));
    assert_eq!(size, size2);
    let (hits, misses) = engine.cache_stats();
    assert_eq!(hits, 1);
    assert_eq!(misses, 1);
}

#[test]
fn embedded_fallback_font_shapes_without_system_fonts() {
    let mut engine = TextEngine::new_without_system_fonts();
    let style = TextStyle::default().with_size(16.0);
    let layout = engine.build_layout_cached("CraftUI Fallback Font", &style, None);
    assert!(layout.width() > 50.0);
    assert!(layout.lines().count() >= 1);
}

#[test]
fn editor_selection_word_motion_undo_redo_and_cursor_clamp() {
    let mut ed = TextEditorState::new("Craft");
    ed.insert_str("UI");
    assert_eq!(ed.text, "CraftUI");
    ed.move_left(true);
    ed.move_left(true);
    assert_eq!(ed.selected_text(), "UI");
    ed.insert_str("Kit");
    assert_eq!(ed.text, "CraftKit");
    ed.preedit = Some("!".into());
    assert_eq!(ed.display_text(), "CraftKit!");
    ed.preedit = None;

    // Undo & Redo
    assert!(ed.undo());
    assert_eq!(ed.text, "CraftUI");
    assert!(ed.redo());
    assert_eq!(ed.text, "CraftKit");

    // Word jump and word delete
    let mut doc = TextEditorState::new("hello world_123 test");
    doc.move_word_left(false);
    assert_eq!(doc.cursor, 16);
    doc.move_word_left(false);
    assert_eq!(doc.cursor, 6);
    doc.delete_word_backward();
    assert_eq!(doc.text, "world_123 test");
    assert!(doc.undo());
    assert_eq!(doc.text, "hello world_123 test");

    // External sync preserves mid-string cursor cleanly clamped
    doc.set_cursor_at(5, false);
    assert!(doc.sync_external("hello updated string"));
    assert_eq!(doc.cursor, 5);
    assert!(doc.sync_external("hi"));
    assert_eq!(doc.cursor, 2);
}
