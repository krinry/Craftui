use super::*;

impl WidgetTree {
    /// Update tree window metrics and notify reactive listeners.
    pub fn set_metrics(&mut self, metrics: WindowMetrics) {
        self.metrics = metrics;
        self.metrics_signal.set(metrics);
    }

    /// Pull dirty flags from `craftui-reactive` and update layout + virtualized windows.
    pub fn layout(&mut self, metrics: WindowMetrics) -> usize {
        self.metrics = metrics;
        // Platform runners assign `metrics` directly before the first frame, so
        // comparing against the field alone would leave the reactive copy at
        // its default and `Window.*` bindings would see the wrong size.
        if self.metrics_signal.get_untracked() != metrics {
            self.metrics_signal.set(metrics);
        }
        let dirty_updates = drain_dirty_nodes();
        let dirty_count = dirty_updates.len();

        let mut drained_masks: HashMap<NodeId, DirtyFlags> = HashMap::new();
        for (id, flags) in dirty_updates {
            if let Some(node) = self.nodes.get_mut(id) {
                node.dirty |= flags;
                drained_masks
                    .entry(id)
                    .and_modify(|f| *f |= flags)
                    .or_insert(flags);
                if flags.contains(DirtyFlags::LAYOUT) {
                    self.layout_engine.mark_dirty(id);
                }
            }
        }

        // Rebuild any reactive dynamic_slot / reactive_for subtrees before syncing properties/solving layout
        self.sync_dynamic_slots();
        for (id, flags) in drain_dirty_nodes() {
            if let Some(node) = self.nodes.get_mut(id) {
                node.dirty |= flags;
                drained_masks
                    .entry(id)
                    .and_modify(|f| *f |= flags)
                    .or_insert(flags);
                if flags.contains(DirtyFlags::LAYOUT) {
                    self.layout_engine.mark_dirty(id);
                }
            }
        }
        self.sync_dynamic_props(&drained_masks);

        // Sync dynamic properties before layout
        let all_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in all_ids {
            if let Some(node) = self.nodes.get_mut(id) {
                let mut layout_changed = false;
                if let Some(ref vis) = node.visible_when {
                    let hidden = !vis.get();
                    if node.layout.hidden != hidden {
                        node.layout.hidden = hidden;
                        layout_changed = true;
                    }
                }
                let node_disabled = node.disabled.as_ref().is_some_and(|d| d.get());
                node.a11y.disabled = node_disabled;
                match &mut node.kind {
                    WidgetKind::Text { content, style } => {
                        if node.dirty.contains(DirtyFlags::TEXT)
                            || node.dirty.contains(DirtyFlags::LAYOUT)
                        {
                            node.cached_text = content.get();
                            node.cached_style = style.get();
                            node.a11y.label = Some(node.cached_text.clone());
                        }
                    }
                    WidgetKind::RichText { content, .. } => {
                        if node.dirty.contains(DirtyFlags::TEXT)
                            || node.dirty.contains(DirtyFlags::LAYOUT)
                        {
                            node.cached_text = content.get().plain_text();
                            node.a11y.label = Some(node.cached_text.clone());
                        }
                    }
                    WidgetKind::Button {
                        label, disabled, ..
                    } => {
                        let txt = label.get();
                        node.cached_text = txt.clone();
                        node.a11y.label = Some(txt);
                        node.a11y.disabled = node_disabled || disabled.get();
                    }
                    WidgetKind::TextInput {
                        editor,
                        bound_signal: Some(sig),
                        ..
                    } => {
                        let next = sig.get_untracked();
                        if editor.text != next {
                            editor.sync_external(&next);
                            node.a11y.value = Some(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        }
                    }
                    WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                        node.a11y.checked = Some(checked.get_untracked());
                    }
                    WidgetKind::Slider { value, .. } => {
                        node.a11y.numeric_value = Some(value.get_untracked() as f64);
                    }
                    WidgetKind::Tabs { active, .. } => {
                        node.a11y.numeric_value = Some(active.get_untracked() as f64);
                    }
                    WidgetKind::SegmentedControl { selected, .. }
                    | WidgetKind::BottomNavBar { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::FloatingActionButton {
                        label,
                        icon,
                        disabled,
                        ..
                    } => {
                        let l = label
                            .as_ref()
                            .map(|s| s.get())
                            .unwrap_or_else(|| icon.get());
                        node.cached_text = l.clone();
                        node.a11y.label = Some(l);
                        node.a11y.disabled = node_disabled || disabled.get();
                    }
                    WidgetKind::Accordion {
                        title, expanded, ..
                    } => {
                        node.a11y.label = Some(title.get());
                        node.a11y.checked = Some(expanded.get_untracked());
                    }
                    WidgetKind::DataTable { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::Select {
                        options, selected, ..
                    } => {
                        let opts = options.get();
                        let idx = selected.get_untracked();
                        node.a11y.value = opts.get(idx).cloned();
                    }
                    WidgetKind::RadioGroup { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::Tooltip { text } => {
                        node.a11y.label = Some(text.get());
                    }
                    WidgetKind::Badge { label, .. } => {
                        node.a11y.label = Some(label.get());
                    }
                    WidgetKind::ToastHost { items, .. } => {
                        let list = items.get();
                        node.a11y.value =
                            list.last().map(|t| format!("{}: {}", t.title, t.message));
                    }
                    WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. } => {
                        let should_show = open.get();
                        let hidden = !should_show;
                        if node.layout.hidden != hidden {
                            node.layout.hidden = hidden;
                            layout_changed = true;
                        }
                    }
                    WidgetKind::Custom(cw) => {
                        if let Ok(guard) = cw.try_borrow() {
                            node.a11y = guard.a11y();
                        }
                    }
                    _ => {}
                }
                if layout_changed {
                    let st = Self::effective_taffy_layout(node);
                    self.layout_engine.upsert_node(id, st);
                    self.layout_engine.mark_dirty(id);
                }
            }
            self.sync_control_anim_target(id);
            self.sync_node_anim_targets(id);
        }

        let Some(root) = self.root else {
            return dirty_count;
        };

        self.sync_accordions();

        // Pass 1: solve layout
        self.solve_taffy(root, metrics);

        // Pass 2: recycle VirtualList / VirtualGrid visible items & sync SplitPane ratios
        let virtual_changed = self.sync_virtual_lists();
        let split_changed = self.sync_split_panes();
        if virtual_changed || split_changed {
            self.solve_taffy(root, metrics);
        }

        // Update ScrollView content_extent
        let scroll_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in scroll_ids {
            let is_scroll = self
                .nodes
                .get(id)
                .is_some_and(|n| matches!(n.kind, WidgetKind::ScrollView { .. }));
            if is_scroll {
                let rect = self.layout_engine.rect(id);
                let mut stack = self
                    .nodes
                    .get(id)
                    .map(|n| n.children.clone())
                    .unwrap_or_default();
                let mut max_bottom = 0.0f32;
                let mut max_right = 0.0f32;
                while let Some(c) = stack.pop() {
                    let cr = self.layout_engine.rect(c);
                    max_bottom = max_bottom.max(cr.max_y() - rect.y);
                    max_right = max_right.max(cr.max_x() - rect.x);
                    if let Some(cn) = self.nodes.get(c) {
                        stack.extend(cn.children.iter().copied());
                    }
                }
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::ScrollView {
                        axis,
                        offset,
                        content_extent,
                        ..
                    } = &mut n.kind
                {
                    *content_extent = match axis {
                        Axis::Vertical => max_bottom,
                        Axis::Horizontal => max_right,
                    };
                    let view = match axis {
                        Axis::Vertical => rect.height,
                        Axis::Horizontal => rect.width,
                    };
                    let max_off = (*content_extent - view).max(0.0);
                    *offset = offset.clamp(0.0, max_off);
                }
            }
        }

        dirty_count
    }

    pub(crate) fn solve_taffy(&mut self, root: NodeId, metrics: WindowMetrics) {
        let nodes = &self.nodes;
        let text_engine = &mut self.text_engine;
        let theme = self.theme_signal.get_untracked();
        let safe_area = metrics.effective_safe_area();

        self.layout_engine.compute_layout(
            root,
            metrics.logical_size.width,
            metrics.logical_size.height,
            safe_area,
            metrics.scale_factor,
            self.direction,
            |node_id, constraints| {
                let Some(node) = nodes.get(node_id) else {
                    return Size::ZERO;
                };
                match &node.kind {
                    WidgetKind::Divider {
                        axis, thickness, ..
                    } => match axis {
                        Axis::Horizontal => Size::new(100.0, *thickness),
                        Axis::Vertical => Size::new(*thickness, 24.0),
                    },
                    WidgetKind::Text { content, style } => {
                        let txt = if node.cached_text.is_empty() {
                            content.get()
                        } else {
                            node.cached_text.clone()
                        };
                        let mut st = style.get();
                        if st.color == Color::TRANSPARENT {
                            st.color = theme.text_primary;
                        }
                        let max_w = if constraints.max_width.is_finite() {
                            Some(constraints.max_width)
                        } else {
                            None
                        };
                        text_engine.measure(&txt, &st, max_w)
                    }
                    WidgetKind::RichText { content, .. } => {
                        let max_w = constraints
                            .max_width
                            .is_finite()
                            .then_some(constraints.max_width);
                        text_engine.measure_rich(&content.get(), max_w)
                    }
                    WidgetKind::Icon { size, .. } => Size::new(*size, *size),
                    WidgetKind::Button { label, .. } => {
                        let txt = label.get();
                        if builtin_icon_svg(&txt).is_some() {
                            Size::new(20.0, 20.0)
                        } else {
                            let st = TextStyle::default().with_size(15.0).with_weight(600.0);
                            text_engine.measure(&txt, &st, None)
                        }
                    }
                    WidgetKind::Checkbox { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(26.0 + s.width, s.height.max(22.0))
                    }
                    WidgetKind::Switch { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(50.0 + s.width, s.height.max(26.0))
                    }
                    WidgetKind::Slider { label, .. } => {
                        Size::new(160.0, if label.is_empty() { 32.0 } else { 48.0 })
                    }
                    WidgetKind::TextInput { multiline, .. } => {
                        Size::new(180.0, if *multiline { 104.0 } else { 40.0 })
                    }
                    WidgetKind::Tabs { tabs, .. } => {
                        let list = tabs.get();
                        let total_w: f32 = list
                            .iter()
                            .map(|t| {
                                text_engine
                                    .measure(t, &TextStyle::default().with_size(14.0), None)
                                    .width
                                    + 28.0
                            })
                            .sum();
                        Size::new(total_w.max(160.0), 40.0)
                    }
                    WidgetKind::SegmentedControl { items, .. } => {
                        let list = items.get();
                        let total_w: f32 = list
                            .iter()
                            .map(|t| {
                                text_engine
                                    .measure(t, &TextStyle::default().with_size(13.5), None)
                                    .width
                                    + 24.0
                            })
                            .sum();
                        Size::new(total_w.max(140.0), 36.0)
                    }
                    WidgetKind::BottomNavBar { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            360.0
                        };
                        Size::new(w, 64.0)
                    }
                    WidgetKind::FloatingActionButton { label, icon, .. } => {
                        if let Some(lbl) = label {
                            let l = lbl.get();
                            let i = icon.get();
                            let st = TextStyle::default().with_size(14.0).with_weight(600.0);
                            let isz = text_engine.measure(&i, &st, None);
                            let lsz = text_engine.measure(&l, &st, None);
                            Size::new((isz.width + lsz.width + 36.0).max(96.0), 52.0)
                        } else {
                            Size::new(56.0, 56.0)
                        }
                    }
                    WidgetKind::HtmlView { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            400.0
                        };
                        let h = if constraints.max_height.is_finite() {
                            constraints.max_height
                        } else {
                            300.0
                        };
                        Size::new(w, h)
                    }
                    WidgetKind::Accordion {
                        title, expanded, ..
                    } => {
                        let title_w = text_engine
                            .measure(
                                &title.get(),
                                &TextStyle::default().with_size(14.0).with_weight(600.0),
                                None,
                            )
                            .width;
                        let h = if expanded.get_untracked() { 0.0 } else { 44.0 };
                        Size::new((title_w + 52.0).max(180.0), h)
                    }
                    WidgetKind::DataTable {
                        columns,
                        rows,
                        row_height,
                        ..
                    } => {
                        let col_count = columns.get().len().max(1);
                        let row_count = rows.get().len();
                        let w = (col_count as f32 * 120.0).max(240.0);
                        let h = 38.0 + (row_count as f32 * *row_height).min(320.0).max(*row_height);
                        Size::new(w, h)
                    }
                    WidgetKind::Select { .. } => Size::new(180.0, 40.0),
                    WidgetKind::RadioGroup { options, axis, .. } => {
                        let opts = options.get();
                        let count = opts.len().max(1);
                        let st = TextStyle::default().with_size(14.0);
                        match axis {
                            Axis::Vertical => {
                                let max_w = opts
                                    .iter()
                                    .map(|o| 28.0 + text_engine.measure(o, &st, None).width)
                                    .fold(100.0f32, f32::max);
                                Size::new(max_w, count as f32 * 30.0)
                            }
                            Axis::Horizontal => {
                                let total_w: f32 = opts
                                    .iter()
                                    .map(|o| 36.0 + text_engine.measure(o, &st, None).width)
                                    .sum();
                                Size::new(total_w.max(120.0), 32.0)
                            }
                        }
                    }
                    WidgetKind::ProgressIndicator { .. } => Size::new(120.0, 8.0),
                    WidgetKind::Spinner { size, .. } => Size::new(*size, *size),
                    WidgetKind::Badge { label, icon, .. } => {
                        let txt = label.get();
                        let st = TextStyle::default().with_size(12.0).with_weight(600.0);
                        let measured = text_engine.measure(&txt, &st, None);
                        let icon_w = if icon.as_ref().is_some_and(|i| !i.get().is_empty()) {
                            16.0
                        } else {
                            0.0
                        };
                        Size::new(measured.width + icon_w, measured.height.max(16.0))
                    }
                    WidgetKind::ListTile { subtitle, .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            320.0
                        };
                        let h = if subtitle.is_some() { 72.0 } else { 56.0 };
                        Size::new(w, h)
                    }
                    WidgetKind::Chip { label, icon, .. } => {
                        let txt = label.get();
                        let st = TextStyle::default().with_size(13.0).with_weight(500.0);
                        let measured = text_engine.measure(&txt, &st, None);
                        let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
                        Size::new((measured.width + icon_w + 24.0).max(48.0), 32.0)
                    }
                    WidgetKind::AppBar { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            360.0
                        };
                        Size::new(w, 64.0)
                    }
                    WidgetKind::Drawer { width, .. } => {
                        let h = if constraints.max_height.is_finite() {
                            constraints.max_height
                        } else {
                            800.0
                        };
                        Size::new(*width, h)
                    }
                    WidgetKind::Custom(cw) => {
                        if let Ok(mut guard) = cw.try_borrow_mut() {
                            guard.measure(constraints, text_engine, &theme)
                        } else {
                            Size::new(100.0, 40.0)
                        }
                    }
                    _ => Size::ZERO,
                }
            },
        );
    }

    pub(crate) fn sync_virtual_lists(&mut self) -> bool {
        let mut changed = false;
        let v_ids: Vec<NodeId> = self.virtual_hosts.keys().collect();

        for host_id in v_ids {
            let rect = self.layout_engine.rect(host_id);
            let (axis, total_count, cols, extent, off) = {
                let Some(node) = self.nodes.get_mut(host_id) else {
                    continue;
                };
                let WidgetKind::VirtualList {
                    axis,
                    columns,
                    item_extent,
                    item_count,
                    offset,
                    ..
                } = &mut node.kind
                else {
                    continue;
                };

                let axis = *axis;
                let total_count = item_count.get();
                let cols = (*columns).max(1);
                let extent = *item_extent;
                let viewport = match axis {
                    Axis::Vertical => rect.height.max(extent),
                    Axis::Horizontal => rect.width.max(extent),
                };
                let total_rows = total_count.div_ceil(cols);
                let max_offset = (total_rows as f32 * extent - viewport).max(0.0);
                let clamped_off = offset.clamp(0.0, max_offset);
                *offset = clamped_off;
                (axis, total_count, cols, extent, clamped_off)
            };
            let viewport = match axis {
                Axis::Vertical => rect.height.max(extent),
                Axis::Horizontal => rect.width.max(extent),
            };
            let overscan_rows = 2usize;
            let first_row =
                ((off / extent).floor() as isize - overscan_rows as isize).max(0) as usize;
            let last_row = (((off + viewport) / extent).ceil() as usize + overscan_rows)
                .min(total_count.div_ceil(cols).saturating_sub(1));

            let first_idx = (first_row * cols).min(total_count);
            let last_idx = ((last_row + 1) * cols).min(total_count);
            let new_range = if total_count == 0 {
                None
            } else {
                Some((first_idx, last_idx))
            };

            let needs_rebuild = self
                .virtual_hosts
                .get(host_id)
                .is_some_and(|h| h.last_range != new_range);
            if !needs_rebuild {
                continue;
            }
            changed = true;

            let Some(mut host) = self.virtual_hosts.remove(host_id) else {
                continue;
            };
            host.last_range = new_range;

            // Dispose out-of-window items
            let mut to_remove = Vec::new();
            for (&idx, &(item_node, item_scope)) in &host.active_items {
                let keep = new_range.is_some_and(|(s, e)| idx >= s && idx < e);
                if !keep {
                    to_remove.push((idx, item_node, item_scope));
                }
            }
            for (idx, item_node, _item_scope) in to_remove {
                host.active_items.remove(&idx);
                self.remove_subtree(item_node);
            }

            let host_scope = self
                .nodes
                .get(host_id)
                .map(|n| n.scope)
                .unwrap_or(self.root_scope);
            let mut ordered_children = Vec::new();

            if let Some((start, end)) = new_range {
                let col_w = if cols > 1 {
                    (rect.width / cols as f32).max(20.0)
                } else {
                    rect.width.max(20.0)
                };
                for idx in start..end {
                    let row = idx / cols;
                    let col = idx % cols;
                    let item_node = if let Some(&(existing, _)) = host.active_items.get(&idx) {
                        existing
                    } else {
                        let item_scope = host_scope.child_scope();
                        let created = (host.factory)(item_scope, idx, self);
                        host.active_items.insert(idx, (created, item_scope));
                        created
                    };

                    // Position each recycled item absolutely at its virtualized slot
                    let mut st = self
                        .nodes
                        .get(item_node)
                        .map(|n| n.layout.clone())
                        .unwrap_or_default();
                    st.absolute = true;
                    st.use_inset = true;
                    match axis {
                        Axis::Vertical => {
                            st.inset = Insets::new(
                                row as f32 * extent - off,
                                0.0,
                                0.0,
                                col as f32 * col_w,
                            );
                            st.width = Length::Dp(col_w);
                            st.height = Length::Dp(extent);
                        }
                        Axis::Horizontal => {
                            st.inset = Insets::new(0.0, 0.0, 0.0, idx as f32 * extent - off);
                            st.width = Length::Dp(extent);
                            st.height = Length::Dp(rect.height.max(20.0));
                        }
                    }
                    self.set_layout(item_node, st);
                    ordered_children.push(item_node);
                }
            }

            self.set_children(host_id, ordered_children);
            self.virtual_hosts.insert(host_id, host);
        }

        changed
    }

    pub(crate) fn split_divider_geometry(
        inner_rect: Rect,
        axis: Axis,
        ratio: f32,
        min_first: f32,
        min_second: f32,
        divider_size: f32,
    ) -> (f32, f32, Rect) {
        let div = divider_size.max(4.0);
        let total = match axis {
            Axis::Horizontal => inner_rect.width,
            Axis::Vertical => inner_rect.height,
        };
        let usable = (total - div).max(0.0);
        let min_f = min_first.max(0.0).min(usable);
        let max_f = (usable - min_second.max(0.0)).max(min_f);
        let first_px = (usable * ratio.clamp(0.0, 1.0)).clamp(min_f, max_f);
        let second_px = (usable - first_px).max(0.0);
        let div_rect = match axis {
            Axis::Horizontal => Rect::new(
                inner_rect.x + first_px,
                inner_rect.y,
                div,
                inner_rect.height,
            ),
            Axis::Vertical => {
                Rect::new(inner_rect.x, inner_rect.y + first_px, inner_rect.width, div)
            }
        };
        (first_px, second_px, div_rect)
    }

    pub(crate) fn sync_split_panes(&mut self) -> bool {
        let mut changed = false;
        let split_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .filter_map(|(id, n)| matches!(n.kind, WidgetKind::SplitPane { .. }).then_some(id))
            .collect();

        for id in split_ids {
            let rect = self.layout_engine.rect(id);
            let Some((axis, ratio_val, min_first, min_second, divider_size, c0, c1, pad)) =
                self.nodes.get(id).and_then(|n| {
                    if n.layout.hidden {
                        return None;
                    }
                    if let WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        ..
                    } = &n.kind
                        && n.children.len() >= 2
                    {
                        Some((
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            *divider_size,
                            n.children[0],
                            n.children[1],
                            n.layout.padding,
                        ))
                    } else {
                        None
                    }
                })
            else {
                continue;
            };

            let inner = rect.inset(pad);
            if inner.width <= 0.0 && inner.height <= 0.0 {
                continue;
            }
            let (first_px, second_px, _) = Self::split_divider_geometry(
                inner,
                axis,
                ratio_val,
                min_first,
                min_second,
                divider_size,
            );

            for (cid, target_dp) in [(c0, first_px), (c1, second_px)] {
                if let Some(cn) = self.nodes.get_mut(cid) {
                    let mut st = cn.layout.clone();
                    let mut child_changed = false;
                    let want = Length::Dp(target_dp);
                    match axis {
                        Axis::Horizontal => {
                            let diff = match st.width {
                                Length::Dp(a) => (a - target_dp).abs() > 0.25,
                                _ => true,
                            };
                            if diff {
                                st.width = want;
                                child_changed = true;
                            }
                        }
                        Axis::Vertical => {
                            let diff = match st.height {
                                Length::Dp(a) => (a - target_dp).abs() > 0.25,
                                _ => true,
                            };
                            if diff {
                                st.height = want;
                                child_changed = true;
                            }
                        }
                    }
                    if st.flex_grow != 0.0 {
                        st.flex_grow = 0.0;
                        child_changed = true;
                    }
                    if st.flex_shrink != 0.0 {
                        st.flex_shrink = 0.0;
                        child_changed = true;
                    }
                    if child_changed {
                        cn.layout = st.clone();
                        cn.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        self.layout_engine.upsert_node(cid, st);
                        self.layout_engine.mark_dirty(cid);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub(crate) fn sync_accordions(&mut self) {
        let mut collapsed_set = HashSet::new();
        let mut collapsed_children = HashSet::new();
        let all_ids: Vec<NodeId> = self.nodes.keys().collect();
        for &id in &all_ids {
            if let Some(n) = self.nodes.get(id)
                && let WidgetKind::Accordion { expanded, .. } = &n.kind
                && !expanded.get_untracked()
            {
                collapsed_set.insert(id);
                for &c in &n.children {
                    collapsed_children.insert(c);
                }
            }
        }
        for id in all_ids {
            let want_collapsed = collapsed_set.contains(&id);
            let want_hidden = collapsed_children.contains(&id);
            let mut changed = false;
            let mut eff_opt = None;
            if let Some(n) = self.nodes.get_mut(id) {
                if n.taffy_collapsed_override != want_collapsed {
                    n.taffy_collapsed_override = want_collapsed;
                    changed = true;
                }
                if n.taffy_hidden_by_parent != want_hidden {
                    n.taffy_hidden_by_parent = want_hidden;
                    changed = true;
                }
                if changed {
                    n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                    eff_opt = Some(Self::effective_taffy_layout(n));
                }
            }
            if let Some(eff) = eff_opt {
                self.layout_engine.upsert_node(id, eff);
                self.layout_engine.mark_dirty(id);
            }
        }
    }

    pub(crate) fn data_table_sorted_indices(
        rows: &[Vec<String>],
        sort_column: Option<usize>,
        sort_ascending: bool,
    ) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..rows.len()).collect();
        if let Some(col) = sort_column {
            indices.sort_by(|&a, &b| {
                let sa = rows
                    .get(a)
                    .and_then(|r| r.get(col))
                    .map(String::as_str)
                    .unwrap_or("");
                let sb = rows
                    .get(b)
                    .and_then(|r| r.get(col))
                    .map(String::as_str)
                    .unwrap_or("");
                let ord = match (sa.trim().parse::<f64>(), sb.trim().parse::<f64>()) {
                    (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
                    _ => sa.to_lowercase().cmp(&sb.to_lowercase()),
                };
                if sort_ascending { ord } else { ord.reverse() }
            });
        }
        indices
    }

    pub(crate) fn toast_card_rects(container_rect: Rect, count: usize) -> (Vec<Rect>, Vec<Rect>) {
        let mut cards = Vec::with_capacity(count);
        let mut closes = Vec::with_capacity(count);
        if count == 0 {
            return (cards, closes);
        }
        let card_w = (container_rect.width - 32.0).clamp(180.0, 340.0);
        let card_h = 64.0f32;
        let gap = 10.0f32;
        for i in 0..count {
            let from_bottom = count - 1 - i;
            let y = container_rect.max_y() - 16.0 - card_h - (from_bottom as f32) * (card_h + gap);
            let x = container_rect.max_x() - 16.0 - card_w;
            let card_r = Rect::new(x, y, card_w, card_h);
            let close_r = Rect::new(card_r.max_x() - 28.0, card_r.y + 10.0, 18.0, 18.0);
            cards.push(card_r);
            closes.push(close_r);
        }
        (cards, closes)
    }

    pub(crate) fn select_dropdown_rect(&self, trigger_rect: Rect, option_count: usize) -> Rect {
        let count = option_count.max(1);
        let h = (count as f32 * 34.0 + 8.0).min(240.0);
        let vh = self.metrics.logical_size.height.max(200.0);
        let y = if trigger_rect.max_y() + 4.0 + h <= vh {
            trigger_rect.max_y() + 4.0
        } else {
            (trigger_rect.y - 4.0 - h).max(4.0)
        };
        Rect::new(trigger_rect.x, y, trigger_rect.width.max(140.0), h)
    }

    pub(crate) fn popup_menu_rect(
        &self,
        anchor: Option<Point>,
        item_count: usize,
        container_rect: Rect,
    ) -> Rect {
        let count = item_count.max(1);
        let w = 210.0f32;
        let h = (count as f32 * 36.0 + 8.0).min(320.0);
        let base =
            anchor.unwrap_or_else(|| Point::new(container_rect.x + 24.0, container_rect.y + 48.0));
        let max_x = (self.metrics.logical_size.width - w - 8.0).max(8.0);
        let max_y = (self.metrics.logical_size.height - h - 8.0).max(8.0);
        Rect::new(base.x.clamp(8.0, max_x), base.y.clamp(8.0, max_y), w, h)
    }
}
