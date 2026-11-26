use super::*;

impl WidgetTree {
    /// Hit-test the deepest interactive/visible node under `point`, respecting open dropdowns,
    /// active transforms, scroll offsets, and `hit_slop`.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let root = self.root?;

        // Open Select dropdowns capture hits above normal tree nodes in topmost paint order
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);
        for id in order.into_iter().rev() {
            if let Some(node) = self.nodes.get(id)
                && !node.layout.hidden
                && let WidgetKind::Select {
                    open: true,
                    options,
                    ..
                } = &node.kind
            {
                let tr = self.node_visual_rect(id);
                let screen_tr = self.node_world_transform(id).transform_rect_bounds(tr);
                let dr = self.select_dropdown_rect(screen_tr, options.get().len());
                if dr.contains(point) {
                    return Some(id);
                }
            }
            if let Some(node) = self.nodes.get(id)
                && !node.layout.hidden
                && let WidgetKind::MenuBar {
                    menus,
                    open_menu: Some(idx),
                    ..
                } = &node.kind
            {
                let bar_r = self
                    .node_world_transform(id)
                    .transform_rect_bounds(self.node_visual_rect(id));
                let parsed = crate::pickers::parse_menubar_specs(&menus.get());
                let hrs = crate::pickers::menubar_header_rects(bar_r, &parsed);
                if let (Some(hr), Some((_, items))) = (hrs.get(*idx), parsed.get(*idx))
                    && !items.is_empty()
                {
                    let dr = crate::pickers::menubar_dropdown_rect(*hr, items.len());
                    if dr.contains(point) {
                        return Some(id);
                    }
                }
            }
        }

        self.hit_test_recursive(root, point, Point::ZERO)
    }

    pub(crate) fn hit_test_recursive(
        &self,
        id: NodeId,
        point: Point,
        scroll_offset: Point,
    ) -> Option<NodeId> {
        let node = self.nodes.get(id)?;
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return None;
        }
        match &node.kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return None;
            }
            WidgetKind::Drawer { open, .. } if !open.get() => {
                return None;
            }
            _ => {}
        }

        let base_rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let local_xf = self.node_effective_transform(id, base_rect.center());
        let local_point = if local_xf.is_identity() {
            point
        } else {
            let inv = local_xf.inverse()?;
            inv.transform_point(point)
        };
        let rect = base_rect.inflate(node.box_style.hit_slop, node.box_style.hit_slop);

        if node.box_style.clip && !rect.contains(local_point) {
            return None;
        }

        if let WidgetKind::SplitPane {
            axis,
            ratio,
            min_first,
            min_second,
            divider_size,
            ..
        } = &node.kind
        {
            let inner = base_rect.inset(node.layout.padding);
            let (_, _, div_rect) = Self::split_divider_geometry(
                inner,
                *axis,
                ratio.get_untracked(),
                *min_first,
                *min_second,
                *divider_size,
            );
            if base_rect.contains(local_point) && div_rect.inflate(4.0, 4.0).contains(local_point) {
                return Some(id);
            }
        }

        let mut child_offset = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &node.kind {
            match axis {
                Axis::Vertical => child_offset.y -= *offset,
                Axis::Horizontal => child_offset.x -= *offset,
            }
        }

        let skip_children = matches!(
            &node.kind,
            WidgetKind::Accordion { expanded, .. } if !expanded.get_untracked()
        );
        if !skip_children {
            // Topmost Z-order children first
            for &child in node.children.iter().rev() {
                if let Some(hit) = self.hit_test_recursive(child, local_point, child_offset) {
                    return Some(hit);
                }
            }
        }

        let hit_self = match &node.kind {
            WidgetKind::SplitPane { .. } => {
                rect.contains(local_point)
                    && (node.on_tap.is_some()
                        || node.on_double_tap.is_some()
                        || node.on_long_press.is_some()
                        || node.on_hover.is_some()
                        || node.on_pinch_rotate.is_some())
            }
            WidgetKind::Accordion { .. } => {
                let header_r = Rect::new(
                    base_rect.x,
                    base_rect.y,
                    base_rect.width,
                    44.0_f32.min(base_rect.height),
                )
                .inflate(node.box_style.hit_slop, node.box_style.hit_slop);
                header_r.contains(local_point)
            }
            WidgetKind::ToastHost { items, .. } => {
                let (cards, _) = Self::toast_card_rects(base_rect, items.get().len());
                cards.iter().any(|c| c.contains(local_point))
            }
            _ => rect.contains(local_point) && self.is_interactive(node),
        };
        if hit_self { Some(id) } else { None }
    }

    pub(crate) fn is_interactive(&self, node: &WidgetNode) -> bool {
        node.on_tap.is_some()
            || node.on_double_tap.is_some()
            || node.on_long_press.is_some()
            || node.on_hover.is_some()
            || node.on_pinch_rotate.is_some()
            || matches!(
                node.kind,
                WidgetKind::Button { .. }
                    | WidgetKind::RichText {
                        on_link: Some(_),
                        ..
                    }
                    | WidgetKind::Canvas {
                        on_pointer: Some(_),
                        ..
                    }
                    | WidgetKind::ScrollView { .. }
                    | WidgetKind::VirtualList { .. }
                    | WidgetKind::TextInput { .. }
                    | WidgetKind::Checkbox { .. }
                    | WidgetKind::Switch { .. }
                    | WidgetKind::Slider { .. }
                    | WidgetKind::Tabs { .. }
                    | WidgetKind::SegmentedControl { .. }
                    | WidgetKind::BottomNavBar { .. }
                    | WidgetKind::FloatingActionButton { .. }
                    | WidgetKind::ListTile { .. }
                    | WidgetKind::Chip { .. }
                    | WidgetKind::AppBar { .. }
                    | WidgetKind::Drawer { .. }
                    | WidgetKind::Accordion { .. }
                    | WidgetKind::DataTable { .. }
                    | WidgetKind::Select { .. }
                    | WidgetKind::RadioGroup { .. }
                    | WidgetKind::PopupMenu { .. }
                    | WidgetKind::Tooltip { .. }
                    | WidgetKind::SplitPane { .. }
                    | WidgetKind::ToastHost { .. }
                    | WidgetKind::Overlay { .. }
                    | WidgetKind::DatePicker { .. }
                    | WidgetKind::TimePicker { .. }
                    | WidgetKind::TreeView { .. }
                    | WidgetKind::ReorderableList { .. }
                    | WidgetKind::MenuBar { .. }
                    | WidgetKind::Draggable { .. }
                    | WidgetKind::DropTarget { .. }
                    | WidgetKind::MediaPlayer { .. }
                    | WidgetKind::RichTextEditor { .. }
                    | WidgetKind::Custom(_)
            )
    }

    /// Dispatch a [`UiEvent`] through the gesture arena, scroll physics, keyboard, and IME.
    /// Returns `true` if the UI state/scene should be re-rendered.
    pub fn handle_event(&mut self, event: &UiEvent) -> bool {
        match event {
            UiEvent::Pointer(pe) => self.handle_pointer(pe),
            UiEvent::Scroll(delta) => self.handle_scroll(delta.position, delta.x, delta.y),
            UiEvent::Key(ke) => self.handle_key(ke),
            UiEvent::Ime(ime) => self.handle_ime(ime),
            UiEvent::Pinch { factor, center } => self.dispatch_pinch_rotate(*center, *factor, 0.0),
            UiEvent::Resize {
                width,
                height,
                scale_factor,
            } => {
                self.metrics.logical_size = Size::new(*width, *height);
                self.metrics.scale_factor = *scale_factor;
                self.metrics_signal.set(self.metrics);
                true
            }
            UiEvent::FocusChanged(_) => false,
        }
    }

    pub(crate) fn dispatch_pinch_rotate(
        &mut self,
        center: Point,
        scale_delta: f32,
        angle_delta_rad: f32,
    ) -> bool {
        let mut cursor = self.hit_test(center).or(self.hovered).or(self.root);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(cb) = self.nodes.get(nid).and_then(|n| n.on_pinch_rotate.clone()) {
                if let Ok(mut f) = cb.try_borrow_mut() {
                    f(center, scale_delta, angle_delta_rad);
                }
                return true;
            }
            cursor = parent;
        }
        false
    }

    pub(crate) fn handle_pointer(&mut self, pe: &PointerEvent) -> bool {
        self.last_pointer_pos = Some(pe.position);
        let mut changed = false;
        let hit = self.hit_test(pe.position);

        // Close open Select dropdowns when clicking outside them
        if matches!(pe.phase, PointerPhase::Down) {
            let open_selects: Vec<NodeId> = self
                .nodes
                .iter()
                .filter_map(|(id, n)| {
                    if matches!(n.kind, WidgetKind::Select { open: true, .. }) && hit != Some(id) {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            for sid in open_selects {
                if let Some(n) = self.nodes.get_mut(sid)
                    && let WidgetKind::Select { open, .. } = &mut n.kind
                {
                    *open = false;
                    n.dirty |= DirtyFlags::PAINT;
                    changed = true;
                }
            }

            let open_menus: Vec<NodeId> = self
                .nodes
                .iter()
                .filter_map(|(id, n)| {
                    if matches!(
                        n.kind,
                        WidgetKind::MenuBar {
                            open_menu: Some(_),
                            ..
                        }
                    ) && hit != Some(id)
                    {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            for mid in open_menus {
                if let Some(n) = self.nodes.get_mut(mid)
                    && let WidgetKind::MenuBar { open_menu, .. } = &mut n.kind
                {
                    *open_menu = None;
                    n.dirty |= DirtyFlags::PAINT;
                    changed = true;
                }
            }

            // Clear stale drag session if its source node was removed from the tree
            if let Some((drag_id, _, _, _, _)) = self.active_drag
                && !self.nodes.contains_key(drag_id)
            {
                self.active_drag = None;
            }
            // Start cross-widget Draggable session if pointer hits inside a Draggable ancestor
            if self.active_drag.is_none() {
                let mut drag_cursor = hit;
                while let Some(cid) = drag_cursor {
                    if let Some(n) = self.nodes.get_mut(cid)
                        && !n.is_disabled()
                        && let WidgetKind::Draggable { payload, dragging } = &mut n.kind
                    {
                        *dragging = true;
                        let val = payload.get();
                        n.dirty |= DirtyFlags::PAINT;
                        self.active_drag = Some((cid, val, pe.position, false, pe.id));
                        changed = true;
                        break;
                    }
                    drag_cursor = self.nodes.get(cid).and_then(|n| n.parent);
                }
            }
        }

        // Track active cross-widget Draggable hover over DropTarget for the initiating pointer
        if matches!(pe.phase, PointerPhase::Move)
            && let Some((drag_id, payload, orig_pos, moved, ptr_id)) = self.active_drag.clone()
            && ptr_id == pe.id
        {
            if !self.nodes.contains_key(drag_id) {
                self.active_drag = None;
                for (_, node) in &mut self.nodes {
                    if let WidgetKind::DropTarget { hovered_drag, .. } = &mut node.kind
                        && *hovered_drag
                    {
                        *hovered_drag = false;
                        node.dirty |= DirtyFlags::PAINT;
                    }
                }
                return true;
            }
            let now_moved = moved || pe.position.distance_to(orig_pos) > 2.0;
            self.active_drag = Some((
                drag_id,
                payload.clone(),
                if now_moved { pe.position } else { orig_pos },
                now_moved,
                ptr_id,
            ));
            if now_moved {
                let mut matched_drop: Option<NodeId> = None;
                let mut dcur = hit;
                while let Some(cid) = dcur {
                    if let Some(n) = self.nodes.get(cid)
                        && !n.is_disabled()
                        && let WidgetKind::DropTarget { accept_prefix, .. } = &n.kind
                        && payload.starts_with(accept_prefix.as_str())
                    {
                        matched_drop = Some(cid);
                        break;
                    }
                    dcur = self.nodes.get(cid).and_then(|n| n.parent);
                }
                for (nid, node) in &mut self.nodes {
                    if let WidgetKind::DropTarget { hovered_drag, .. } = &mut node.kind {
                        let want = matched_drop == Some(nid);
                        if *hovered_drag != want {
                            *hovered_drag = want;
                            node.dirty |= DirtyFlags::PAINT;
                        }
                    }
                }
            }
            changed = true;
        }

        // Update hovered state, hover control animations, and fire on_hover callbacks
        if hit != self.hovered {
            if let Some(prev) = self.hovered {
                if let Some(n) = self.nodes.get_mut(prev) {
                    let mut unhover = false;
                    match &mut n.kind {
                        WidgetKind::Button { hovered, .. }
                        | WidgetKind::FloatingActionButton { hovered, .. }
                        | WidgetKind::ListTile { hovered, .. }
                        | WidgetKind::Chip { hovered, .. }
                            if *hovered =>
                        {
                            *hovered = false;
                            unhover = true;
                        }
                        _ => {}
                    }
                    if unhover {
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                }
                if let Some(ControlVisualAnim::Slider { hover_t, .. }) =
                    self.control_anims.get_mut(prev)
                {
                    hover_t.set_target(0.0);
                    if let Some(n) = self.nodes.get_mut(prev) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
                self.sync_control_anim_target(prev);
                if let Some(cb) = self.nodes.get(prev).and_then(|n| n.on_hover.clone())
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(false);
                    changed = true;
                }
            }
            self.hovered = hit;
            if let Some(curr) = hit {
                let dis = self.nodes.get(curr).is_some_and(WidgetNode::is_disabled);
                if let Some(n) = self.nodes.get_mut(curr) {
                    let mut do_hover = false;
                    match &mut n.kind {
                        WidgetKind::Button { hovered, .. }
                        | WidgetKind::FloatingActionButton { hovered, .. }
                        | WidgetKind::ListTile { hovered, .. }
                        | WidgetKind::Chip { hovered, .. }
                            if !*hovered =>
                        {
                            *hovered = true;
                            do_hover = true;
                        }
                        _ => {}
                    }
                    if do_hover {
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                }
                if !dis
                    && let Some(ControlVisualAnim::Slider { hover_t, .. }) =
                        self.control_anims.get_mut(curr)
                {
                    hover_t.set_target(1.0);
                    if let Some(n) = self.nodes.get_mut(curr) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
                self.sync_control_anim_target(curr);
                if let Some(cb) = self.nodes.get(curr).and_then(|n| n.on_hover.clone())
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(true);
                    changed = true;
                }
            }
            if self.inspector.enabled {
                self.inspector.hovered_node = hit.map(|h| {
                    let name = self.nodes.get(h).map(|n| n.debug_name).unwrap_or("Node");
                    let xr = self
                        .node_world_transform(h)
                        .transform_rect_bounds(self.node_visual_rect(h));
                    (name.to_string(), xr)
                });
                changed = true;
            } else if self
                .nodes
                .values()
                .any(|n| matches!(n.kind, WidgetKind::Tooltip { .. }))
            {
                changed = true;
            }
        }

        // Route raw pointer (mapped into local widget coords) to captured Canvas or CustomWidget
        let target = self.gesture_arena.active_capture(pe.id).or(hit);
        if let Some(t) = target {
            let rect = self.node_visual_rect(t);
            let mut local_pe = *pe;
            local_pe.position = self.screen_to_node_local_point(t, pe.position);
            let kind = self.nodes.get(t).map(|n| n.kind.clone());
            match kind {
                Some(WidgetKind::Canvas {
                    on_pointer: Some(cb),
                    ..
                }) => {
                    if let Ok(mut func) = cb.try_borrow_mut() {
                        func(rect, &local_pe);
                        if !matches!(local_pe.phase, PointerPhase::Move)
                            || local_pe.button.is_some()
                        {
                            changed = true;
                        }
                    }
                }
                Some(WidgetKind::Custom(cw)) => {
                    let focusable = cw.try_borrow().is_ok_and(|g| g.focusable());
                    if matches!(pe.phase, PointerPhase::Down) && focusable {
                        self.set_focus(Some(t), false);
                        changed = true;
                    }
                    if let Ok(mut guard) = cw.try_borrow_mut()
                        && guard.handle_event(rect, &UiEvent::Pointer(local_pe))
                    {
                        changed = true;
                    }
                }
                _ => {}
            }
        }

        // Button / Slider / SplitPane sash press feedback
        if matches!(pe.phase, PointerPhase::Down)
            && let Some(h) = hit
            && !self.nodes.get(h).is_some_and(WidgetNode::is_disabled)
        {
            let h_rect = self.node_visual_rect(h);
            let h_local = self.screen_to_node_local_point(h, pe.position);
            if self
                .reorder_drag_owner
                .as_ref()
                .is_some_and(|(id, _, _)| !self.nodes.contains_key(*id))
            {
                self.reorder_drag_owner = None;
            }
            let can_start_reorder = self.reorder_drag_owner.is_none();
            let mut started_reorder_item: Option<Option<String>> = None;
            if let Some(n) = self.nodes.get_mut(h) {
                match &mut n.kind {
                    WidgetKind::Button { pressed, .. }
                    | WidgetKind::FloatingActionButton { pressed, .. }
                    | WidgetKind::ListTile { pressed, .. } => {
                        *pressed = true;
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                    WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        dragging,
                        ..
                    } => {
                        let inner = h_rect.inset(n.layout.padding);
                        let (_, _, div_rect) = Self::split_divider_geometry(
                            inner,
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            *divider_size,
                        );
                        if div_rect.inflate(4.0, 4.0).contains(h_local) {
                            *dragging = true;
                            n.dirty |= DirtyFlags::PAINT;
                            changed = true;
                        }
                    }
                    WidgetKind::ReorderableList {
                        items,
                        dragging_index,
                        target_index,
                        row_height,
                        scroll_offset,
                        ..
                    } if can_start_reorder => {
                        let rh = row_height.max(24.0);
                        let list_snap = items.get_untracked();
                        let len = list_snap.len();
                        let idx =
                            ((h_local.y - (h_rect.y + 4.0) + *scroll_offset) / rh).floor() as isize;
                        if idx >= 0 && (idx as usize) < len && h_local.x <= h_rect.x + 40.0 {
                            let u = idx as usize;
                            *dragging_index = Some(u);
                            *target_index = Some(u);
                            started_reorder_item = Some(list_snap.get(u).cloned());
                            n.dirty |= DirtyFlags::PAINT;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(item_opt) = started_reorder_item {
                self.reorder_drag_owner = Some((h, pe.id, item_opt));
            }
            if let Some(ControlVisualAnim::Slider {
                dragging, drag_t, ..
            }) = self.control_anims.get_mut(h)
            {
                *dragging = true;
                drag_t.set_target(1.0);
                if let Some(n) = self.nodes.get_mut(h) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                changed = true;
            }
            self.sync_control_anim_target(h);
        }
        if matches!(pe.phase, PointerPhase::Up | PointerPhase::Cancel) {
            let is_cancel = matches!(pe.phase, PointerPhase::Cancel);
            let mut released = Vec::new();
            let mut reorder_cbs: Vec<(crate::ReorderCallback, usize, usize)> = Vec::new();
            for (nid, node) in &mut self.nodes {
                match &mut node.kind {
                    WidgetKind::Button { pressed, .. }
                    | WidgetKind::FloatingActionButton { pressed, .. }
                    | WidgetKind::ListTile { pressed, .. }
                        if *pressed =>
                    {
                        *pressed = false;
                        node.dirty |= DirtyFlags::PAINT;
                        released.push(nid);
                        changed = true;
                    }
                    WidgetKind::SplitPane { dragging, .. } if *dragging => {
                        *dragging = false;
                        node.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                    WidgetKind::ReorderableList {
                        items,
                        selected,
                        dragging_index,
                        target_index,
                        on_reorder,
                        ..
                    } if dragging_index.is_some()
                        && self
                            .reorder_drag_owner
                            .as_ref()
                            .is_some_and(|(id, pid, _)| *id == nid && *pid == pe.id) =>
                    {
                        let grabbed = self.reorder_drag_owner.take().and_then(|(_, _, item)| item);
                        let from = dragging_index.take();
                        let to = target_index.take();
                        if !is_cancel
                            && let (Some(f), Some(t)) = (from, to)
                            && f != t
                        {
                            let mut moved = false;
                            items.update(|list| {
                                if f < list.len()
                                    && t < list.len()
                                    && grabbed.as_ref().is_none_or(|exp| list.get(f) == Some(exp))
                                {
                                    let el = list.remove(f);
                                    list.insert(t, el);
                                    moved = true;
                                }
                            });
                            if moved {
                                selected.set(t);
                                node.a11y.numeric_value = Some(t as f64);
                                if let Some(cb) = on_reorder.clone() {
                                    reorder_cbs.push((cb, f, t));
                                }
                            }
                        }
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y;
                        changed = true;
                    }
                    _ => {}
                }
            }
            for (cb, f, t) in reorder_cbs {
                if let Ok(mut func) = cb.try_borrow_mut() {
                    func(f, t);
                }
            }

            if let Some((_, _, _, _, ptr_id)) = self.active_drag
                && ptr_id == pe.id
            {
                let (drag_id, payload, _, moved, _) = self.active_drag.take().unwrap();
                if let Some(dn) = self.nodes.get_mut(drag_id)
                    && let WidgetKind::Draggable { dragging, .. } = &mut dn.kind
                {
                    *dragging = false;
                    dn.dirty |= DirtyFlags::PAINT;
                }
                let mut drop_cb: Option<(crate::TextCallback, String)> = None;
                if !is_cancel && moved && self.nodes.contains_key(drag_id) {
                    let mut dcur = hit;
                    while let Some(cid) = dcur {
                        if let Some(n) = self.nodes.get(cid)
                            && !n.is_disabled()
                            && let WidgetKind::DropTarget {
                                accept_prefix,
                                on_drop,
                                ..
                            } = &n.kind
                            && payload.starts_with(accept_prefix.as_str())
                        {
                            if let Some(cb) = on_drop.clone() {
                                drop_cb = Some((cb, payload.clone()));
                            }
                            break;
                        }
                        dcur = self.nodes.get(cid).and_then(|n| n.parent);
                    }
                }
                for (_, node) in &mut self.nodes {
                    if let WidgetKind::DropTarget { hovered_drag, .. } = &mut node.kind
                        && *hovered_drag
                    {
                        *hovered_drag = false;
                        node.dirty |= DirtyFlags::PAINT;
                    }
                }
                if let Some((cb, p)) = drop_cb
                    && let Ok(mut func) = cb.try_borrow_mut()
                {
                    func(p);
                }
                changed = true;
            }
            for nid in released {
                self.sync_control_anim_target(nid);
            }
            let slider_ids: Vec<NodeId> = self.control_anims.keys().collect();
            for sid in slider_ids {
                if let Some(ControlVisualAnim::Slider {
                    dragging, drag_t, ..
                }) = self.control_anims.get_mut(sid)
                    && *dragging
                {
                    *dragging = false;
                    drag_t.set_target(0.0);
                    if let Some(n) = self.nodes.get_mut(sid) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
            }
        }

        let gestures = self.gesture_arena.process_pointer(pe, hit);
        for g in gestures {
            match g {
                RecognizedGesture::Tap { node, position } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled) {
                        self.focused = Some(node);
                        self.focus_visible = false;
                        self.activate_tap(node, position, true);
                        changed = true;
                    }
                }
                RecognizedGesture::DoubleTap { node, .. } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled)
                        && let Some(cb) = self.nodes.get(node).and_then(|n| n.on_double_tap.clone())
                        && let Ok(mut f) = cb.try_borrow_mut()
                    {
                        f();
                        changed = true;
                    }
                }
                RecognizedGesture::LongPress { node, .. } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled)
                        && let Some(cb) = self.nodes.get(node).and_then(|n| n.on_long_press.clone())
                        && let Ok(mut f) = cb.try_borrow_mut()
                    {
                        f();
                        changed = true;
                    }
                }
                RecognizedGesture::DragStart { node, position } => {
                    self.handle_drag(node, position, Point::ZERO);
                    changed = true;
                }
                RecognizedGesture::DragUpdate {
                    node,
                    position,
                    delta,
                } => {
                    self.handle_drag(node, position, delta);
                    changed = true;
                }
                RecognizedGesture::DragEnd { node, velocity } => {
                    self.handle_drag_end(node, velocity);
                    changed = true;
                }
                RecognizedGesture::PinchRotate {
                    center,
                    scale_delta,
                    angle_delta_rad,
                } => {
                    if self.dispatch_pinch_rotate(center, scale_delta, angle_delta_rad) {
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    /// Programmatically activate a tap on `node_id` (e.g. for accessibility actions).
    pub fn activate_node_tap(&mut self, node_id: NodeId) -> bool {
        let Some(node) = self.nodes.get(node_id) else {
            return false;
        };
        if node.is_disabled() {
            return false;
        }
        if node.a11y.focusable {
            if let Some(old) = self.focused
                && old != node_id
                && let Some(n) = self.nodes.get_mut(old)
            {
                n.dirty |= DirtyFlags::PAINT;
            }
            self.focused = Some(node_id);
            self.focus_visible = false;
            if let Some(n) = self.nodes.get_mut(node_id) {
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        let center = self.node_visual_rect(node_id).center();
        self.activate_tap(node_id, center, false);
        true
    }

    pub(crate) fn activate_tap(&mut self, node_id: NodeId, pos: Point, is_screen_pos: bool) {
        let rect = self.node_visual_rect(node_id);
        let node_xform = self.node_world_transform(node_id);

        if self.nodes.get(node_id).is_some_and(WidgetNode::is_disabled) {
            return;
        }

        let (local_pos, screen_pos) = if is_screen_pos {
            (self.screen_to_node_local_point(node_id, pos), pos)
        } else {
            (pos, self.node_world_transform(node_id).transform_point(pos))
        };

        // Generic node on_tap handler
        let extra_tap = self.nodes.get(node_id).and_then(|n| n.on_tap.clone());
        if let Some(cb) = extra_tap
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }

        let mut action: Option<TapHandler> = None;
        let mut link_cb: Option<(crate::widgets::TextCallback, String)> = None;
        let mut bool_cb: Option<(BoolCallback, bool)> = None;
        let mut float_cb: Option<(FloatCallback, f32)> = None;
        let mut idx_cb: Option<(IndexCallback, usize)> = None;
        let mut sort_cb: Option<(IndexCallback, usize)> = None;
        let mut dismiss_cb: Option<(ToastDismissCallback, u64)> = None;
        let mut reset_caret = false;
        let mut relayout_node = false;

        // Compute popup/dropdown geometry upfront to satisfy borrow checker
        let popup_rect_opt = match self.nodes.get(node_id).map(|n| &n.kind) {
            Some(WidgetKind::PopupMenu { items, anchor, .. }) => {
                Some(self.popup_menu_rect(*anchor, items.get().len(), rect))
            }
            _ => None,
        };
        let select_drop_rect_opt = match self.nodes.get(node_id).map(|n| &n.kind) {
            Some(WidgetKind::Select { options, .. }) => {
                let screen_tr = self
                    .node_world_transform(node_id)
                    .transform_rect_bounds(rect);
                Some(self.select_dropdown_rect(screen_tr, options.get().len()))
            }
            _ => None,
        };

        if let Some(node) = self.nodes.get_mut(node_id) {
            match &mut node.kind {
                WidgetKind::Button {
                    disabled, on_tap, ..
                }
                | WidgetKind::FloatingActionButton {
                    disabled, on_tap, ..
                } => {
                    if !disabled.get() {
                        action = on_tap.clone();
                    }
                }
                WidgetKind::Checkbox {
                    checked, on_change, ..
                }
                | WidgetKind::Switch {
                    checked, on_change, ..
                } => {
                    let next = !checked.get_untracked();
                    checked.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(cb) = on_change.clone() {
                        bool_cb = Some((cb, next));
                    }
                }
                WidgetKind::Slider {
                    value,
                    min,
                    max,
                    on_change,
                    ..
                } => {
                    let track_x = rect.x + 8.0;
                    let track_w = (rect.width - 16.0).max(1.0);
                    let t = ((local_pos.x - track_x) / track_w).clamp(0.0, 1.0);
                    let next = *min + (*max - *min) * t;
                    value.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(cb) = on_change.clone() {
                        float_cb = Some((cb, next));
                    }
                }
                WidgetKind::RichText {
                    content,
                    on_link: Some(callback),
                } => {
                    let mut rich = content.get();
                    if self.direction == Direction::Rtl {
                        rich.align = match rich.align {
                            craftui_text::TextAlign::Start => craftui_text::TextAlign::End,
                            craftui_text::TextAlign::End => craftui_text::TextAlign::Start,
                            other => other,
                        };
                    }
                    let layout = self
                        .text_engine
                        .build_rich_layout(&rich, Some(rect.width.max(1.0)));
                    let point = Point::new(local_pos.x - rect.x, local_pos.y - rect.y);
                    let mut start = 0;
                    for span in &rich.spans {
                        let end = start + span.text.len();
                        if let Some(target) = &span.link
                            && start != end
                            && self
                                .text_engine
                                .selection_rects(&layout, start, end)
                                .iter()
                                .any(|bounds| bounds.contains(point))
                        {
                            link_cb = Some((callback.clone(), target.clone()));
                            break;
                        }
                        start = end;
                    }
                }
                WidgetKind::TextInput {
                    editor,
                    style,
                    multiline,
                    ..
                } => {
                    let pad_top = if *multiline { 10.0 } else { 8.0 };
                    let inner_w = (rect.width - 24.0).max(40.0);
                    let wrap = if *multiline { Some(inner_w) } else { None };
                    let inner_x = local_pos.x - (rect.x + 12.0);
                    let inner_y = local_pos.y - (rect.y + pad_top);
                    let mut hit_style = style.clone();
                    if self.direction == Direction::Rtl {
                        hit_style.align = match hit_style.align {
                            craftui_text::TextAlign::Start => craftui_text::TextAlign::End,
                            craftui_text::TextAlign::End => craftui_text::TextAlign::Start,
                            other => other,
                        };
                    }
                    let layout = self.text_engine.build_layout_cached(
                        &editor.display_text(),
                        &hit_style,
                        wrap,
                    );
                    let byte_idx = self.text_engine.hit_test_point(&layout, inner_x, inner_y);
                    editor.set_cursor_at(byte_idx, false);
                    reset_caret = true;
                }
                WidgetKind::Tabs {
                    tabs,
                    active,
                    on_change,
                } => {
                    let count = tabs.get().len();
                    if count > 0 && rect.width > 0.0 {
                        let idx =
                            (((local_pos.x - rect.x) / rect.width) * count as f32).floor() as isize;
                        let next = (idx.clamp(0, (count - 1) as isize)) as usize;
                        active.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::SegmentedControl {
                    items,
                    selected,
                    on_change,
                }
                | WidgetKind::BottomNavBar {
                    items,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = items.get().len();
                    if count > 0 && rect.width > 0.0 {
                        let idx =
                            (((local_pos.x - rect.x) / rect.width) * count as f32).floor() as isize;
                        let next = idx.clamp(0, (count - 1) as isize) as usize;
                        selected.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::Accordion {
                    expanded,
                    on_change,
                    ..
                } => {
                    if !is_screen_pos || local_pos.y <= rect.y + 44.0 {
                        let next = !expanded.get_untracked();
                        expanded.set(next);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y;
                        relayout_node = true;
                        if let Some(cb) = on_change.clone() {
                            bool_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::DataTable {
                    columns,
                    rows,
                    selected,
                    sort_column,
                    sort_ascending,
                    scroll_offset,
                    row_height,
                    on_select,
                    on_sort,
                    ..
                } => {
                    let cols = columns.get();
                    let row_list = rows.get();
                    let header_h = 38.0f32.min(rect.height);
                    if is_screen_pos && local_pos.y <= rect.y + header_h && !cols.is_empty() {
                        let col_w = (rect.width / cols.len() as f32).max(1.0);
                        let ci = (((local_pos.x - rect.x) / col_w).floor() as isize)
                            .clamp(0, (cols.len() - 1) as isize)
                            as usize;
                        if *sort_column == Some(ci) {
                            *sort_ascending = !*sort_ascending;
                        } else {
                            *sort_column = Some(ci);
                            *sort_ascending = true;
                        }
                        node.dirty |= DirtyFlags::PAINT;
                        if let Some(cb) = on_sort.clone() {
                            sort_cb = Some((cb, ci));
                        }
                    } else if !row_list.is_empty() && *row_height > 0.0 {
                        let sorted = Self::data_table_sorted_indices(
                            &row_list,
                            *sort_column,
                            *sort_ascending,
                        );
                        let rel_y = if is_screen_pos {
                            (local_pos.y - (rect.y + header_h) + *scroll_offset).max(0.0)
                        } else {
                            0.0
                        };
                        let vis_idx = ((rel_y / *row_height).floor() as isize)
                            .clamp(0, (sorted.len() - 1) as isize)
                            as usize;
                        let orig_idx = sorted[vis_idx];
                        selected.set(orig_idx);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_select.clone() {
                            idx_cb = Some((cb, orig_idx));
                        }
                    }
                }
                WidgetKind::Select {
                    options,
                    selected,
                    open,
                    dropdown_scroll,
                    on_change,
                    ..
                } => {
                    let opts = options.get();
                    if *open {
                        if let Some(dr) = select_drop_rect_opt
                            && dr.contains(screen_pos)
                            && !opts.is_empty()
                        {
                            let row = ((screen_pos.y - dr.y - 4.0 + *dropdown_scroll) / 34.0)
                                .floor() as isize;
                            let next = row.clamp(0, (opts.len() - 1) as isize) as usize;
                            selected.set(next);
                            *open = false;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        } else {
                            *open = false;
                        }
                    } else {
                        *open = true;
                        if let Some(dr) = select_drop_rect_opt {
                            let total_h = opts.len() as f32 * 34.0 + 8.0;
                            let max_scroll = (total_h - dr.height).max(0.0);
                            if max_scroll <= 0.0 {
                                *dropdown_scroll = 0.0;
                            } else {
                                let sel_idx =
                                    selected.get_untracked().min(opts.len().saturating_sub(1));
                                let item_top = sel_idx as f32 * 34.0;
                                let item_bottom = item_top + 34.0;
                                let vis_h = (dr.height - 8.0).max(34.0);
                                let mut next = (*dropdown_scroll).clamp(0.0, max_scroll);
                                if item_top < next {
                                    next = item_top;
                                } else if item_bottom > next + vis_h {
                                    next = (item_bottom - vis_h).clamp(0.0, max_scroll);
                                }
                                *dropdown_scroll = next.clamp(0.0, max_scroll);
                            }
                        }
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                }
                WidgetKind::RadioGroup {
                    options,
                    selected,
                    axis,
                    on_change,
                } => {
                    let count = options.get().len();
                    if count > 0 {
                        let next = match axis {
                            Axis::Vertical => (((local_pos.y - rect.y) / 30.0).floor() as isize)
                                .clamp(0, (count - 1) as isize)
                                as usize,
                            Axis::Horizontal => (((local_pos.x - rect.x)
                                / (rect.width.max(1.0) / count as f32))
                                .floor() as isize)
                                .clamp(0, (count - 1) as isize)
                                as usize,
                        };
                        selected.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::PopupMenu {
                    items,
                    on_select,
                    on_close,
                    ..
                } => {
                    let count = items.get().len();
                    if let Some(mr) = popup_rect_opt
                        && mr.contains(local_pos)
                        && count > 0
                    {
                        let idx = (((local_pos.y - mr.y - 4.0) / 36.0).floor() as isize)
                            .clamp(0, (count - 1) as isize)
                            as usize;
                        if let Some(cb) = on_select.clone() {
                            idx_cb = Some((cb, idx));
                        }
                    }
                    action = on_close.clone();
                }
                WidgetKind::ToastHost {
                    items,
                    on_dismiss,
                    manager,
                } => {
                    let list = items.get();
                    let (_cards, closes) = Self::toast_card_rects(rect, list.len());
                    for (idx, close_r) in closes.iter().enumerate() {
                        if close_r.inflate(8.0, 8.0).contains(local_pos) {
                            let dismissed_id = list[idx].id;
                            if let Some(mgr) = manager {
                                mgr.dismiss(dismissed_id);
                            }
                            if let Some(cb) = on_dismiss.clone() {
                                dismiss_cb = Some((cb, dismissed_id));
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            break;
                        }
                    }
                }
                WidgetKind::ListTile { on_tap, .. } => {
                    action = on_tap.clone();
                }
                WidgetKind::Chip {
                    selected, on_tap, ..
                } => {
                    let next = !selected.get_untracked();
                    selected.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    action = on_tap.clone();
                }
                WidgetKind::AppBar { on_leading_tap, .. } => {
                    action = on_leading_tap.clone();
                }
                WidgetKind::Drawer {
                    open,
                    width,
                    on_close,
                } => {
                    if open.get_untracked() {
                        let drawer_w = (*width).min(rect.width * 0.85).max(240.0);
                        if local_pos.x > rect.x + drawer_w {
                            open.set(false);
                            node.dirty |= DirtyFlags::PAINT;
                            action = on_close.clone();
                        }
                    }
                }
                WidgetKind::Overlay { on_close, .. } => {
                    action = on_close.clone();
                }
                WidgetKind::DatePicker {
                    value,
                    view_year,
                    view_month,
                    on_change,
                } => {
                    let prev_r = Rect::new(rect.x + 8.0, rect.y + 8.0, 28.0, 28.0);
                    let next_r = Rect::new(rect.max_x() - 36.0, rect.y + 8.0, 28.0, 28.0);
                    if prev_r.contains(local_pos) {
                        let (ny, nm) = crate::pickers::shift_month(*view_year, *view_month, -1);
                        *view_year = ny;
                        *view_month = nm;
                        node.dirty |= DirtyFlags::PAINT;
                    } else if next_r.contains(local_pos) {
                        let (ny, nm) = crate::pickers::shift_month(*view_year, *view_month, 1);
                        *view_year = ny;
                        *view_month = nm;
                        node.dirty |= DirtyFlags::PAINT;
                    } else {
                        let cell_w = ((rect.width - 16.0) / 7.0).max(20.0);
                        let cell_h = ((rect.height - 74.0) / 6.0).max(22.0);
                        let first_wd = crate::pickers::weekday_sunday0(*view_year, *view_month, 1);
                        let total_days = crate::pickers::days_in_month(*view_year, *view_month);
                        for day in 1..=total_days {
                            let slot = first_wd + (day - 1);
                            let r = slot / 7;
                            let c = slot % 7;
                            let cell = Rect::new(
                                rect.x + 8.0 + c as f32 * cell_w,
                                rect.y + 66.0 + r as f32 * cell_h,
                                cell_w,
                                cell_h,
                            );
                            if cell.contains(local_pos) {
                                let formatted =
                                    crate::pickers::format_iso_date(*view_year, *view_month, day);
                                value.set(formatted.clone());
                                node.a11y.value = Some(formatted.clone());
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_change.clone() {
                                    link_cb = Some((cb, formatted));
                                }
                                break;
                            }
                        }
                    }
                }
                WidgetKind::TimePicker {
                    value,
                    use_24h,
                    on_change,
                } => {
                    let (mut h, mut m) = crate::pickers::parse_time_hm(&value.get_untracked());
                    let h_plus = Rect::new(rect.x + 12.0, rect.y + 10.0, 58.0, 28.0);
                    let h_minus = Rect::new(rect.x + 12.0, rect.y + 92.0, 58.0, 28.0);
                    let m_plus = Rect::new(rect.x + 92.0, rect.y + 10.0, 58.0, 28.0);
                    let m_minus = Rect::new(rect.x + 92.0, rect.y + 92.0, 58.0, 28.0);
                    let am_pm_r = Rect::new(rect.x + 162.0, rect.y + 42.0, 54.0, 46.0);
                    let mut changed = true;
                    if h_plus.contains(local_pos) {
                        h = (h + 1) % 24;
                    } else if h_minus.contains(local_pos) {
                        h = (h + 23) % 24;
                    } else if m_plus.contains(local_pos) {
                        m = (m + 1) % 60;
                    } else if m_minus.contains(local_pos) {
                        m = (m + 59) % 60;
                    } else if !*use_24h && am_pm_r.contains(local_pos) {
                        h = (h + 12) % 24;
                    } else {
                        changed = false;
                    }
                    if changed {
                        let formatted = crate::pickers::format_time_hm(h, m);
                        value.set(formatted.clone());
                        node.a11y.value = Some(formatted.clone());
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            link_cb = Some((cb, formatted));
                        }
                    }
                }
                WidgetKind::TreeView {
                    items,
                    selected,
                    collapsed,
                    row_height,
                    scroll_offset,
                    on_select,
                } => {
                    let rh = (*row_height).max(22.0);
                    let list = items.get();
                    let vis_idx =
                        ((local_pos.y - (rect.y + 4.0) + *scroll_offset) / rh).floor() as isize;
                    if vis_idx >= 0 {
                        let mut col = collapsed.borrow_mut();
                        let vis = crate::pickers::derive_visible_tree_rows(&list, &col);
                        if let Some(row) = vis.get(vis_idx as usize) {
                            let indent = row.depth as f32 * 16.0;
                            let chev_x = rect.x + 12.0 + indent;
                            if row.has_children && local_pos.x <= chev_x + 16.0 {
                                if !col.remove(&row.path) {
                                    col.insert(row.path.clone());
                                }
                                relayout_node = true;
                                node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            } else {
                                selected.set(row.raw_index);
                                node.a11y.numeric_value = Some(row.raw_index as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_select.clone() {
                                    idx_cb = Some((cb, row.raw_index));
                                }
                            }
                        }
                    }
                }
                WidgetKind::ReorderableList {
                    items,
                    selected,
                    row_height,
                    scroll_offset,
                    on_select,
                    ..
                } => {
                    let rh = (*row_height).max(24.0);
                    let len = items.get_untracked().len();
                    let idx =
                        ((local_pos.y - (rect.y + 4.0) + *scroll_offset) / rh).floor() as isize;
                    if idx >= 0 && (idx as usize) < len {
                        let u = idx as usize;
                        selected.set(u);
                        node.a11y.numeric_value = Some(u as f64);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_select.clone() {
                            idx_cb = Some((cb, u));
                        }
                    }
                }
                WidgetKind::MenuBar {
                    menus,
                    open_menu,
                    on_select,
                } => {
                    let parsed = crate::pickers::parse_menubar_specs(&menus.get());
                    let hrs = crate::pickers::menubar_header_rects(rect, &parsed);
                    let mut handled_drop = false;
                    if let Some(oidx) = *open_menu
                        && let (Some(hr), Some((menu_title, items))) =
                            (hrs.get(oidx), parsed.get(oidx))
                        && !items.is_empty()
                    {
                        let screen_pos = node_xform.transform_point(local_pos);
                        let dr = crate::pickers::menubar_dropdown_rect(
                            node_xform.transform_rect_bounds(*hr),
                            items.len(),
                        );
                        if dr.contains(screen_pos) {
                            let item_idx = ((screen_pos.y - (dr.y + 4.0)) / 30.0).floor() as isize;
                            if item_idx >= 0 && (item_idx as usize) < items.len() {
                                let chosen = format!("{}/{}", menu_title, items[item_idx as usize]);
                                if let Some(cb) = on_select.clone() {
                                    link_cb = Some((cb, chosen));
                                }
                            }
                            *open_menu = None;
                            node.dirty |= DirtyFlags::PAINT;
                            handled_drop = true;
                        }
                    }
                    if !handled_drop {
                        let clicked_hdr = hrs.iter().position(|hr| hr.contains(local_pos));
                        *open_menu = match (clicked_hdr, *open_menu) {
                            (Some(i), Some(cur)) if i == cur => None,
                            (Some(i), _) => Some(i),
                            (None, _) => None,
                        };
                        node.dirty |= DirtyFlags::PAINT;
                    }
                }
                WidgetKind::MediaPlayer {
                    kind,
                    controller,
                    show_controls,
                    on_toggle_play,
                    ..
                } if *show_controls => {
                    let (_, play_r, scrub_r, mute_r, _) =
                        crate::media_editor::media_control_rects(rect, *kind);
                    if play_r.contains(local_pos) {
                        let mut playing_now = false;
                        controller.update(|c| {
                            c.toggle_play();
                            playing_now = c.playing;
                        });
                        if let Some(cb) = on_toggle_play.clone() {
                            bool_cb = Some((cb, playing_now));
                        }
                        node.dirty |= DirtyFlags::PAINT;
                    } else if mute_r.contains(local_pos) {
                        controller.update(|c| c.toggle_mute());
                        node.dirty |= DirtyFlags::PAINT;
                    } else if scrub_r.contains(local_pos) && scrub_r.width > 0.0 {
                        let frac = ((local_pos.x - scrub_r.x) / scrub_r.width).clamp(0.0, 1.0);
                        controller.update(|c| c.seek_fraction(frac));
                        node.dirty |= DirtyFlags::PAINT;
                    }
                }
                WidgetKind::RichTextEditor {
                    controller, style, ..
                } => {
                    if let Some(btn_idx) =
                        crate::media_editor::rich_editor_hit_toolbar(rect, local_pos)
                    {
                        controller.update(|c| match btn_idx {
                            0 => c.toggle_bold(),
                            1 => c.toggle_italic(),
                            _ => c.toggle_underline(),
                        });
                    } else {
                        let (_, text_r) = crate::media_editor::rich_editor_layout(rect);
                        let mut eff_st = style.clone();
                        if self.direction == Direction::Rtl {
                            eff_st.align = match eff_st.align {
                                craftui_text::TextAlign::Start => craftui_text::TextAlign::End,
                                craftui_text::TextAlign::End => craftui_text::TextAlign::Start,
                                other => other,
                            };
                        }
                        let snap = controller.get_untracked();
                        let rich = snap.to_rich_text(&eff_st);
                        let layout = self
                            .text_engine
                            .build_rich_layout(&rich, Some(text_r.width));
                        let cr = self.text_engine.cursor_rect(&layout, snap.editor.cursor);
                        let scroll_y = (cr.max_y() - text_r.height).max(0.0);
                        let inner_x = local_pos.x - text_r.x;
                        let inner_y = local_pos.y - (text_r.y - scroll_y);
                        let byte_idx = self.text_engine.hit_test_point(&layout, inner_x, inner_y);
                        controller.update(|c| c.editor.set_cursor_at(byte_idx, false));
                    }
                    node.dirty |= DirtyFlags::PAINT;
                    reset_caret = true;
                }
                _ => {}
            }
        }

        self.sync_control_anim_target(node_id);

        if relayout_node {
            self.layout_engine.mark_dirty(node_id);
        }

        if reset_caret {
            self.reset_caret_blink(craftui_core::monotonic_ms());
        }
        if let Some((cb, target)) = link_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(target);
        }
        if let Some((cb, v)) = bool_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }
        if let Some((cb, v)) = float_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }
        if let Some((cb, i)) = idx_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(i);
        }
        if let Some((cb, col)) = sort_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(col);
        }
        if let Some((cb, id)) = dismiss_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(id);
        }
        if let Some(cb) = action
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }
    }

    pub(crate) fn handle_drag(&mut self, node_id: NodeId, pos: Point, delta: Point) {
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            let r = self.node_visual_rect(nid);
            let layout_r = self.layout_engine.rect(nid);
            let local_pos = self.screen_to_node_local_point(nid, pos);
            let world_xf = self.node_world_transform(nid);
            let local_delta = if world_xf.is_identity() {
                delta
            } else if let Some(inv) = world_xf.inverse() {
                let [a, b, c, d, _, _] = inv.matrix;
                Point::new(a * delta.x + c * delta.y, b * delta.x + d * delta.y)
            } else {
                delta
            };
            let mut slider_cb: Option<(FloatCallback, f32, f32)> = None;
            let mut matched = false;
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::Slider {
                        value,
                        min,
                        max,
                        on_change,
                        ..
                    } => {
                        let track_x = r.x + 8.0;
                        let track_w = (r.width - 16.0).max(1.0);
                        let t = ((local_pos.x - track_x) / track_w).clamp(0.0, 1.0);
                        let next = *min + (*max - *min) * t;
                        value.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        slider_cb = Some((
                            on_change.clone().unwrap_or_else(|| {
                                std::rc::Rc::new(std::cell::RefCell::new(|_| {}))
                            }),
                            t,
                            next,
                        ));
                        matched = true;
                    }
                    WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        dragging,
                        on_change,
                    } => {
                        let inner = r.inset(node.layout.padding);
                        let div = (*divider_size).max(4.0);
                        let (_, _, div_rect) = Self::split_divider_geometry(
                            inner,
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            div,
                        );
                        if *dragging || div_rect.inflate(8.0, 8.0).contains(local_pos) {
                            let total = match axis {
                                Axis::Horizontal => inner.width,
                                Axis::Vertical => inner.height,
                            };
                            let usable = (total - div).max(1.0);
                            let min_f = (*min_first).max(0.0).min(usable);
                            let max_f = (usable - (*min_second).max(0.0)).max(min_f);
                            let coord = match axis {
                                Axis::Horizontal => local_pos.x - inner.x,
                                Axis::Vertical => local_pos.y - inner.y,
                            };
                            let first_px = (coord - div * 0.5).clamp(min_f, max_f);
                            let next_ratio = (first_px / usable).clamp(0.0, 1.0);
                            *dragging = true;
                            ratio.set(next_ratio);
                            node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            slider_cb = Some((
                                on_change.clone().unwrap_or_else(|| {
                                    std::rc::Rc::new(std::cell::RefCell::new(|_| {}))
                                }),
                                next_ratio,
                                next_ratio,
                            ));
                            matched = true;
                        }
                    }
                    WidgetKind::ReorderableList {
                        items,
                        dragging_index: Some(_from),
                        target_index,
                        row_height,
                        scroll_offset,
                        ..
                    } => {
                        let len = items.get_untracked().len();
                        if len > 0 {
                            let rh = (*row_height).max(24.0);
                            let raw_to = ((local_pos.y - (r.y + 4.0) + *scroll_offset) / rh).floor()
                                as isize;
                            let to = raw_to.clamp(0, len as isize - 1) as usize;
                            if *target_index != Some(to) {
                                *target_index = Some(to);
                                node.dirty |= DirtyFlags::PAINT;
                            }
                        }
                        matched = true;
                    }
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => layout_r.height,
                            Axis::Horizontal => layout_r.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -local_delta.y,
                            Axis::Horizontal => -local_delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => layout_r.height,
                            Axis::Horizontal => layout_r.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -local_delta.y,
                            Axis::Horizontal => -local_delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return;
                    }
                    _ => {}
                }
            }
            if matched {
                if let Some((cb, frac, next)) = slider_cb {
                    if let Some(ControlVisualAnim::Slider {
                        frac_t,
                        dragging,
                        drag_t,
                        ..
                    }) = self.control_anims.get_mut(nid)
                    {
                        *dragging = true;
                        drag_t.set_target(1.0);
                        frac_t.snap(frac);
                    }
                    if let Ok(mut f) = cb.try_borrow_mut() {
                        f(next);
                    }
                }
                return;
            }
            cursor = parent;
        }
    }

    pub(crate) fn handle_drag_end(&mut self, node_id: NodeId, velocity: (f32, f32)) {
        if let Some(ControlVisualAnim::Slider {
            dragging, drag_t, ..
        }) = self.control_anims.get_mut(node_id)
        {
            *dragging = false;
            drag_t.set_target(0.0);
            if let Some(n) = self.nodes.get_mut(node_id) {
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        if let Some(n) = self.nodes.get_mut(node_id)
            && let WidgetKind::SplitPane { dragging, .. } = &mut n.kind
            && *dragging
        {
            *dragging = false;
            n.dirty |= DirtyFlags::PAINT;
        }
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        velocity: vel,
                        ..
                    }
                    | WidgetKind::VirtualList {
                        axis,
                        velocity: vel,
                        ..
                    } => {
                        *vel = match axis {
                            Axis::Vertical => velocity.1,
                            Axis::Horizontal => velocity.0,
                        };
                        return;
                    }
                    _ => {}
                }
            }
            cursor = parent;
        }
    }

    pub(crate) fn handle_scroll(&mut self, pos: Point, dx: f32, dy: f32) -> bool {
        if let Some(root) = self.root {
            let mut order = Vec::new();
            self.collect_preorder(root, &mut order);
            for id in order.into_iter().rev() {
                let drop_info = self.nodes.get(id).and_then(|node| {
                    if !node.layout.hidden
                        && let WidgetKind::Select {
                            open: true,
                            options,
                            ..
                        } = &node.kind
                    {
                        let count = options.get().len();
                        let tr = self.node_visual_rect(id);
                        let screen_tr = self.node_world_transform(id).transform_rect_bounds(tr);
                        let dr = self.select_dropdown_rect(screen_tr, count);
                        if dr.contains(pos) {
                            return Some((dr, count));
                        }
                    }
                    None
                });
                if let Some((dr, count)) = drop_info
                    && let Some(node) = self.nodes.get_mut(id)
                    && let WidgetKind::Select {
                        dropdown_scroll, ..
                    } = &mut node.kind
                {
                    let total_h = count as f32 * 34.0 + 8.0;
                    let max_scroll = (total_h - dr.height).max(0.0);
                    *dropdown_scroll = (*dropdown_scroll - dy).clamp(0.0, max_scroll);
                    node.dirty |= DirtyFlags::PAINT;
                    return true;
                }
            }
        }

        let Some(mut cursor) = self.hit_test(pos).or(self.root) else {
            return false;
        };
        loop {
            let rect = self.layout_engine.rect(cursor);
            let parent = self.nodes.get(cursor).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(cursor) {
                match &mut node.kind {
                    WidgetKind::TreeView {
                        items,
                        collapsed,
                        row_height,
                        scroll_offset,
                        ..
                    } => {
                        let rh = (*row_height).max(22.0);
                        let list = items.get();
                        let vis_len =
                            crate::pickers::derive_visible_tree_rows(&list, &collapsed.borrow())
                                .len();
                        let total_h = vis_len as f32 * rh + 8.0;
                        let max_off = (total_h - rect.height).max(0.0);
                        let prev_off = *scroll_offset;
                        let next_off = (*scroll_offset - dy).clamp(0.0, max_off);
                        if max_off > 0.0 && (next_off - prev_off).abs() > f32::EPSILON {
                            *scroll_offset = next_off;
                            node.dirty |= DirtyFlags::PAINT;
                            return true;
                        }
                    }
                    WidgetKind::ReorderableList {
                        items,
                        row_height,
                        scroll_offset,
                        ..
                    } => {
                        let rh = (*row_height).max(24.0);
                        let total_h = items.get_untracked().len() as f32 * rh + 8.0;
                        let max_off = (total_h - rect.height).max(0.0);
                        let prev_off = *scroll_offset;
                        let next_off = (*scroll_offset - dy).clamp(0.0, max_off);
                        if max_off > 0.0 && (next_off - prev_off).abs() > f32::EPSILON {
                            *scroll_offset = next_off;
                            node.dirty |= DirtyFlags::PAINT;
                            return true;
                        }
                    }
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return true;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return true;
                    }
                    WidgetKind::DataTable {
                        rows,
                        scroll_offset,
                        row_height,
                        ..
                    } => {
                        let header_h = 38.0f32.min(rect.height);
                        let body_h = (rect.height - header_h).max(0.0);
                        let total_h = rows.get().len() as f32 * *row_height;
                        let max_off = (total_h - body_h).max(0.0);
                        *scroll_offset = (*scroll_offset - dy).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return true;
                    }
                    _ => {}
                }
            }
            let Some(p) = parent else {
                break;
            };
            cursor = p;
        }
        false
    }

    /// Dismiss the topmost open `Select` dropdown, `PopupMenu`, or `Overlay` (`Modal`/`BottomSheet`).
    pub(crate) fn dismiss_top_overlay(&mut self) -> bool {
        let Some(root) = self.root else {
            return false;
        };
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);

        for id in order.into_iter().rev() {
            if let Some(node) = self.nodes.get_mut(id) {
                match &mut node.kind {
                    WidgetKind::Select { open: true, .. } => {
                        if let WidgetKind::Select { open, .. } = &mut node.kind {
                            *open = false;
                            node.dirty |= DirtyFlags::PAINT;
                        }
                        return true;
                    }
                    WidgetKind::MenuBar {
                        open_menu: Some(_), ..
                    } => {
                        if let WidgetKind::MenuBar { open_menu, .. } = &mut node.kind {
                            *open_menu = None;
                            node.dirty |= DirtyFlags::PAINT;
                        }
                        return true;
                    }
                    WidgetKind::PopupMenu { open, on_close, .. } if open.get() => {
                        let cb = on_close.clone();
                        if let Some(f) = cb
                            && let Ok(mut func) = f.try_borrow_mut()
                        {
                            func();
                            return true;
                        }
                    }
                    WidgetKind::Overlay { open, on_close, .. } if open.get() => {
                        let cb = on_close.clone();
                        if let Some(f) = cb
                            && let Ok(mut func) = f.try_borrow_mut()
                        {
                            func();
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    pub(crate) fn active_focus_root(&self) -> Option<NodeId> {
        let root = self.root?;
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);
        for id in order.into_iter().rev() {
            if let Some(n) = self.nodes.get(id)
                && !n.layout.hidden
            {
                match &n.kind {
                    WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                        if open.get() =>
                    {
                        return Some(id);
                    }
                    _ => {}
                }
            }
        }
        Some(root)
    }

    pub(crate) fn collect_preorder(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        match &node.kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return;
            }
            WidgetKind::Drawer { open, .. } if !open.get() => {
                return;
            }
            WidgetKind::Accordion { expanded, .. } if !expanded.get() => {
                out.push(id);
                return;
            }
            _ => {}
        }
        out.push(id);
        for &c in &node.children {
            self.collect_preorder(c, out);
        }
    }

    pub(crate) fn advance_focus(&mut self, reverse: bool) -> bool {
        let Some(scope_root) = self.active_focus_root() else {
            return false;
        };
        let mut nodes = Vec::new();
        self.collect_preorder(scope_root, &mut nodes);
        let focusables: Vec<NodeId> = nodes
            .into_iter()
            .filter(|&id| {
                self.nodes.get(id).is_some_and(|n| {
                    if n.is_disabled() {
                        return false;
                    }
                    if n.a11y.focusable {
                        return true;
                    }
                    matches!(&n.kind, WidgetKind::Custom(cw) if cw.try_borrow().is_ok_and(|g| g.focusable()))
                })
            })
            .collect();
        if focusables.is_empty() {
            return false;
        }
        let next = if let Some(cur) = self.focused
            && let Some(pos) = focusables.iter().position(|&id| id == cur)
        {
            if reverse {
                let prev = if pos == 0 {
                    focusables.len() - 1
                } else {
                    pos - 1
                };
                focusables[prev]
            } else {
                focusables[(pos + 1) % focusables.len()]
            }
        } else if reverse {
            *focusables.last().expect("non-empty focusables")
        } else {
            focusables[0]
        };

        if let Some(old) = self.focused
            && let Some(on) = self.nodes.get_mut(old)
        {
            on.dirty |= DirtyFlags::PAINT;
        }
        self.focused = Some(next);
        self.focus_visible = true;
        if let Some(nn) = self.nodes.get_mut(next) {
            nn.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }
        self.reset_caret_blink(craftui_core::monotonic_ms());
        true
    }

    pub(crate) fn handle_key(&mut self, ke: &KeyEvent) -> bool {
        if !ke.pressed {
            if let Some(focused) = self.focused
                && let Some(WidgetKind::Custom(cw)) =
                    self.nodes.get(focused).map(|n| n.kind.clone())
                && cw.try_borrow().is_ok_and(|g| g.wants_key_up())
            {
                let rect = self.node_visual_rect(focused);
                if let Ok(mut guard) = cw.try_borrow_mut() {
                    return guard.handle_event(rect, &UiEvent::Key(ke.clone()));
                }
            }
            return false;
        }

        // Ctrl+Shift+I / Cmd+Shift+I toggles Live Inspector Overlay (PRD FR-9.2)
        if ke.modifiers.command() && ke.modifiers.shift && ke.code.matches_ascii_letter('I') {
            self.inspector.enabled = !self.inspector.enabled;
            return true;
        }

        // Escape dismisses topmost active dropdown/popup/modal
        if matches!(ke.code, KeyCode::Escape) && self.dismiss_top_overlay() {
            return true;
        }

        // Let focused custom widgets that opt into `wants_tab()` consume Tab (e.g. typing shortcuts)
        if matches!(ke.code, KeyCode::Tab) {
            if let Some(focused) = self.focused
                && let Some(WidgetKind::Custom(cw)) =
                    self.nodes.get(focused).map(|n| n.kind.clone())
                && cw.try_borrow().is_ok_and(|g| g.wants_tab())
            {
                let rect = self.node_visual_rect(focused);
                if let Ok(mut guard) = cw.try_borrow_mut()
                    && guard.handle_event(rect, &UiEvent::Key(ke.clone()))
                {
                    return true;
                }
            }
            return self.advance_focus(ke.modifiers.shift);
        }

        // If no visible widget is currently focused, route keyboard input to the first visible autofocus CustomWidget.
        let focus_is_visible = self.focused.is_some_and(|mut id| {
            while let Some(n) = self.nodes.get(id) {
                if n.layout.hidden || n.taffy_hidden_by_parent || n.is_disabled() {
                    return false;
                }
                match n.parent {
                    Some(p) => id = p,
                    None => return true,
                }
            }
            false
        });
        if !focus_is_visible && let Some(scope_root) = self.active_focus_root() {
            let mut order = Vec::new();
            self.collect_preorder(scope_root, &mut order);
            if let Some(auto_id) = order.into_iter().find(|&id| {
                self.nodes.get(id).is_some_and(|n| {
                    !n.is_disabled()
                        && matches!(&n.kind, WidgetKind::Custom(cw) if cw.try_borrow().is_ok_and(|g| g.autofocus()))
                })
            }) {
                self.set_focus(Some(auto_id), true);
            }
        }

        let Some(focused) = self.focused else {
            return false;
        };
        let focused_rect = self.layout_engine.rect(focused);

        // Forward keys directly to focused CustomWidget if applicable
        if let Some(WidgetKind::Custom(cw)) = self.nodes.get(focused).map(|n| n.kind.clone()) {
            let rect = self.node_visual_rect(focused);
            if let Ok(mut guard) = cw.try_borrow_mut()
                && guard.handle_event(rect, &UiEvent::Key(ke.clone()))
            {
                return true;
            }
        }

        let mut text_changed = None;
        let mut submitted = None;
        let mut activated = false;
        let mut reset_caret = false;
        let mut idx_cb: Option<(IndexCallback, usize)> = None;
        let mut float_cb: Option<(FloatCallback, f32)> = None;
        let mut reorder_cb: Option<(crate::ReorderCallback, usize, usize)> = None;

        // Read clipboard outside node borrow
        let paste_clip = if ke.modifiers.command() && ke.code.matches_ascii_letter('V') {
            self.services
                .read_clipboard()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    if self.clipboard_text.is_empty() {
                        None
                    } else {
                        Some(self.clipboard_text.clone())
                    }
                })
        } else {
            None
        };
        let mut copy_clip: Option<String> = None;

        if let Some(node) = self.nodes.get_mut(focused) {
            if node.is_disabled() {
                return false;
            }
            match &mut node.kind {
                WidgetKind::TextInput {
                    editor,
                    multiline,
                    bound_signal,
                    ..
                } => {
                    let prev = editor.text.clone();
                    match &ke.code {
                        KeyCode::Backspace if ke.modifiers.word_jump() => {
                            editor.delete_word_backward();
                        }
                        KeyCode::Backspace => editor.backspace(),
                        KeyCode::Delete if ke.modifiers.word_jump() => {
                            editor.delete_word_forward();
                        }
                        KeyCode::Delete => editor.delete_forward(),
                        KeyCode::ArrowLeft if ke.modifiers.word_jump() => {
                            editor.move_word_left(ke.modifiers.shift);
                        }
                        KeyCode::ArrowLeft => editor.move_left(ke.modifiers.shift),
                        KeyCode::ArrowRight if ke.modifiers.word_jump() => {
                            editor.move_word_right(ke.modifiers.shift);
                        }
                        KeyCode::ArrowRight => editor.move_right(ke.modifiers.shift),
                        KeyCode::ArrowUp if *multiline => editor.move_up(ke.modifiers.shift),
                        KeyCode::ArrowDown if *multiline => editor.move_down(ke.modifiers.shift),
                        KeyCode::Home => editor.move_home(ke.modifiers.shift),
                        KeyCode::End => editor.move_end(ke.modifiers.shift),
                        code if ke.modifiers.command() && code.matches_ascii_letter('A') => {
                            editor.select_all();
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('C') => {
                            let sel = editor.selected_text().to_string();
                            copy_clip = Some(sel);
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('X') => {
                            let sel = editor.selected_text().to_string();
                            copy_clip = Some(sel);
                            editor.delete_selection();
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('V') => {
                            if let Some(ref clip) = paste_clip {
                                editor.insert_str(clip);
                            }
                        }
                        code if ke.modifiers.command()
                            && !ke.modifiers.shift
                            && code.matches_ascii_letter('Z') =>
                        {
                            let _ = editor.undo();
                        }
                        code if (ke.modifiers.command()
                            && ke.modifiers.shift
                            && code.matches_ascii_letter('Z'))
                            || (ke.modifiers.ctrl
                                && !ke.modifiers.shift
                                && code.matches_ascii_letter('Y')) =>
                        {
                            let _ = editor.redo();
                        }
                        KeyCode::Enter => {
                            if *multiline && !ke.modifiers.command() {
                                editor.insert_str("\n");
                            } else {
                                submitted = Some(editor.text.clone());
                            }
                        }
                        _ => {
                            if let Some(ref txt) = ke.text
                                && !ke.modifiers.command()
                                && !ke.modifiers.ctrl
                                && !ke.modifiers.alt
                                && !txt.chars().all(|c| c.is_control())
                            {
                                editor.insert_str(txt);
                            }
                        }
                    }
                    if editor.text != prev {
                        if let Some(sig) = bound_signal {
                            sig.set(editor.text.clone());
                        }
                        node.a11y.value = Some(editor.text.clone());
                        text_changed = Some(editor.text.clone());
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    reset_caret = true;
                }
                WidgetKind::RichTextEditor { controller, .. } => {
                    let mut next_text: Option<String> = None;
                    controller.update(|ctrl| {
                        let prev = ctrl.editor.text.clone();
                        match &ke.code {
                            KeyCode::Backspace => ctrl.backspace(),
                            KeyCode::Delete => ctrl.delete_forward(),
                            KeyCode::ArrowLeft => ctrl.editor.move_left(ke.modifiers.shift),
                            KeyCode::ArrowRight => ctrl.editor.move_right(ke.modifiers.shift),
                            KeyCode::ArrowUp => ctrl.editor.move_up(ke.modifiers.shift),
                            KeyCode::ArrowDown => ctrl.editor.move_down(ke.modifiers.shift),
                            KeyCode::Home => ctrl.editor.move_home(ke.modifiers.shift),
                            KeyCode::End => ctrl.editor.move_end(ke.modifiers.shift),
                            code if ke.modifiers.command() && code.matches_ascii_letter('A') => {
                                ctrl.editor.select_all();
                            }
                            code if ke.modifiers.command() && code.matches_ascii_letter('B') => {
                                ctrl.toggle_bold();
                            }
                            code if ke.modifiers.command() && code.matches_ascii_letter('I') => {
                                ctrl.toggle_italic();
                            }
                            code if ke.modifiers.command() && code.matches_ascii_letter('U') => {
                                ctrl.toggle_underline();
                            }
                            code if ke.modifiers.command() && code.matches_ascii_letter('V') => {
                                if let Some(ref clip) = paste_clip {
                                    ctrl.insert_str(clip);
                                }
                            }
                            KeyCode::Enter => ctrl.insert_str("\n"),
                            _ => {
                                if let Some(ref txt) = ke.text
                                    && !ke.modifiers.command()
                                    && !ke.modifiers.ctrl
                                    && !ke.modifiers.alt
                                    && !txt.chars().all(|c| c.is_control())
                                {
                                    ctrl.insert_str(txt);
                                }
                            }
                        }
                        if ctrl.editor.text != prev {
                            next_text = Some(ctrl.editor.text.clone());
                        }
                    });
                    if let Some(txt) = next_text {
                        node.a11y.value = Some(txt.clone());
                        text_changed = Some(txt);
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    reset_caret = true;
                }
                WidgetKind::MediaPlayer {
                    controller,
                    on_toggle_play,
                    ..
                } => {
                    if matches!(ke.code, KeyCode::Space | KeyCode::Enter) {
                        let mut playing_now = false;
                        controller.update(|c| {
                            c.toggle_play();
                            playing_now = c.playing;
                        });
                        if let Some(cb) = on_toggle_play.clone() {
                            if let Ok(mut f) = cb.try_borrow_mut() {
                                f(playing_now);
                            }
                        }
                        node.dirty |= DirtyFlags::PAINT;
                    }
                }
                WidgetKind::Button { .. }
                | WidgetKind::Checkbox { .. }
                | WidgetKind::Switch { .. }
                | WidgetKind::Accordion { .. }
                | WidgetKind::ListTile { .. }
                | WidgetKind::Chip { .. } => {
                    if matches!(ke.code, KeyCode::Enter | KeyCode::Space) {
                        activated = true;
                    }
                }
                WidgetKind::Tabs {
                    tabs,
                    active,
                    on_change,
                } => {
                    let count = tabs.get().len();
                    if count > 0 {
                        let cur = active.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            _ => cur,
                        };
                        if next != cur {
                            active.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::SegmentedControl {
                    items,
                    selected,
                    on_change,
                }
                | WidgetKind::BottomNavBar {
                    items,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = items.get().len();
                    if count > 0 {
                        let cur = selected.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            KeyCode::Home => 0,
                            KeyCode::End => count - 1,
                            _ => cur,
                        };
                        if next != cur {
                            selected.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::DataTable {
                    rows,
                    selected,
                    sort_column,
                    sort_ascending,
                    scroll_offset,
                    row_height,
                    on_select,
                    ..
                } => {
                    let row_list = rows.get();
                    if !row_list.is_empty() {
                        let sorted = Self::data_table_sorted_indices(
                            &row_list,
                            *sort_column,
                            *sort_ascending,
                        );
                        let cur_orig = selected.get_untracked();
                        let cur_vis = sorted.iter().position(|&r| r == cur_orig).unwrap_or(0);
                        let next_vis = match ke.code {
                            KeyCode::ArrowDown => (cur_vis + 1).min(sorted.len() - 1),
                            KeyCode::ArrowUp => cur_vis.saturating_sub(1),
                            KeyCode::Home => 0,
                            KeyCode::End => sorted.len() - 1,
                            _ => cur_vis,
                        };
                        if next_vis != cur_vis || sorted[next_vis] != cur_orig {
                            let orig_idx = sorted[next_vis];
                            selected.set(orig_idx);
                            let body_h = (focused_rect.height - 38.0).max(0.0);
                            if *row_height > 0.0 && body_h > 0.0 {
                                let total_h = sorted.len() as f32 * *row_height;
                                let max_scroll = (total_h - body_h).max(0.0);
                                let item_top = next_vis as f32 * *row_height;
                                let item_bottom = item_top + *row_height;
                                if item_top < *scroll_offset {
                                    *scroll_offset = item_top.clamp(0.0, max_scroll);
                                } else if item_bottom > *scroll_offset + body_h {
                                    *scroll_offset = (item_bottom - body_h).clamp(0.0, max_scroll);
                                }
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_select.clone() {
                                idx_cb = Some((cb, orig_idx));
                            }
                        }
                    }
                }
                WidgetKind::Select {
                    options,
                    selected,
                    open,
                    dropdown_scroll,
                    on_change,
                    ..
                } => {
                    let count = options.get().len();
                    let drop_h = (count.max(1) as f32 * 34.0 + 8.0).min(240.0);
                    let total_h = count as f32 * 34.0 + 8.0;
                    let max_scroll = (total_h - drop_h).max(0.0);
                    let ensure_sel_visible = |sel: usize, scroll: &mut f32| {
                        if max_scroll <= 0.0 {
                            *scroll = 0.0;
                            return;
                        }
                        let item_top = sel as f32 * 34.0;
                        let item_bottom = item_top + 34.0;
                        let vis_h = (drop_h - 8.0).max(34.0);
                        let mut next = (*scroll).clamp(0.0, max_scroll);
                        if item_top < next {
                            next = item_top;
                        } else if item_bottom > next + vis_h {
                            next = (item_bottom - vis_h).clamp(0.0, max_scroll);
                        }
                        *scroll = next.clamp(0.0, max_scroll);
                    };
                    match ke.code {
                        KeyCode::Enter | KeyCode::Space => {
                            *open = !*open;
                            if *open && count > 0 {
                                let cur = selected.get_untracked().min(count - 1);
                                ensure_sel_visible(cur, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT;
                        }
                        KeyCode::ArrowDown if count > 0 => {
                            let next = (selected.get_untracked() + 1).min(count - 1);
                            selected.set(next);
                            if *open {
                                ensure_sel_visible(next, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                        KeyCode::ArrowUp if count > 0 => {
                            let next = selected.get_untracked().saturating_sub(1);
                            selected.set(next);
                            if *open {
                                ensure_sel_visible(next, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                        _ => {}
                    }
                }
                WidgetKind::RadioGroup {
                    options,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = options.get().len();
                    if count > 0 {
                        let cur = selected.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowDown | KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowUp | KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            _ => cur,
                        };
                        if next != cur {
                            selected.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::Slider {
                    value,
                    min,
                    max,
                    on_change,
                    ..
                } => {
                    let step = (*max - *min) * 0.05;
                    match ke.code {
                        KeyCode::ArrowRight | KeyCode::ArrowUp => {
                            let next = (value.get_untracked() + step).clamp(*min, *max);
                            value.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                float_cb = Some((cb, next));
                            }
                        }
                        KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                            let next = (value.get_untracked() - step).clamp(*min, *max);
                            value.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                float_cb = Some((cb, next));
                            }
                        }
                        _ => {}
                    }
                }
                WidgetKind::DatePicker {
                    value,
                    view_year,
                    view_month,
                    ..
                } => {
                    let (mut y, mut m, mut d) =
                        crate::pickers::parse_iso_date(&value.get_untracked());
                    let delta: i32 = match ke.code {
                        KeyCode::ArrowRight => 1,
                        KeyCode::ArrowLeft => -1,
                        KeyCode::ArrowDown => 7,
                        KeyCode::ArrowUp => -7,
                        _ => 0,
                    };
                    if delta != 0 {
                        let mut nd = d as i32 + delta;
                        while nd < 1 {
                            if y <= 1900 && m <= 1 {
                                nd = 1;
                                break;
                            }
                            let (py, pm) = crate::pickers::shift_month(y, m, -1);
                            y = py;
                            m = pm;
                            nd += crate::pickers::days_in_month(y, m) as i32;
                        }
                        while nd > crate::pickers::days_in_month(y, m) as i32 {
                            let dim = crate::pickers::days_in_month(y, m) as i32;
                            if y >= 2200 && m >= 12 {
                                nd = dim;
                                break;
                            }
                            nd -= dim;
                            let (ny, nm) = crate::pickers::shift_month(y, m, 1);
                            y = ny;
                            m = nm;
                        }
                        d = (nd as u32).clamp(1, crate::pickers::days_in_month(y, m));
                        *view_year = y;
                        *view_month = m;
                        let formatted = crate::pickers::format_iso_date(y, m, d);
                        value.set(formatted.clone());
                        node.a11y.value = Some(formatted.clone());
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        text_changed = Some(formatted);
                    }
                }
                WidgetKind::TimePicker { value, .. } => {
                    let (mut h, mut m) = crate::pickers::parse_time_hm(&value.get_untracked());
                    let mut changed = true;
                    match ke.code {
                        KeyCode::ArrowUp => h = (h + 1) % 24,
                        KeyCode::ArrowDown => h = (h + 23) % 24,
                        KeyCode::ArrowRight => m = (m + 1) % 60,
                        KeyCode::ArrowLeft => m = (m + 59) % 60,
                        _ => changed = false,
                    }
                    if changed {
                        let formatted = crate::pickers::format_time_hm(h, m);
                        value.set(formatted.clone());
                        node.a11y.value = Some(formatted.clone());
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        text_changed = Some(formatted);
                    }
                }
                WidgetKind::TreeView {
                    items,
                    selected,
                    collapsed,
                    row_height,
                    scroll_offset,
                    on_select,
                } => {
                    let rh = (*row_height).max(22.0);
                    let list = items.get();
                    let mut col = collapsed.borrow_mut();
                    let vis = crate::pickers::derive_visible_tree_rows(&list, &col);
                    if !vis.is_empty() {
                        let cur_raw = selected.get_untracked();
                        let cur_pos = vis.iter().position(|r| r.raw_index == cur_raw).unwrap_or(0);
                        let mut reveal_pos: Option<usize> = None;
                        match ke.code {
                            KeyCode::ArrowDown => {
                                let next_pos = (cur_pos + 1).min(vis.len() - 1);
                                reveal_pos = Some(next_pos);
                                let raw_idx = vis[next_pos].raw_index;
                                selected.set(raw_idx);
                                node.a11y.numeric_value = Some(raw_idx as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_select.clone() {
                                    idx_cb = Some((cb, raw_idx));
                                }
                            }
                            KeyCode::ArrowUp => {
                                let next_pos = cur_pos.saturating_sub(1);
                                reveal_pos = Some(next_pos);
                                let raw_idx = vis[next_pos].raw_index;
                                selected.set(raw_idx);
                                node.a11y.numeric_value = Some(raw_idx as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_select.clone() {
                                    idx_cb = Some((cb, raw_idx));
                                }
                            }
                            KeyCode::ArrowRight
                                if vis[cur_pos].has_children && !vis[cur_pos].expanded =>
                            {
                                col.remove(&vis[cur_pos].path);
                                node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            }
                            KeyCode::ArrowLeft
                                if vis[cur_pos].has_children && vis[cur_pos].expanded =>
                            {
                                col.insert(vis[cur_pos].path.clone());
                                node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            }
                            _ => {}
                        }
                        if let Some(pos) = reveal_pos {
                            let view_h = (focused_rect.height - 8.0).max(rh);
                            let row_top = pos as f32 * rh;
                            if row_top < *scroll_offset {
                                *scroll_offset = row_top;
                            } else if row_top + rh > *scroll_offset + view_h {
                                *scroll_offset = (row_top + rh - view_h).max(0.0);
                            }
                        }
                    }
                }
                WidgetKind::ReorderableList {
                    items,
                    selected,
                    row_height,
                    scroll_offset,
                    on_reorder,
                    on_select,
                    ..
                } => {
                    let rh = (*row_height).max(24.0);
                    let len = items.get_untracked().len();
                    if len > 0 {
                        let cur = selected.get_untracked().min(len - 1);
                        let mut reveal_pos: Option<usize> = None;
                        match ke.code {
                            KeyCode::ArrowDown if ke.modifiers.alt && cur + 1 < len => {
                                let next = cur + 1;
                                reveal_pos = Some(next);
                                items.update(|list| list.swap(cur, next));
                                selected.set(next);
                                node.a11y.numeric_value = Some(next as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_reorder.clone() {
                                    reorder_cb = Some((cb, cur, next));
                                }
                            }
                            KeyCode::ArrowUp if ke.modifiers.alt && cur > 0 => {
                                let next = cur - 1;
                                reveal_pos = Some(next);
                                items.update(|list| list.swap(cur, next));
                                selected.set(next);
                                node.a11y.numeric_value = Some(next as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_reorder.clone() {
                                    reorder_cb = Some((cb, cur, next));
                                }
                            }
                            KeyCode::ArrowDown => {
                                let next = (cur + 1).min(len - 1);
                                reveal_pos = Some(next);
                                selected.set(next);
                                node.a11y.numeric_value = Some(next as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_select.clone() {
                                    idx_cb = Some((cb, next));
                                }
                            }
                            KeyCode::ArrowUp => {
                                let next = cur.saturating_sub(1);
                                reveal_pos = Some(next);
                                selected.set(next);
                                node.a11y.numeric_value = Some(next as f64);
                                node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                                if let Some(cb) = on_select.clone() {
                                    idx_cb = Some((cb, next));
                                }
                            }
                            _ => {}
                        }
                        if let Some(pos) = reveal_pos {
                            let view_h = (focused_rect.height - 8.0).max(rh);
                            let row_top = pos as f32 * rh;
                            if row_top < *scroll_offset {
                                *scroll_offset = row_top;
                            } else if row_top + rh > *scroll_offset + view_h {
                                *scroll_offset = (row_top + rh - view_h).max(0.0);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        self.sync_control_anim_target(focused);

        if reset_caret {
            self.reset_caret_blink(craftui_core::monotonic_ms());
        }

        if let Some(clip) = copy_clip {
            self.clipboard_text = clip.clone();
            self.services.write_clipboard(&clip);
        }

        if activated {
            let center = self.node_visual_rect(focused).center();
            self.activate_tap(focused, center, false);
        }

        if let Some((cb, i)) = idx_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(i);
        }
        if let Some((cb, v)) = float_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }
        if let Some((cb, from, to)) = reorder_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(from, to);
        }

        if let Some(txt) = text_changed {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_change, .. }
                | WidgetKind::DatePicker { on_change, .. }
                | WidgetKind::TimePicker { on_change, .. }
                | WidgetKind::RichTextEditor { on_change, .. } => on_change.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        if let Some(txt) = submitted {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_submit, .. } => on_submit.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        true
    }

    pub(crate) fn handle_ime(&mut self, ime: &craftui_core::ImeEvent) -> bool {
        let Some(focused) = self.focused else {
            return false;
        };
        let Some(node) = self.nodes.get_mut(focused) else {
            return false;
        };
        if node.is_disabled() {
            return false;
        }
        let mut changed_cb: Option<(crate::widgets::TextCallback, String)> = None;
        match &mut node.kind {
            WidgetKind::TextInput {
                editor,
                bound_signal,
                on_change,
                ..
            } => match ime {
                craftui_core::ImeEvent::Preedit(s, cur) => {
                    editor.preedit = if s.is_empty() { None } else { Some(s.clone()) };
                    editor.preedit_cursor = *cur;
                }
                craftui_core::ImeEvent::Commit(s) => {
                    editor.preedit = None;
                    editor.preedit_cursor = None;
                    editor.insert_str(s);
                    node.a11y.value = Some(editor.text.clone());
                    if let Some(sig) = bound_signal {
                        sig.set(editor.text.clone());
                    }
                    if let Some(cb) = on_change.clone() {
                        changed_cb = Some((cb, editor.text.clone()));
                    }
                }
                craftui_core::ImeEvent::Disabled => {
                    editor.preedit = None;
                    editor.preedit_cursor = None;
                }
                craftui_core::ImeEvent::Enabled => {}
            },
            WidgetKind::RichTextEditor {
                controller,
                on_change,
                ..
            } => match ime {
                craftui_core::ImeEvent::Preedit(s, cur) => {
                    controller.update(|c| {
                        c.editor.preedit = if s.is_empty() { None } else { Some(s.clone()) };
                        c.editor.preedit_cursor = *cur;
                    });
                }
                craftui_core::ImeEvent::Commit(s) => {
                    let mut updated = String::new();
                    controller.update(|c| {
                        c.editor.preedit = None;
                        c.editor.preedit_cursor = None;
                        c.insert_str(s);
                        updated = c.editor.text.clone();
                    });
                    node.a11y.value = Some(updated.clone());
                    if let Some(cb) = on_change.clone() {
                        changed_cb = Some((cb, updated));
                    }
                }
                craftui_core::ImeEvent::Disabled => {
                    controller.update(|c| {
                        c.editor.preedit = None;
                        c.editor.preedit_cursor = None;
                    });
                }
                craftui_core::ImeEvent::Enabled => {}
            },
            _ => return false,
        }
        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        if let Some((cb, txt)) = changed_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(txt);
        }
        self.reset_caret_blink(craftui_core::monotonic_ms());
        true
    }
}
