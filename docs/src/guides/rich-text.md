# Rich text (low-level API)

`craftui-text` can shape multiple styled spans as one paragraph using Parley.
That allows line breaking and bidirectional ordering to operate across span
boundaries. `PaintScene::draw_rich_text` paints glyph runs with their span
colors on the Vello, Vello CPU, tiny-skia and SDF backends.

```rust,ignore
use craftui::core::{Color, Point};
use craftui::render::PaintScene;
use craftui::text::{RichText, TextSpan, TextStyle};

let paragraph = RichText::new()
    .with_span(TextSpan::new("Hello ", TextStyle::default().with_weight(700.0)))
    .with_span(TextSpan::new("world", TextStyle::default().with_color(Color::BLUE)));
let mut scene = PaintScene::new();
scene.draw_rich_text(Point::new(0.0, 0.0), paragraph, Some(300.0));
```

Rust applications can also use `tree.rich_text(cx, paragraph)` to create a
reactive widget; it reports the concatenated paragraph as an accessibility
label. Mark a span with `.with_link(target)` and attach `tree.set_on_link(node,
handler)` to receive pointer activations. The handler is responsible for
validating and opening targets; CraftUI does not navigate automatically.
Individual links are not yet exposed as keyboard-focusable accessibility nodes.

A `.cui` RichText widget, line limits, and color emoji glyph rendering are not
implemented yet. `TextStyle::underline` is drawn using the shaped font metrics. The embedded fallback font does not cover
all scripts; register suitable fonts through `TextEngine::register_font_bytes`
when system fonts are unavailable. Font coverage and color-emoji support must
be tested on each target platform.
