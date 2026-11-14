use super::*;

impl WidgetTree {
    /// Record [`PaintScene`] commands from the retained widget tree.
    pub fn paint(&mut self, scene: &mut PaintScene) {
        scene.clear();
        let Some(root) = self.root else {
            return;
        };
        let theme = self.theme_signal.get_untracked();
        let mut floating_selects: Vec<(Rect, Vec<String>, usize, f32)> = Vec::new();
        let mut active_tooltips: Vec<(Rect, String)> = Vec::new();
        self.paint_node_recursive(
            root,
            Point::ZERO,
            Transform2D::IDENTITY,
            &theme,
            scene,
            &mut floating_selects,
            &mut active_tooltips,
        );

        // Paint floating Select dropdown popups on top of main UI
        for (drop_rect, opts, sel_idx, dropdown_scroll) in floating_selects {
            scene.drop_shadow(
                drop_rect,
                CornerRadii::all(8.0),
                BoxShadow::drop(0.0, 4.0, 12.0, 0.0, Color::rgba(0, 0, 0, 110)),
            );
            scene.fill_rect(drop_rect, theme.surface, CornerRadii::all(8.0));
            scene.stroke_rect(drop_rect, theme.border, 1.0, CornerRadii::all(8.0));
            let total_h = opts.len() as f32 * 34.0 + 8.0;
            let max_scroll = (total_h - drop_rect.height).max(0.0);
            let scroll = dropdown_scroll.clamp(0.0, max_scroll);
            scene.push(PaintCommand::PushClip {
                rect: drop_rect.inset(Insets::all(2.0)),
                radius: CornerRadii::all(6.0),
            });
            for (i, opt) in opts.iter().enumerate() {
                let row_y = drop_rect.y + 4.0 + i as f32 * 34.0 - scroll;
                if row_y + 32.0 < drop_rect.y || row_y > drop_rect.max_y() {
                    continue;
                }
                let item_r = Rect::new(drop_rect.x + 4.0, row_y, drop_rect.width - 8.0, 32.0);
                if i == sel_idx {
                    scene.fill_rect(
                        item_r,
                        theme.primary.with_alpha(0.18),
                        CornerRadii::all(6.0),
                    );
                }
                scene.draw_text(
                    Point::new(item_r.x + 10.0, item_r.y + 8.0),
                    opt,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_weight(if i == sel_idx { 600.0 } else { 400.0 })
                        .with_color(if i == sel_idx {
                            theme.primary
                        } else {
                            theme.text_primary
                        }),
                    Some((item_r.width - 20.0).max(20.0)),
                );
            }
            if max_scroll > 0.5 {
                let ratio = (drop_rect.height / total_h).clamp(0.12, 1.0);
                let track_h = (drop_rect.height - 8.0).max(8.0);
                let thumb_h = (track_h * ratio).max(16.0);
                let pos_ratio = (scroll / max_scroll).clamp(0.0, 1.0);
                let thumb_y = drop_rect.y + 4.0 + (track_h - thumb_h) * pos_ratio;
                scene.fill_rect(
                    Rect::new(drop_rect.max_x() - 6.0, thumb_y, 3.5, thumb_h),
                    theme.text_secondary.with_alpha(0.55),
                    CornerRadii::all(2.0),
                );
            }
            scene.push(PaintCommand::PopClip);
        }

        // Paint active hover tooltips on top of main UI & dropdowns
        for (target_rect, tip_text) in active_tooltips {
            if tip_text.is_empty() {
                continue;
            }
            let st = TextStyle::default()
                .with_size(12.0)
                .with_color(Color::WHITE);
            let measured = self.text_engine.measure(&tip_text, &st, Some(240.0));
            let w = (measured.width + 18.0).max(40.0);
            let h = measured.height + 10.0;
            let tx = (target_rect.center().x - w * 0.5)
                .clamp(6.0, (self.metrics.logical_size.width - w - 6.0).max(6.0));
            let ty = if target_rect.y >= h + 10.0 {
                target_rect.y - h - 6.0
            } else {
                target_rect.max_y() + 6.0
            };
            let tip_rect = Rect::new(tx, ty, w, h);
            scene.drop_shadow(
                tip_rect,
                CornerRadii::all(6.0),
                BoxShadow::drop(0.0, 3.0, 8.0, 0.0, Color::rgba(0, 0, 0, 90)),
            );
            scene.fill_rect(
                tip_rect,
                Color::rgba(15, 23, 42, 240),
                CornerRadii::all(6.0),
            );
            scene.stroke_rect(tip_rect, theme.border, 1.0, CornerRadii::all(6.0));
            scene.draw_text(Point::new(tx + 9.0, ty + 5.0), &tip_text, st, Some(240.0));
        }

        if self.inspector.enabled {
            let mut boxes = Vec::new();
            for (id, node) in &self.nodes {
                let r = self.layout_engine.rect(id);
                if !r.size().is_empty() {
                    let xr = self
                        .node_world_transform(id)
                        .transform_rect_bounds(self.node_visual_rect(id));
                    boxes.push((xr, node.debug_name));
                }
            }
            self.inspector
                .paint_overlay(scene, &boxes, self.metrics.logical_size.width);
        }
    }

    #[inline]
    pub(crate) fn union_rects(a: Rect, b: Rect) -> Rect {
        let min_x = a.x.min(b.x);
        let min_y = a.y.min(b.y);
        let max_x = a.max_x().max(b.max_x());
        let max_y = a.max_y().max(b.max_y());
        Rect::new(
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        )
    }

    #[inline]
    pub(crate) fn expand_rect_for_shadow(rect: Rect, s: BoxShadow) -> Rect {
        if s.inset {
            return rect;
        }
        let pad = s.blur_radius.max(0.0) * 2.5 + s.spread.max(0.0) + 4.0;
        let sr = rect.translate(s.offset_x, s.offset_y).inflate(pad, pad);
        Self::union_rects(rect, sr)
    }

    pub(crate) fn accumulate_subtree_bounds(
        &self,
        id: NodeId,
        scroll_offset: Point,
        to_layer_local: Transform2D,
        bounds: &mut Rect,
    ) {
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
            _ => {}
        }

        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let local_xf = self.node_effective_transform(id, rect.center());
        let xform = to_layer_local * local_xf;

        let mut local_r = rect.inflate(4.0, 4.0);
        if let Some(s) = node.box_style.shadow {
            local_r = Self::expand_rect_for_shadow(local_r, s);
        }
        for &s in &node.box_style.shadows {
            local_r = Self::expand_rect_for_shadow(local_r, s);
        }
        let mapped = if xform.is_identity() {
            local_r
        } else {
            xform.transform_rect_bounds(local_r)
        };
        *bounds = Self::union_rects(*bounds, mapped);

        if node.box_style.clip
            || matches!(
                &node.kind,
                WidgetKind::Accordion { expanded, .. } if !expanded.get_untracked()
            )
        {
            return;
        }
        let mut child_scroll = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &node.kind {
            match axis {
                Axis::Vertical => child_scroll.y -= *offset,
                Axis::Horizontal => child_scroll.x -= *offset,
            }
        }
        for &child in &node.children {
            self.accumulate_subtree_bounds(child, child_scroll, xform, bounds);
        }
    }

    pub(crate) fn compute_subtree_layer_bounds(
        &self,
        _id: NodeId,
        rect: Rect,
        child_offset: Point,
        box_style: &BoxStyle,
        children: &[NodeId],
    ) -> Rect {
        let mut bounds = rect.inflate(4.0, 4.0);
        if let Some(s) = box_style.shadow {
            bounds = Self::expand_rect_for_shadow(bounds, s);
        }
        for &s in &box_style.shadows {
            bounds = Self::expand_rect_for_shadow(bounds, s);
        }
        if !box_style.clip {
            for &child in children {
                self.accumulate_subtree_bounds(
                    child,
                    child_offset,
                    Transform2D::IDENTITY,
                    &mut bounds,
                );
            }
        }
        bounds
    }

    pub(crate) fn hovered_has_ancestor(&self, ancestor: NodeId) -> bool {
        let mut cursor = self.hovered;
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self.nodes.get(id).and_then(|n| n.parent);
        }
        false
    }

    pub(crate) fn adapt_brush_to_rect(brush: &Brush, rect: Rect) -> Brush {
        match brush {
            Brush::Solid(c) => Brush::Solid(*c),
            Brush::LinearGradient { start, end, stops } => {
                let s = if start.x <= 1.0 && start.y <= 1.0 && rect.width > 1.0 && rect.height > 1.0
                {
                    Point::new(
                        rect.x + start.x * rect.width,
                        rect.y + start.y * rect.height,
                    )
                } else {
                    *start
                };
                let e = if end.x <= 1.0 && end.y <= 1.0 && rect.width > 1.0 && rect.height > 1.0 {
                    Point::new(rect.x + end.x * rect.width, rect.y + end.y * rect.height)
                } else {
                    *end
                };
                Brush::LinearGradient {
                    start: s,
                    end: e,
                    stops: stops.clone(),
                }
            }
            Brush::RadialGradient {
                center,
                radius,
                stops,
            } => {
                let c = if center.x <= 1.0
                    && center.y <= 1.0
                    && rect.width > 1.0
                    && rect.height > 1.0
                {
                    Point::new(
                        rect.x + center.x * rect.width,
                        rect.y + center.y * rect.height,
                    )
                } else {
                    *center
                };
                let r = if *radius <= 1.0 && rect.width > 1.0 && rect.height > 1.0 {
                    *radius * rect.width.max(rect.height) * 0.5
                } else {
                    *radius
                };
                Brush::RadialGradient {
                    center: c,
                    radius: r,
                    stops: stops.clone(),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_node_recursive(
        &mut self,
        id: NodeId,
        scroll_offset: Point,
        current_xform: Transform2D,
        theme: &ThemeTokens,
        scene: &mut PaintScene,
        floating_selects: &mut Vec<(Rect, Vec<String>, usize, f32)>,
        active_tooltips: &mut Vec<(Rect, String)>,
    ) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let box_style = node.box_style.clone();
        let node_padding = node.layout.padding;
        let children = node.children.clone();
        let kind = node.kind.clone();
        let is_focused = self.focused == Some(id) && self.focus_visible;
        let is_disabled = node.is_disabled();

        match &kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return;
            }
            _ => {}
        }

        let node_anim = self.node_anims.get(id);
        let eff_opacity = node_anim
            .and_then(|a| a.opacity.as_ref())
            .map_or(box_style.opacity, |v| v.value)
            .clamp(0.0, 1.0);
        let anim_bg = node_anim.and_then(|a| a.bg_color.as_ref()).map(|c| c.value);
        let anim_border = node_anim
            .and_then(|a| a.border_color.as_ref())
            .map(|c| c.value);
        let ctrl_anim = self.control_anims.get(id).cloned();

        if eff_opacity <= 0.001 {
            if let Some(n) = self.nodes.get_mut(id) {
                n.dirty = DirtyFlags::NONE;
            }
            return;
        }

        let base_xf = self.node_effective_transform(id, rect.center());
        let button_press_xf = if let Some(ControlVisualAnim::Button { press_t, .. }) = &ctrl_anim
            && press_t.value > 0.001
        {
            let s = 1.0 - 0.025 * press_t.value.clamp(0.0, 1.0);
            Transform2D::around_pivot(rect.center(), Transform2D::scale(s, s))
        } else {
            Transform2D::IDENTITY
        };
        let local_xf = base_xf * button_press_xf;
        let has_xform = !local_xf.is_identity();
        let node_xform = if has_xform {
            current_xform * local_xf
        } else {
            current_xform
        };
        if has_xform {
            scene.push_transform(local_xf);
        }

        // Backdrop blur must execute BEFORE PushLayer because pooled offscreen layers start clear.
        if box_style.backdrop_blur > 0.2 {
            let mut tint = box_style.backdrop_tint;
            tint.a = ((tint.a as f32) * eff_opacity).round().clamp(0.0, 255.0) as u8;
            scene.backdrop_blur(rect, box_style.radius, box_style.backdrop_blur, tint);
        }

        let mut child_offset = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &kind {
            match axis {
                Axis::Vertical => child_offset.y -= *offset,
                Axis::Horizontal => child_offset.x -= *offset,
            }
        }

        let needs_layer = eff_opacity < 0.999 || box_style.blend_mode != BlendMode::Normal;
        if needs_layer {
            let layer_bounds =
                self.compute_subtree_layer_bounds(id, rect, child_offset, &box_style, &children);
            scene.push_layer(layer_bounds, eff_opacity, box_style.blend_mode);
        }

        // Outer drop shadows
        for s in box_style
            .shadow
            .into_iter()
            .chain(box_style.shadows.iter().copied())
            .filter(|s| !s.inset)
        {
            scene.drop_shadow(rect, box_style.radius, s);
        }

        // Background fill (Button & FAB paint their own interactive hover/press fill once)
        if !matches!(
            kind,
            WidgetKind::Button { .. } | WidgetKind::FloatingActionButton { .. }
        ) {
            let effective_bg = match (anim_bg, &box_style.background) {
                (Some(c), Some(rv)) if matches!(rv.get(), Brush::Solid(_)) => Some(Brush::Solid(c)),
                (_, Some(rv)) => Some(rv.get()),
                (Some(c), None) => Some(Brush::Solid(c)),
                (None, None) => None,
            };
            if let Some(bg) = effective_bg {
                let adapted = Self::adapt_brush_to_rect(&bg, rect);
                scene.fill_rect(rect, adapted, box_style.radius);
            }
        }

        // Inset inner shadows on top of background fill
        for s in box_style
            .shadow
            .into_iter()
            .chain(box_style.shadows.iter().copied())
            .filter(|s| s.inset)
        {
            scene.drop_shadow(rect, box_style.radius, s);
        }

        let mut paint_children = true;
        let mut extra_overlay_anim: Option<(Transform2D, f32)> = None;

        match kind {
            WidgetKind::Container | WidgetKind::Spacer | WidgetKind::SafeArea => {}
            WidgetKind::Divider { axis, color, .. } => {
                let c = color.map(|cv| cv.get()).unwrap_or(theme.border);
                let draw_r = match axis {
                    Axis::Horizontal | Axis::Vertical => {
                        Rect::new(rect.x, rect.y, rect.width.max(1.0), rect.height.max(1.0))
                    }
                };
                scene.fill_rect(draw_r, c, CornerRadii::ZERO);
            }
            WidgetKind::Text { content, style } => {
                let txt = content.get();
                let mut st = style.get();
                if st.color == Color::TRANSPARENT {
                    st.color = theme.text_primary;
                }
                scene.draw_text(rect.origin(), txt, st, Some(rect.width.max(1.0)));
            }
            WidgetKind::Icon { svg, color, .. } => {
                let tint = color.map(|c| c.get()).unwrap_or(theme.text_primary);
                let _ = paint_svg_icon(scene, rect, &svg.get(), tint);
            }
            WidgetKind::Button {
                label,
                disabled,
                foreground,
                hovered,
                pressed,
                ..
            } => {
                let disabled_eff = is_disabled || disabled.get();
                let (hover_t, press_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::Button { hover_t, press_t }) => {
                        (hover_t.value.clamp(0.0, 1.0), press_t.value.clamp(0.0, 1.0))
                    }
                    _ => (
                        if hovered { 1.0 } else { 0.0 },
                        if pressed { 1.0 } else { 0.0 },
                    ),
                };
                let bg_brush = if disabled_eff {
                    Brush::Solid(theme.surface_variant)
                } else {
                    match (anim_bg, &box_style.background) {
                        (Some(c), Some(rv)) if matches!(rv.get(), Brush::Solid(_)) => {
                            Brush::Solid(c)
                        }
                        (_, Some(rv)) => rv.get(),
                        (Some(c), None) => Brush::Solid(c),
                        (None, None) => Brush::Solid(theme.primary),
                    }
                };
                match bg_brush {
                    Brush::Solid(c) => {
                        let modulated = if disabled_eff {
                            c
                        } else {
                            c.lerp(Color::WHITE, 0.12 * hover_t)
                                .lerp(Color::BLACK, 0.22 * press_t)
                        };
                        scene.fill_rect(rect, modulated, box_style.radius);
                    }
                    other_brush => {
                        let adapted = Self::adapt_brush_to_rect(&other_brush, rect);
                        scene.fill_rect(rect, adapted, box_style.radius);
                        if !disabled_eff && hover_t > 0.01 {
                            scene.fill_rect(
                                rect,
                                Color::WHITE.with_alpha(0.12 * hover_t),
                                box_style.radius,
                            );
                        }
                        if !disabled_eff && press_t > 0.01 {
                            scene.fill_rect(
                                rect,
                                Color::BLACK.with_alpha(0.20 * press_t),
                                box_style.radius,
                            );
                        }
                    }
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(1.5, 1.5),
                        theme.background,
                        1.5,
                        box_style.radius,
                    );
                    scene.stroke_rect(rect.inflate(3.5, 3.5), theme.primary, 2.0, box_style.radius);
                }
                if children.is_empty() {
                    let txt = label.get();
                    if !txt.is_empty() {
                        let fg = if disabled_eff {
                            theme.text_secondary
                        } else if let Some(ref custom_fg) = foreground {
                            custom_fg.get()
                        } else {
                            theme.on_primary
                        };
                        if builtin_icon_svg(&txt).is_some() {
                            let icon_sz = (rect.width - 8.0).min(rect.height - 8.0).clamp(12.0, 24.0);
                            let ix = rect.x + (rect.width - icon_sz) * 0.5;
                            let iy = rect.y + (rect.height - icon_sz) * 0.5;
                            let icon_rect = Rect::new(ix, iy, icon_sz, icon_sz);
                            let _ = paint_svg_icon(scene, icon_rect, &txt, fg);
                        } else {
                            let tstyle = TextStyle::default()
                                .with_size(15.0)
                                .with_weight(600.0)
                                .with_color(fg);
                            let measured = self.text_engine.measure(&txt, &tstyle, None);
                            let tx = rect.x + (rect.width - measured.width) * 0.5;
                            let ty = rect.y + (rect.height - measured.height) * 0.5;
                            scene.draw_text(Point::new(tx, ty), txt, tstyle, None);
                        }
                    }
                }
            }
            WidgetKind::Image {
                texture,
                fit,
                radius,
                opacity,
            } => {
                if let Some(tex_id) = texture.get() {
                    scene.push(PaintCommand::DrawImage {
                        rect,
                        texture: tex_id,
                        fit,
                        radius,
                        opacity,
                    });
                } else {
                    scene.fill_rect(rect, theme.surface_variant, radius);
                }
            }
            WidgetKind::Canvas { on_paint, .. } => {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                on_paint(rect, scene);
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::ScrollView {
                axis,
                offset,
                content_extent,
                ..
            } => {
                paint_children = false;
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(
                        child,
                        child_offset,
                        node_xform,
                        theme,
                        scene,
                        floating_selects,
                        active_tooltips,
                    );
                }
                let view = match axis {
                    Axis::Vertical => rect.height,
                    Axis::Horizontal => rect.width,
                };
                if content_extent > view + 1.0 {
                    let ratio = (view / content_extent).clamp(0.08, 1.0);
                    let pos_ratio = (offset / (content_extent - view)).clamp(0.0, 1.0);
                    match axis {
                        Axis::Vertical => {
                            let thumb_h = view * ratio;
                            let thumb_y = rect.y + (view - thumb_h) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(rect.max_x() - 6.0, thumb_y, 4.0, thumb_h),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                        Axis::Horizontal => {
                            let thumb_w = view * ratio;
                            let thumb_x = rect.x + (view - thumb_w) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(thumb_x, rect.max_y() - 6.0, thumb_w, 4.0),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                    }
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::VirtualList { .. } => {
                paint_children = false;
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(
                        child,
                        scroll_offset,
                        node_xform,
                        theme,
                        scene,
                        floating_selects,
                        active_tooltips,
                    );
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::TextInput {
                editor,
                placeholder,
                style,
                multiline,
                ..
            } => {
                scene.fill_rect(rect, theme.surface, box_style.radius);
                let is_input_active = self.focused == Some(id);
                let border_color = if is_input_active {
                    theme.primary
                } else {
                    theme.border
                };
                scene.stroke_rect(
                    rect,
                    border_color,
                    if is_input_active { 1.8 } else { 1.0 },
                    box_style.radius,
                );

                let pad = if multiline {
                    Insets::symmetric(12.0, 10.0)
                } else {
                    Insets::symmetric(12.0, 8.0)
                };
                let inner = rect.inset(pad);
                let wrap_w = if multiline {
                    Some(inner.width.max(40.0))
                } else {
                    None
                };
                scene.push(PaintCommand::PushClip {
                    rect: inner,
                    radius: CornerRadii::ZERO,
                });
                let disp = editor.display_text().into_owned();
                let mut draw_style = style.clone();
                let font_sz = draw_style.font_size;
                if disp.is_empty() {
                    draw_style.color = theme.text_secondary;
                    scene.draw_text(inner.origin(), placeholder, draw_style, wrap_w);
                    if is_input_active && self.caret_visible {
                        scene.fill_rect(
                            Rect::new(inner.x, inner.y, 2.0, font_sz * 1.25),
                            theme.primary,
                            CornerRadii::ZERO,
                        );
                    }
                } else {
                    draw_style.color = theme.text_primary;
                    let layout = self
                        .text_engine
                        .build_layout_cached(&disp, &draw_style, wrap_w);
                    if let Some((s, e)) = editor.selection_range() {
                        for sr in self.text_engine.selection_rects(&layout, s, e) {
                            scene.fill_rect(
                                sr.translate(inner.x, inner.y),
                                theme.primary.with_alpha(0.32),
                                CornerRadii::ZERO,
                            );
                        }
                    }
                    scene.draw_text(inner.origin(), &disp, draw_style, wrap_w);
                    if is_input_active && self.caret_visible {
                        let cr = self.text_engine.cursor_rect(&layout, editor.cursor);
                        scene.fill_rect(
                            Rect::new(inner.x + cr.x, inner.y + cr.y, 2.0, cr.height),
                            theme.primary,
                            CornerRadii::ZERO,
                        );
                    }
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::Checkbox { checked, label, .. } => {
                let is_on = checked.get_untracked();
                let check_t = match &ctrl_anim {
                    Some(ControlVisualAnim::Checkbox { check_t }) => check_t.value.clamp(0.0, 1.0),
                    _ => {
                        if is_on {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                let box_rect = Rect::new(rect.x, rect.y + (rect.height - 20.0) * 0.5, 20.0, 20.0);
                let fill = theme.surface.lerp(theme.primary, check_t);
                let bcol = theme.border.lerp(theme.primary, check_t);
                scene.fill_rect(box_rect, fill, CornerRadii::all(5.0));
                scene.stroke_rect(box_rect, bcol, 1.5, CornerRadii::all(5.0));
                if check_t > 0.02 {
                    let p0 = (box_rect.x as f64 + 4.5, box_rect.y as f64 + 10.5);
                    let p1 = (box_rect.x as f64 + 8.5, box_rect.y as f64 + 14.5);
                    let p2 = (box_rect.x as f64 + 15.5, box_rect.y as f64 + 5.5);
                    let f1 = (check_t as f64 / 0.45).clamp(0.0, 1.0);
                    let mut path = kurbo::BezPath::new();
                    path.move_to(p0);
                    path.line_to((p0.0 + (p1.0 - p0.0) * f1, p0.1 + (p1.1 - p0.1) * f1));
                    if check_t > 0.45 {
                        let f2 = ((check_t as f64 - 0.45) / 0.55).clamp(0.0, 1.0);
                        path.line_to((p1.0 + (p2.0 - p1.0) * f2, p1.1 + (p2.1 - p1.1) * f2));
                    }
                    scene.stroke_path(path, theme.on_primary.with_alpha(check_t), 2.2);
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
                scene.draw_text(
                    Point::new(rect.x + 28.0, rect.y + (rect.height - 16.0) * 0.5),
                    label,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_color(theme.text_primary),
                    None,
                );
            }
            WidgetKind::Switch { checked, label, .. } => {
                let is_on = checked.get_untracked();
                let knob_t = match &ctrl_anim {
                    Some(ControlVisualAnim::Switch { knob_t }) => knob_t.value,
                    _ => {
                        if is_on {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                let track = Rect::new(rect.x, rect.y + (rect.height - 24.0) * 0.5, 44.0, 24.0);
                let track_col = theme
                    .surface_variant
                    .lerp(theme.primary, knob_t.clamp(0.0, 1.0));
                scene.fill_rect(track, track_col, CornerRadii::all(12.0));
                let knob_x = track.x + 3.0 + knob_t * 19.0;
                let knob_r = Rect::new(knob_x, track.y + 3.0, 18.0, 18.0);
                scene.drop_shadow(
                    knob_r,
                    CornerRadii::all(9.0),
                    BoxShadow::drop(0.0, 1.0, 3.0, 0.0, Color::rgba(0, 0, 0, 55)),
                );
                scene.fill_rect(knob_r, Color::WHITE, CornerRadii::all(9.0));
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(14.0),
                    );
                }
                if !label.is_empty() {
                    scene.draw_text(
                        Point::new(rect.x + 54.0, rect.y + (rect.height - 16.0) * 0.5),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        None,
                    );
                }
            }
            WidgetKind::Slider {
                value,
                min,
                max,
                label,
                ..
            } => {
                let v = value.get_untracked();
                let raw_frac = if (max - min).abs() > 1e-6 {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (frac, hover_t, drag_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::Slider {
                        frac_t,
                        hover_t,
                        drag_t,
                        ..
                    }) => (
                        frac_t.value.clamp(0.0, 1.0),
                        hover_t.value.clamp(0.0, 1.0),
                        drag_t.value.clamp(0.0, 1.0),
                    ),
                    _ => (raw_frac, 0.0, 0.0),
                };
                let cy = if !label.is_empty() && rect.height >= 40.0 {
                    let lbl_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(500.0)
                        .with_color(theme.text_secondary);
                    scene.draw_text(
                        Point::new(rect.x + 8.0, rect.y + 2.0),
                        &label,
                        lbl_style,
                        None,
                    );
                    let val_txt = if (v - v.round()).abs() < 0.05 {
                        format!("{:.0}", v)
                    } else {
                        format!("{:.1}", v)
                    };
                    let val_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(600.0)
                        .with_color(theme.text_primary);
                    let val_w = self.text_engine.measure(&val_txt, &val_style, None).width;
                    scene.draw_text(
                        Point::new((rect.max_x() - 8.0 - val_w).max(rect.x + 8.0), rect.y + 2.0),
                        val_txt,
                        val_style,
                        None,
                    );
                    rect.max_y() - 12.0
                } else {
                    rect.y + rect.height * 0.5
                };
                let track = Rect::new(rect.x + 8.0, cy - 3.0, (rect.width - 16.0).max(8.0), 6.0);
                scene.fill_rect(track, theme.surface_variant, CornerRadii::all(3.0));
                let active_w = track.width * frac;
                scene.fill_rect(
                    Rect::new(track.x, track.y, active_w, track.height),
                    theme.primary,
                    CornerRadii::all(3.0),
                );
                let kx_center = track.x + active_w;
                if hover_t > 0.01 || drag_t > 0.01 {
                    let halo_r = 9.0 + 4.0 * hover_t.max(drag_t) + 2.0 * drag_t;
                    let halo_alpha = (0.14 * hover_t + 0.18 * drag_t).clamp(0.0, 0.28);
                    scene.fill_rect(
                        Rect::new(kx_center - halo_r, cy - halo_r, halo_r * 2.0, halo_r * 2.0),
                        theme.primary.with_alpha(halo_alpha),
                        CornerRadii::all(halo_r),
                    );
                }
                let thumb_r = 9.0 + 1.5 * drag_t;
                let thumb_rect = Rect::new(
                    kx_center - thumb_r,
                    cy - thumb_r,
                    thumb_r * 2.0,
                    thumb_r * 2.0,
                );
                scene.drop_shadow(
                    thumb_rect,
                    CornerRadii::all(thumb_r),
                    BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 70)),
                );
                scene.fill_rect(thumb_rect, Color::WHITE, CornerRadii::all(thumb_r));
                scene.stroke_rect(
                    thumb_rect,
                    theme.primary,
                    2.0 + 0.5 * drag_t,
                    CornerRadii::all(thumb_r),
                );
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(1.5, 1.5),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
            }
            WidgetKind::Tabs { tabs, active, .. } => {
                let list = tabs.get();
                let count = list.len().max(1);
                let active_idx = active.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::Tabs { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => active_idx as f32,
                };
                scene.fill_rect(rect, theme.surface_variant, CornerRadii::all(8.0));
                let tab_w = rect.width / count as f32;
                let pill_x = rect.x + anim_idx * tab_w;
                let pill_r = Rect::new(
                    pill_x + 3.0,
                    rect.y + 3.0,
                    (tab_w - 6.0).max(4.0),
                    (rect.height - 6.0).max(4.0),
                );
                scene.fill_rect(pill_r, theme.surface, CornerRadii::all(6.0));
                scene.fill_rect(
                    Rect::new(
                        pill_r.x + 8.0,
                        rect.max_y() - 3.0,
                        (pill_r.width - 16.0).max(4.0),
                        2.5,
                    ),
                    theme.primary,
                    CornerRadii::all(1.25),
                );
                for (i, title) in list.iter().enumerate() {
                    let tx = rect.x + i as f32 * tab_w;
                    let tr = Rect::new(
                        tx + 3.0,
                        rect.y + 3.0,
                        (tab_w - 6.0).max(4.0),
                        rect.height - 6.0,
                    );
                    let act_weight = (1.0 - (anim_idx - i as f32).abs()).clamp(0.0, 1.0);
                    let tst = TextStyle::default()
                        .with_size(14.0)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 400.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let measured = self.text_engine.measure(title, &tst, None);
                    scene.draw_text(
                        Point::new(
                            tr.x + (tr.width - measured.width).max(0.0) * 0.5,
                            tr.y + (tr.height - measured.height).max(0.0) * 0.5,
                        ),
                        title,
                        tst,
                        Some(tr.width),
                    );
                }
                if is_focused {
                    scene.stroke_rect(rect, theme.primary, 1.5, CornerRadii::all(8.0));
                }
            }
            WidgetKind::Select {
                options,
                selected,
                open,
                dropdown_scroll,
                placeholder,
                ..
            } => {
                let opts = options.get();
                let sel_idx = selected.get_untracked();
                scene.fill_rect(rect, theme.surface, box_style.radius);
                scene.stroke_rect(
                    rect,
                    if is_focused || open {
                        theme.primary
                    } else {
                        theme.border
                    },
                    if is_focused || open { 1.6 } else { 1.0 },
                    box_style.radius,
                );
                let display = opts.get(sel_idx).cloned().unwrap_or(placeholder);
                let is_placeholder = opts.get(sel_idx).is_none();
                scene.draw_text(
                    Point::new(rect.x + 12.0, rect.y + (rect.height - 16.0) * 0.5),
                    display,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_color(if is_placeholder {
                            theme.text_secondary
                        } else {
                            theme.text_primary
                        }),
                    Some((rect.width - 42.0).max(20.0)),
                );
                let chev = Rect::new(
                    rect.max_x() - 28.0,
                    rect.y + (rect.height - 16.0) * 0.5,
                    16.0,
                    16.0,
                );
                paint_svg_icon(
                    scene,
                    chev,
                    if open { "chevron_up" } else { "chevron_down" },
                    theme.text_secondary,
                );
                if open {
                    let screen_trigger = node_xform.transform_rect_bounds(rect);
                    let drop_rect = self.select_dropdown_rect(screen_trigger, opts.len());
                    floating_selects.push((drop_rect, opts, sel_idx, dropdown_scroll));
                }
            }
            WidgetKind::RadioGroup {
                options,
                selected,
                axis,
                ..
            } => {
                let opts = options.get();
                let sel_idx = selected.get_untracked();
                let count = opts.len().max(1);
                let mut cursor_x = rect.x;
                let mut cursor_y = rect.y;
                let horiz_step = rect.width / count as f32;
                for (i, label) in opts.iter().enumerate() {
                    let (ix, iy) = match axis {
                        Axis::Vertical => (rect.x, cursor_y + 4.0),
                        Axis::Horizontal => (cursor_x + 2.0, rect.y + (rect.height - 20.0) * 0.5),
                    };
                    let circle = Rect::new(ix, iy, 18.0, 18.0);
                    let is_sel = i == sel_idx;
                    scene.fill_rect(circle, theme.surface, CornerRadii::all(9.0));
                    scene.stroke_rect(
                        circle,
                        if is_sel { theme.primary } else { theme.border },
                        2.0,
                        CornerRadii::all(9.0),
                    );
                    if is_sel {
                        scene.fill_rect(
                            Rect::new(ix + 4.0, iy + 4.0, 10.0, 10.0),
                            theme.primary,
                            CornerRadii::all(5.0),
                        );
                    }
                    scene.draw_text(
                        Point::new(ix + 26.0, iy + 1.0),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        None,
                    );
                    match axis {
                        Axis::Vertical => cursor_y += 30.0,
                        Axis::Horizontal => cursor_x += horiz_step,
                    }
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
            }
            WidgetKind::SegmentedControl {
                items, selected, ..
            } => {
                let list = items.get();
                let count = list.len().max(1);
                let sel_idx = selected.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::SegmentedControl { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => sel_idx as f32,
                };
                let track_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(10.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface_variant, track_rad);
                }
                let seg_w = (rect.width - 6.0).max(4.0) / count as f32;
                let pill_r = Rect::new(
                    rect.x + 3.0 + anim_idx * seg_w,
                    rect.y + 3.0,
                    seg_w,
                    (rect.height - 6.0).max(4.0),
                );
                scene.drop_shadow(
                    pill_r,
                    CornerRadii::all(8.0),
                    BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 48)),
                );
                scene.fill_rect(pill_r, theme.surface, CornerRadii::all(8.0));
                scene.stroke_rect(
                    pill_r,
                    theme.border.with_alpha(0.6),
                    1.0,
                    CornerRadii::all(8.0),
                );
                for (i, title) in list.iter().enumerate() {
                    let sr = Rect::new(
                        rect.x + 3.0 + i as f32 * seg_w,
                        rect.y + 3.0,
                        seg_w,
                        (rect.height - 6.0).max(4.0),
                    );
                    let act_weight = (1.0 - (anim_idx - i as f32).abs()).clamp(0.0, 1.0);
                    let tst = TextStyle::default()
                        .with_size(12.5)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 500.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let max_w = (sr.width - 8.0).max(10.0);
                    let display_title = self.text_engine.truncate_with_ellipsis(title, &tst, max_w);
                    let measured = self.text_engine.measure(&display_title, &tst, None);
                    scene.push_clip(sr, CornerRadii::all(6.0));
                    scene.draw_text(
                        Point::new(
                            sr.x + (sr.width - measured.width).max(0.0) * 0.5,
                            sr.y + (sr.height - measured.height).max(0.0) * 0.5,
                        ),
                        display_title,
                        tst,
                        Some(max_w),
                    );
                    scene.pop_clip();
                }
                if is_focused {
                    scene.stroke_rect(rect.inflate(1.5, 1.5), theme.primary, 1.5, track_rad);
                }
            }
            WidgetKind::BottomNavBar {
                items,
                icons,
                selected,
                ..
            } => {
                let list = items.get();
                let icon_list = icons.get();
                let count = list.len().max(1);
                let sel_idx = selected.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::BottomNavBar { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => sel_idx as f32,
                };

                let bg_color = match &box_style.background {
                    Some(b) => match b.get() {
                        Brush::Solid(c) => c,
                        _ => theme.surface,
                    },
                    None => theme.surface,
                };
                scene.fill_rect(rect, bg_color, box_style.radius);
                scene.stroke_rect(
                    Rect::new(rect.x, rect.y, rect.width, 1.0),
                    theme.border.with_alpha(0.4),
                    1.0,
                    CornerRadii::ZERO,
                );

                let item_w = rect.width / count as f32;

                // Active indicator pill (Material 3 style: 64x32dp rounded pill)
                let pill_w = (item_w * 0.65).clamp(44.0, 72.0);
                let pill_h = 32.0f32.min(rect.height * 0.55);
                let pill_cx = rect.x + (anim_idx + 0.5) * item_w;
                let pill_cy = rect.y + 18.0;
                let pill_r = Rect::new(
                    pill_cx - pill_w * 0.5,
                    pill_cy - pill_h * 0.5,
                    pill_w,
                    pill_h,
                );

                let pill_bg = theme.primary.with_alpha(0.18);
                scene.fill_rect(pill_r, pill_bg, CornerRadii::all(16.0));

                for (i, title) in list.iter().enumerate() {
                    let cx = rect.x + (i as f32 + 0.5) * item_w;
                    let act_dist = (anim_idx - i as f32).abs();
                    let act_weight = (1.0 - act_dist).clamp(0.0, 1.0);

                    let icon_glyph = icon_list.get(i).map(|s| s.as_str()).unwrap_or("");
                    let icon_color = theme.text_secondary.lerp(theme.primary, act_weight);

                    if !icon_glyph.is_empty() {
                        let icon_st = TextStyle::default().with_size(18.0).with_color(icon_color);
                        let isz = self.text_engine.measure(icon_glyph, &icon_st, None);
                        scene.draw_text(
                            Point::new(cx - isz.width * 0.5, rect.y + 9.0),
                            icon_glyph,
                            icon_st,
                            None,
                        );
                    }

                    let lbl_st = TextStyle::default()
                        .with_size(11.5)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 500.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let max_lbl_w = (item_w - 6.0).max(10.0);
                    let display_lbl = self
                        .text_engine
                        .truncate_with_ellipsis(title, &lbl_st, max_lbl_w);
                    let lsz = self.text_engine.measure(&display_lbl, &lbl_st, None);

                    let item_rect =
                        Rect::new(rect.x + i as f32 * item_w, rect.y, item_w, rect.height);
                    scene.push_clip(item_rect, CornerRadii::ZERO);
                    scene.draw_text(
                        Point::new(
                            cx - lsz.width * 0.5,
                            rect.y + rect.height - lsz.height - 6.0,
                        ),
                        display_lbl,
                        lbl_st,
                        Some(max_lbl_w),
                    );
                    scene.pop_clip();
                }
            }
            WidgetKind::FloatingActionButton {
                icon,
                label,
                disabled,
                foreground,
                background,
                hovered,
                pressed,
                ..
            } => {
                let is_dis = is_disabled || disabled.get();
                let (hover_t, press_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::FloatingActionButton { hover_t, press_t }) => {
                        (hover_t.value.clamp(0.0, 1.0), press_t.value.clamp(0.0, 1.0))
                    }
                    _ => (
                        if hovered { 1.0 } else { 0.0 },
                        if pressed { 1.0 } else { 0.0 },
                    ),
                };
                let bg = if is_dis {
                    theme.surface_variant
                } else if let Some(bg_rv) = background {
                    bg_rv.get()
                } else {
                    theme.primary
                };
                let fg = if is_dis {
                    theme.text_secondary
                } else if let Some(fg_rv) = foreground {
                    fg_rv.get()
                } else {
                    Color::WHITE
                };

                let rad = CornerRadii::all(rect.height * 0.5);
                let shadow_blur = 10.0 + hover_t * 6.0 - press_t * 3.0;
                let shadow_y = 3.5 + hover_t * 2.0 - press_t * 1.5;
                scene.drop_shadow(
                    rect,
                    rad,
                    BoxShadow::drop(0.0, shadow_y, shadow_blur, 0.0, Color::rgba(0, 0, 0, 70)),
                );

                scene.fill_rect(rect, bg, rad);
                if hover_t > 0.01 {
                    scene.fill_rect(
                        rect,
                        Color::rgba(255, 255, 255, (hover_t * 30.0) as u8),
                        rad,
                    );
                }

                let icon_str = icon.get();
                let label_str = label.as_ref().map(|l| l.get());

                if let Some(lbl) = label_str {
                    let icon_st = TextStyle::default().with_size(18.0).with_color(fg);
                    let lbl_st = TextStyle::default()
                        .with_size(14.0)
                        .with_weight(600.0)
                        .with_color(fg);
                    let isz = self.text_engine.measure(&icon_str, &icon_st, None);
                    let lsz = self.text_engine.measure(&lbl, &lbl_st, None);
                    let gap = 8.0;
                    let total_w = isz.width + gap + lsz.width;
                    let start_x = rect.x + (rect.width - total_w).max(0.0) * 0.5;

                    scene.draw_text(
                        Point::new(start_x, rect.y + (rect.height - isz.height) * 0.5),
                        icon_str,
                        icon_st,
                        None,
                    );
                    scene.draw_text(
                        Point::new(
                            start_x + isz.width + gap,
                            rect.y + (rect.height - lsz.height) * 0.5,
                        ),
                        lbl,
                        lbl_st,
                        None,
                    );
                } else {
                    let icon_st = TextStyle::default().with_size(24.0).with_color(fg);
                    let isz = self.text_engine.measure(&icon_str, &icon_st, None);
                    scene.draw_text(
                        Point::new(
                            rect.x + (rect.width - isz.width) * 0.5,
                            rect.y + (rect.height - isz.height) * 0.5,
                        ),
                        icon_str,
                        icon_st,
                        None,
                    );
                }
            }
            WidgetKind::Accordion {
                title, expanded, ..
            } => {
                let is_exp = expanded.get_untracked();
                let card_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(10.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, card_rad);
                }
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, theme.border, 1.0, card_rad);
                }
                let header_h = 44.0f32.min(rect.height);
                let header_r = Rect::new(rect.x, rect.y, rect.width, header_h);
                if is_exp {
                    scene.fill_rect(
                        Rect::new(
                            rect.x + 1.0,
                            rect.y + 1.0,
                            (rect.width - 2.0).max(0.0),
                            (header_h - 2.0).max(0.0),
                        ),
                        theme.surface_variant.with_alpha(0.32),
                        CornerRadii::all(8.0),
                    );
                    scene.fill_rect(
                        Rect::new(
                            rect.x + 12.0,
                            rect.y + header_h - 1.0,
                            (rect.width - 24.0).max(0.0),
                            1.0,
                        ),
                        theme.border,
                        CornerRadii::ZERO,
                    );
                }
                let t_str = title.get();
                let tst = TextStyle::default()
                    .with_size(14.0)
                    .with_weight(600.0)
                    .with_color(theme.text_primary);
                let measured = self.text_engine.measure(&t_str, &tst, None);
                scene.draw_text(
                    Point::new(
                        rect.x + 14.0,
                        rect.y + (header_h - measured.height).max(0.0) * 0.5,
                    ),
                    t_str,
                    tst,
                    Some((rect.width - 52.0).max(20.0)),
                );
                let chev_r = Rect::new(
                    rect.max_x() - 28.0,
                    rect.y + (header_h - 16.0) * 0.5,
                    16.0,
                    16.0,
                );
                let _ = paint_svg_icon(
                    scene,
                    chev_r,
                    if is_exp { "chevron_up" } else { "chevron_down" },
                    theme.text_secondary,
                );
                if is_focused {
                    scene.stroke_rect(header_r, theme.primary, 1.6, card_rad);
                }
                paint_children = is_exp;
            }
            WidgetKind::DataTable {
                columns,
                rows,
                selected,
                sort_column,
                sort_ascending,
                scroll_offset,
                striped,
                row_height,
                ..
            } => {
                let cols = columns.get();
                let row_list = rows.get();
                let tbl_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(8.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, tbl_rad);
                }
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: tbl_rad,
                });
                let header_h = 38.0f32.min(rect.height);
                let header_r = Rect::new(rect.x, rect.y, rect.width, header_h);
                scene.fill_rect(header_r, theme.surface_variant, CornerRadii::ZERO);
                scene.fill_rect(
                    Rect::new(rect.x, rect.y + header_h - 1.0, rect.width, 1.0),
                    theme.border,
                    CornerRadii::ZERO,
                );
                let col_count = cols.len().max(1);
                let col_w = (rect.width / col_count as f32).max(40.0);
                for (ci, col_title) in cols.iter().enumerate() {
                    let cx = rect.x + ci as f32 * col_w;
                    let is_sorted = sort_column == Some(ci);
                    let hst = TextStyle::default()
                        .with_size(12.5)
                        .with_weight(600.0)
                        .with_color(if is_sorted {
                            theme.primary
                        } else {
                            theme.text_secondary
                        });
                    let measured = self.text_engine.measure(col_title, &hst, None);
                    scene.draw_text(
                        Point::new(
                            cx + 12.0,
                            rect.y + (header_h - measured.height).max(0.0) * 0.5,
                        ),
                        col_title,
                        hst,
                        Some((col_w - if is_sorted { 34.0 } else { 20.0 }).max(16.0)),
                    );
                    if is_sorted {
                        let chev = Rect::new(
                            cx + col_w - 20.0,
                            rect.y + (header_h - 12.0) * 0.5,
                            12.0,
                            12.0,
                        );
                        let _ = paint_svg_icon(
                            scene,
                            chev,
                            if sort_ascending {
                                "chevron_up"
                            } else {
                                "chevron_down"
                            },
                            theme.primary,
                        );
                    }
                }
                let body_rect = Rect::new(
                    rect.x,
                    rect.y + header_h,
                    rect.width,
                    (rect.height - header_h).max(0.0),
                );
                scene.push(PaintCommand::PushClip {
                    rect: body_rect,
                    radius: CornerRadii::ZERO,
                });
                let sorted =
                    Self::data_table_sorted_indices(&row_list, sort_column, sort_ascending);
                let total_h = sorted.len() as f32 * row_height;
                let max_scroll = (total_h - body_rect.height).max(0.0);
                let scroll = scroll_offset.clamp(0.0, max_scroll);
                let sel_orig = selected.get_untracked();
                for (vis_i, &orig_i) in sorted.iter().enumerate() {
                    let ry = body_rect.y + vis_i as f32 * row_height - scroll;
                    if ry + row_height < body_rect.y || ry > body_rect.max_y() {
                        continue;
                    }
                    let row_r = Rect::new(body_rect.x, ry, body_rect.width, row_height);
                    let is_sel = orig_i == sel_orig;
                    if is_sel {
                        scene.fill_rect(row_r, theme.primary.with_alpha(0.16), CornerRadii::ZERO);
                    } else if striped && vis_i % 2 == 1 {
                        scene.fill_rect(
                            row_r,
                            theme.surface_variant.with_alpha(0.32),
                            CornerRadii::ZERO,
                        );
                    }
                    scene.fill_rect(
                        Rect::new(row_r.x, row_r.max_y() - 1.0, row_r.width, 1.0),
                        theme.border.with_alpha(0.45),
                        CornerRadii::ZERO,
                    );
                    if let Some(cells) = row_list.get(orig_i) {
                        for (ci, cell) in cells.iter().take(col_count).enumerate() {
                            let cx = rect.x + ci as f32 * col_w;
                            let cst = TextStyle::default()
                                .with_size(13.0)
                                .with_weight(if is_sel { 500.0 } else { 400.0 })
                                .with_color(if is_sel {
                                    theme.primary
                                } else {
                                    theme.text_primary
                                });
                            let measured = self.text_engine.measure(cell, &cst, None);
                            scene.draw_text(
                                Point::new(
                                    cx + 12.0,
                                    row_r.y + (row_r.height - measured.height).max(0.0) * 0.5,
                                ),
                                cell,
                                cst,
                                Some((col_w - 20.0).max(16.0)),
                            );
                        }
                    }
                }
                if max_scroll > 0.5 && body_rect.height > 8.0 {
                    let ratio = (body_rect.height / total_h).clamp(0.12, 1.0);
                    let track_h = (body_rect.height - 8.0).max(8.0);
                    let thumb_h = (track_h * ratio).max(16.0);
                    let pos_ratio = (scroll / max_scroll).clamp(0.0, 1.0);
                    let thumb_y = body_rect.y + 4.0 + (track_h - thumb_h) * pos_ratio;
                    scene.fill_rect(
                        Rect::new(body_rect.max_x() - 6.0, thumb_y, 3.5, thumb_h),
                        theme.text_secondary.with_alpha(0.55),
                        CornerRadii::all(2.0),
                    );
                }
                scene.push(PaintCommand::PopClip);
                scene.push(PaintCommand::PopClip);
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, theme.border, 1.0, tbl_rad);
                }
                if is_focused {
                    scene.stroke_rect(rect.inflate(1.5, 1.5), theme.primary, 1.5, tbl_rad);
                }
            }
            WidgetKind::PopupMenu { items, anchor, .. } => {
                let reveal = match &ctrl_anim {
                    Some(ControlVisualAnim::Overlay { reveal, .. }) => reveal.value.clamp(0.0, 1.2),
                    _ => 1.0,
                };
                let reveal_op = reveal.clamp(0.0, 1.0);
                let list = items.get();
                let menu_r = self.popup_menu_rect(anchor, list.len(), rect);
                scene.fill_rect(
                    rect,
                    Color::rgba(0, 0, 0, (60.0 * reveal_op).round() as u8),
                    CornerRadii::ZERO,
                );
                let animate_menu = reveal_op < 0.999;
                if animate_menu {
                    let s = (0.94 + 0.06 * reveal).clamp(0.85, 1.04);
                    scene.push_transform(Transform2D::around_pivot(
                        menu_r.center(),
                        Transform2D::scale(s, s),
                    ));
                    scene.push_layer(menu_r.inflate(28.0, 28.0), reveal_op, BlendMode::Normal);
                }
                scene.drop_shadow(
                    menu_r,
                    CornerRadii::all(8.0),
                    BoxShadow::drop(0.0, 6.0, 14.0, 0.0, Color::rgba(0, 0, 0, 120)),
                );
                scene.fill_rect(menu_r, theme.surface, CornerRadii::all(8.0));
                scene.stroke_rect(menu_r, theme.border, 1.0, CornerRadii::all(8.0));
                for (i, label) in list.iter().enumerate() {
                    let iy = menu_r.y + 4.0 + i as f32 * 36.0;
                    if iy + 34.0 > menu_r.max_y() {
                        break;
                    }
                    scene.draw_text(
                        Point::new(menu_r.x + 14.0, iy + 9.0),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        Some((menu_r.width - 28.0).max(20.0)),
                    );
                }
                if animate_menu {
                    scene.pop_layer();
                    scene.pop_transform();
                }
            }
            WidgetKind::Tooltip { text } => {
                if self.hovered_has_ancestor(id) {
                    let screen_r = node_xform.transform_rect_bounds(rect);
                    active_tooltips.push((screen_r, text.get()));
                }
            }
            WidgetKind::ProgressIndicator { progress, phase } => {
                scene.fill_rect(
                    rect,
                    theme.surface_variant,
                    CornerRadii::all(rect.height * 0.5),
                );
                match progress.get() {
                    Some(p) => {
                        let w = rect.width * p.clamp(0.0, 1.0);
                        scene.fill_rect(
                            Rect::new(rect.x, rect.y, w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                    None => {
                        let bar_w = rect.width * 0.35;
                        let start_x = rect.x + (rect.width - bar_w) * phase;
                        scene.fill_rect(
                            Rect::new(start_x, rect.y, bar_w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                }
            }
            WidgetKind::Spinner {
                size,
                thickness,
                color,
                phase,
            } => {
                use kurbo::Shape as _;
                let dia = rect
                    .width
                    .min(rect.height)
                    .max(size.min(rect.width.max(rect.height)));
                let stroke_w = thickness.clamp(1.5, (dia * 0.35).max(1.5));
                let radius = ((dia - stroke_w) * 0.5).max(2.0) as f64;
                let center = rect.center();
                let c = (center.x as f64, center.y as f64);
                let track_path = kurbo::Circle::new(c, radius).to_path(0.1);
                scene.stroke_path(track_path, theme.surface_variant, stroke_w);
                let start_angle =
                    (phase as f64) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
                let sweep = std::f64::consts::PI * 1.5;
                let arc_path =
                    kurbo::Arc::new(c, (radius, radius), start_angle, sweep, 0.0).to_path(0.1);
                let fg = color.map(|cv| cv.get()).unwrap_or(theme.primary);
                scene.stroke_path(arc_path, fg, stroke_w);
            }
            WidgetKind::Badge {
                label,
                foreground,
                background,
                icon,
            } => {
                let pill_radius = if box_style.radius == CornerRadii::ZERO
                    || box_style.radius == CornerRadii::all(12.0)
                {
                    CornerRadii::all(rect.height * 0.5)
                } else {
                    box_style.radius
                };
                let fg = foreground.map(|f| f.get()).unwrap_or(theme.primary);
                let bg = if let Some(b) = background {
                    b.get()
                } else if box_style.background.is_none() {
                    fg.with_alpha(0.16)
                } else {
                    Color::TRANSPARENT
                };
                if bg != Color::TRANSPARENT {
                    scene.fill_rect(rect, bg, pill_radius);
                }
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, fg.with_alpha(0.32), 1.0, pill_radius);
                }
                let txt = label.get();
                let tstyle = TextStyle::default()
                    .with_size(12.0)
                    .with_weight(600.0)
                    .with_color(fg);
                let measured = self.text_engine.measure(&txt, &tstyle, None);
                let icon_spec = icon.map(|iv| iv.get()).filter(|s| !s.is_empty());
                let icon_total = if icon_spec.is_some() { 16.0 } else { 0.0 };
                let content_w = icon_total + measured.width;
                let start_x = rect.x + (rect.width - content_w).max(0.0) * 0.5;
                if let Some(ref ic) = icon_spec {
                    let ir = Rect::new(start_x, rect.y + (rect.height - 12.0) * 0.5, 12.0, 12.0);
                    let _ = paint_svg_icon(scene, ir, ic, fg);
                }
                let tx = start_x + icon_total;
                let ty = rect.y + (rect.height - measured.height) * 0.5;
                scene.draw_text(Point::new(tx, ty), txt, tstyle, None);
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
                let inner = rect.inset(node_padding);
                let (_, _, div_rect) = Self::split_divider_geometry(
                    inner,
                    axis,
                    ratio.get_untracked(),
                    min_first,
                    min_second,
                    divider_size,
                );
                let track_bg = if dragging {
                    theme.primary.with_alpha(0.18)
                } else {
                    theme.surface_variant.with_alpha(0.65)
                };
                scene.fill_rect(div_rect, track_bg, CornerRadii::ZERO);
                let handle = match axis {
                    Axis::Horizontal => Rect::new(
                        div_rect.center().x - 1.5,
                        div_rect.center().y - 14.0,
                        3.0,
                        28.0,
                    ),
                    Axis::Vertical => Rect::new(
                        div_rect.center().x - 14.0,
                        div_rect.center().y - 1.5,
                        28.0,
                        3.0,
                    ),
                };
                let handle_col = if dragging {
                    theme.primary
                } else {
                    theme.text_secondary.with_alpha(0.55)
                };
                scene.fill_rect(handle, handle_col, CornerRadii::all(1.5));
            }
            WidgetKind::ToastHost { items, .. } => {
                let list = items.get();
                let (cards, closes) = Self::toast_card_rects(rect, list.len());
                for ((item, card_r), close_r) in list.iter().zip(cards).zip(closes) {
                    if card_r.max_y() < rect.y {
                        continue;
                    }
                    let (accent, icon_name) = match item.severity {
                        ToastSeverity::Info => (theme.primary, "info"),
                        ToastSeverity::Success => (Color::rgb(16, 185, 129), "check"),
                        ToastSeverity::Warning => (Color::rgb(245, 158, 11), "warning"),
                        ToastSeverity::Error => (theme.danger, "warning"),
                    };
                    scene.drop_shadow(
                        card_r,
                        CornerRadii::all(10.0),
                        BoxShadow::drop(0.0, 6.0, 16.0, 0.0, Color::rgba(0, 0, 0, 110)),
                    );
                    scene.fill_rect(card_r, theme.surface, CornerRadii::all(10.0));
                    scene.stroke_rect(card_r, theme.border, 1.0, CornerRadii::all(10.0));
                    scene.fill_rect(
                        Rect::new(card_r.x + 6.0, card_r.y + 10.0, 4.0, card_r.height - 20.0),
                        accent,
                        CornerRadii::all(2.0),
                    );
                    let _ = paint_svg_icon(
                        scene,
                        Rect::new(card_r.x + 17.0, card_r.y + 14.0, 16.0, 16.0),
                        icon_name,
                        accent,
                    );
                    let max_text_w = (card_r.width - 78.0).max(40.0);
                    scene.draw_text(
                        Point::new(card_r.x + 41.0, card_r.y + 11.0),
                        &item.title,
                        TextStyle::default()
                            .with_size(13.0)
                            .with_weight(600.0)
                            .with_color(theme.text_primary),
                        Some(max_text_w),
                    );
                    scene.draw_text(
                        Point::new(card_r.x + 41.0, card_r.y + 32.0),
                        &item.message,
                        TextStyle::default()
                            .with_size(12.0)
                            .with_color(theme.text_secondary),
                        Some(max_text_w),
                    );
                    let _ = paint_svg_icon(
                        scene,
                        close_r.inset(Insets::all(3.0)),
                        "close",
                        theme.text_secondary,
                    );
                }
            }
            WidgetKind::Overlay {
                is_bottom_sheet, ..
            } => {
                let reveal = match &ctrl_anim {
                    Some(ControlVisualAnim::Overlay { reveal, .. }) => reveal.value.clamp(0.0, 1.2),
                    _ => 1.0,
                };
                let reveal_op = reveal.clamp(0.0, 1.0);
                scene.fill_rect(
                    rect,
                    Color::rgba(0, 0, 0, (145.0 * reveal_op).round() as u8),
                    CornerRadii::ZERO,
                );
                if reveal_op < 0.999 {
                    let overlay_xf = if is_bottom_sheet {
                        Transform2D::translation(0.0, (1.0 - reveal) * 44.0)
                    } else {
                        let s = (0.94 + 0.06 * reveal).clamp(0.85, 1.04);
                        Transform2D::around_pivot(rect.center(), Transform2D::scale(s, s))
                    };
                    extra_overlay_anim = Some((overlay_xf, reveal_op));
                }
            }
            WidgetKind::PlatformView { id: pv_id } => {
                scene.push(PaintCommand::PlatformViewHole { id: pv_id, rect });
            }
            WidgetKind::HtmlView { .. } => {
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, box_style.radius);
                }
                scene.push(PaintCommand::PlatformViewHole {
                    id: PlatformViewId::from_raw(rect.x as u64 ^ rect.y as u64),
                    rect,
                });
            }
            WidgetKind::Card { variant } => {
                let radius = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(12.0)
                } else {
                    box_style.radius
                };
                match variant {
                    CardVariant::Elevated => {
                        scene.drop_shadow(
                            rect,
                            radius,
                            BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 45)),
                        );
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                    }
                    CardVariant::Filled => {
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface_variant));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                    }
                    CardVariant::Outlined => {
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                        let border_c = box_style
                            .border_color
                            .as_ref()
                            .map(|c| c.get())
                            .unwrap_or(theme.border);
                        scene.stroke_rect(rect, border_c, 1.0, radius);
                    }
                }
            }
            WidgetKind::ListTile {
                leading,
                title,
                subtitle,
                trailing,
                hovered,
                pressed,
                selected,
                ..
            } => {
                let is_sel = selected.get();
                let bg = if pressed {
                    Some(theme.primary.with_alpha(0.18))
                } else if hovered {
                    Some(theme.surface_variant.with_alpha(0.6))
                } else if is_sel {
                    Some(theme.primary.with_alpha(0.10))
                } else {
                    None
                };
                if let Some(c) = bg {
                    scene.fill_rect(rect, c, box_style.radius);
                }

                let pad_x = 16.0;
                let mut cur_x = rect.x + pad_x;

                if let Some(lead) = leading {
                    let lead_str = lead.get();
                    if !lead_str.is_empty() {
                        let tint = if is_sel {
                            theme.primary
                        } else {
                            theme.text_secondary
                        };
                        if builtin_icon_svg(&lead_str).is_some() {
                            let icon_sz = 20.0;
                            let icon_y = rect.y + (rect.height - icon_sz) * 0.5;
                            let icon_rect = Rect::new(cur_x, icon_y, icon_sz, icon_sz);
                            let _ = paint_svg_icon(scene, icon_rect, &lead_str, tint);
                            cur_x += icon_sz + 16.0;
                        } else {
                            let lead_st = TextStyle::default().with_size(20.0).with_color(tint);
                            let lsz = self.text_engine.measure(&lead_str, &lead_st, None);
                            let icon_y = rect.y + (rect.height - lsz.height) * 0.5;
                            scene.draw_text(Point::new(cur_x, icon_y), lead_str, lead_st, None);
                            cur_x += lsz.width + 16.0;
                        }
                    }
                }

                let mut trailing_w = 0.0;
                if let Some(trail) = trailing {
                    let trail_str = trail.get();
                    if !trail_str.is_empty() {
                        let trail_st =
                            TextStyle::default().with_size(18.0).with_color(theme.text_secondary);
                        let tsz = self.text_engine.measure(&trail_str, &trail_st, None);
                        trailing_w = tsz.width + 16.0;
                        let trail_x = rect.x + rect.width - pad_x - tsz.width;
                        let trail_y = rect.y + (rect.height - tsz.height) * 0.5;
                        scene.draw_text(Point::new(trail_x, trail_y), trail_str, trail_st, None);
                    }
                }

                let title_str = title.get();
                let avail_w = (rect.x + rect.width - cur_x - trailing_w - pad_x).max(10.0);
                let title_col = if is_sel {
                    theme.primary
                } else {
                    theme.text_primary
                };
                let title_st = TextStyle::default()
                    .with_size(15.0)
                    .with_weight(500.0)
                    .with_color(title_col);

                if let Some(sub) = subtitle {
                    let sub_str = sub.get();
                    let sub_st = TextStyle::default()
                        .with_size(13.0)
                        .with_color(theme.text_secondary);
                    let t_sz = self.text_engine.measure(&title_str, &title_st, Some(avail_w));
                    let s_sz = self.text_engine.measure(&sub_str, &sub_st, Some(avail_w));
                    let total_h = t_sz.height + s_sz.height + 2.0;
                    let start_y = rect.y + (rect.height - total_h) * 0.5;

                    scene.draw_text(Point::new(cur_x, start_y), title_str, title_st, Some(avail_w));
                    scene.draw_text(
                        Point::new(cur_x, start_y + t_sz.height + 2.0),
                        sub_str,
                        sub_st,
                        Some(avail_w),
                    );
                } else {
                    let t_sz = self.text_engine.measure(&title_str, &title_st, Some(avail_w));
                    let start_y = rect.y + (rect.height - t_sz.height) * 0.5;
                    scene.draw_text(Point::new(cur_x, start_y), title_str, title_st, Some(avail_w));
                }
            }
            WidgetKind::Chip {
                label,
                icon,
                selected,
                hovered,
                ..
            } => {
                let is_sel = selected.get();
                let rad = CornerRadii::all(8.0);
                let bg = if is_sel {
                    theme.primary.with_alpha(0.2)
                } else if hovered {
                    theme.surface_variant.with_alpha(0.6)
                } else {
                    theme.surface
                };
                scene.fill_rect(rect, bg, rad);
                let border_color = if is_sel {
                    theme.primary
                } else {
                    theme.border
                };
                scene.stroke_rect(rect, border_color, 1.0, rad);

                let mut cur_x = rect.x + 10.0;
                if is_sel && icon.is_none() {
                    let check_st = TextStyle::default().with_size(14.0).with_color(theme.primary);
                    let csz = self.text_engine.measure("✓", &check_st, None);
                    let cy = rect.y + (rect.height - csz.height) * 0.5;
                    scene.draw_text(Point::new(cur_x, cy), "✓", check_st, None);
                    cur_x += csz.width + 6.0;
                } else if let Some(ic) = icon {
                    let ic_str = ic.get();
                    if !ic_str.is_empty() {
                        let ic_st = TextStyle::default().with_size(14.0).with_color(
                            if is_sel {
                                theme.primary
                            } else {
                                theme.text_secondary
                            },
                        );
                        let isz = self.text_engine.measure(&ic_str, &ic_st, None);
                        let iy = rect.y + (rect.height - isz.height) * 0.5;
                        scene.draw_text(Point::new(cur_x, iy), ic_str, ic_st, None);
                        cur_x += isz.width + 6.0;
                    }
                }

                let lbl_str = label.get();
                let lbl_col = if is_sel {
                    theme.primary
                } else {
                    theme.text_primary
                };
                let lbl_st = TextStyle::default()
                    .with_size(13.0)
                    .with_weight(500.0)
                    .with_color(lbl_col);
                let lsz = self.text_engine.measure(&lbl_str, &lbl_st, None);
                let ly = rect.y + (rect.height - lsz.height) * 0.5;
                scene.draw_text(Point::new(cur_x, ly), lbl_str, lbl_st, None);
            }
            WidgetKind::AppBar {
                title,
                leading_icon,
                elevation,
                ..
            } => {
                let bg = box_style
                    .background
                    .as_ref()
                    .map(|b| b.get())
                    .unwrap_or(Brush::Solid(theme.surface));
                let adapted = Self::adapt_brush_to_rect(&bg, rect);
                scene.fill_rect(rect, adapted, CornerRadii::ZERO);

                if elevation > 0.0 {
                    scene.drop_shadow(
                        rect,
                        CornerRadii::ZERO,
                        BoxShadow::drop(
                            0.0,
                            elevation * 1.5,
                            elevation * 4.0,
                            0.0,
                            Color::rgba(0, 0, 0, 45),
                        ),
                    );
                }

                let mut cur_x = rect.x + 16.0;
                if let Some(lead) = leading_icon {
                    let lead_str = lead.get();
                    if !lead_str.is_empty() {
                        let lead_st =
                            TextStyle::default().with_size(22.0).with_color(theme.text_primary);
                        let lsz = self.text_engine.measure(&lead_str, &lead_st, None);
                        let ly = rect.y + (rect.height - lsz.height) * 0.5;
                        scene.draw_text(Point::new(cur_x, ly), lead_str, lead_st, None);
                        cur_x += lsz.width + 16.0;
                    }
                }

                let title_str = title.get();
                let title_st = TextStyle::default()
                    .with_size(18.0)
                    .with_weight(600.0)
                    .with_color(theme.text_primary);
                let tsz = self.text_engine.measure(&title_str, &title_st, None);
                let ty = rect.y + (rect.height - tsz.height) * 0.5;
                scene.draw_text(Point::new(cur_x, ty), title_str, title_st, None);
            }
            WidgetKind::Drawer { open, width, .. } => {
                if !open.get() {
                    paint_children = false;
                } else {
                    scene.fill_rect(rect, Color::rgba(0, 0, 0, 120), CornerRadii::ZERO);

                    let drawer_w = width.min(rect.width * 0.85).max(240.0);
                    let drawer_r = Rect::new(rect.x, rect.y, drawer_w, rect.height);

                    scene.drop_shadow(
                        drawer_r,
                        CornerRadii::new(0.0, 16.0, 16.0, 0.0),
                        BoxShadow::drop(4.0, 0.0, 16.0, 0.0, Color::rgba(0, 0, 0, 60)),
                    );

                    let bg = box_style
                        .background
                        .as_ref()
                        .map(|b| b.get())
                        .unwrap_or(Brush::Solid(theme.surface));
                    let adapted = Self::adapt_brush_to_rect(&bg, drawer_r);
                    scene.fill_rect(drawer_r, adapted, CornerRadii::new(0.0, 16.0, 16.0, 0.0));
                }
            }
            WidgetKind::Custom(cw) => {
                if let Ok(mut guard) = cw.try_borrow_mut() {
                    guard.paint(rect, scene, &mut self.text_engine, theme, is_focused);
                }
            }
        }

        let eff_border_col =
            anim_border.or_else(|| box_style.border_color.as_ref().map(ReactiveValue::get));
        if let Some(border) = box_style.border
            && border.width > 0.0
        {
            let bcol = eff_border_col.unwrap_or(border.color);
            scene.stroke_rect(rect, bcol, border.width, border.radius);
        }

        if paint_children {
            if box_style.clip {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
            }
            let (child_xf_for_sub, pushed_overlay_anim) =
                if let Some((ov_xf, ov_op)) = extra_overlay_anim {
                    let mut child_bounds = rect;
                    for &child in &children {
                        self.accumulate_subtree_bounds(
                            child,
                            child_offset,
                            Transform2D::IDENTITY,
                            &mut child_bounds,
                        );
                    }
                    scene.push_transform(ov_xf);
                    scene.push_layer(child_bounds, ov_op, BlendMode::Normal);
                    (node_xform * ov_xf, true)
                } else {
                    (node_xform, false)
                };
            for child in children {
                self.paint_node_recursive(
                    child,
                    child_offset,
                    child_xf_for_sub,
                    theme,
                    scene,
                    floating_selects,
                    active_tooltips,
                );
            }
            if pushed_overlay_anim {
                scene.pop_layer();
                scene.pop_transform();
            }
            if box_style.clip {
                scene.push(PaintCommand::PopClip);
            }
        }

        if needs_layer {
            scene.pop_layer();
        }
        if has_xform {
            scene.pop_transform();
        }

        if let Some(n) = self.nodes.get_mut(id) {
            n.dirty = DirtyFlags::NONE;
        }
    }

    // === Event Routing & Hit-Testing ===

    pub(crate) fn node_world_transform(&self, id: NodeId) -> Transform2D {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        while let Some(cid) = cur {
            chain.push(cid);
            cur = self.nodes.get(cid).and_then(|n| n.parent);
        }
        let mut world = Transform2D::IDENTITY;
        for aid in chain.into_iter().rev() {
            let r = self.node_visual_rect(aid);
            let local_xf = self.node_effective_transform(aid, r.center());
            if !local_xf.is_identity() {
                world = world * local_xf;
            }
        }
        world
    }

    pub(crate) fn screen_to_node_local_point(&self, id: NodeId, point: Point) -> Point {
        let world = self.node_world_transform(id);
        if world.is_identity() {
            point
        } else {
            world
                .inverse()
                .map_or(point, |inv| inv.transform_point(point))
        }
    }

}
