use std::sync::Arc;

use craftui_core::{Color, Rect, Size};
use parley::editing::{Cursor, Selection};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, GenericFamily,
    Layout, LayoutContext, LineHeight, StyleProperty,
};

use crate::cache::{DEFAULT_LAYOUT_CACHE_CAPACITY, LayoutCacheKey, ShapedLayoutCache};
use crate::style::{TextAlign, TextStyle};

#[cfg(feature = "embedded-fallback-font")]
const EMBEDDED_FALLBACK_FONT: &[u8] = include_bytes!("../assets/Vera.ttf");

/// Shared font database, LRU shaped-layout cache, and shaping scratchpad.
pub struct TextEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<Color>,
    cache: ShapedLayoutCache,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            cache: ShapedLayoutCache::new(DEFAULT_LAYOUT_CACHE_CAPACITY),
        };
        engine.install_embedded_fallback_font();
        engine
    }

    /// Create a [`TextEngine`] without discovering system fonts (relies solely on
    /// registered or embedded fallback fonts — useful for deterministic headless tests and Web/WASM).
    pub fn new_without_system_fonts() -> Self {
        let collection = fontique::Collection::new(fontique::CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let font_cx = FontContext {
            collection,
            source_cache: fontique::SourceCache::default(),
        };
        let mut engine = Self {
            font_cx,
            layout_cx: LayoutContext::new(),
            cache: ShapedLayoutCache::new(DEFAULT_LAYOUT_CACHE_CAPACITY),
        };
        engine.install_embedded_fallback_font();
        engine
    }

    fn install_embedded_fallback_font(&mut self) {
        #[cfg(feature = "embedded-fallback-font")]
        {
            let blob = fontique::Blob::new(Arc::new(EMBEDDED_FALLBACK_FONT));
            let registered = self.font_cx.collection.register_fonts(blob, None);
            let ids: Vec<_> = registered.into_iter().map(|(id, _)| id).collect();
            if !ids.is_empty() {
                self.font_cx
                    .collection
                    .append_generic_families(GenericFamily::SansSerif, ids.iter().copied());
                self.font_cx
                    .collection
                    .append_generic_families(GenericFamily::SystemUi, ids.iter().copied());
            }
        }
    }

    /// Register custom font bytes (e.g., bundled app icons or fallback font on minimal systems).
    pub fn register_font_bytes(&mut self, bytes: Vec<u8>) {
        self.font_cx.collection.register_fonts(bytes.into(), None);
        self.cache.clear();
    }

    /// Number of cache hits and misses recorded by the shaped layout LRU cache.
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.cache.hits, self.cache.misses)
    }

    /// Clear all cached shaped text layouts.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    fn shape_uncached(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        brush_color: Color,
    ) -> Layout<Color> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(style.font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(
            style.font_weight,
        )));
        builder.push_default(StyleProperty::FontStyle(if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        builder.push_default(StyleProperty::Brush(brush_color));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        builder.push_default(StyleProperty::Underline(style.underline));

        if let Some(family) = style
            .font_family
            .as_deref()
            .filter(|f| !f.trim().is_empty())
        {
            builder.push_default(StyleProperty::FontFamily(FontFamily::from(family)));
        } else {
            builder.push_default(StyleProperty::FontFamily(FontFamily::from(
                GenericFamily::SansSerif,
            )));
        }

        let mut layout = builder.build(text);
        layout.break_all_lines(max_width);
        let parley_align = match style.align {
            TextAlign::Start => Alignment::Start,
            TextAlign::Center => Alignment::Center,
            TextAlign::End => Alignment::End,
            TextAlign::Justify => Alignment::Justify,
        };
        layout.align(parley_align, AlignmentOptions::default());
        layout
    }

    /// Shape and break text lines into a cached shared [`Arc<parley::Layout<Color>>`].
    ///
    /// Shaping geometry is independent of color, so hover/theme color transitions hit the cache cleanly;
    /// renderers should paint glyph runs using the caller's `style.color`.
    pub fn build_layout_cached(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Arc<Layout<Color>> {
        let key = LayoutCacheKey::from_inputs(text, style, max_width);
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }
        let layout = Arc::new(self.shape_uncached(text, style, max_width, Color::WHITE));
        self.cache.insert(key, Arc::clone(&layout));
        layout
    }

    /// Shape and break text lines into a [`parley::Layout<Color>`].
    pub fn build_layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Layout<Color> {
        let shared = self.build_layout_cached(text, style, max_width);
        (*shared).clone()
    }

    /// Measure bounding size of shaped text under optional `max_width` using the LRU cache.
    pub fn measure(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Size {
        if text.is_empty() {
            return Size::new(0.0, style.font_size * style.line_height);
        }
        let layout = self.build_layout_cached(text, style, max_width);
        Size::new(
            layout.width().ceil(),
            layout
                .height()
                .max(style.font_size * style.line_height)
                .ceil(),
        )
    }

    /// Truncate text with ellipsis `...` so that its rendered width fits strictly within `max_width`.
    pub fn truncate_with_ellipsis(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: f32,
    ) -> String {
        let max_w = max_width.max(0.0);
        if text.is_empty() || max_w <= 0.0 {
            return String::new();
        }
        let measured = self.measure(text, style, None);
        if measured.width <= max_w {
            return text.to_string();
        }
        let ellipsis = "…";
        let el_size = self.measure(ellipsis, style, None);
        if el_size.width >= max_w {
            return ellipsis.to_string();
        }
        let avail_w = max_w - el_size.width;
        let chars: Vec<char> = text.chars().collect();
        let mut low = 0;
        let mut high = chars.len();
        let mut best = 0;
        while low <= high {
            let mid = (low + high) / 2;
            let sub: String = chars[..mid].iter().collect();
            let sub_w = self.measure(&sub, style, None).width;
            if sub_w <= avail_w {
                best = mid;
                low = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }
        let mut res: String = chars[..best].iter().collect();
        res.push_str(ellipsis);
        res
    }

    /// Compute cursor rectangle in local layout coordinates.
    pub fn cursor_rect(&self, layout: &Layout<Color>, byte_index: usize) -> Rect {
        let cursor = Cursor::from_byte_index(layout, byte_index, parley::Affinity::Downstream);
        let bbox = cursor.geometry(layout, 2.0);
        Rect::new(
            bbox.x0 as f32,
            bbox.y0 as f32,
            (bbox.x1 - bbox.x0).max(1.5) as f32,
            (bbox.y1 - bbox.y0).max(12.0) as f32,
        )
    }

    /// Compute selection highlight rectangles in local layout coordinates.
    pub fn selection_rects(
        &self,
        layout: &Layout<Color>,
        anchor_byte: usize,
        focus_byte: usize,
    ) -> Vec<Rect> {
        let anchor = Cursor::from_byte_index(layout, anchor_byte, parley::Affinity::Downstream);
        let focus = Cursor::from_byte_index(layout, focus_byte, parley::Affinity::Downstream);
        let sel = Selection::new(anchor, focus);
        sel.geometry(layout)
            .into_iter()
            .map(|(b, _)| {
                Rect::new(
                    b.x0 as f32,
                    b.y0 as f32,
                    (b.x1 - b.x0) as f32,
                    (b.y1 - b.y0) as f32,
                )
            })
            .collect()
    }

    /// Map local point `(x, y)` to nearest UTF-8 byte index in `layout`.
    pub fn hit_test_point(&self, layout: &Layout<Color>, x: f32, y: f32) -> usize {
        let cursor = Cursor::from_point(layout, x, y);
        cursor.index()
    }
}
