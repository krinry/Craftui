//! Flexbox, CSS Grid, Stack, SafeArea, and RTL layout engine powered by `taffy`.

use craftui_core::{
    Alignment, Constraints, Direction, Insets, Justify, Length, NodeId, Rect, Size,
};
use serde::{Deserialize, Serialize};
use slotmap::SecondaryMap;
use std::collections::{HashMap, HashSet};
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

    fn to_taffy(
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

/// Manages Taffy layout nodes synchronized with the CraftUI widget arena.
pub struct LayoutEngine {
    taffy: taffy::TaffyTree<NodeId>,
    node_map: SecondaryMap<NodeId, taffy::NodeId>,
    reverse_map: HashMap<taffy::NodeId, NodeId>,
    styles: SecondaryMap<NodeId, LayoutStyle>,
    children_map: SecondaryMap<NodeId, Vec<NodeId>>,
    rects: SecondaryMap<NodeId, Rect>,
    content_sizes: SecondaryMap<NodeId, Size>,
    dirty_styles: HashSet<NodeId>,
    last_env: Option<(Insets, f32, Direction)>,
    synced_node_context: SecondaryMap<NodeId, (Direction, bool)>,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            taffy: taffy::TaffyTree::new(),
            node_map: SecondaryMap::new(),
            reverse_map: HashMap::new(),
            styles: SecondaryMap::new(),
            children_map: SecondaryMap::new(),
            rects: SecondaryMap::new(),
            content_sizes: SecondaryMap::new(),
            dirty_styles: HashSet::new(),
            last_env: None,
            synced_node_context: SecondaryMap::new(),
        }
    }

    pub fn upsert_node(&mut self, node: NodeId, style: LayoutStyle) {
        let is_new = !self.node_map.contains_key(node);
        let style_changed = is_new || self.styles.get(node) != Some(&style);
        if style_changed {
            self.styles.insert(node, style);
            self.dirty_styles.insert(node);
        }
        if is_new {
            let taffy_id = self
                .taffy
                .new_leaf_with_context(taffy::Style::default(), node)
                .expect("taffy new_leaf");
            self.node_map.insert(node, taffy_id);
            self.reverse_map.insert(taffy_id, node);
            self.rects.insert(node, Rect::ZERO);
            self.content_sizes.insert(node, Size::ZERO);
        } else if style_changed {
            self.mark_dirty(node);
        }
    }

    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        if self
            .children_map
            .get(parent)
            .is_some_and(|existing| existing.as_slice() == children)
        {
            return;
        }
        self.children_map.insert(parent, children.to_vec());
        self.dirty_styles.insert(parent);
        for &c in children {
            self.dirty_styles.insert(c);
        }
        if let Some(&parent_taffy) = self.node_map.get(parent) {
            let taffy_children: Vec<taffy::NodeId> = children
                .iter()
                .filter_map(|c| self.node_map.get(*c).copied())
                .collect();
            let _ = self.taffy.set_children(parent_taffy, &taffy_children);
        }
    }

    pub fn remove_node(&mut self, node: NodeId) {
        self.styles.remove(node);
        self.children_map.remove(node);
        self.rects.remove(node);
        self.content_sizes.remove(node);
        self.dirty_styles.remove(&node);
        self.synced_node_context.remove(node);
        if let Some(taffy_id) = self.node_map.remove(node) {
            self.reverse_map.remove(&taffy_id);
            let _ = self.taffy.remove(taffy_id);
        }
    }

    pub fn mark_dirty(&mut self, node: NodeId) {
        if let Some(&taffy_id) = self.node_map.get(node) {
            let _ = self.taffy.mark_dirty(taffy_id);
        }
    }

    /// Recompute Taffy styles (only for dirty nodes or global metric changes) and solve layout.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_layout<F>(
        &mut self,
        root: NodeId,
        available_width: f32,
        available_height: f32,
        safe_area: Insets,
        scale_factor: f32,
        root_direction: Direction,
        mut measure_leaf: F,
    ) where
        F: FnMut(NodeId, Constraints) -> Size,
    {
        let env_now = (safe_area, scale_factor, root_direction);
        let env_changed = self.last_env != Some(env_now);
        self.last_env = Some(env_now);

        sync_styles_recursive(
            &mut self.taffy,
            &self.node_map,
            &self.styles,
            &self.children_map,
            &mut self.dirty_styles,
            &mut self.synced_node_context,
            node_params(safe_area, scale_factor, env_changed),
            root,
            root_direction,
            false,
        );
        self.dirty_styles.clear();

        let Some(&root_taffy) = self.node_map.get(root) else {
            return;
        };

        let avail = taffy::Size {
            width: taffy::AvailableSpace::Definite(available_width),
            height: taffy::AvailableSpace::Definite(available_height),
        };

        let _ = self.taffy.compute_layout_with_measure(
            root_taffy,
            avail,
            |inputs, _taffy_id, ctx, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, avail| {
                        let Some(&node_id) = ctx.as_deref() else {
                            return taffy::Size::ZERO;
                        };
                        let max_w = match (known.width, avail.width) {
                            (Some(w), _) => w,
                            (None, taffy::AvailableSpace::Definite(w)) => w,
                            (None, taffy::AvailableSpace::MaxContent) => f32::INFINITY,
                            (None, taffy::AvailableSpace::MinContent) => 0.0,
                        };
                        let max_h = match (known.height, avail.height) {
                            (Some(h), _) => h,
                            (None, taffy::AvailableSpace::Definite(h)) => h,
                            (None, taffy::AvailableSpace::MaxContent) => f32::INFINITY,
                            (None, taffy::AvailableSpace::MinContent) => 0.0,
                        };
                        let constraints = Constraints {
                            min_width: known.width.unwrap_or(0.0),
                            max_width: max_w,
                            min_height: known.height.unwrap_or(0.0),
                            max_height: max_h,
                        };
                        let measured = measure_leaf(node_id, constraints);
                        taffy::Size {
                            width: known.width.unwrap_or(measured.width),
                            height: known.height.unwrap_or(measured.height),
                        }
                    },
                )
            },
        );

        collect_rects_recursive(
            &self.taffy,
            &self.node_map,
            &self.children_map,
            &mut self.rects,
            &mut self.content_sizes,
            root,
            0.0,
            0.0,
        );
    }

    pub fn rect(&self, node: NodeId) -> Rect {
        self.rects.get(node).copied().unwrap_or(Rect::ZERO)
    }

    pub fn content_size(&self, node: NodeId) -> Size {
        self.content_sizes.get(node).copied().unwrap_or(Size::ZERO)
    }
}

#[derive(Clone, Copy)]
struct SyncEnv {
    safe_area: Insets,
    scale_factor: f32,
    env_changed: bool,
}

#[inline]
fn node_params(safe_area: Insets, scale_factor: f32, env_changed: bool) -> SyncEnv {
    SyncEnv {
        safe_area,
        scale_factor,
        env_changed,
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_styles_recursive(
    taffy: &mut taffy::TaffyTree<NodeId>,
    node_map: &SecondaryMap<NodeId, taffy::NodeId>,
    styles: &SecondaryMap<NodeId, LayoutStyle>,
    children_map: &SecondaryMap<NodeId, Vec<NodeId>>,
    dirty_styles: &mut HashSet<NodeId>,
    synced_node_context: &mut SecondaryMap<NodeId, (Direction, bool)>,
    env: SyncEnv,
    node: NodeId,
    inherited_dir: Direction,
    force_absolute: bool,
) {
    let Some(style) = styles.get(node) else {
        return;
    };
    let eff_dir = style.direction.unwrap_or(inherited_dir);
    let is_stack = style.mode == LayoutMode::Stack;
    let ctx_now = (eff_dir, force_absolute);
    let ctx_changed = synced_node_context.get(node) != Some(&ctx_now);

    if env.env_changed || ctx_changed || dirty_styles.contains(&node) {
        let taffy_style = style.to_taffy(env.scale_factor, env.safe_area, eff_dir, force_absolute);
        if let Some(&taffy_id) = node_map.get(node) {
            let _ = taffy.set_style(taffy_id, taffy_style);
        }
        synced_node_context.insert(node, ctx_now);
    }

    if let Some(children) = children_map.get(node) {
        for (idx, &child) in children.iter().enumerate() {
            let child_abs = is_stack && idx > 0;
            sync_styles_recursive(
                taffy,
                node_map,
                styles,
                children_map,
                dirty_styles,
                synced_node_context,
                env,
                child,
                eff_dir,
                child_abs,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_rects_recursive(
    taffy: &taffy::TaffyTree<NodeId>,
    node_map: &SecondaryMap<NodeId, taffy::NodeId>,
    children_map: &SecondaryMap<NodeId, Vec<NodeId>>,
    rects: &mut SecondaryMap<NodeId, Rect>,
    content_sizes: &mut SecondaryMap<NodeId, Size>,
    node: NodeId,
    parent_x: f32,
    parent_y: f32,
) {
    let Some(&taffy_id) = node_map.get(node) else {
        return;
    };
    let Ok(layout) = taffy.layout(taffy_id) else {
        return;
    };
    let abs_x = parent_x + layout.location.x;
    let abs_y = parent_y + layout.location.y;
    rects.insert(
        node,
        Rect::new(abs_x, abs_y, layout.size.width, layout.size.height),
    );
    content_sizes.insert(
        node,
        Size::new(
            layout.size.width + layout.scroll_width(),
            layout.size.height + layout.scroll_height(),
        ),
    );
    if let Some(children) = children_map.get(node) {
        for &child in children {
            collect_rects_recursive(
                taffy,
                node_map,
                children_map,
                rects,
                content_sizes,
                child,
                abs_x,
                abs_y,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{dp, pct};
    use slotmap::SlotMap;

    #[test]
    fn flex_row_and_safe_area_and_rtl() {
        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let root = nodes.insert(());
        let a = nodes.insert(());
        let b = nodes.insert(());

        let mut engine = LayoutEngine::new();
        engine.upsert_node(
            root,
            LayoutStyle::row()
                .with_size(dp(300.0), dp(100.0))
                .with_gap(10.0)
                .with_safe_area(true),
        );
        engine.upsert_node(a, LayoutStyle::column().with_size(dp(80.0), pct(100.0)));
        engine.upsert_node(
            b,
            LayoutStyle::column()
                .with_flex_grow(1.0)
                .with_height(pct(100.0)),
        );
        engine.set_children(root, &[a, b]);

        let safe = Insets::new(20.0, 0.0, 10.0, 16.0);
        engine.compute_layout(root, 300.0, 100.0, safe, 1.0, Direction::Ltr, |_, _| {
            Size::ZERO
        });

        let ra = engine.rect(a);
        let rb = engine.rect(b);
        assert_eq!(ra.x, 16.0);
        assert_eq!(ra.y, 20.0);
        assert_eq!(ra.width, 80.0);
        assert_eq!(ra.height, 70.0);
        assert_eq!(rb.x, 106.0);
        assert_eq!(rb.width, 194.0);

        // Verify RTL mirroring swaps a and b horizontally!
        engine.compute_layout(
            root,
            300.0,
            100.0,
            Insets::ZERO,
            1.0,
            Direction::Rtl,
            |_, _| Size::ZERO,
        );
        let ra_rtl = engine.rect(a);
        let rb_rtl = engine.rect(b);
        assert!(
            ra_rtl.x > rb_rtl.x,
            "RTL should place first child on the right"
        );
    }

    #[test]
    fn grid_layout_and_leaf_measure() {
        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let grid = nodes.insert(());
        let c1 = nodes.insert(());
        let c2 = nodes.insert(());

        let mut engine = LayoutEngine::new();
        engine.upsert_node(
            grid,
            LayoutStyle::grid(2).with_width(dp(220.0)).with_gap(20.0),
        );
        engine.upsert_node(c1, LayoutStyle::column());
        engine.upsert_node(c2, LayoutStyle::column());
        engine.set_children(grid, &[c1, c2]);

        engine.compute_layout(
            grid,
            220.0,
            400.0,
            Insets::ZERO,
            1.0,
            Direction::Ltr,
            |_, _| Size::new(50.0, 40.0),
        );
        assert_eq!(engine.rect(c1), Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(engine.rect(c2), Rect::new(120.0, 0.0, 100.0, 40.0));
    }

    #[test]
    fn header_top_inset_combines_with_padding_and_safe_area() {
        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let card = nodes.insert(());
        let child = nodes.insert(());

        let mut engine = LayoutEngine::new();
        let mut card_style = LayoutStyle::column()
            .with_width(dp(240.0))
            .with_padding(Insets::all(10.0))
            .with_safe_area(true);
        card_style.header_top_inset = 44.0;
        engine.upsert_node(card, card_style);
        engine.upsert_node(child, LayoutStyle::column().with_height(dp(30.0)));
        engine.set_children(card, &[child]);

        let safe = Insets::new(12.0, 0.0, 8.0, 4.0);
        engine.compute_layout(card, 240.0, 300.0, safe, 1.0, Direction::Ltr, |_, _| {
            Size::ZERO
        });

        let rc = engine.rect(child);
        assert_eq!(rc.x, 14.0);
        assert_eq!(rc.y, 10.0 + 12.0 + 44.0);
        assert_eq!(
            engine.rect(card).height,
            10.0 + 12.0 + 44.0 + 30.0 + 10.0 + 8.0
        );
    }
}
