use craftui_core::{Alignment, Direction, Insets, Justify, Length};
use serde::{Deserialize, Serialize};
use taffy::prelude as tp;

/// Container layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayoutMode {
    #[default]
    Column,
    Row,
    Grid,
    Stack,
}

/// CraftUI declarative layout styling translated into `taffy::Style`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutStyle {
    pub mode: LayoutMode,
    pub hidden: bool,
    pub direction: Option<Direction>,
    pub flex_wrap: bool,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    /// The size the node starts from on its parent's main axis before spare
    /// space is shared out. `Auto` means the node's own size or content.
    #[serde(default)]
    pub flex_basis: Length,
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub aspect_ratio: Option<f32>,
    pub padding: Insets,
    pub margin: Insets,
    pub gap_x: f32,
    pub gap_y: f32,
    pub align_items: Alignment,
    pub justify_content: Justify,
    pub grid_columns: u16,
    pub grid_column_min_width: Option<f32>,
    pub absolute: bool,
    pub inset: Insets,
    pub use_inset: bool,
    pub inset_top: Option<f32>,
    pub inset_right: Option<f32>,
    pub inset_bottom: Option<f32>,
    pub inset_left: Option<f32>,
    pub safe_area: bool,
    pub clip_overflow: bool,
    #[serde(default)]
    pub header_top_inset: f32,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            mode: LayoutMode::Column,
            hidden: false,
            direction: None,
            flex_wrap: false,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            aspect_ratio: None,
            padding: Insets::ZERO,
            margin: Insets::ZERO,
            gap_x: 0.0,
            gap_y: 0.0,
            align_items: Alignment::Stretch,
            justify_content: Justify::Start,
            grid_columns: 2,
            grid_column_min_width: None,
            absolute: false,
            inset: Insets::ZERO,
            use_inset: false,
            inset_top: None,
            inset_right: None,
            inset_bottom: None,
            inset_left: None,
            safe_area: false,
            clip_overflow: false,
            header_top_inset: 0.0,
        }
    }
}

impl LayoutStyle {
    pub fn column() -> Self {
        Self {
            mode: LayoutMode::Column,
            ..Default::default()
        }
    }

    pub fn row() -> Self {
        Self {
            mode: LayoutMode::Row,
            align_items: Alignment::Center,
            ..Default::default()
        }
    }

    pub fn grid(columns: u16) -> Self {
        Self {
            mode: LayoutMode::Grid,
            grid_columns: columns.max(1),
            ..Default::default()
        }
    }

    pub fn stack() -> Self {
        Self {
            mode: LayoutMode::Stack,
            ..Default::default()
        }
    }

    pub fn with_width(mut self, w: Length) -> Self {
        self.width = w;
        self
    }

    pub fn with_height(mut self, h: Length) -> Self {
        self.height = h;
        self
    }

    pub fn with_size(mut self, w: Length, h: Length) -> Self {
        self.width = w;
        self.height = h;
        self
    }

    /// Take exactly the space the parent has left on its main axis, however
    /// large the content is.
    ///
    /// The node starts from zero, grows by `grow`, and may be smaller than its
    /// content. Use it for a scrolling area that sits under fixed headers: the
    /// headers keep their size and the area scrolls inside what remains.
    pub fn fill_remaining(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self.flex_basis = Length::Dp(0.0);
        self.min_width = Length::Dp(0.0);
        self.min_height = Length::Dp(0.0);
        self
    }

    /// Set the smallest size the node may be given.
    ///
    /// A flex child is normally never made smaller than its content. Passing
    /// zero lifts that limit, so the node can be shrunk to the space that is
    /// left and its content can scroll or be clipped inside it.
    pub fn with_min_size(mut self, w: Length, h: Length) -> Self {
        self.min_width = w;
        self.min_height = h;
        self
    }

    pub fn with_padding(mut self, p: Insets) -> Self {
        self.padding = p;
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.margin = m;
        self
    }

    pub fn with_gap(mut self, g: f32) -> Self {
        self.gap_x = g;
        self.gap_y = g;
        self
    }

    pub fn with_flex_grow(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }

    pub fn with_align(mut self, align: Alignment) -> Self {
        self.align_items = align;
        self
    }

    pub fn with_justify(mut self, justify: Justify) -> Self {
        self.justify_content = justify;
        self
    }

    pub fn with_inset_top(mut self, v: f32) -> Self {
        self.absolute = true;
        self.inset_top = Some(v);
        self
    }

    pub fn with_inset_right(mut self, v: f32) -> Self {
        self.absolute = true;
        self.inset_right = Some(v);
        self
    }

    pub fn with_inset_bottom(mut self, v: f32) -> Self {
        self.absolute = true;
        self.inset_bottom = Some(v);
        self
    }

    pub fn with_inset_left(mut self, v: f32) -> Self {
        self.absolute = true;
        self.inset_left = Some(v);
        self
    }

    pub fn with_safe_area(mut self, enabled: bool) -> Self {
        self.safe_area = enabled;
        self
    }

    pub(crate) fn to_taffy(
        &self,
        scale_factor: f32,
        safe_area: Insets,
        inherited_dir: Direction,
        force_absolute: bool,
    ) -> taffy::Style {
        let mut style = taffy::Style::default();
        let dir = self.direction.unwrap_or(inherited_dir);
        style.direction = match dir {
            Direction::Ltr => taffy::Direction::Ltr,
            Direction::Rtl => taffy::Direction::Rtl,
        };

        if self.hidden {
            style.display = taffy::Display::None;
            return style;
        }

        match self.mode {
            LayoutMode::Column => {
                style.display = taffy::Display::Flex;
                style.flex_direction = taffy::FlexDirection::Column;
            }
            LayoutMode::Row => {
                style.display = taffy::Display::Flex;
                style.flex_direction = taffy::FlexDirection::Row;
            }
            LayoutMode::Grid => {
                style.display = taffy::Display::Grid;
                style.grid_template_columns = (0..self.grid_columns.max(1))
                    .map(|_| {
                        if let Some(min_w) = self.grid_column_min_width {
                            tp::minmax(tp::length(min_w), tp::fr(1.0))
                        } else {
                            tp::fr(1.0)
                        }
                    })
                    .collect();
            }
            LayoutMode::Stack => {
                style.display = taffy::Display::Flex;
                style.flex_direction = taffy::FlexDirection::Column;
            }
        }

        style.flex_wrap = if self.flex_wrap {
            taffy::FlexWrap::Wrap
        } else {
            taffy::FlexWrap::NoWrap
        };
        style.flex_grow = self.flex_grow;
        style.flex_shrink = self.flex_shrink;
        if self.flex_basis != Length::Auto {
            style.flex_basis = len_to_dim(self.flex_basis, scale_factor);
        }
        style.aspect_ratio = self.aspect_ratio;

        style.size = taffy::Size {
            width: len_to_dim(self.width, scale_factor),
            height: len_to_dim(self.height, scale_factor),
        };
        style.min_size = taffy::Size {
            width: len_to_lpa(self.min_width, scale_factor),
            height: len_to_lpa(self.min_height, scale_factor),
        };
        style.max_size = taffy::Size {
            width: len_to_lpa(self.max_width, scale_factor),
            height: len_to_lpa(self.max_height, scale_factor),
        };

        let mut eff_pad = if self.safe_area {
            Insets::new(
                self.padding.top + safe_area.top,
                self.padding.right + safe_area.right,
                self.padding.bottom + safe_area.bottom,
                self.padding.left + safe_area.left,
            )
        } else {
            self.padding
        };
        eff_pad.top += self.header_top_inset;

        style.padding = taffy::Rect {
            top: tp::length(eff_pad.top),
            right: tp::length(eff_pad.right),
            bottom: tp::length(eff_pad.bottom),
            left: tp::length(eff_pad.left),
        };
        style.margin = taffy::Rect {
            top: tp::length(self.margin.top),
            right: tp::length(self.margin.right),
            bottom: tp::length(self.margin.bottom),
            left: tp::length(self.margin.left),
        };
        style.gap = taffy::Size {
            width: tp::length(self.gap_x),
            height: tp::length(self.gap_y),
        };

        style.align_items = match self.align_items {
            Alignment::Start => taffy::AlignItems::FLEX_START,
            Alignment::Center => taffy::AlignItems::CENTER,
            Alignment::End => taffy::AlignItems::FLEX_END,
            Alignment::Stretch => taffy::AlignItems::STRETCH,
        };
        style.justify_content = match self.justify_content {
            Justify::Start => taffy::JustifyContent::FLEX_START,
            Justify::Center => taffy::JustifyContent::CENTER,
            Justify::End => taffy::JustifyContent::FLEX_END,
            Justify::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
            Justify::SpaceAround => taffy::JustifyContent::SPACE_AROUND,
            Justify::SpaceEvenly => taffy::JustifyContent::SPACE_EVENLY,
        };

        if self.absolute || force_absolute {
            style.position = taffy::Position::Absolute;
            let has_individual_insets = self.inset_top.is_some()
                || self.inset_right.is_some()
                || self.inset_bottom.is_some()
                || self.inset_left.is_some();

            if has_individual_insets {
                style.inset = taffy::Rect {
                    top: self.inset_top.map(tp::length).unwrap_or_else(tp::auto),
                    right: self.inset_right.map(tp::length).unwrap_or_else(tp::auto),
                    bottom: self.inset_bottom.map(tp::length).unwrap_or_else(tp::auto),
                    left: self.inset_left.map(tp::length).unwrap_or_else(tp::auto),
                };
            } else if self.use_inset {
                style.inset = taffy::Rect {
                    top: tp::length(self.inset.top),
                    right: tp::length(self.inset.right),
                    bottom: tp::length(self.inset.bottom),
                    left: tp::length(self.inset.left),
                };
            } else if force_absolute {
                style.inset = taffy::Rect {
                    top: tp::length(0.0),
                    right: tp::auto(),
                    bottom: tp::auto(),
                    left: tp::length(0.0),
                };
            }
        }

        if self.clip_overflow {
            style.overflow = taffy::Point {
                x: taffy::Overflow::Hidden,
                y: taffy::Overflow::Hidden,
            };
        }

        style
    }
}

fn len_to_dim(len: Length, scale_factor: f32) -> taffy::Dimension {
    match len {
        Length::Auto => tp::auto(),
        Length::Dp(v) => tp::length(v),
        Length::Px(v) => tp::length(v / scale_factor.max(0.5)),
        Length::Pct(p) => tp::percent(p / 100.0),
    }
}

fn len_to_lpa(len: Length, scale_factor: f32) -> taffy::LengthPercentageAuto {
    match len {
        Length::Auto => tp::auto(),
        Length::Dp(v) => tp::length(v),
        Length::Px(v) => tp::length(v / scale_factor.max(0.5)),
        Length::Pct(p) => tp::percent(p / 100.0),
    }
}
