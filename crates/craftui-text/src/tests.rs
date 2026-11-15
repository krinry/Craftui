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

#[test]
fn richtext_spans_plain_text_and_emoji_detection() {
    use crate::style::{RichText, TextSpan, TextStyle, is_emoji_char};

    let mut rich = RichText::new();
    rich.push(TextSpan::new(
        "Hello ",
        TextStyle::default().with_weight(700.0),
    ));
    rich.push(TextSpan::new(
        "World 🚀✨",
        TextStyle::default().with_color(Color::rgb(0, 200, 255)),
    ));

    assert_eq!(rich.plain_text(), "Hello World 🚀✨");
    assert!(!rich.is_empty());
    assert!(rich.spans[1].has_emojis());
    assert!(!rich.spans[0].has_emojis());

    // Emoji code point range detection
    assert!(is_emoji_char('🚀'));
    assert!(is_emoji_char('✨'));
    assert!(is_emoji_char('🎉'));
    assert!(is_emoji_char('⭐')); // U+2B50 White Medium Star
    assert!(!is_emoji_char('\u{1F700}')); // U+1F700 Alchemical Symbol is not emoji
    assert!(!is_emoji_char('A'));
    assert!(!is_emoji_char('9'));
    assert!(!is_emoji_char(' '));

    let star_span = TextSpan::new("Rating: ⭐⭐⭐", TextStyle::default());
    assert!(star_span.has_emojis());
    let alchem_span = TextSpan::new("Formula: \u{1F700}", TextStyle::default());
    assert!(!alchem_span.has_emojis());
}

#[test]
fn rich_layout_preserves_span_styles_across_utf8_boundaries() {
    use crate::style::{RichText, TextSpan};
    use parley::PositionedLayoutItem;

    let red = Color::rgb(220, 30, 40);
    let blue = Color::rgb(20, 70, 220);
    let rich = RichText::new()
        .with_span(TextSpan::new("café ", TextStyle::default().with_color(red)))
        .with_span(TextSpan::new(
            "World",
            TextStyle::default().with_weight(700.0).with_color(blue),
        ));
    let mut engine = TextEngine::new_without_system_fonts();
    let layout = engine.build_rich_layout(&rich, Some(300.0));
    assert!(layout.width() > 0.0);
    let colors: Vec<_> = layout
        .lines()
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => Some(run.style().brush),
            _ => None,
        })
        .collect();
    assert!(colors.contains(&red));
    assert!(colors.contains(&blue));
}

#[test]
fn rich_layout_supports_mixed_direction_paragraphs() {
    use crate::style::{RichText, TextSpan};

    let rich = RichText::new()
        .with_span(TextSpan::plain("ABC "))
        .with_span(TextSpan::plain("שלום 123"));
    let mut engine = TextEngine::new();
    let layout = engine.build_rich_layout(&rich, Some(200.0));
    assert!(layout.width() > 0.0);
    assert!(layout.lines().count() > 0);
    let size = engine.measure_rich(&rich, Some(200.0));
    assert!(size.width > 0.0 && size.height > 0.0);
}
