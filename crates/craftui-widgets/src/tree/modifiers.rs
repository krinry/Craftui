use super::*;

impl WidgetTree {
    pub fn set_layout(&mut self, node: NodeId, mut layout: LayoutStyle) {
        if let Some(n) = self.nodes.get_mut(node) {
            if matches!(n.kind, WidgetKind::SafeArea) {
                layout.safe_area = true;
            }
            if matches!(n.kind, WidgetKind::Accordion { .. }) {
                layout.header_top_inset = 44.0;
            }
            n.layout = layout;
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            let eff = Self::effective_taffy_layout(n);
            self.layout_engine.upsert_node(node, eff);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn set_box_style(&mut self, node: NodeId, box_style: BoxStyle) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if let Some(bg) = box_style.background.clone()
            && bg.is_dynamic()
        {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.box_bg) {
                eff_scope.effect(move || {
                    let _ = bg.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.box_bg);
        }
        if let Some(bc) = box_style.border_color.clone()
            && bc.is_dynamic()
        {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.box_border_color) {
                eff_scope.effect(move || {
                    let _ = bc.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.box_border_color);
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style = box_style;
            n.dirty |= DirtyFlags::PAINT;
        }
        self.sync_node_anim_targets(node);
    }

    pub fn set_transform(&mut self, node: NodeId, transform: Transform2D) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.transform = transform;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_backdrop_blur(&mut self, node: NodeId, blur_radius: f32, tint: Color) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.backdrop_blur = blur_radius.max(0.0);
            n.box_style.backdrop_tint = tint;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_shadows(&mut self, node: NodeId, shadows: Vec<BoxShadow>) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.shadows = shadows;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_blend_mode(&mut self, node: NodeId, blend_mode: BlendMode) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.blend_mode = blend_mode;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_cursor(&mut self, node: NodeId, cursor: Option<CursorIcon>) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.cursor = cursor;
        }
    }

    /// Replace draw and/or pointer callbacks on an existing [`WidgetKind::Canvas`] node.
    pub fn set_canvas_callbacks(
        &mut self,
        node: NodeId,
        on_paint: Option<crate::widgets::CanvasPaintFn>,
        on_pointer: Option<CanvasPointerFn>,
    ) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Canvas {
                on_paint: p_slot,
                on_pointer: ptr_slot,
            } = &mut n.kind
        {
            if let Some(p) = on_paint {
                *p_slot = p;
            }
            if let Some(ptr) = on_pointer {
                *ptr_slot = Some(ptr);
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    /// Bind a reactive property updater that re-runs during `layout()` whenever any tracked signal changes.
    pub fn bind_dynamic_prop(
        &mut self,
        node: NodeId,
        dirty_mask: DirtyFlags,
        track: impl Fn() + 'static,
        mut apply: impl FnMut(NodeId, &mut WidgetTree) + 'static,
    ) {
        if !self.nodes.contains_key(node) {
            return;
        }
        apply(node, self);

        let Some(node_scope) = self.nodes.get(node).map(|n| n.scope) else {
            return;
        };
        let eff_scope = node_scope.child_scope();
        eff_scope.effect(move || {
            track();
            mark_node_dirty(node, dirty_mask);
        });
        if !self.prop_scopes.contains_key(node) {
            self.prop_scopes.insert(node, PropEffectScopes::default());
        }
        if let Some(scopes) = self.prop_scopes.get_mut(node) {
            scopes.dynamic_props.push(eff_scope);
        }
        if !self.dynamic_prop_updaters.contains_key(node) {
            self.dynamic_prop_updaters.insert(node, Vec::new());
        }
        if let Some(updaters) = self.dynamic_prop_updaters.get_mut(node) {
            updaters.push((dirty_mask, Box::new(apply)));
        }
    }

    /// Configure a smooth transition curve for a specific property on `node`.
    pub fn set_transition(&mut self, node: NodeId, prop: AnimatableProp, curve: TransitionCurve) {
        let Some(n) = self.nodes.get(node) else {
            return;
        };
        let initial_opacity = n.box_style.opacity;
        let initial_bg = n
            .box_style
            .background
            .as_ref()
            .and_then(|rv| match rv.get() {
                Brush::Solid(c) => Some(c),
                _ => None,
            });
        let initial_bc = n
            .box_style
            .border_color
            .as_ref()
            .map(ReactiveValue::get)
            .or_else(|| n.box_style.border.map(|b| b.color));

        if !self.node_anims.contains_key(node) {
            self.node_anims.insert(node, NodeAnimState::default());
        }
        let Some(anim) = self.node_anims.get_mut(node) else {
            return;
        };
        anim.curves.insert(prop, curve);
        match prop {
            AnimatableProp::Opacity => {
                if let Some(a) = anim.opacity.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.opacity = Some(AnimatedFloat::new(initial_opacity, curve));
                }
            }
            AnimatableProp::TranslateX => {
                if let Some(a) = anim.translate_x.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.translate_x = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::TranslateY => {
                if let Some(a) = anim.translate_y.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.translate_y = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::ScaleX => {
                if let Some(a) = anim.scale_x.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.scale_x = Some(AnimatedFloat::new(1.0, curve));
                }
            }
            AnimatableProp::ScaleY => {
                if let Some(a) = anim.scale_y.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.scale_y = Some(AnimatedFloat::new(1.0, curve));
                }
            }
            AnimatableProp::Rotation => {
                if let Some(a) = anim.rotation.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.rotation = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::BackgroundColor => {
                if let Some(ac) = anim.bg_color.as_mut() {
                    ac.set_curve(curve);
                } else if let Some(c) = initial_bg {
                    anim.bg_color = Some(AnimatedColor::new(c, curve));
                }
            }
            AnimatableProp::BorderColor => {
                if let Some(ac) = anim.border_color.as_mut() {
                    ac.set_curve(curve);
                } else if let Some(c) = initial_bc {
                    anim.border_color = Some(AnimatedColor::new(c, curve));
                }
            }
        }
    }

    pub fn set_opacity(&mut self, node: NodeId, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.opacity = clamped;
            n.dirty |= DirtyFlags::PAINT;
        }
        if let Some(anim) = self.node_anims.get_mut(node)
            && let Some(&curve) = anim.curves.get(&AnimatableProp::Opacity)
        {
            if let Some(a) = anim.opacity.as_mut() {
                a.set_curve(curve);
                a.set_target(clamped);
            } else {
                let mut a = AnimatedFloat::new(clamped, curve);
                a.set_target(clamped);
                anim.opacity = Some(a);
            }
        }
    }

    pub fn set_translate(&mut self, node: NodeId, dx: f32, dy: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cx) = anim.curves.get(&AnimatableProp::TranslateX) {
                let slot = anim
                    .translate_x
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cx));
                slot.set_curve(cx);
                slot.set_target(dx);
            } else {
                let slot = anim
                    .translate_x
                    .get_or_insert_with(|| AnimatedFloat::new(dx, TransitionCurve::instant()));
                slot.snap(dx);
            }
            if let Some(&cy) = anim.curves.get(&AnimatableProp::TranslateY) {
                let slot = anim
                    .translate_y
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cy));
                slot.set_curve(cy);
                slot.set_target(dy);
            } else {
                let slot = anim
                    .translate_y
                    .get_or_insert_with(|| AnimatedFloat::new(dy, TransitionCurve::instant()));
                slot.snap(dy);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_scale(&mut self, node: NodeId, sx: f32, sy: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if (sx - 1.0).abs() < 1e-6 && (sy - 1.0).abs() < 1e-6 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cx) = anim.curves.get(&AnimatableProp::ScaleX) {
                let slot = anim
                    .scale_x
                    .get_or_insert_with(|| AnimatedFloat::new(1.0, cx));
                slot.set_curve(cx);
                slot.set_target(sx);
            } else {
                let slot = anim
                    .scale_x
                    .get_or_insert_with(|| AnimatedFloat::new(sx, TransitionCurve::instant()));
                slot.snap(sx);
            }
            if let Some(&cy) = anim.curves.get(&AnimatableProp::ScaleY) {
                let slot = anim
                    .scale_y
                    .get_or_insert_with(|| AnimatedFloat::new(1.0, cy));
                slot.set_curve(cy);
                slot.set_target(sy);
            } else {
                let slot = anim
                    .scale_y
                    .get_or_insert_with(|| AnimatedFloat::new(sy, TransitionCurve::instant()));
                slot.snap(sy);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_rotation(&mut self, node: NodeId, radians: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if radians == 0.0 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cr) = anim.curves.get(&AnimatableProp::Rotation) {
                let slot = anim
                    .rotation
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cr));
                slot.set_curve(cr);
                slot.set_target(radians);
            } else {
                let slot = anim
                    .rotation
                    .get_or_insert_with(|| AnimatedFloat::new(radians, TransitionCurve::instant()));
                slot.snap(radians);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub(crate) fn sync_node_anim_targets(&mut self, node: NodeId) {
        let Some(n) = self.nodes.get(node) else {
            return;
        };
        let Some(anim) = self.node_anims.get_mut(node) else {
            return;
        };
        if let Some(&curve) = anim.curves.get(&AnimatableProp::Opacity) {
            let slot = anim
                .opacity
                .get_or_insert_with(|| AnimatedFloat::new(n.box_style.opacity, curve));
            slot.set_target(n.box_style.opacity.clamp(0.0, 1.0));
        }
        if let Some(&curve) = anim.curves.get(&AnimatableProp::BackgroundColor)
            && let Some(ref bg) = n.box_style.background
            && let Brush::Solid(target_c) = bg.get()
        {
            if let Some(ac) = anim.bg_color.as_mut() {
                ac.set_target(target_c);
            } else {
                anim.bg_color = Some(AnimatedColor::new(target_c, curve));
            }
        }
        if let Some(&curve) = anim.curves.get(&AnimatableProp::BorderColor) {
            let target_bc = n
                .box_style
                .border_color
                .as_ref()
                .map(ReactiveValue::get)
                .or_else(|| n.box_style.border.map(|b| b.color));
            if let Some(target_c) = target_bc {
                if let Some(ac) = anim.border_color.as_mut() {
                    ac.set_target(target_c);
                } else {
                    anim.border_color = Some(AnimatedColor::new(target_c, curve));
                }
            }
        }
    }

    pub(crate) fn node_effective_transform(&self, id: NodeId, pivot: Point) -> Transform2D {
        let base = self
            .nodes
            .get(id)
            .map(|n| n.box_style.transform)
            .unwrap_or(Transform2D::IDENTITY);
        let anim = self.node_anims.get(id);
        let sx = anim
            .and_then(|a| a.scale_x.as_ref())
            .map_or(1.0, |v| v.value);
        let sy = anim
            .and_then(|a| a.scale_y.as_ref())
            .map_or(1.0, |v| v.value);
        let rot = anim
            .and_then(|a| a.rotation.as_ref())
            .map_or(0.0, |v| v.value);
        let tx = anim
            .and_then(|a| a.translate_x.as_ref())
            .map_or(0.0, |v| v.value);
        let ty = anim
            .and_then(|a| a.translate_y.as_ref())
            .map_or(0.0, |v| v.value);

        let has_scale = (sx - 1.0).abs() > 1e-5 || (sy - 1.0).abs() > 1e-5;
        let has_rot = rot.abs() > 1e-5;
        let has_trans = tx.abs() > 1e-5 || ty.abs() > 1e-5;
        if base.is_identity() && !has_scale && !has_rot && !has_trans {
            return Transform2D::IDENTITY;
        }
        let mut inner = base;
        if has_scale {
            inner = inner.then(Transform2D::scale(sx, sy));
        }
        if has_rot {
            inner = inner.then(Transform2D::rotation(rot));
        }
        let mut xf = if inner.is_identity() {
            Transform2D::IDENTITY
        } else {
            Transform2D::around_pivot(pivot, inner)
        };
        if has_trans {
            xf = xf.then(Transform2D::translation(tx, ty));
        }
        xf
    }

    pub(crate) fn default_cursor_for_node(node: &WidgetNode) -> CursorIcon {
        if node.is_disabled() {
            return CursorIcon::NotAllowed;
        }
        match &node.kind {
            WidgetKind::Button { .. }
            | WidgetKind::FloatingActionButton { .. }
            | WidgetKind::Checkbox { .. }
            | WidgetKind::Switch { .. }
            | WidgetKind::Tabs { .. }
            | WidgetKind::SegmentedControl { .. }
            | WidgetKind::BottomNavBar { .. }
            | WidgetKind::Accordion { .. }
            | WidgetKind::DataTable { .. }
            | WidgetKind::Select { .. }
            | WidgetKind::RadioGroup { .. } => CursorIcon::Pointer,
            WidgetKind::RichText {
                on_link: Some(_), ..
            } => CursorIcon::Pointer,
            WidgetKind::TextInput { .. } => CursorIcon::Text,
            WidgetKind::Slider { .. } => CursorIcon::Grab,
            WidgetKind::SplitPane { axis, .. } => match axis {
                Axis::Horizontal => CursorIcon::ResizeHorizontal,
                Axis::Vertical => CursorIcon::ResizeVertical,
            },
            _ if node.on_tap.is_some()
                || node.on_double_tap.is_some()
                || node.on_long_press.is_some() =>
            {
                CursorIcon::Pointer
            }
            _ => CursorIcon::Default,
        }
    }

    /// Resolve cursor shape hint for a hovered node, checking active drags and ancestor overrides first.
    pub fn hovered_cursor(&self) -> CursorIcon {
        self.cursor_for_node_at(self.hovered, self.last_pointer_pos)
    }

    /// Hit-test `point` and return the effective [`CursorIcon`] under the pointer.
    pub fn cursor_at(&self, point: Point) -> CursorIcon {
        let hit = self.hit_test(point);
        self.cursor_for_node_at(hit, Some(point))
    }

    pub(crate) fn cursor_for_node_at(
        &self,
        start: Option<NodeId>,
        point_opt: Option<Point>,
    ) -> CursorIcon {
        for n in self.nodes.values() {
            if let WidgetKind::SplitPane {
                axis,
                dragging: true,
                ..
            } = &n.kind
            {
                return match axis {
                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                    Axis::Vertical => CursorIcon::ResizeVertical,
                };
            }
        }
        if self
            .control_anims
            .values()
            .any(|anim| matches!(anim, ControlVisualAnim::Slider { dragging: true, .. }))
        {
            return CursorIcon::Grabbing;
        }

        if let Some(id) = start {
            if let Some(ControlVisualAnim::Slider { dragging: true, .. }) =
                self.control_anims.get(id)
            {
                return CursorIcon::Grabbing;
            }
            let mut cur = Some(id);
            while let Some(nid) = cur {
                if let Some(n) = self.nodes.get(nid) {
                    if let Some(c) = n.box_style.cursor {
                        return if n.is_disabled() {
                            CursorIcon::NotAllowed
                        } else {
                            c
                        };
                    }
                    if nid == id {
                        if let WidgetKind::SplitPane {
                            axis,
                            ratio,
                            min_first,
                            min_second,
                            divider_size,
                            ..
                        } = &n.kind
                        {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let inner = self.node_visual_rect(nid).inset(n.layout.padding);
                                let (_, _, div_rect) = Self::split_divider_geometry(
                                    inner,
                                    *axis,
                                    ratio.get_untracked(),
                                    *min_first,
                                    *min_second,
                                    *divider_size,
                                );
                                if div_rect.inflate(4.0, 4.0).contains(local_pt) {
                                    return match axis {
                                        Axis::Horizontal => CursorIcon::ResizeHorizontal,
                                        Axis::Vertical => CursorIcon::ResizeVertical,
                                    };
                                }
                            } else {
                                return match axis {
                                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                                    Axis::Vertical => CursorIcon::ResizeVertical,
                                };
                            }
                        } else if let WidgetKind::Accordion { .. } = &n.kind {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let base_rect = self.node_visual_rect(nid);
                                if local_pt.y <= base_rect.y + 44.0 {
                                    return CursorIcon::Pointer;
                                }
                                return CursorIcon::Default;
                            }
                            return CursorIcon::Pointer;
                        } else if let WidgetKind::ToastHost { items, .. } = &n.kind {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let base_rect = self.node_visual_rect(nid);
                                let (_, closes) =
                                    Self::toast_card_rects(base_rect, items.get().len());
                                if closes
                                    .iter()
                                    .any(|cr| cr.inflate(6.0, 6.0).contains(local_pt))
                                {
                                    return CursorIcon::Pointer;
                                }
                            }
                        } else {
                            let sem = Self::default_cursor_for_node(n);
                            if sem != CursorIcon::Default {
                                return sem;
                            }
                        }
                    }
                    cur = n.parent;
                } else {
                    break;
                }
            }
        }
        CursorIcon::Default
    }

    pub fn set_text_input_style(&mut self, node: NodeId, style: TextStyle) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { style: st, .. } = &mut n.kind
        {
            *st = style;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_button_foreground(&mut self, node: NodeId, fg: impl Into<ReactiveValue<Color>>) {
        let rv = fg.into();
        let is_btn = self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Button { .. }));
        if !is_btn {
            return;
        }
        if rv.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.button_fg) {
                let rv_c = rv.clone();
                eff_scope.effect(move || {
                    let _ = rv_c.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.button_fg);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Button { foreground, .. } = &mut n.kind
        {
            *foreground = Some(rv);
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_disabled(&mut self, node: NodeId, disabled: impl Into<ReactiveValue<bool>>) {
        let rv = disabled.into();
        if !self.nodes.contains_key(node) {
            return;
        }
        if rv.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.disabled) {
                let rv_c = rv.clone();
                eff_scope.effect(move || {
                    let _ = rv_c.get();
                    mark_node_dirty(node, DirtyFlags::PAINT | DirtyFlags::A11Y);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.disabled);
        }
        if let Some(n) = self.nodes.get_mut(node) {
            if let WidgetKind::Button { disabled: d, .. } = &mut n.kind {
                *d = rv.clone();
            }
            n.disabled = Some(rv);
            n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }
    }

    pub fn set_image_radius(&mut self, node: NodeId, rad: CornerRadii) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Image { radius, .. } = &mut n.kind
        {
            *radius = rad;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_on_tap(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: TapHandler = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Button { on_tap, .. }
                | WidgetKind::FloatingActionButton { on_tap, .. } => {
                    *on_tap = Some(rc);
                }
                _ => {
                    n.on_tap = Some(rc);
                }
            }
        }
    }

    /// Handle link activation in a rich-text widget. Targets are passed to
    /// the application; CraftUI does not open them or assume they are URLs.
    pub fn set_on_link(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::RichText { on_link, .. } = &mut n.kind
        {
            *on_link = Some(Rc::new(RefCell::new(cb)));
            n.dirty |= DirtyFlags::A11Y;
        }
    }

    pub fn set_on_double_tap(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_double_tap = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_long_press(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_long_press = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_hover(&mut self, node: NodeId, cb: impl FnMut(bool) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_hover = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_pinch_rotate(&mut self, node: NodeId, cb: impl FnMut(Point, f32, f32) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_pinch_rotate = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_change(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: crate::TextCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::TextInput { on_change, .. }
                | WidgetKind::DatePicker { on_change, .. }
                | WidgetKind::TimePicker { on_change, .. } => {
                    *on_change = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_bool_change(&mut self, node: NodeId, cb: impl FnMut(bool) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: BoolCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Checkbox { on_change, .. }
                | WidgetKind::Switch { on_change, .. }
                | WidgetKind::Accordion { on_change, .. } => {
                    *on_change = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_float_change(&mut self, node: NodeId, cb: impl FnMut(f32) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: FloatCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Slider { on_change, .. } | WidgetKind::SplitPane { on_change, .. } => {
                    *on_change = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_index_change(&mut self, node: NodeId, cb: impl FnMut(usize) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: IndexCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Tabs { on_change, .. }
                | WidgetKind::SegmentedControl { on_change, .. }
                | WidgetKind::BottomNavBar { on_change, .. }
                | WidgetKind::Select { on_change, .. }
                | WidgetKind::RadioGroup { on_change, .. } => {
                    *on_change = Some(rc);
                }
                WidgetKind::DataTable { on_select, .. }
                | WidgetKind::PopupMenu { on_select, .. }
                | WidgetKind::TreeView { on_select, .. } => {
                    *on_select = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_submit(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { on_submit, .. } = &mut n.kind
        {
            *on_submit = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_close(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: TapHandler = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Overlay { on_close, .. } | WidgetKind::PopupMenu { on_close, .. } => {
                    *on_close = Some(rc);
                }
                _ => {}
            }
        }
    }

    /// Reactively toggle whether `node` participates in layout and painting.
    pub fn set_visible_when(&mut self, node: NodeId, pred: impl Fn() -> bool + 'static) {
        let rv = ReactiveValue::Dynamic(Rc::new(pred));
        if !self.nodes.contains_key(node) {
            return;
        }
        if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.visible_when) {
            let rv_c = rv.clone();
            eff_scope.effect(move || {
                let _ = rv_c.get();
                mark_node_dirty(node, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
            });
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.visible_when = Some(rv);
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
        }
    }
}
