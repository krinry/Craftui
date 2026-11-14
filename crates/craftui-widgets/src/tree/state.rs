use super::*;

impl WidgetTree {

    /// Capture transient focus, scroll offsets, text cursor positions, and open selects for hot-reload preservation.
    pub fn capture_ui_state(&self) -> TreeUiSnapshot {
        let mut snap = TreeUiSnapshot {
            focused_path: self.focused.and_then(|id| self.node_path(id)),
            ..Default::default()
        };
        let Some(root) = self.root else {
            return snap;
        };
        let mut stack = vec![(root, Vec::<usize>::new())];
        while let Some((id, path)) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            match &node.kind {
                WidgetKind::ScrollView { offset, .. }
                | WidgetKind::VirtualList { offset, .. }
                | WidgetKind::DataTable {
                    scroll_offset: offset,
                    ..
                } => {
                    if *offset > 0.0 {
                        snap.scroll_offsets.push((path.clone(), *offset));
                    }
                }
                WidgetKind::TextInput { editor, .. } => {
                    snap.editor_states
                        .push((path.clone(), editor.cursor, editor.anchor));
                }
                WidgetKind::Select { open: true, .. } => {
                    snap.select_open_paths.push(path.clone());
                }
                _ => {}
            }
            for (idx, &child) in node.children.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(idx);
                stack.push((child, child_path));
            }
        }
        snap
    }

    /// Restore transient UI state captured before a hot-reload subtree replacement.
    pub fn restore_ui_state(&mut self, snap: &TreeUiSnapshot) {
        if let Some(ref fpath) = snap.focused_path
            && let Some(fid) = self.node_at_path(fpath)
            && self
                .nodes
                .get(fid)
                .is_some_and(|n| n.a11y.focusable && !n.is_disabled())
        {
            self.focused = Some(fid);
            if self.focused_is_text_input() {
                self.reset_caret_blink(craftui_core::monotonic_ms());
            }
        }
        for (path, off) in &snap.scroll_offsets {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
            {
                match &mut n.kind {
                    WidgetKind::ScrollView { offset, .. }
                    | WidgetKind::VirtualList { offset, .. }
                    | WidgetKind::DataTable {
                        scroll_offset: offset,
                        ..
                    } => {
                        *offset = (*off).max(0.0);
                        n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                    }
                    _ => {}
                }
            }
        }
        let clamp_utf8 = |s: &str, pos: usize| -> usize {
            let mut p = pos.min(s.len());
            while p > 0 && !s.is_char_boundary(p) {
                p -= 1;
            }
            p
        };
        for (path, cursor, anchor) in &snap.editor_states {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
                && let WidgetKind::TextInput { editor, .. } = &mut n.kind
            {
                editor.cursor = clamp_utf8(&editor.text, *cursor);
                editor.anchor = clamp_utf8(&editor.text, *anchor);
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        for path in &snap.select_open_paths {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
                && let WidgetKind::Select { open, .. } = &mut n.kind
            {
                *open = true;
                n.dirty |= DirtyFlags::PAINT;
            }
        }
    }
}
