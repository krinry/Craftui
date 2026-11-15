use super::*;

impl WidgetTree {
    pub fn rect(&self, node: NodeId) -> Rect {
        self.layout_engine.rect(node)
    }

    /// Whether a node is mounted and shown: neither it nor any ancestor is
    /// hidden by `visible`, a false `if` condition, a closed overlay or a
    /// collapsed accordion.
    pub fn is_visible(&self, node: NodeId) -> bool {
        !self.is_effectively_hidden(node)
    }

    pub(crate) fn is_effectively_hidden(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(nid) = cur {
            let Some(node) = self.nodes.get(nid) else {
                return true;
            };
            if node.layout.hidden || node.taffy_hidden_by_parent {
                return true;
            }
            if node
                .visible_when
                .as_ref()
                .is_some_and(|v| craftui_reactive::untrack(|| !v.get()))
            {
                return true;
            }
            match &node.kind {
                WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                    if craftui_reactive::untrack(|| !open.get()) =>
                {
                    return true;
                }
                WidgetKind::Accordion { expanded, .. }
                    if nid != id && craftui_reactive::untrack(|| !expanded.get()) =>
                {
                    return true;
                }
                _ => {}
            }
            cur = node.parent;
        }
        false
    }

    // === Frame Step: Animations, Dirty Sync, Layout, Virtualization, Paint, A11y ===

    /// Advance spring animations, inertial scroll flings, and return whether another animation frame is needed.
    pub fn tick_animations(&mut self, dt_sec: f32) -> bool {
        let mut active = false;

        if let Some((from, to, mut spring)) = self.theme_spring.take() {
            let still_running = spring.step(dt_sec);
            let lerped = from.lerp(&to, spring.value);
            self.theme_signal.set(lerped);
            if still_running {
                self.theme_spring = Some((from, to, spring));
                active = true;
            }
        }

        let anim_ids: Vec<NodeId> = self.node_anims.keys().collect();
        for id in anim_ids {
            if let Some(anim) = self.node_anims.get_mut(id) {
                let (changed, still_running) = anim.step(dt_sec);
                if changed && let Some(n) = self.nodes.get_mut(id) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                if still_running {
                    active = true;
                }
            }
        }

        let ctrl_ids: Vec<NodeId> = self.control_anims.keys().collect();
        for id in ctrl_ids {
            if let Some(anim) = self.control_anims.get_mut(id) {
                let changed = anim.step(dt_sec);
                if changed && let Some(n) = self.nodes.get_mut(id) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                if anim.is_animating() {
                    active = true;
                }
            }
        }

        let ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in ids {
            let rect = self.layout_engine.rect(id);
            let effectively_hidden = self.is_effectively_hidden(id);
            let physics = self.scroll_physics;
            if let Some(node) = self.nodes.get_mut(id) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (*content_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            if *offset <= 0.0 || *offset >= max_off {
                                *velocity = 0.0;
                            }
                            node.dirty |= DirtyFlags::PAINT;
                            if running && *velocity != 0.0 {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let total_rows = item_count.get().div_ceil((*columns).max(1));
                            let total_extent = total_rows as f32 * *item_extent;
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (total_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            if running {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::ProgressIndicator { progress, phase }
                        if !effectively_hidden && progress.get().is_none() =>
                    {
                        *phase = (*phase + dt_sec * 1.4) % 1.0;
                        node.dirty |= DirtyFlags::PAINT;
                        active = true;
                    }
                    WidgetKind::Spinner { phase, .. } if !effectively_hidden => {
                        *phase = (*phase + dt_sec * 1.25) % 1.0;
                        node.dirty |= DirtyFlags::PAINT;
                        active = true;
                    }
                    _ => {}
                }
            }
        }

        active
    }

    /// Returns `true` if any transition, control spring, theme transition, fling, or spinner is currently running.
    pub fn has_active_animations(&self) -> bool {
        if self.theme_spring.is_some() {
            return true;
        }
        if self.node_anims.values().any(NodeAnimState::is_animating) {
            return true;
        }
        if self
            .control_anims
            .values()
            .any(ControlVisualAnim::is_animating)
        {
            return true;
        }
        self.nodes.iter().any(|(id, node)| match &node.kind {
            WidgetKind::ScrollView { velocity, .. } | WidgetKind::VirtualList { velocity, .. } => {
                velocity.abs() > 0.0
            }
            WidgetKind::ProgressIndicator { progress, .. } => {
                !self.is_effectively_hidden(id) && progress.get().is_none()
            }
            WidgetKind::Spinner { .. } => !self.is_effectively_hidden(id),
            _ => false,
        })
    }
}
