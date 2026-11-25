use super::*;

impl WidgetTree {
    // === Ergonomic Widget Builders ===

    pub fn container(&mut self, scope: Scope, layout: LayoutStyle, style: BoxStyle) -> NodeId {
        let id = self.spawn_node("Container", WidgetKind::Container, layout, scope);
        self.set_box_style(id, style);
        id
    }

    pub fn row(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::row(), BoxStyle::new())
    }

    pub fn column(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::column(), BoxStyle::new())
    }

    pub fn grid(&mut self, scope: Scope, columns: u16, gap: f32) -> NodeId {
        self.container(
            scope,
            LayoutStyle::grid(columns).with_gap(gap),
            BoxStyle::new(),
        )
    }

    pub fn stack(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::stack(), BoxStyle::new())
    }

    pub fn spacer(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "Spacer",
            WidgetKind::Spacer,
            LayoutStyle::column().with_flex_grow(1.0),
            scope,
        )
    }

    pub fn safe_area(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "SafeArea",
            WidgetKind::SafeArea,
            LayoutStyle::column()
                .with_size(Length::Pct(100.0), Length::Pct(100.0))
                .with_safe_area(true),
            scope,
        )
    }

    pub fn divider(&mut self, scope: Scope, axis: Axis, thickness: f32) -> NodeId {
        let th = thickness.max(1.0);
        let mut layout = match axis {
            Axis::Horizontal => LayoutStyle::row().with_height(Length::Dp(th)),
            Axis::Vertical => LayoutStyle::column().with_width(Length::Dp(th)),
        };
        if matches!(axis, Axis::Horizontal) {
            layout.width = Length::Pct(100.0);
        } else {
            layout.height = Length::Pct(100.0);
        }
        let id = self.spawn_node(
            "Divider",
            WidgetKind::Divider {
                axis,
                thickness: th,
                color: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Divider;
        }
        id
    }

    pub fn set_divider_color(&mut self, node: NodeId, color: impl Into<ReactiveValue<Color>>) {
        let c = color.into();
        let is_div = self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Divider { .. }));
        if !is_div {
            return;
        }
        if c.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.divider_color) {
                let cc = c.clone();
                eff_scope.effect(move || {
                    let _ = cc.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.divider_color);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Divider { color: slot, .. } = &mut n.kind
        {
            *slot = Some(c);
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn text(
        &mut self,
        scope: Scope,
        content: impl Into<ReactiveValue<String>>,
        style: impl Into<ReactiveValue<TextStyle>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Text",
            WidgetKind::Text {
                content: content.into(),
                style: style.into(),
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Label;
        }
        id
    }

    /// Create a reactive rich-text paragraph. The full plain text is exposed
    /// to accessibility services as a single label.
    pub fn rich_text(
        &mut self,
        scope: Scope,
        content: impl Into<ReactiveValue<RichText>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "RichText",
            WidgetKind::RichText {
                content: content.into(),
                on_link: None,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(node) = self.nodes.get_mut(id) {
            node.a11y.role = A11yRole::Label;
        }
        id
    }

    pub fn icon(
        &mut self,
        scope: Scope,
        svg: impl Into<ReactiveValue<String>>,
        size: f32,
        color: Option<ReactiveValue<Color>>,
    ) -> NodeId {
        let sz = size.max(8.0);
        let svg_rv = svg.into();
        let label = svg_rv.get();
        let id = self.spawn_node(
            "Icon",
            WidgetKind::Icon {
                svg: svg_rv,
                size: sz,
                color,
            },
            LayoutStyle::row().with_size(Length::Dp(sz), Length::Dp(sz)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Image;
            n.a11y.label = Some(if label.starts_with('<') {
                "Icon".to_string()
            } else {
                label
            });
        }
        id
    }

    pub fn button(
        &mut self,
        scope: Scope,
        label: impl Into<ReactiveValue<String>>,
        on_tap: impl FnMut() + 'static,
    ) -> NodeId {
        let label = label.into();
        let label_text = label.get();
        let id = self.spawn_node(
            "Button",
            WidgetKind::Button {
                label,
                disabled: ReactiveValue::Static(false),
                foreground: None,
                on_tap: Some(Rc::new(RefCell::new(on_tap))),
                hovered: false,
                pressed: false,
            },
            LayoutStyle::row()
                .with_padding(Insets::symmetric(18.0, 10.0))
                .with_align(Alignment::Center),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::Button;
            n.a11y.label = Some(label_text);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn image(
        &mut self,
        scope: Scope,
        texture: impl Into<ReactiveValue<Option<TextureId>>>,
        fit: Fit,
        radius: CornerRadii,
    ) -> NodeId {
        let id = self.spawn_node(
            "ImageView",
            WidgetKind::Image {
                texture: texture.into(),
                fit,
                radius,
                opacity: 1.0,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Image;
        }
        id
    }

    pub fn canvas(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        on_paint: impl Fn(Rect, &mut PaintScene) + 'static,
        on_pointer: Option<CanvasPointerFn>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Canvas",
            WidgetKind::Canvas {
                on_paint: Rc::new(on_paint),
                on_pointer,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Canvas;
        }
        id
    }

    pub fn scroll_view(&mut self, scope: Scope, axis: Axis, mut layout: LayoutStyle) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "ScrollView",
            WidgetKind::ScrollView {
                axis,
                offset: 0.0,
                velocity: 0.0,
                content_extent: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::ScrollView;
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn virtual_list(
        &mut self,
        scope: Scope,
        axis: Axis,
        columns: usize,
        item_extent: f32,
        item_count: impl Into<ReactiveValue<usize>>,
        mut layout: LayoutStyle,
        factory: VirtualItemFactory,
    ) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "VirtualList",
            WidgetKind::VirtualList {
                axis,
                columns: columns.max(1),
                item_extent: item_extent.max(8.0),
                item_count: item_count.into(),
                offset: 0.0,
                velocity: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::List;
        }
        self.virtual_hosts.insert(
            id,
            VirtualListHost {
                factory,
                active_items: HashMap::new(),
                last_range: None,
            },
        );
        id
    }

    pub fn text_input(
        &mut self,
        scope: Scope,
        initial: impl Into<String>,
        placeholder: impl Into<String>,
        bound_signal: Option<Signal<String>>,
    ) -> NodeId {
        let initial_s = bound_signal
            .map(|s| s.get_untracked())
            .unwrap_or_else(|| initial.into());
        let placeholder = placeholder.into();
        let accessible_name = (!placeholder.is_empty()).then(|| placeholder.clone());
        let id = self.spawn_node(
            "TextInput",
            WidgetKind::TextInput {
                editor: TextEditorState::new(initial_s.clone()),
                placeholder,
                style: TextStyle::default().with_size(14.0),
                multiline: false,
                bound_signal,
                on_change: None,
                on_submit: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(40.0))
                .with_padding(Insets::symmetric(12.0, 8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::TextInput;
            // Without a name a screen reader only announces "edit text".
            n.a11y.label = accessible_name;
            n.a11y.value = Some(initial_s);
            n.a11y.focusable = true;
        }
        id
    }

    /// Multi-line editable text area widget with wrapping, newline Enter, and Up/Down line navigation.
    pub fn text_area(
        &mut self,
        scope: Scope,
        initial: impl Into<String>,
        placeholder: impl Into<String>,
        bound_signal: Option<Signal<String>>,
    ) -> NodeId {
        let initial_s = bound_signal
            .map(|s| s.get_untracked())
            .unwrap_or_else(|| initial.into());
        let placeholder = placeholder.into();
        let accessible_name = (!placeholder.is_empty()).then(|| placeholder.clone());
        let id = self.spawn_node(
            "TextArea",
            WidgetKind::TextInput {
                editor: TextEditorState::new(initial_s.clone()),
                placeholder,
                style: TextStyle::default().with_size(14.0),
                multiline: true,
                bound_signal,
                on_change: None,
                on_submit: None,
            },
            LayoutStyle::column()
                .with_height(Length::Dp(104.0))
                .with_padding(Insets::symmetric(12.0, 10.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::MultilineTextInput;
            // Without a name a screen reader only announces "edit text".
            n.a11y.label = accessible_name;
            n.a11y.value = Some(initial_s);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn checkbox(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Checkbox",
            WidgetKind::Checkbox {
                checked,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(8.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::CheckBox;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn switch(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Switch",
            WidgetKind::Switch {
                checked,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(10.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Switch;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn slider(
        &mut self,
        scope: Scope,
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let default_h = if lbl.is_empty() { 32.0 } else { 48.0 };
        let id = self.spawn_node(
            "Slider",
            WidgetKind::Slider {
                value,
                min,
                max,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(default_h))
                .with_padding(Insets::symmetric(4.0, 0.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.hit_slop = 10.0;
            n.a11y.role = A11yRole::Slider;
            n.a11y.label = Some(lbl);
            n.a11y.numeric_value = Some(value.get_untracked() as f64);
            n.a11y.min_value = Some(min as f64);
            n.a11y.max_value = Some(max as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn tabs(
        &mut self,
        scope: Scope,
        tabs: impl Into<ReactiveValue<Vec<String>>>,
        active: Signal<usize>,
    ) -> NodeId {
        let tabs_rv = tabs.into();
        let id = self.spawn_node(
            "Tabs",
            WidgetKind::Tabs {
                tabs: tabs_rv,
                active,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(40.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(active.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn select(
        &mut self,
        scope: Scope,
        options: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
        placeholder: impl Into<String>,
    ) -> NodeId {
        let opts_rv = options.into();
        let opts = opts_rv.get();
        let cur = opts.get(selected.get_untracked()).cloned();
        let id = self.spawn_node(
            "Select",
            WidgetKind::Select {
                options: opts_rv,
                selected,
                open: false,
                dropdown_scroll: 0.0,
                placeholder: placeholder.into(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(40.0))
                .with_padding(Insets::symmetric(12.0, 8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::ComboBox;
            n.a11y.value = cur;
            n.a11y.focusable = true;
        }
        id
    }

    pub fn radio_group(
        &mut self,
        scope: Scope,
        options: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
        axis: Axis,
    ) -> NodeId {
        let id = self.spawn_node(
            "RadioGroup",
            WidgetKind::RadioGroup {
                options: options.into(),
                selected,
                axis,
                on_change: None,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::RadioButton;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn segmented_control(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "SegmentedControl",
            WidgetKind::SegmentedControl {
                items: items.into(),
                selected,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(36.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn bottom_nav_bar(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<String>>>,
        icons: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "BottomNavBar",
            WidgetKind::BottomNavBar {
                items: items.into(),
                icons: icons.into(),
                selected,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(64.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn floating_action_button(
        &mut self,
        scope: Scope,
        icon: impl Into<ReactiveValue<String>>,
        label: Option<impl Into<ReactiveValue<String>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "FloatingActionButton",
            WidgetKind::FloatingActionButton {
                icon: icon.into(),
                label: label.map(|l| l.into()),
                disabled: false.into(),
                foreground: None,
                background: None,
                on_tap: None,
                hovered: false,
                pressed: false,
            },
            LayoutStyle::row().with_size(Length::Dp(56.0), Length::Dp(56.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Button;
            n.a11y.focusable = true;
        }
        id
    }

    pub fn accordion(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        expanded: Signal<bool>,
    ) -> NodeId {
        let title_rv = title.into();
        let lbl = title_rv.get();
        let is_exp = expanded.get_untracked();
        let mut layout = LayoutStyle::column();
        layout.header_top_inset = 44.0;
        let id = self.spawn_node(
            "Accordion",
            WidgetKind::Accordion {
                title: title_rv,
                expanded,
                on_change: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::Button;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(is_exp);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn data_table(
        &mut self,
        scope: Scope,
        columns: impl Into<ReactiveValue<Vec<String>>>,
        rows: impl Into<ReactiveValue<Vec<Vec<String>>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "DataTable",
            WidgetKind::DataTable {
                columns: columns.into(),
                rows: rows.into(),
                selected,
                sort_column: None,
                sort_ascending: true,
                scroll_offset: 0.0,
                striped: true,
                row_height: 36.0,
                on_select: None,
                on_sort: None,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.box_style.clip = true;
            n.a11y.role = A11yRole::List;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn set_data_table_options(
        &mut self,
        node: NodeId,
        striped_opt: Option<bool>,
        row_height_opt: Option<f32>,
    ) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::DataTable {
                striped,
                row_height,
                ..
            } = &mut n.kind
        {
            if let Some(s) = striped_opt {
                *striped = s;
            }
            if let Some(rh) = row_height_opt {
                *row_height = rh.clamp(24.0, 96.0);
                n.dirty |= DirtyFlags::LAYOUT;
                self.layout_engine.mark_dirty(node);
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_on_sort(&mut self, node: NodeId, cb: impl FnMut(usize) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::DataTable { on_sort, .. } = &mut n.kind
        {
            *on_sort = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn popup_menu(
        &mut self,
        scope: Scope,
        open: impl Into<ReactiveValue<bool>>,
        items: impl Into<ReactiveValue<Vec<String>>>,
        anchor: Option<Point>,
        on_select: Option<IndexCallback>,
        on_close: Option<TapHandler>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "PopupMenu",
            WidgetKind::PopupMenu {
                open: open.into(),
                items: items.into(),
                anchor,
                on_select,
                on_close,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Menu;
        }
        id
    }

    pub fn tooltip(&mut self, scope: Scope, text: impl Into<ReactiveValue<String>>) -> NodeId {
        let rv = text.into();
        let lbl = rv.get();
        let id = self.spawn_node(
            "Tooltip",
            WidgetKind::Tooltip { text: rv },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Tooltip;
            n.a11y.label = Some(lbl);
        }
        id
    }

    pub fn progress_indicator(
        &mut self,
        scope: Scope,
        progress: impl Into<ReactiveValue<Option<f32>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "ProgressIndicator",
            WidgetKind::ProgressIndicator {
                progress: progress.into(),
                phase: 0.0,
            },
            LayoutStyle::row().with_height(Length::Dp(8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ProgressIndicator;
        }
        id
    }

    pub fn spinner(&mut self, scope: Scope, size: f32) -> NodeId {
        let sz = size.max(12.0);
        let id = self.spawn_node(
            "Spinner",
            WidgetKind::Spinner {
                size: sz,
                thickness: (sz * 0.12).clamp(2.0, 6.0),
                color: None,
                phase: 0.0,
            },
            LayoutStyle::row().with_size(Length::Dp(sz), Length::Dp(sz)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ProgressIndicator;
            n.a11y.label = Some("Loading".to_string());
        }
        id
    }

    pub fn set_spinner_style(
        &mut self,
        node: NodeId,
        size: Option<f32>,
        thickness: Option<f32>,
        color: Option<ReactiveValue<Color>>,
    ) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Spinner { .. }))
        {
            return;
        }
        if let Some(c) = color.clone() {
            if c.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.spinner_color) {
                    let cc = c.clone();
                    eff_scope.effect(move || {
                        let _ = cc.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.spinner_color);
            }
        }
        let mut updated_layout = None;
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Spinner {
                size: sz_slot,
                thickness: th_slot,
                color: col_slot,
                ..
            } = &mut n.kind
        {
            if let Some(sz) = size {
                let next_sz = sz.max(12.0);
                *sz_slot = next_sz;
                n.layout.width = Length::Dp(next_sz);
                n.layout.height = Length::Dp(next_sz);
                updated_layout = Some(n.layout.clone());
                n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            }
            if let Some(th) = thickness {
                *th_slot = th.max(1.5);
                n.dirty |= DirtyFlags::PAINT;
            }
            if let Some(c) = color {
                *col_slot = Some(c);
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        if let Some(st) = updated_layout {
            self.layout_engine.upsert_node(node, st);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn badge(&mut self, scope: Scope, label: impl Into<ReactiveValue<String>>) -> NodeId {
        let rv = label.into();
        let text = rv.get();
        let id = self.spawn_node(
            "Badge",
            WidgetKind::Badge {
                label: rv,
                foreground: None,
                background: None,
                icon: None,
            },
            LayoutStyle::row()
                .with_padding(Insets::symmetric(10.0, 4.0))
                .with_align(Alignment::Center),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(12.0);
            n.a11y.role = A11yRole::Label;
            n.a11y.label = Some(text);
        }
        id
    }

    pub fn set_badge_colors(
        &mut self,
        node: NodeId,
        foreground: Option<ReactiveValue<Color>>,
        background: Option<ReactiveValue<Color>>,
    ) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Badge { .. }))
        {
            return;
        }
        if let Some(fg) = foreground.clone() {
            if fg.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_fg) {
                    let fg_c = fg.clone();
                    eff_scope.effect(move || {
                        let _ = fg_c.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_fg);
            }
        }
        if let Some(bg) = background.clone() {
            if bg.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_bg) {
                    let bg_c = bg.clone();
                    eff_scope.effect(move || {
                        let _ = bg_c.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_bg);
            }
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Badge {
                foreground: fg_slot,
                background: bg_slot,
                ..
            } = &mut n.kind
        {
            if foreground.is_some() {
                *fg_slot = foreground;
            }
            if background.is_some() {
                *bg_slot = background;
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_badge_icon(&mut self, node: NodeId, icon: Option<ReactiveValue<String>>) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Badge { .. }))
        {
            return;
        }
        if let Some(ic) = icon.clone() {
            if ic.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_icon) {
                    let ic_c = ic.clone();
                    eff_scope.effect(move || {
                        let _ = ic_c.get();
                        mark_node_dirty(node, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_icon);
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.badge_icon);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Badge { icon: slot, .. } = &mut n.kind
        {
            *slot = icon;
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn split_pane(&mut self, scope: Scope, axis: Axis, ratio: Signal<f32>) -> NodeId {
        let div = 8.0;
        let mut layout = match axis {
            Axis::Horizontal => LayoutStyle::row(),
            Axis::Vertical => LayoutStyle::column(),
        };
        layout.align_items = Alignment::Stretch;
        layout.width = Length::Pct(100.0);
        layout.height = Length::Pct(100.0);
        layout.clip_overflow = true;
        match axis {
            Axis::Horizontal => layout.gap_x = div,
            Axis::Vertical => layout.gap_y = div,
        }
        let id = self.spawn_node(
            "SplitPane",
            WidgetKind::SplitPane {
                axis,
                ratio,
                min_first: 80.0,
                min_second: 80.0,
                divider_size: div,
                dragging: false,
                on_change: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    pub fn set_split_pane_config(
        &mut self,
        node: NodeId,
        min_first: f32,
        min_second: f32,
        divider_size: f32,
    ) {
        let mut updated_layout = None;
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::SplitPane {
                axis,
                min_first: mf,
                min_second: ms,
                divider_size: ds,
                ..
            } = &mut n.kind
        {
            *mf = min_first.max(0.0);
            *ms = min_second.max(0.0);
            *ds = divider_size.max(4.0);
            match axis {
                Axis::Horizontal => n.layout.gap_x = *ds,
                Axis::Vertical => n.layout.gap_y = *ds,
            }
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            updated_layout = Some(n.layout.clone());
        }
        if let Some(st) = updated_layout {
            self.layout_engine.upsert_node(node, st);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn toast_host(&mut self, scope: Scope, manager: ToastManager) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "ToastHost",
            WidgetKind::ToastHost {
                items: ReactiveValue::from(manager.items_signal()),
                on_dismiss: None,
                manager: Some(manager),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Alert;
            n.a11y.label = Some("Notifications".to_string());
        }
        id
    }

    pub fn toast_host_custom(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<ToastItem>>>,
        on_dismiss: Option<ToastDismissCallback>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "ToastHost",
            WidgetKind::ToastHost {
                items: items.into(),
                on_dismiss,
                manager: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Alert;
            n.a11y.label = Some("Notifications".to_string());
        }
        id
    }

    pub fn set_on_dismiss(&mut self, node: NodeId, cb: impl FnMut(u64) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::ToastHost { on_dismiss, .. } = &mut n.kind
        {
            *on_dismiss = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn overlay(
        &mut self,
        scope: Scope,
        open: impl Into<ReactiveValue<bool>>,
        is_bottom_sheet: bool,
        on_close: Option<TapHandler>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        layout.justify_content = if is_bottom_sheet {
            craftui_core::Justify::End
        } else {
            craftui_core::Justify::Center
        };
        layout.align_items = Alignment::Center;
        let id = self.spawn_node(
            if is_bottom_sheet {
                "BottomSheet"
            } else {
                "Modal"
            },
            WidgetKind::Overlay {
                open: open.into(),
                on_close,
                is_bottom_sheet,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Dialog;
        }
        id
    }

    pub fn platform_view(
        &mut self,
        scope: Scope,
        view_id: PlatformViewId,
        layout: LayoutStyle,
    ) -> NodeId {
        self.spawn_node(
            "PlatformView",
            WidgetKind::PlatformView { id: view_id },
            layout,
            scope,
        )
    }

    pub fn html_view(
        &mut self,
        scope: Scope,
        src: impl Into<ReactiveValue<String>>,
        html: impl Into<ReactiveValue<String>>,
        router: Option<Rc<RefCell<craftui_core::IpcRouter>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "HtmlView",
            WidgetKind::HtmlView {
                src: src.into(),
                html: html.into(),
                router,
            },
            LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    /// Mount a Material 3 Card panel container matching Flutter's `Card` (`Elevated`, `Filled`, `Outlined`).
    pub fn card(&mut self, scope: Scope, variant: CardVariant) -> NodeId {
        let layout = LayoutStyle::column().with_padding(Insets::all(16.0));
        let box_style = BoxStyle {
            radius: CornerRadii::all(12.0),
            ..Default::default()
        };
        let id = self.spawn_node("Card", WidgetKind::Card { variant }, layout, scope);
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style = box_style;
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    /// Mount a Material 3 item row matching Flutter's `ListTile`.
    pub fn list_tile(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        subtitle: Option<impl Into<ReactiveValue<String>>>,
        leading: Option<impl Into<ReactiveValue<String>>>,
        trailing: Option<impl Into<ReactiveValue<String>>>,
        on_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let title_rv = title.into();
        let subtitle_rv = subtitle.map(|s| s.into());
        let leading_rv = leading.map(|l| l.into());
        let trailing_rv = trailing.map(|t| t.into());
        let h = if subtitle_rv.is_some() { 72.0 } else { 56.0 };
        let layout = LayoutStyle::row()
            .with_size(Length::Pct(100.0), Length::Dp(h))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "ListTile",
            WidgetKind::ListTile {
                leading: leading_rv,
                title: title_rv.clone(),
                subtitle: subtitle_rv,
                trailing: trailing_rv,
                on_tap: on_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
                hovered: false,
                pressed: false,
                selected: ReactiveValue::Static(false),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ListItem;
            n.a11y.label = Some(title_rv.get());
            n.a11y.focusable = true;
        }
        id
    }

    /// Mount a Material 3 selectable or action chip matching Flutter's `Chip` / `FilterChip`.
    pub fn chip(
        &mut self,
        scope: Scope,
        label: impl Into<ReactiveValue<String>>,
        selected: Signal<bool>,
        icon: Option<impl Into<ReactiveValue<String>>>,
        on_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let label_rv = label.into();
        let icon_rv = icon.map(|i| i.into());
        let layout = LayoutStyle::row()
            .with_size(Length::Auto, Length::Dp(32.0))
            .with_padding(Insets::symmetric(12.0, 4.0))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "Chip",
            WidgetKind::Chip {
                label: label_rv.clone(),
                icon: icon_rv,
                selected,
                on_tap: on_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
                hovered: false,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::CheckBox;
            n.a11y.label = Some(label_rv.get());
            n.a11y.focusable = true;
        }
        id
    }

    /// Mount a Material 3 Top App Bar header matching Flutter's `AppBar`.
    pub fn app_bar(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        leading_icon: Option<impl Into<ReactiveValue<String>>>,
        elevation: f32,
        on_leading_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let title_rv = title.into();
        let leading_rv = leading_icon.map(|l| l.into());
        let layout = LayoutStyle::row()
            .with_size(Length::Pct(100.0), Length::Dp(64.0))
            .with_padding(Insets::symmetric(16.0, 8.0))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "AppBar",
            WidgetKind::AppBar {
                title: title_rv.clone(),
                leading_icon: leading_rv,
                elevation,
                on_leading_tap: on_leading_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Container;
            n.a11y.label = Some(title_rv.get());
        }
        id
    }

    /// Mount a slide-out navigation drawer panel matching Flutter's `Drawer`.
    pub fn drawer(
        &mut self,
        scope: Scope,
        open: Signal<bool>,
        width: f32,
        on_close: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "Drawer",
            WidgetKind::Drawer {
                open,
                width,
                on_close: on_close.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Dialog;
            n.a11y.label = Some("Navigation Drawer".to_string());
        }
        id
    }

    /// Compose a Flutter-style Scaffold layout managing app bar, body, bottom nav, FAB, and drawer.
    pub fn scaffold(
        &mut self,
        scope: Scope,
        app_bar: Option<NodeId>,
        body: NodeId,
        bottom_nav_bar: Option<NodeId>,
        floating_action_button: Option<NodeId>,
        drawer: Option<NodeId>,
    ) -> NodeId {
        let root_layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        let root = self.container(scope, root_layout, BoxStyle::default());
        if let Some(ab) = app_bar {
            self.append_child(root, ab);
        }
        if let Some(mut layout) = self.nodes.get(body).map(|n| n.layout.clone()) {
            layout.flex_grow = 1.0;
            self.set_layout(body, layout);
        }
        self.append_child(root, body);
        if let Some(bnb) = bottom_nav_bar {
            self.append_child(root, bnb);
        }
        if let Some(fab) = floating_action_button {
            if let Some(mut layout) = self.nodes.get(fab).map(|n| n.layout.clone()) {
                layout.absolute = true;
                layout.inset_bottom = Some(24.0);
                layout.inset_right = Some(24.0);
                self.set_layout(fab, layout);
            }
            self.append_child(root, fab);
        }
        if let Some(dr) = drawer {
            if let Some(mut layout) = self.nodes.get(dr).map(|n| n.layout.clone()) {
                layout.absolute = true;
                layout.use_inset = true;
                layout.inset = Insets::ZERO;
                self.set_layout(dr, layout);
            }
            self.append_child(root, dr);
        }
        root
    }

    /// Build an interactive calendar `DatePicker` bound to an ISO `"YYYY-MM-DD"` signal.
    pub fn date_picker(
        &mut self,
        scope: Scope,
        value: Signal<String>,
        on_change: Option<impl FnMut(String) + 'static>,
    ) -> NodeId {
        let (view_year, view_month, _) = crate::pickers::parse_iso_date(&value.get_untracked());
        let id = self.spawn_node(
            "DatePicker",
            WidgetKind::DatePicker {
                value,
                view_year,
                view_month,
                on_change: on_change.map(|f| Rc::new(RefCell::new(f)) as crate::TextCallback),
            },
            LayoutStyle::default(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = craftui_a11y::A11yRole::Container;
            n.a11y.focusable = true;
            n.a11y.label = Some("DatePicker".into());
        }
        id
    }

    /// Build an interactive `TimePicker` bound to an `"HH:MM"` signal.
    pub fn time_picker(
        &mut self,
        scope: Scope,
        value: Signal<String>,
        use_24h: bool,
        on_change: Option<impl FnMut(String) + 'static>,
    ) -> NodeId {
        let id = self.spawn_node(
            "TimePicker",
            WidgetKind::TimePicker {
                value,
                use_24h,
                on_change: on_change.map(|f| Rc::new(RefCell::new(f)) as crate::TextCallback),
            },
            LayoutStyle::default(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = craftui_a11y::A11yRole::Container;
            n.a11y.focusable = true;
            n.a11y.label = Some("TimePicker".into());
        }
        id
    }

    /// Build a collapsible `TreeView` from slash-delimited item paths (e.g. `"src/main.rs"`).
    pub fn tree_view(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
        on_select: Option<impl FnMut(usize) + 'static>,
    ) -> NodeId {
        let id = self.spawn_node(
            "TreeView",
            WidgetKind::TreeView {
                items: items.into(),
                selected,
                collapsed: Rc::new(RefCell::new(std::collections::HashSet::new())),
                row_height: 30.0,
                scroll_offset: 0.0,
                on_select: on_select.map(|f| Rc::new(RefCell::new(f)) as IndexCallback),
            },
            LayoutStyle::default(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = craftui_a11y::A11yRole::List;
            n.a11y.focusable = true;
            n.a11y.label = Some("TreeView".into());
        }
        id
    }

    /// Mount any custom user-defined widget implementing [`CustomWidget`].
    pub fn custom_widget<W: CustomWidget + 'static>(
        &mut self,
        scope: Scope,
        widget: W,
        layout: LayoutStyle,
    ) -> NodeId {
        let dbg = widget.debug_name();
        let a11y = widget.a11y();
        let is_focusable = widget.focusable() || a11y.focusable;
        let id = self.spawn_node(
            dbg,
            WidgetKind::Custom(Rc::new(RefCell::new(widget))),
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y = a11y;
            n.a11y.focusable = is_focusable;
        }
        id
    }

    pub(crate) fn effective_taffy_layout(n: &WidgetNode) -> LayoutStyle {
        let mut st = n.layout.clone();
        if n.taffy_collapsed_override {
            st.height = Length::Dp(44.0);
            st.min_height = Length::Dp(44.0);
            st.max_height = Length::Dp(44.0);
            st.padding.top = 0.0;
            st.padding.bottom = 0.0;
        }
        if n.taffy_hidden_by_parent {
            st.hidden = true;
        }
        st
    }
}
