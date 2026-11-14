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
