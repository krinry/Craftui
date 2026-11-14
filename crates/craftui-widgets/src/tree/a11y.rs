use super::*;

impl WidgetTree {

    /// Reset incremental AccessKit cache so the next [`build_a11y_update`](Self::build_a11y_update)
    /// emits a full initial tree snapshot.
    pub fn invalidate_a11y(&mut self) {
        self.a11y_bridge.invalidate();
    }

    /// Collect incremental AccessKit tree update for screen readers.
    /// Returns `None` when no accessibility node or focus state changed since the last update.
    pub fn build_a11y_update(&mut self) -> Option<craftui_a11y::accesskit::TreeUpdate> {
        let root = self.root?;
        let mut snaps = Vec::new();
        self.collect_a11y_recursive(root, &mut snaps, &mut HashSet::new());
        self.a11y_bridge
            .build_incremental_update(root, self.focused, &snaps)
    }

    pub(crate) fn collect_a11y_recursive(
        &self,
        id: NodeId,
        out: &mut Vec<A11ySnapshotNode>,
        seen: &mut HashSet<NodeId>,
    ) {
        if !seen.insert(id) {
            return;
        }
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        let is_collapsed = matches!(
            &node.kind,
            WidgetKind::Accordion { expanded, .. } if !expanded.get()
        ) || matches!(
            &node.kind,
            WidgetKind::Drawer { open, .. } if !open.get()
        );
        let child_ids: Vec<NodeId> = if is_collapsed {
            Vec::new()
        } else {
            node.children
                .iter()
                .copied()
                .filter(|&cid| {
                    self.nodes
                        .get(cid)
                        .is_some_and(|cn| !cn.layout.hidden && !cn.taffy_hidden_by_parent)
                })
                .collect()
        };
        let bounds = self.layout_engine.rect(id);
        out.push(A11ySnapshotNode {
            id,
            bounds,
            meta: node.a11y.clone(),
            children: child_ids.clone(),
        });
        for c in child_ids {
            self.collect_a11y_recursive(c, out, seen);
        }
    }

    pub fn handle_a11y_action(&mut self, req: &craftui_a11y::accesskit::ActionRequest) -> bool {
        let Some(action) = self.a11y_bridge.translate_action(req) else {
            return false;
        };
        match action {
            A11yAction::Click(id) => {
                let c = self.node_visual_rect(id).center();
                self.activate_tap(id, c, false);
                true
            }
            A11yAction::Focus(id) => {
                self.focused = Some(id);
                self.reset_caret_blink(craftui_core::monotonic_ms());
                true
            }
            A11yAction::SetValue(id, v) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::TextInput {
                        editor,
                        bound_signal,
                        ..
                    } = &mut n.kind
                {
                    editor.sync_external(&v);
                    n.a11y.value = Some(v.clone());
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(sig) = bound_signal {
                        sig.set(v);
                    }
                    self.reset_caret_blink(craftui_core::monotonic_ms());
                    return true;
                }
                false
            }
            A11yAction::SetNumericValue(id, v) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    value.set((v as f32).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::Increment(id) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v + step).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::Decrement(id) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v - step).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::ScrollIntoView(_) => false,
        }
    }

    pub(crate) fn node_path(&self, target: NodeId) -> Option<Vec<usize>> {
        let root = self.root?;
        let mut rev = Vec::new();
        let mut cur = target;
        while cur != root {
            let parent = self.nodes.get(cur)?.parent?;
            let pnode = self.nodes.get(parent)?;
            let idx = pnode.children.iter().position(|&c| c == cur)?;
            rev.push(idx);
            cur = parent;
        }
        rev.reverse();
        Some(rev)
    }

    pub(crate) fn node_at_path(&self, path: &[usize]) -> Option<NodeId> {
        let mut cur = self.root?;
        for &idx in path {
            cur = *self.nodes.get(cur)?.children.get(idx)?;
        }
        Some(cur)
    }

}
