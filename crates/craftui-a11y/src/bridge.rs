use std::collections::{HashMap, HashSet};
use slotmap::Key;
use craftui_core::NodeId;

use crate::types::{A11yAction, A11yRole, A11ySnapshotNode, CachedA11yNode};

/// Synchronizes CraftUI widget snapshots with `accesskit::TreeUpdate` incrementally.
pub struct A11yBridge {
    id_lookup: HashMap<u64, NodeId>,
    window_title: String,
    initialized: bool,
    last_root: Option<NodeId>,
    last_focused: Option<NodeId>,
    cached_nodes: HashMap<NodeId, CachedA11yNode>,
}

impl Default for A11yBridge {
    fn default() -> Self {
        Self::new("CraftUI")
    }
}

impl A11yBridge {
    pub fn new(window_title: impl Into<String>) -> Self {
        Self {
            id_lookup: HashMap::new(),
            window_title: window_title.into(),
            initialized: false,
            last_root: None,
            last_focused: None,
            cached_nodes: HashMap::new(),
        }
    }

    pub fn set_window_title(&mut self, title: impl Into<String>) {
        self.window_title = title.into();
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Force the next incremental update to emit a full tree snapshot (e.g. on `InitialTreeRequested`).
    pub fn invalidate(&mut self) {
        self.initialized = false;
        self.cached_nodes.clear();
    }

    #[inline]
    pub fn to_ak_id(node: NodeId) -> accesskit::NodeId {
        accesskit::NodeId(node.data().as_ffi())
    }

    fn build_ak_node(&self, root: NodeId, snap: &A11ySnapshotNode) -> accesskit::Node {
        let mut ak_node = accesskit::Node::new(if snap.id == root {
            accesskit::Role::Window
        } else {
            snap.meta.role.to_accesskit()
        });

        if snap.id == root {
            ak_node.set_label(self.window_title.clone());
        } else if let Some(ref label) = snap.meta.label {
            ak_node.set_label(label.clone());
        }

        if let Some(ref val) = snap.meta.value {
            ak_node.set_value(val.clone());
        }
        if let Some(num) = snap.meta.numeric_value {
            ak_node.set_numeric_value(num);
        }
        if let Some(min) = snap.meta.min_value {
            ak_node.set_min_numeric_value(min);
        }
        if let Some(max) = snap.meta.max_value {
            ak_node.set_max_numeric_value(max);
        }
        if let Some(checked) = snap.meta.checked {
            ak_node.set_toggled(if checked {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        }
        if snap.meta.disabled {
            ak_node.set_disabled();
        }

        ak_node.set_bounds(accesskit::Rect {
            x0: snap.bounds.x as f64,
            y0: snap.bounds.y as f64,
            x1: snap.bounds.max_x() as f64,
            y1: snap.bounds.max_y() as f64,
        });

        match snap.meta.role {
            A11yRole::Button
            | A11yRole::CheckBox
            | A11yRole::Switch
            | A11yRole::RadioButton
            | A11yRole::Tab
            | A11yRole::MenuItem
            | A11yRole::ComboBox => {
                ak_node.add_action(accesskit::Action::Click);
            }
            A11yRole::Slider => {
                ak_node.add_action(accesskit::Action::Increment);
                ak_node.add_action(accesskit::Action::Decrement);
                ak_node.add_action(accesskit::Action::SetValue);
            }
            A11yRole::TextInput | A11yRole::MultilineTextInput => {
                ak_node.add_action(accesskit::Action::SetValue);
                ak_node.add_action(accesskit::Action::Focus);
            }
            _ => {}
        }
        if snap.meta.focusable {
            ak_node.add_action(accesskit::Action::Focus);
        }

        let children: Vec<accesskit::NodeId> =
            snap.children.iter().copied().map(Self::to_ak_id).collect();
        if !children.is_empty() {
            ak_node.set_children(children);
        }

        ak_node
    }

    /// Build a complete `accesskit::TreeUpdate` from widget snapshots and seed the incremental diff cache.
    pub fn build_tree_update(
        &mut self,
        root: NodeId,
        focused: Option<NodeId>,
        nodes: &[A11ySnapshotNode],
    ) -> accesskit::TreeUpdate {
        self.id_lookup.clear();
        self.cached_nodes.clear();
        let mut ak_nodes = Vec::with_capacity(nodes.len());

        for snap in nodes {
            let raw = snap.id.data().as_ffi();
            self.id_lookup.insert(raw, snap.id);
            let ak_node = self.build_ak_node(root, snap);
            ak_nodes.push((Self::to_ak_id(snap.id), ak_node));
            self.cached_nodes.insert(
                snap.id,
                CachedA11yNode {
                    is_root: snap.id == root,
                    bounds: snap.bounds,
                    meta: snap.meta.clone(),
                    children: snap.children.clone(),
                    window_title: if snap.id == root {
                        self.window_title.clone()
                    } else {
                        String::new()
                    },
                },
            );
        }

        self.initialized = true;
        self.last_root = Some(root);
        self.last_focused = focused;

        let focus_ak = focused
            .map(Self::to_ak_id)
            .unwrap_or_else(|| Self::to_ak_id(root));

        accesskit::TreeUpdate {
            nodes: ak_nodes,
            tree: Some(accesskit::TreeInfo::new(Self::to_ak_id(root))),
            tree_id: accesskit::TreeId::ROOT,
            focus: focus_ak,
        }
    }

    /// Build a minimal incremental `accesskit::TreeUpdate` containing only changed nodes or focus transitions.
    /// Returns `None` when nothing accessibility-relevant changed since the previous update.
    pub fn build_incremental_update(
        &mut self,
        root: NodeId,
        focused: Option<NodeId>,
        nodes: &[A11ySnapshotNode],
    ) -> Option<accesskit::TreeUpdate> {
        if !self.initialized || self.last_root != Some(root) {
            return Some(self.build_tree_update(root, focused, nodes));
        }

        let mut ak_nodes = Vec::new();
        let mut seen = HashSet::with_capacity(nodes.len());

        for snap in nodes {
            seen.insert(snap.id);
            let raw = snap.id.data().as_ffi();
            self.id_lookup.insert(raw, snap.id);

            let next_cache = CachedA11yNode {
                is_root: snap.id == root,
                bounds: snap.bounds,
                meta: snap.meta.clone(),
                children: snap.children.clone(),
                window_title: if snap.id == root {
                    self.window_title.clone()
                } else {
                    String::new()
                },
            };

            if self.cached_nodes.get(&snap.id) != Some(&next_cache) {
                ak_nodes.push((Self::to_ak_id(snap.id), self.build_ak_node(root, snap)));
                self.cached_nodes.insert(snap.id, next_cache);
            }
        }

        self.cached_nodes.retain(|id, _| seen.contains(id));
        self.id_lookup.retain(|_, id| seen.contains(id));

        let focus_changed = self.last_focused != focused;
        self.last_focused = focused;

        if ak_nodes.is_empty() && !focus_changed {
            return None;
        }

        let focus_ak = focused
            .map(Self::to_ak_id)
            .unwrap_or_else(|| Self::to_ak_id(root));

        Some(accesskit::TreeUpdate {
            nodes: ak_nodes,
            tree: None,
            tree_id: accesskit::TreeId::ROOT,
            focus: focus_ak,
        })
    }

    /// Translate an incoming OS screen-reader action into a CraftUI [`A11yAction`].
    pub fn translate_action(&self, req: &accesskit::ActionRequest) -> Option<A11yAction> {
        let node = *self.id_lookup.get(&req.target_node.0)?;
        match req.action {
            accesskit::Action::Click => Some(A11yAction::Click(node)),
            accesskit::Action::Focus => Some(A11yAction::Focus(node)),
            accesskit::Action::Increment => Some(A11yAction::Increment(node)),
            accesskit::Action::Decrement => Some(A11yAction::Decrement(node)),
            accesskit::Action::ScrollIntoView => Some(A11yAction::ScrollIntoView(node)),
            accesskit::Action::SetValue => match &req.data {
                Some(accesskit::ActionData::Value(s)) => {
                    Some(A11yAction::SetValue(node, s.to_string()))
                }
                Some(accesskit::ActionData::NumericValue(v)) => {
                    Some(A11yAction::SetNumericValue(node, *v))
                }
                _ => None,
            },
            _ => None,
        }
    }
}
