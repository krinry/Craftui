use std::collections::{HashMap, HashSet};

use craftui_core::{Constraints, Direction, Insets, NodeId, Rect, Size};
use slotmap::SecondaryMap;

use crate::style::{LayoutMode, LayoutStyle};

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
