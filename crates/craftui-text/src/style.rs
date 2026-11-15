use craftui_core::Color;
use serde::{Deserialize, Serialize};

/// Text horizontal alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// How text overflow should be handled when exceeding maximum width/bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
    Wrap,
}

/// Text styling configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_size: f32,
    pub font_weight: f32,
    pub italic: bool,
    pub font_family: Option<String>,
    pub color: Color,
    pub line_height: f32,
    pub align: TextAlign,
    pub underline: bool,
    pub overflow: TextOverflow,
    pub max_lines: Option<usize>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            font_weight: 400.0,
            italic: false,
            font_family: None,
            color: Color::WHITE,
            line_height: 1.3,
            align: TextAlign::Start,
            underline: false,
            overflow: TextOverflow::Clip,
            max_lines: None,
        }
    }
}

impl TextStyle {
    pub fn with_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.font_weight = weight;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }
}

/// A styled segment of text within a [`RichText`] container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub style: TextStyle,
}

impl TextSpan {
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::default(),
        }
    }

    /// Check if this text span contains any Unicode emoji characters.
    pub fn has_emojis(&self) -> bool {
        self.text.chars().any(is_emoji_char)
    }
}

/// Rich composite text container holding heterogeneous styled spans.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RichText {
    pub spans: Vec<TextSpan>,
    pub align: TextAlign,
    pub max_lines: Option<usize>,
}

impl RichText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, span: TextSpan) -> &mut Self {
        self.spans.push(span);
        self
    }

    pub fn with_span(mut self, span: TextSpan) -> Self {
        self.spans.push(span);
        self
    }

    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for s in &self.spans {
            out.push_str(&s.text);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|s| s.text.is_empty())
    }
}

/// Determine whether a Unicode character falls within standardized Emoji code point ranges.
pub fn is_emoji_char(c: char) -> bool {
    matches!(
        c as u32,
        0x1F600..=0x1F64F | // Emoticons
        0x1F300..=0x1F5FF | // Misc Symbols and Pictographs
        0x1F680..=0x1F6FF | // Transport and Map
        0x1F700..=0x1F77F | // Alchemical Symbols
        0x1F780..=0x1F7FF | // Geometric Shapes Extended
        0x1F800..=0x1F8FF | // Supplemental Arrows-C
        0x1F900..=0x1F9FF | // Supplemental Symbols and Pictographs
        0x1FA00..=0x1FA6F | // Chess Symbols
        0x1FA70..=0x1FAFF | // Symbols and Pictographs Extended-A
        0x2600..=0x26FF   | // Misc symbols
        0x2700..=0x27BF   | // Dingbats
        0xFE00..=0xFE0F   | // Variation Selectors
        0x1F1E6..=0x1F1FF   // Regional Indicator Symbols (Flags)
    )
}
