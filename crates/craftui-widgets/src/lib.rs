//! Retained widget tree, standard widget primitives, gestures, animations, router, and inspector.

pub mod anim;
pub mod gesture;
pub mod inspector;
pub mod router;
pub mod toast;
pub mod tree;
pub mod widgets;

pub use anim::{
    AnimatableProp, AnimatedColor, AnimatedFloat, Easing, Spring, TransitionCurve, Tween,
};
pub use gesture::{GestureArena, RecognizedGesture, ScrollPhysics, VelocityTracker};
pub use inspector::InspectorState;
pub use router::{Router, RouterTransition};
pub use toast::{ToastItem, ToastManager, ToastSeverity};
pub use tree::{TreeUiSnapshot, VirtualItemFactory, WidgetNode, WidgetTree};
pub use widgets::{
    BoolCallback, BoxStyle, CanvasPaintFn, CanvasPointerFn, CardVariant, CustomWidget,
    FloatCallback, HoverCallback, IndexCallback, PinchRotateCallback, ReactiveValue, TapHandler,
    TextCallback, ToastDismissCallback, WidgetKind,
};

/// Composable declarative view mountable onto a [`WidgetTree`].
pub trait View {
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId;
}

impl View for craftui_core::NodeId {
    fn build(self, _cx: craftui_reactive::Scope, _tree: &mut WidgetTree) -> craftui_core::NodeId {
        self
    }
}

/// Closure-backed [`View`] returned by `cui!` macro invocations and `#[component]` functions.
pub struct ViewFn<F>(pub F);

impl<F> ViewFn<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> View for ViewFn<F>
where
    F: FnOnce(craftui_reactive::Scope, &mut WidgetTree) -> craftui_core::NodeId,
{
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId {
        (self.0)(cx, tree)
    }
}

impl WidgetTree {
    pub fn mount(&mut self, cx: craftui_reactive::Scope, view: impl View) -> craftui_core::NodeId {
        let id = view.build(cx, self);
        self.set_root(id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{
        Axis, Insets, KeyCode, KeyEvent, Length, Modifiers, Point, PointerButton, PointerEvent,
        PointerId, PointerPhase, Size, ThemeMode, UiEvent, WindowMetrics,
    };
    use craftui_layout::LayoutStyle;
    use craftui_reactive::{Scope, reset_runtime};
    use craftui_render::{
        BackendKind, PaintCommand, PaintScene, TextureRegistry, render_scene_to_rgba,
    };
    use craftui_text::TextStyle;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn button_click_updates_signal_and_repaints() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0i32);
        let mut tree = WidgetTree::new(cx);

        let col = tree.column(cx);
        tree.set_layout(
            col,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(200.0)),
        );
        let label = tree.text(
            cx,
            move || format!("Count: {}", count.get()),
            TextStyle::default().with_size(18.0),
        );
        let btn = tree.button(cx, "Increment", move || {
            count.update(|c| *c += 1);
        });
        tree.append_child(col, label);
        tree.append_child(col, btn);
        tree.set_root(col);

        let metrics = WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        };
        tree.layout(metrics);
        let btn_center = tree.rect(btn).center();

        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));

        assert_eq!(count.get(), 1);
        let dirty = tree.layout(metrics);
        assert!(dirty > 0);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &scene,
            300,
            200,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 300 * 200 * 4);
    }

    #[test]
    fn virtual_list_recycles_10000_items_windowed() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let built = Rc::new(Cell::new(0usize));
        let built_c = built.clone();

        let vlist = tree.virtual_list(
            cx,
            Axis::Vertical,
            1,
            40.0,
            10_000usize,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(200.0)),
            Rc::new(move |scope, idx, t| {
                built_c.set(built_c.get() + 1);
                t.text(scope, format!("Item {idx}"), TextStyle::default())
            }),
        );
        tree.set_root(vlist);
        tree.layout(WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        });

        // 200px viewport / 40px row = 5 visible rows + overscan -> far less than 10,000!
        assert!(built.get() > 0 && built.get() < 20);
        assert!(tree.nodes.len() < 25);
    }

    #[test]
    fn text_input_and_slider_and_theme_interpolation() {
        reset_runtime();
        let cx = Scope::root();
        let val = cx.signal(0.0f32);
        let text_sig = cx.signal(String::new());
        let mut tree = WidgetTree::new(cx);
        let col = tree.column(cx);
        let input = tree.text_input(cx, "", "Search...", Some(text_sig));
        let slider = tree.slider(cx, val, -100.0, 100.0, "Brightness");
        tree.append_child(col, input);
        tree.append_child(col, slider);
        tree.set_root(col);
        tree.layout(WindowMetrics {
            logical_size: Size::new(400.0, 200.0),
            ..Default::default()
        });

        let ic = tree.rect(input).center();
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: ic,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 0,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: ic,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Character("A".into()),
            text: Some("A".into()),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert_eq!(text_sig.get(), "A");

        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..120 {
            if !tree.tick_animations(1.0 / 60.0) {
                break;
            }
        }
        assert_eq!(tree.theme().mode, ThemeMode::Light);
    }

    struct CounterBadgeWidget {
        hits: Rc<Cell<usize>>,
    }

    impl CustomWidget for CounterBadgeWidget {
        fn debug_name(&self) -> &'static str {
            "CounterBadgeWidget"
        }

        fn measure(
            &mut self,
            _constraints: craftui_core::Constraints,
            _text_engine: &mut craftui_text::TextEngine,
            _theme: &craftui_core::ThemeTokens,
        ) -> Size {
            Size::new(90.0, 32.0)
        }

        fn paint(
            &mut self,
            rect: craftui_core::Rect,
            scene: &mut PaintScene,
            _text_engine: &mut craftui_text::TextEngine,
            theme: &craftui_core::ThemeTokens,
            focused: bool,
        ) {
            scene.fill_rect(
                rect,
                if focused {
                    theme.primary
                } else {
                    theme.surface_variant
                },
                craftui_core::CornerRadii::all(8.0),
            );
        }

        fn handle_event(&mut self, _rect: craftui_core::Rect, event: &UiEvent) -> bool {
            if let UiEvent::Pointer(pe) = event
                && matches!(pe.phase, PointerPhase::Down)
            {
                self.hits.set(self.hits.get() + 1);
                return true;
            }
            false
        }
    }

    #[test]
    fn custom_widget_new_controls_gestures_and_keyboard_traversal() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let metrics = WindowMetrics {
            logical_size: Size::new(500.0, 500.0),
            ..Default::default()
        };

        let root = tree.column(cx);
        let tab_idx = cx.signal(0usize);
        let sel_idx = cx.signal(0usize);
        let rad_idx = cx.signal(0usize);
        let area_sig = cx.signal("line1".to_string());
        let modal_open = cx.signal(false);

        let tabs = tree.tabs(cx, vec!["Overview".into(), "Logs".into()], tab_idx);
        let select = tree.select(
            cx,
            vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
            sel_idx,
            "Choose...",
        );
        let radio = tree.radio_group(
            cx,
            vec!["Small".into(), "Large".into()],
            rad_idx,
            Axis::Horizontal,
        );
        let textarea = tree.text_area(cx, "", "Notes...", Some(area_sig));
        let div = tree.divider(cx, Axis::Horizontal, 2.0);
        let ico = tree.icon(cx, "check", 24.0, None);
        let badge_hits = Rc::new(Cell::new(0usize));
        let badge = tree.custom_widget(
            cx,
            CounterBadgeWidget {
                hits: badge_hits.clone(),
            },
            LayoutStyle::row(),
        );
        let modal = tree.overlay(
            cx,
            modal_open,
            false,
            Some(Rc::new(RefCell::new(move || modal_open.set(false)))),
        );
        let modal_btn = tree.button(cx, "Confirm", move || modal_open.set(false));
        tree.append_child(modal, modal_btn);

        for child in [tabs, select, radio, textarea, div, ico, badge, modal] {
            tree.append_child(root, child);
        }
        tree.set_root(root);
        tree.layout(metrics);

        // Verify gestures: DoubleTap, LongPress, Hover, PinchRotate
        let dbl_hits = Rc::new(Cell::new(0usize));
        let dbl_c = dbl_hits.clone();
        tree.set_on_double_tap(badge, move || dbl_c.set(dbl_c.get() + 1));
        let lp_hits = Rc::new(Cell::new(0usize));
        let lp_c = lp_hits.clone();
        tree.set_on_long_press(badge, move || lp_c.set(lp_c.get() + 1));
        let hovered = Rc::new(Cell::new(false));
        let hov_c = hovered.clone();
        tree.set_on_hover(badge, move |h| hov_c.set(h));
        let pinched = Rc::new(Cell::new(false));
        let pin_c = pinched.clone();
        tree.set_on_pinch_rotate(root, move |_, _, _| pin_c.set(true));

        let bc = tree.rect(badge).center();
        let make_pt = |phase, ts| PointerEvent {
            id: PointerId(1),
            phase,
            position: bc,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: ts,
        };
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 10)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 30)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 80)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 110)));
        assert_eq!(badge_hits.get(), 2);
        assert_eq!(dbl_hits.get(), 1);
        assert!(hovered.get());

        // LongPress
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 500)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Move, 1100)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 1150)));
        assert_eq!(lp_hits.get(), 1);

        // Pinch
        tree.handle_event(&UiEvent::Pinch {
            factor: 1.25,
            center: bc,
        });
        assert!(pinched.get());

        // Tab / Shift+Tab focus traversal
        let key_evt = |code, shift| KeyEvent {
            code,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                shift,
                ..Default::default()
            },
        };
        tree.focused = None;
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(tabs));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::ArrowRight, false)));
        assert_eq!(tab_idx.get(), 1);
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(select));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(radio));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(textarea));

        // TextArea newline with Enter + Up/Down
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Enter, false)));
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Character("B".into()),
            text: Some("B".into()),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert_eq!(area_sig.get(), "line1\nB");

        // Clipboard via PlatformServices
        tree.services.write_clipboard("+C");
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyV,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(area_sig.get(), "line1\nB+C");

        // Modal open traps Tab focus & Escape closes it
        modal_open.set(true);
        tree.layout(metrics);
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(modal_btn));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Escape, false)));
        assert!(!modal_open.get());

        // Render full scene verifying SVG Icon, CustomWidget, Select, Radio, Tabs, Divider all paint
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &scene,
            500,
            500,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 500 * 500 * 4);
    }

    #[test]
    fn reactive_for_reuses_keyed_nodes_on_reorder() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let list_sig = cx.signal(vec![(1u32, "One"), (2u32, "Two"), (3u32, "Three")]);
        let build_count = Rc::new(Cell::new(0usize));
        let bc = build_count.clone();

        let host = tree.reactive_for(
            cx,
            LayoutStyle::column(),
            move || list_sig.get(),
            |_, (k, _)| *k,
            move |scope, (_k, lbl), _, t| {
                bc.set(bc.get() + 1);
                t.text(scope, lbl, TextStyle::default())
            },
        );
        tree.set_root(host);
        let metrics = WindowMetrics::default();
        tree.layout(metrics);
        assert_eq!(build_count.get(), 3);
        let initial_children = tree.nodes.get(host).unwrap().children.clone();

        // Reorder and drop #2, add #4: #1 and #3 keep exact same NodeId, only #4 built!
        list_sig.set(vec![(3u32, "Three"), (1u32, "One"), (4u32, "Four")]);
        tree.layout(metrics);
        assert_eq!(build_count.get(), 4);
        let new_children = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(new_children[0], initial_children[2]);
        assert_eq!(new_children[1], initial_children[0]);
        assert!(!tree.nodes.contains_key(initial_children[1]));
    }

    #[test]
    fn scrollview_nested_child_drag_uses_scrollview_viewport() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let sv = tree.scroll_view(
            cx,
            Axis::Vertical,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(200.0)),
        );
        let col = tree.column(cx);
        // 15 small rows of 40dp => 600dp total content in a 200dp viewport (max_off = 400dp)
        let mut first_row = None;
        for i in 0..15 {
            let row = tree.container(
                cx,
                LayoutStyle::row().with_size(Length::Dp(280.0), Length::Dp(40.0)),
                BoxStyle::new(),
            );
            let lbl = tree.text(cx, format!("Row {i}"), TextStyle::default());
            tree.append_child(row, lbl);
            tree.append_child(col, row);
            if first_row.is_none() {
                first_row = Some(row);
            }
        }
        tree.append_child(sv, col);
        tree.set_root(sv);
        tree.layout(WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        });

        // Dragging UP by 120dp starting on the 40dp tall child row (max_off for viewport=200 is 400).
        // Previously BUG-01 used child.height (40) so max_off over-clamped to 600-40 instead of 400!
        let start = tree.rect(first_row.unwrap()).center();
        for (idx, dy) in [0.0f32, -25.0, -35.0, -60.0].into_iter().enumerate() {
            let phase = match idx {
                0 => PointerPhase::Down,
                3 => PointerPhase::Move,
                _ => PointerPhase::Move,
            };
            let pos =
                craftui_core::Point::new(start.x, start.y + [-0.0, -25.0, -60.0, -120.0][idx]);
            let _ = dy;
            tree.handle_event(&UiEvent::Pointer(PointerEvent {
                id: PointerId(1),
                phase,
                position: pos,
                button: Some(PointerButton::Primary),
                modifiers: Modifiers::default(),
                is_touch: true,
                timestamp_ms: idx as u64 * 16,
            }));
        }
        // Now scroll far down past 450dp and ensure it clamps strictly at content_extent (600) - sv.height (200) = 400.0, not 560.0!
        for step in 1..=10 {
            tree.handle_event(&UiEvent::Pointer(PointerEvent {
                id: PointerId(1),
                phase: PointerPhase::Move,
                position: craftui_core::Point::new(start.x, start.y - 120.0 - step as f32 * 50.0),
                button: Some(PointerButton::Primary),
                modifiers: Modifiers::default(),
                is_touch: true,
                timestamp_ms: 80 + step as u64 * 16,
            }));
        }
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: craftui_core::Point::new(start.x, start.y - 620.0),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 300,
        }));
        match &tree.nodes.get(sv).unwrap().kind {
            WidgetKind::ScrollView { offset, .. } => {
                assert!(
                    (*offset - 400.0).abs() < 1.0,
                    "expected max_off 400, got {offset}"
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn text_input_undo_redo_word_nav_and_demand_caret_blink() {
        reset_runtime();
        let cx = Scope::root();
        let text_sig = cx.signal("hello world".to_string());
        let mut tree = WidgetTree::new(cx);
        let input = tree.text_input(cx, "", "Type...", Some(text_sig));
        tree.set_root(input);
        tree.layout(WindowMetrics {
            logical_size: Size::new(300.0, 100.0),
            ..Default::default()
        });
        tree.focused = Some(input);

        // Idle focused TextInput MUST NOT force continuous 60FPS animation ticks
        assert!(!tree.tick_animations(1.0 / 60.0));
        // Demand blink timer arms at now and only fires after 530ms
        assert!(!tree.tick_blink_timers(1000));
        assert!(tree.next_internal_delay_ms(1000).is_some_and(|d| d <= 530));
        assert!(tree.tick_blink_timers(1530));
        assert!(!tree.tick_blink_timers(1600));

        // Word delete backward ("world" -> "hello "). Word editing uses Option
        // on Apple platforms and Ctrl everywhere else.
        let word_modifier = if cfg!(any(target_os = "macos", target_os = "ios")) {
            Modifiers {
                alt: true,
                ..Default::default()
            }
        } else {
            Modifiers {
                ctrl: true,
                ..Default::default()
            }
        };
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: word_modifier,
        }));
        assert_eq!(text_sig.get(), "hello ");

        // Ctrl+Z undo restores "hello world"
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyZ,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(text_sig.get(), "hello world");

        // Ctrl+Y redo restores "hello "
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyY,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(text_sig.get(), "hello ");

        // Move left 3 chars then externally update bound signal to same length: cursor stays clamped cleanly
        for _ in 0..3 {
            tree.handle_event(&UiEvent::Key(KeyEvent {
                code: KeyCode::ArrowLeft,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
        }
        let cursor_before = match &tree.nodes.get(input).unwrap().kind {
            WidgetKind::TextInput { editor, .. } => editor.cursor,
            _ => unreachable!(),
        };
        assert_eq!(cursor_before, 3);
        text_sig.set("hey there!".to_string());
        tree.layout(WindowMetrics {
            logical_size: Size::new(300.0, 100.0),
            ..Default::default()
        });
        let cursor_after = match &tree.nodes.get(input).unwrap().kind {
            WidgetKind::TextInput { editor, .. } => editor.cursor,
            _ => unreachable!(),
        };
        assert_eq!(cursor_after, 3);
    }

    #[test]
    fn select_dropdown_scrolls_and_respects_button_children_and_a11y_diff() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let col = tree.column(cx);
        let sel_sig = cx.signal(0usize);
        let opts: Vec<String> = (0..20).map(|i| format!("Option {i}")).collect();
        let sel = tree.select(cx, opts, sel_sig, "Pick");

        let clicks = Rc::new(Cell::new(0usize));
        let clicks_c = clicks.clone();
        let btn = tree.button(cx, "", move || clicks_c.set(clicks_c.get() + 1));
        let ico = tree.icon(cx, "check", 16.0, None);
        let lbl = tree.text(cx, "Custom Child Button", TextStyle::default());
        tree.append_child(btn, ico);
        tree.append_child(btn, lbl);

        tree.append_child(col, sel);
        tree.append_child(col, btn);
        tree.set_root(col);
        let metrics = WindowMetrics {
            logical_size: Size::new(400.0, 500.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Incremental A11y emits first snapshot, then returns None when unchanged
        assert!(tree.build_a11y_update().is_some());
        assert!(tree.build_a11y_update().is_none());

        // Clicking composed Button child still activates parent button without overlap crash
        let lbl_center = tree.rect(lbl).center();
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: lbl_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: lbl_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 20,
        }));
        assert_eq!(clicks.get(), 1);
        assert!(tree.build_a11y_update().is_some()); // focus changed to btn

        // Open Select, scroll down the dropdown list, and click first visible row (~row 10)
        let sel_center = tree.rect(sel).center();
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: sel_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 30,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: sel_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));
        let drop_pos = craftui_core::Point::new(sel_center.x, tree.rect(sel).max_y() + 20.0);
        assert!(
            tree.handle_event(&UiEvent::Scroll(craftui_core::ScrollDelta {
                x: 0.0,
                y: -340.0, // scroll down by 10 rows (10 * 34px)
                position: drop_pos,
            }))
        );
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: drop_pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 50,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: drop_pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 60,
        }));
        assert!(
            sel_sig.get() >= 9,
            "expected scrolled option >= 9, got {}",
            sel_sig.get()
        );
    }

    #[test]
    fn property_effect_reassignment_disposes_old_effects() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let sig_old = cx.signal(false);
        let sig_new = cx.signal(false);
        let btn = tree.button(cx, "Go", || {});
        tree.set_disabled(btn, sig_old);
        let _ = craftui_reactive::drain_dirty_nodes();

        tree.set_disabled(btn, sig_new);
        let _ = craftui_reactive::drain_dirty_nodes();

        // Updating disposed sig_old should NOT mark btn dirty!
        sig_old.set(true);
        let dirty_after_old = craftui_reactive::drain_dirty_nodes();
        assert!(dirty_after_old.is_empty());

        // Updating active sig_new SHOULD mark btn dirty!
        sig_new.set(true);
        let dirty_after_new = craftui_reactive::drain_dirty_nodes();
        assert!(dirty_after_new.iter().any(|(id, _)| *id == btn));
    }

    #[test]
    fn spring_controls_node_transitions_and_router_transitions_settle_to_idle() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let checked = cx.signal(false);
        let active_tab = cx.signal(0usize);
        let modal_open = cx.signal(false);
        let router = Router::new(cx, "/home".to_string());

        let root = tree.column(cx);
        let sw = tree.switch(cx, checked, "Dark");
        let tabs = tree.tabs(cx, vec!["A".into(), "B".into(), "C".into()], active_tab);
        let card = tree.container(
            cx,
            LayoutStyle::column().with_size(Length::Dp(200.0), Length::Dp(100.0)),
            BoxStyle::new(),
        );
        tree.set_transition(
            card,
            AnimatableProp::Opacity,
            TransitionCurve::tween(0.12, Easing::EaseOut),
        );
        tree.set_transition(
            card,
            AnimatableProp::TranslateX,
            TransitionCurve::spring(360.0, 28.0),
        );
        let modal = tree.overlay(cx, modal_open, false, None);
        let route_slot = tree.animated_route_slot(
            cx,
            LayoutStyle::column(),
            router,
            RouterTransition::SlideLeft,
            |s, path, t| t.text(s, path.clone(), TextStyle::default()),
        );

        for c in [sw, tabs, card, modal, route_slot] {
            tree.append_child(root, c);
        }
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(500.0, 400.0),
            ..Default::default()
        };
        tree.layout(metrics);
        assert!(!tree.has_active_animations());

        // Trigger programmatic Switch, Tabs, Node opacity/translate, Modal, and Router navigation
        checked.set(true);
        active_tab.set(2);
        modal_open.set(true);
        tree.set_opacity(card, 0.25);
        tree.set_translate(card, 48.0, 0.0);
        router.push("/details".to_string());

        tree.layout(metrics);
        assert!(tree.has_active_animations());

        // Step animations mid-way and paint
        assert!(tree.tick_animations(0.016));
        let mut mid_scene = PaintScene::new();
        tree.paint(&mut mid_scene);
        assert!(
            mid_scene
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::PushTransform { .. }))
        );
        assert!(
            mid_scene
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::PushLayer { .. }))
        );

        // Step to completion -> 0% idle animations left
        for _ in 0..240 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        assert!(!tree.has_active_animations());
    }

    #[test]
    fn transform2d_hit_test_local_slider_drag_and_cursors() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);

        let root = tree.container(
            cx,
            LayoutStyle::column()
                .with_size(Length::Dp(400.0), Length::Dp(400.0))
                .with_gap(12.0),
            BoxStyle::new(),
        );
        // Translate the container right by +80dp and down by +40dp
        tree.set_transform(root, craftui_core::Transform2D::translation(80.0, 40.0));

        let val = cx.signal(0.0f32);
        let slider = tree.slider(cx, val, 0.0, 100.0, "");
        let clicks = Rc::new(Cell::new(0usize));
        let clicks_c = clicks.clone();
        let btn = tree.button(cx, "Hit", move || clicks_c.set(clicks_c.get() + 1));
        tree.set_cursor(btn, Some(craftui_core::CursorIcon::Crosshair));
        let input = tree.text_input(cx, "", "Type", None);
        let dis_btn = tree.button(cx, "Locked", || {});
        tree.set_disabled(dis_btn, true);

        tree.append_child(root, slider);
        tree.append_child(root, btn);
        tree.append_child(root, input);
        tree.append_child(root, dis_btn);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(500.0, 500.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Button un-transformed rect center wouldn't hit without transform; shifted +80, +40 hits!
        let btn_screen_center = craftui_core::Point::new(
            tree.rect(btn).center().x + 80.0,
            tree.rect(btn).center().y + 40.0,
        );
        assert_eq!(tree.hit_test(btn_screen_center), Some(btn));
        assert_eq!(
            tree.cursor_at(btn_screen_center),
            craftui_core::CursorIcon::Crosshair
        );

        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: btn_screen_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: btn_screen_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 30,
        }));
        assert_eq!(clicks.get(), 1);

        // Semantic cursors for TextInput, Slider, Disabled Button
        let input_screen = craftui_core::Point::new(
            tree.rect(input).center().x + 80.0,
            tree.rect(input).center().y + 40.0,
        );
        assert_eq!(tree.cursor_at(input_screen), craftui_core::CursorIcon::Text);

        let dis_screen = craftui_core::Point::new(
            tree.rect(dis_btn).center().x + 80.0,
            tree.rect(dis_btn).center().y + 40.0,
        );
        assert_eq!(
            tree.cursor_at(dis_screen),
            craftui_core::CursorIcon::NotAllowed
        );

        // Slider tap near 75% width inside translated root
        let sr = tree.rect(slider);
        let slider_target_screen = craftui_core::Point::new(
            sr.x + 8.0 + (sr.width - 16.0) * 0.75 + 80.0,
            sr.center().y + 40.0,
        );
        assert_eq!(
            tree.cursor_at(slider_target_screen),
            craftui_core::CursorIcon::Grab
        );
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Down,
            position: slider_target_screen,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 50,
        }));
        assert_eq!(tree.hovered_cursor(), craftui_core::CursorIcon::Grabbing);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Up,
            position: slider_target_screen,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 70,
        }));
        assert!(
            (val.get() - 75.0).abs() < 3.0,
            "expected ~75, got {}",
            val.get()
        );
    }

    #[test]
    fn backdrop_blur_multi_shadow_inset_and_bounded_push_layer_order() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);

        let mut glass_style = BoxStyle::new()
            .with_backdrop_blur(16.0, craftui_core::Color::rgba(255, 255, 255, 32))
            .with_shadow(craftui_core::BoxShadow::drop(
                0.0,
                8.0,
                20.0,
                2.0,
                craftui_core::Color::rgba(0, 0, 0, 120),
            ))
            .with_shadow(craftui_core::BoxShadow::inner(
                0.0,
                2.0,
                6.0,
                0.0,
                craftui_core::Color::rgba(255, 255, 255, 60),
            ))
            .with_blend_mode(craftui_core::BlendMode::Screen);
        glass_style.opacity = 0.85;
        glass_style.background =
            Some(craftui_core::Brush::Solid(craftui_core::Color::rgba(30, 41, 59, 180)).into());
        glass_style.radius = craftui_core::CornerRadii::all(12.0);

        let glass = tree.container(
            cx,
            LayoutStyle::column().with_size(Length::Dp(240.0), Length::Dp(120.0)),
            glass_style,
        );
        tree.set_root(glass);
        tree.layout(WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        });

        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        // BackdropBlur MUST be emitted before PushLayer so parent scene content is blurred!
        let blur_idx = scene
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::BackdropBlur { .. }))
            .expect("BackdropBlur emitted");
        let layer_idx = scene
            .commands
            .iter()
            .position(|c| {
                matches!(
                    c,
                    PaintCommand::PushLayer {
                        blend_mode: craftui_core::BlendMode::Screen,
                        ..
                    }
                )
            })
            .expect("PushLayer with BlendMode::Screen emitted");
        assert!(blur_idx < layer_idx);

        // Verify PushLayer has bounded finite bounds, not dummy ±1e5
        if let PaintCommand::PushLayer { rect, .. } = scene.commands[layer_idx] {
            assert!(rect.width < 500.0 && rect.height < 400.0);
        }

        // Verify both outer DropShadow (inset=false) and inner shadow (inset=true) exist
        let outer_shadow = scene
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::DropShadow { inset: false, .. }));
        let inner_shadow = scene
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::DropShadow { inset: true, .. }));
        assert!(outer_shadow.is_some() && inner_shadow.is_some());
        assert!(outer_shadow.unwrap() < inner_shadow.unwrap());

        // Renders cleanly through active backend
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &scene,
            400,
            300,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 400 * 300 * 4);
    }

    #[test]
    fn reactive_for_preserves_unique_keys_and_disambiguates_duplicates() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let list = cx.signal(vec!["A".to_string(), "B".to_string(), "A".to_string()]);
        let built_count = Rc::new(Cell::new(0usize));
        let built_c = built_count.clone();

        let host = tree.reactive_for(
            cx,
            LayoutStyle::column(),
            move || list.get(),
            |_idx, it| it.clone(),
            move |scope, item, _idx, t| {
                built_c.set(built_c.get() + 1);
                t.text(scope, item, TextStyle::default())
            },
        );
        tree.set_root(host);
        let metrics = WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        };
        tree.layout(metrics);

        let initial_children = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(initial_children.len(), 3);
        assert_ne!(
            initial_children[0], initial_children[2],
            "duplicate keys must produce distinct NodeIds"
        );
        let b_node = initial_children[1];
        assert_eq!(built_count.get(), 3);

        // Delete the first "A": remaining items ["B", "A"] -> "B" keeps its exact NodeId!
        list.set(vec!["B".to_string(), "A".to_string()]);
        tree.layout(metrics);
        let after_children = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(after_children.len(), 2);
        assert_eq!(after_children[0], b_node, "B node must be preserved");
        assert_ne!(after_children[0], after_children[1]);
    }

    #[test]
    fn bind_dynamic_prop_and_ui_snapshot_restore() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let op = cx.signal(0.25f32);
        let txt_sig = cx.signal("hello world".to_string());

        let col = tree.column(cx);
        let card = tree.container(
            cx,
            LayoutStyle::column().with_size(Length::Dp(120.0), Length::Dp(60.0)),
            BoxStyle::new(),
        );
        tree.bind_dynamic_prop(
            card,
            craftui_core::DirtyFlags::PAINT,
            move || {
                let _ = op.get();
            },
            move |id, t| {
                t.set_opacity(id, op.get_untracked());
            },
        );
        tree.set_transition(
            card,
            AnimatableProp::Opacity,
            TransitionCurve::tween(0.15, Easing::EaseOut),
        );
        let input = tree.text_input(cx, "", "Type...", Some(txt_sig));
        tree.append_child(col, card);
        tree.append_child(col, input);
        tree.set_root(col);

        let metrics = WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        };
        tree.layout(metrics);
        assert!((tree.nodes.get(card).unwrap().box_style.opacity - 0.25).abs() < 1e-4);

        // Update signal -> layout() applies new target -> opacity transitions smoothly
        op.set(0.9);
        tree.layout(metrics);
        assert!((tree.nodes.get(card).unwrap().box_style.opacity - 0.9).abs() < 1e-4);

        // Move cursor and focus input, capture snapshot, restore onto same topology
        tree.focused = Some(input);
        if let Some(n) = tree.nodes.get_mut(input)
            && let WidgetKind::TextInput { editor, .. } = &mut n.kind
        {
            editor.cursor = 5;
            editor.anchor = 2;
        }
        let snap = tree.capture_ui_state();
        tree.focused = None;
        if let Some(n) = tree.nodes.get_mut(input)
            && let WidgetKind::TextInput { editor, .. } = &mut n.kind
        {
            editor.cursor = 0;
            editor.anchor = 0;
        }
        tree.restore_ui_state(&snap);
        assert_eq!(tree.focused, Some(input));
        if let Some(n) = tree.nodes.get(input)
            && let WidgetKind::TextInput { editor, .. } = &n.kind
        {
            assert_eq!(editor.cursor, 5);
            assert_eq!(editor.anchor, 2);
        }
    }

    #[test]
    fn reactive_for_keyed_reuses_nodes_and_updates_signals_in_place() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        #[derive(Clone, PartialEq)]
        struct Item {
            id: u32,
            label: String,
        }
        let items = cx.signal(vec![
            Item {
                id: 1,
                label: "Alpha".into(),
            },
            Item {
                id: 2,
                label: "Beta".into(),
            },
        ]);
        let build_count = Rc::new(Cell::new(0usize));
        let build_c = build_count.clone();

        let host = tree.reactive_for_keyed(
            cx,
            LayoutStyle::column(),
            move || items.get(),
            |_, it| it.id,
            move |scope, it_sig, idx_sig, t| {
                build_c.set(build_c.get() + 1);
                t.text(
                    scope,
                    ReactiveValue::Dynamic(Rc::new(move || {
                        format!("{}:{}", idx_sig.get(), it_sig.get().label)
                    })),
                    TextStyle::default(),
                )
            },
        );
        tree.set_root(host);
        let metrics = WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        };
        tree.layout(metrics);

        let text_of = |t: &WidgetTree, id: craftui_core::NodeId| -> String {
            match &t.nodes.get(id).unwrap().kind {
                WidgetKind::Text { content, .. } => content.get(),
                _ => unreachable!(),
            }
        };

        let children0 = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(children0.len(), 2);
        assert_eq!(build_count.get(), 2);
        assert_eq!(text_of(&tree, children0[0]), "0:Alpha");
        assert_eq!(text_of(&tree, children0[1]), "1:Beta");
        let n_alpha = children0[0];
        let n_beta = children0[1];

        // Reorder and mutate Beta's payload without changing key! Zero rebuilds!
        items.set(vec![
            Item {
                id: 2,
                label: "Beta-Edited".into(),
            },
            Item {
                id: 1,
                label: "Alpha".into(),
            },
        ]);
        tree.layout(metrics);

        let children1 = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(children1, vec![n_beta, n_alpha]);
        assert_eq!(build_count.get(), 2, "keyed items must NOT rebuild");
        assert_eq!(text_of(&tree, n_beta), "0:Beta-Edited");
        assert_eq!(text_of(&tree, n_alpha), "1:Alpha");

        // Same key order, update Alpha payload -> still zero rebuilds and live text update
        items.set(vec![
            Item {
                id: 2,
                label: "Beta-Edited".into(),
            },
            Item {
                id: 1,
                label: "Alpha-2".into(),
            },
        ]);
        tree.layout(metrics);
        assert_eq!(build_count.get(), 2);
        assert_eq!(text_of(&tree, n_alpha), "1:Alpha-2");
    }

    #[test]
    fn spinner_visibility_gates_animation_and_badge_measures_cleanly() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let spin_visible = cx.signal(false);
        let modal_open = cx.signal(false);

        let root = tree.column(cx);
        let sp1 = tree.spinner(cx, 28.0);
        tree.set_visible_when(sp1, move || spin_visible.get());
        tree.set_spinner_style(
            sp1,
            Some(30.0),
            Some(3.5),
            Some(ReactiveValue::Static(craftui_core::Color::rgb(
                99, 102, 241,
            ))),
        );

        // Hidden via closed Modal ancestor should ALSO pause animation
        let modal = tree.overlay(cx, modal_open, false, None);
        let sp_modal = tree.spinner(cx, 24.0);
        tree.append_child(modal, sp_modal);

        let badge_row = tree.row(cx);
        let badge_plain = tree.badge(cx, "Status");
        let badge_icon = tree.badge(cx, "Status");
        tree.set_badge_icon(badge_icon, Some(ReactiveValue::Static("check".into())));
        tree.set_badge_colors(
            badge_icon,
            Some(ReactiveValue::Static(craftui_core::Color::rgb(
                16, 185, 129,
            ))),
            Some(ReactiveValue::Static(craftui_core::Color::rgba(
                16, 185, 129, 40,
            ))),
        );
        tree.append_child(badge_row, badge_plain);
        tree.append_child(badge_row, badge_icon);

        for c in [sp1, modal, badge_row] {
            tree.append_child(root, c);
        }
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Both spinners effectively hidden -> 0 active animations!
        assert!(!tree.has_active_animations());
        assert!(!tree.tick_animations(0.016));

        // Badge with icon measures wider than Badge without icon
        assert!(tree.rect(badge_icon).width > tree.rect(badge_plain).width);
        assert!(tree.rect(badge_plain).height >= 20.0);

        // Reveal sp1 -> phase advances and scene paints arc + badge pill
        spin_visible.set(true);
        tree.layout(metrics);
        assert!(tree.has_active_animations());
        assert!(tree.tick_animations(0.25));
        let phase = match &tree.nodes.get(sp1).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => unreachable!(),
        };
        assert!(phase > 0.1);

        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        assert!(
            scene
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::StrokePath { .. }))
        );
        assert!(
            scene
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::DrawText { text, .. } if text == "Status"))
        );
    }

    #[test]
    fn split_pane_drags_with_clamping_and_toast_host_clicks_through_except_cards() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let ratio = cx.signal(0.5f32);
        let bg_clicks = Rc::new(Cell::new(0usize));
        let bg_clicks_c = bg_clicks.clone();
        let dismissed_ids = Rc::new(RefCell::new(Vec::<u64>::new()));
        let dismissed_c = dismissed_ids.clone();

        let mgr = ToastManager::new(cx);
        let tid = mgr.push_at(0, "Saved", "Preferences updated", ToastSeverity::Success, 0);

        let root = tree.stack(cx);
        tree.set_layout(
            root,
            LayoutStyle::stack().with_size(Length::Dp(500.0), Length::Dp(300.0)),
        );

        let split = tree.split_pane(cx, Axis::Horizontal, ratio);
        tree.set_split_pane_config(split, 100.0, 120.0, 8.0);
        let ratio_changes = Rc::new(Cell::new(0usize));
        let rc_c = ratio_changes.clone();
        tree.set_on_float_change(split, move |_| rc_c.set(rc_c.get() + 1));

        let left_btn = tree.button(cx, "Left", move || bg_clicks_c.set(bg_clicks_c.get() + 1));
        let right_box = tree.container(cx, LayoutStyle::column(), BoxStyle::new());
        tree.append_child(split, left_btn);
        tree.append_child(split, right_box);

        let th = tree.toast_host(cx, mgr);
        tree.set_on_dismiss(th, move |id| dismissed_c.borrow_mut().push(id));

        tree.append_child(root, split);
        tree.append_child(root, th);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(500.0, 300.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Pass-2 SplitPane layout: 500 - 8 = 492 usable -> 246 / 246
        assert!((tree.rect(left_btn).width - 246.0).abs() < 1.0);
        assert!((tree.rect(right_box).width - 246.0).abs() < 1.0);

        // Clicking left_btn passes right through ToastHost!
        let btn_center = tree.rect(left_btn).center();
        assert_eq!(tree.hit_test(btn_center), Some(left_btn));
        assert_eq!(
            tree.cursor_at(btn_center),
            craftui_core::CursorIcon::Pointer
        );
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 30,
        }));
        assert_eq!(bg_clicks.get(), 1);

        // Empty right pane hits `right_box` -> Default cursor (not ResizeHorizontal)
        let empty_right = tree.rect(right_box).center();
        assert_eq!(
            tree.cursor_at(empty_right),
            craftui_core::CursorIcon::Default
        );

        // Sash hits `split` -> ResizeHorizontal cursor
        let sash_pt = craftui_core::Point::new(246.0 + 4.0, 150.0);
        assert_eq!(tree.hit_test(sash_pt), Some(split));
        assert_eq!(
            tree.cursor_at(sash_pt),
            craftui_core::CursorIcon::ResizeHorizontal
        );

        // Drag sash far right to x=490 -> clamps at min_second (120dp => first <= 372dp)
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Down,
            position: sash_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));
        assert_eq!(
            tree.hovered_cursor(),
            craftui_core::CursorIcon::ResizeHorizontal
        );
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Move,
            position: craftui_core::Point::new(490.0, 150.0),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 60,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Up,
            position: craftui_core::Point::new(490.0, 150.0),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 80,
        }));
        tree.layout(metrics);
        assert!((tree.rect(left_btn).width - 372.0).abs() < 1.5);
        assert!((tree.rect(right_box).width - 120.0).abs() < 1.5);
        assert!(ratio_changes.get() >= 1);

        // Click close button of bottom-right toast card (card y=220..284, close center=(465, 239))
        let close_pt = craftui_core::Point::new(465.0, 239.0);
        assert_eq!(tree.hit_test(close_pt), Some(th));
        assert_eq!(tree.cursor_at(close_pt), craftui_core::CursorIcon::Pointer);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(3),
            phase: PointerPhase::Down,
            position: close_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 100,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(3),
            phase: PointerPhase::Up,
            position: close_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 120,
        }));
        assert!(mgr.is_empty());
        assert_eq!(*dismissed_ids.borrow(), vec![tid]);
    }

    #[test]
    fn safe_area_dynamic_insets_keyboard_and_set_layout_preservation() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let sa = tree.safe_area(cx);
        tree.set_layout(
            sa,
            LayoutStyle::column()
                .with_size(Length::Pct(100.0), Length::Pct(100.0))
                .with_padding(Insets::all(8.0)),
        );
        let child = tree.container(
            cx,
            LayoutStyle::column()
                .with_width(Length::Pct(100.0))
                .with_flex_grow(1.0),
            BoxStyle::new(),
        );
        tree.append_child(sa, child);
        tree.set_root(sa);

        let metrics1 = WindowMetrics {
            logical_size: Size::new(400.0, 800.0),
            safe_area: Insets::new(44.0, 0.0, 34.0, 0.0),
            keyboard_inset: 0.0,
            ..Default::default()
        };
        tree.layout(metrics1);
        let rc1 = tree.rect(child);
        assert_eq!(rc1.y, 44.0 + 8.0);
        assert_eq!(rc1.height, 800.0 - 44.0 - 34.0 - 16.0);

        let metrics2 = WindowMetrics {
            logical_size: Size::new(400.0, 800.0),
            safe_area: Insets::new(44.0, 0.0, 34.0, 0.0),
            keyboard_inset: 280.0,
            ..Default::default()
        };
        tree.layout(metrics2);
        let rc2 = tree.rect(child);
        assert_eq!(rc2.y, 44.0 + 8.0);
        assert_eq!(rc2.height, 800.0 - 44.0 - 280.0 - 16.0);
    }

    #[test]
    fn segmented_control_click_spring_and_keyboard() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let sel = cx.signal(0usize);
        let events = Rc::new(RefCell::new(Vec::<usize>::new()));
        let ev_c = events.clone();

        let root = tree.column(cx);
        tree.set_layout(
            root,
            LayoutStyle::column().with_size(Length::Dp(360.0), Length::Dp(100.0)),
        );
        let seg = tree.segmented_control(
            cx,
            vec!["Day".to_string(), "Week".to_string(), "Month".to_string()],
            sel,
        );
        tree.set_layout(
            seg,
            LayoutStyle::row()
                .with_width(Length::Dp(300.0))
                .with_height(Length::Dp(36.0)),
        );
        tree.set_on_index_change(seg, move |idx| ev_c.borrow_mut().push(idx));
        tree.append_child(root, seg);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(360.0, 100.0),
            ..Default::default()
        };
        tree.layout(metrics);
        let r = tree.rect(seg);
        let click_third = Point::new(r.x + r.width * 0.85, r.y + r.height * 0.5);
        assert_eq!(tree.hit_test(click_third), Some(seg));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: click_third,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: click_third,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 20,
        }));
        assert_eq!(sel.get(), 2);
        assert!(tree.has_active_animations());
        for _ in 0..8 {
            tree.tick_animations(0.016);
        }

        // Keyboard Home / ArrowRight
        assert!(tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Home,
            modifiers: Modifiers::default(),
            pressed: true,
            repeat: false,
            text: None,
        })));
        assert_eq!(sel.get(), 0);
        assert!(tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::ArrowRight,
            modifiers: Modifiers::default(),
            pressed: true,
            repeat: false,
            text: None,
        })));
        assert_eq!(sel.get(), 1);
        assert_eq!(*events.borrow(), vec![2, 0, 1]);

        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        assert!(
            scene
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::DrawText { text, .. } if text == "Week"))
        );
    }

    #[test]
    fn accordion_collapse_expand_header_hit_a11y_and_keyboard() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let expanded = cx.signal(false);
        let toggles = Rc::new(RefCell::new(Vec::<bool>::new()));
        let tog_c = toggles.clone();
        let child_clicks = Rc::new(Cell::new(0usize));
        let cc = child_clicks.clone();

        let root = tree.column(cx);
        tree.set_layout(
            root,
            LayoutStyle::column().with_size(Length::Dp(340.0), Length::Dp(320.0)),
        );
        let acc = tree.accordion(cx, "Permissions", expanded);
        tree.set_layout(
            acc,
            LayoutStyle::column()
                .with_width(Length::Dp(320.0))
                .with_padding(Insets::all(12.0)),
        );
        tree.set_on_bool_change(acc, move |v| tog_c.borrow_mut().push(v));
        let btn = tree.button(cx, "Grant Camera", move || cc.set(cc.get() + 1));
        tree.set_layout(btn, LayoutStyle::row().with_height(Length::Dp(40.0)));
        tree.append_child(acc, btn);
        tree.append_child(root, acc);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(340.0, 320.0),
            ..Default::default()
        };
        tree.layout(metrics);
        // Collapsed -> header only (44dp) and child hidden in A11y tree
        assert!((tree.rect(acc).height - 44.0).abs() < 0.5);
        let upd1 = tree.build_a11y_update().expect("initial a11y");
        let btn_id = craftui_a11y::accesskit::NodeId::from(slotmap::Key::data(&btn).as_ffi());
        assert!(!upd1.nodes.iter().any(|(id, _)| *id == btn_id));

        // Clicking header expands accordion
        let hdr_pt = Point::new(40.0, 20.0);
        assert_eq!(tree.hit_test(hdr_pt), Some(acc));
        assert_eq!(tree.cursor_at(hdr_pt), craftui_core::CursorIcon::Pointer);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: hdr_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: hdr_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 25,
        }));
        assert!(expanded.get());
        tree.layout(metrics);
        // Expanded height = 44 header + 12 top + 40 child + 12 bottom = 108dp
        assert!((tree.rect(acc).height - 108.0).abs() < 1.0);
        assert!((tree.rect(btn).y - 56.0).abs() < 1.0);
        let upd2 = tree.build_a11y_update().expect("expanded a11y");
        assert!(upd2.nodes.iter().any(|(id, _)| *id == btn_id));

        // Body below header hits child button (or Default on padding), NOT accordion
        let below_hdr = Point::new(6.0, 80.0);
        assert_eq!(tree.hit_test(below_hdr), None);
        let btn_center = tree.rect(btn).center();
        assert_eq!(tree.hit_test(btn_center), Some(btn));

        // Keyboard Space on focused Accordion toggles closed even when expanded
        tree.focused = Some(acc);
        assert!(tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Space,
            modifiers: Modifiers::default(),
            pressed: true,
            repeat: false,
            text: None,
        })));
        assert!(!expanded.get());
        tree.layout(metrics);
        assert!((tree.rect(acc).height - 44.0).abs() < 0.5);
        assert_eq!(*toggles.borrow(), vec![true, false]);

        // A11y click re-expands
        let acc_a11y_id = craftui_a11y::accesskit::NodeId::from(slotmap::Key::data(&acc).as_ffi());
        assert!(
            tree.handle_a11y_action(&craftui_a11y::accesskit::ActionRequest {
                action: craftui_a11y::accesskit::Action::Click,
                target_tree: craftui_a11y::accesskit::TreeId::ROOT,
                target_node: acc_a11y_id,
                data: None,
            })
        );
        assert!(expanded.get());
    }

    #[test]
    fn data_table_sort_selection_preservation_scroll_and_keyboard() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let sel = cx.signal(0usize);
        let sorts = Rc::new(RefCell::new(Vec::<usize>::new()));
        let sorts_c = sorts.clone();
        let selects = Rc::new(RefCell::new(Vec::<usize>::new()));
        let selects_c = selects.clone();

        let cols = vec!["Capability".to_string(), "Score".to_string()];
        let rows = vec![
            vec!["Camera".to_string(), "30".to_string()],
            vec!["Biometrics".to_string(), "10".to_string()],
            vec!["Location".to_string(), "20".to_string()],
            vec!["Bluetooth".to_string(), "40".to_string()],
            vec!["Microphone".to_string(), "50".to_string()],
        ];

        let tbl = tree.data_table(cx, cols, rows, sel);
        // Header 38 + 2 rows visible (72dp) -> viewport 110dp so scroll is active
        tree.set_layout(
            tbl,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(110.0)),
        );
        tree.set_on_index_change(tbl, move |idx| selects_c.borrow_mut().push(idx));
        tree.set_on_sort(tbl, move |col| sorts_c.borrow_mut().push(col));
        tree.set_root(tbl);

        let metrics = WindowMetrics {
            logical_size: Size::new(300.0, 110.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Click second column header ("Score") -> sorts ascending: [10(orig 1), 20(orig 2), 30(orig 0), ...]
        let hdr_col1 = Point::new(220.0, 18.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: hdr_col1,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: hdr_col1,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 20,
        }));
        assert_eq!(*sorts.borrow(), vec![1]);

        // Click first body row (y = 38 + 18 = 56) -> corresponds to orig_idx 1 ("Biometrics", "10")
        let row0_pt = Point::new(80.0, 56.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Down,
            position: row0_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 30,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Up,
            position: row0_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));
        assert_eq!(sel.get(), 1);

        // Keyboard End selects last sorted row ("Microphone", orig_idx 4) and auto-scrolls down
        assert!(tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::End,
            modifiers: Modifiers::default(),
            pressed: true,
            repeat: false,
            text: None,
        })));
        assert_eq!(sel.get(), 4);
        assert!(matches!(
            tree.nodes.get(tbl).map(|n| &n.kind),
            Some(WidgetKind::DataTable { scroll_offset, .. }) if *scroll_offset > 50.0
        ));

        // Wheel scroll back up
        assert!(
            tree.handle_event(&UiEvent::Scroll(craftui_core::ScrollDelta {
                position: Point::new(100.0, 70.0),
                x: 0.0,
                y: 120.0,
            }))
        );
        assert!(matches!(
            tree.nodes.get(tbl).map(|n| &n.kind),
            Some(WidgetKind::DataTable { scroll_offset, .. }) if *scroll_offset == 0.0
        ));
        assert_eq!(*selects.borrow(), vec![1, 4]);
    }

    #[test]
    fn bottom_nav_bar_and_fab_layout_paint_and_tap() {
        reset_runtime();
        let cx = Scope::root();
        let selected_tab = cx.signal(0usize);
        let tab_changes = Rc::new(RefCell::new(Vec::new()));
        let tc = tab_changes.clone();
        let fab_tapped = Rc::new(Cell::new(false));
        let ft = fab_tapped.clone();

        let mut tree = WidgetTree::new(cx);
        let root = tree.column(cx);
        tree.set_layout(
            root,
            LayoutStyle::column().with_size(Length::Dp(360.0), Length::Dp(640.0)),
        );

        let nav = tree.bottom_nav_bar(
            cx,
            vec![
                "Scan".into(),
                "Studio".into(),
                "Files".into(),
                "Settings".into(),
            ],
            vec!["📷".into(), "🎨".into(), "📁".into(), "⚙️".into()],
            selected_tab,
        );
        tree.set_on_index_change(nav, move |i| tc.borrow_mut().push(i));
        tree.append_child(root, nav);

        let fab = tree.floating_action_button(cx, "📷", Some("New Scan"));
        tree.set_on_tap(fab, move || ft.set(true));
        tree.append_child(root, fab);
        tree.set_root(root);

        tree.layout(WindowMetrics {
            logical_size: Size::new(360.0, 640.0),
            ..Default::default()
        });

        let nav_rect = tree.rect(nav);
        assert_eq!(nav_rect.height, 64.0);
        assert_eq!(nav_rect.width, 360.0);

        let fab_rect = tree.rect(fab);
        assert!(fab_rect.width >= 56.0);
        assert_eq!(fab_rect.height, 56.0);

        // Tap tab #2 ("Files" -> local x = 360 / 4 * 2.5 = 225)
        let tab2_pt = Point::new(nav_rect.x + 225.0, nav_rect.y + 32.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: tab2_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: tab2_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 20,
        }));
        assert_eq!(selected_tab.get(), 2);
        assert_eq!(*tab_changes.borrow(), vec![2]);

        // Tap FAB
        let fab_pt = Point::new(fab_rect.x + 20.0, fab_rect.y + 20.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Down,
            position: fab_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 30,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Up,
            position: fab_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 40,
        }));
        assert!(fab_tapped.get());

        // Verify scene painting produces PushClip, DrawText and DropShadow
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let has_clip = scene
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::PushClip { .. }));
        assert!(has_clip);
    }

    #[test]
    fn text_ellipsis_truncation_fits_bounds() {
        let mut te = craftui_text::TextEngine::new();
        let st = TextStyle::default().with_size(14.0);
        let long_str = "A very long document scanner title that exceeds mobile bounds";
        let truncated = te.truncate_with_ellipsis(long_str, &st, 80.0);
        assert!(truncated.ends_with('…'));
        let measured = te.measure(&truncated, &st, None);
        assert!(measured.width <= 80.0);
    }

    #[test]
    fn flutter_design_widgets_card_list_tile_chip_app_bar_drawer_and_scaffold() {
        reset_runtime();
        let cx = Scope::root();
        let drawer_open = cx.signal(false);
        let chip_selected = cx.signal(false);
        let list_tapped = Rc::new(Cell::new(false));
        let lt = list_tapped.clone();
        let menu_tapped = Rc::new(Cell::new(false));
        let mt = menu_tapped.clone();

        let mut tree = WidgetTree::new(cx);

        let app_bar = tree.app_bar(
            cx,
            "CraftUI Flutter Hybrid",
            Some("☰"),
            4.0,
            Some(move || mt.set(true)),
        );

        let body = tree.column(cx);
        let card = tree.card(cx, CardVariant::Elevated);
        let tile = tree.list_tile(
            cx,
            "System Admin",
            Some("Device Policy & Root Privileges"),
            Some("🔒"),
            Some("›"),
            Some(move || lt.set(true)),
        );
        let chip = tree.chip(cx, "Security Mode", chip_selected, Some("🛡️"), None::<fn()>);
        tree.append_child(card, tile);
        tree.append_child(card, chip);
        tree.append_child(body, card);

        let drawer = tree.drawer(cx, drawer_open, 304.0, None::<fn()>);
        let fab = tree.floating_action_button(cx, "+", Some("Add"));

        let root = tree.scaffold(cx, Some(app_bar), body, None, Some(fab), Some(drawer));
        tree.set_root(root);

        tree.layout(WindowMetrics {
            logical_size: Size::new(360.0, 640.0),
            ..Default::default()
        });

        // 1. Verify layout sizes
        let ab_rect = tree.rect(app_bar);
        assert_eq!(ab_rect.height, 64.0);
        assert_eq!(ab_rect.width, 360.0);

        let tile_rect = tree.rect(tile);
        assert_eq!(tile_rect.height, 72.0); // subtitle present -> 72dp

        let chip_rect = tree.rect(chip);
        assert_eq!(chip_rect.height, 32.0);

        // 2. Interaction: Tap ListTile
        let tile_pt = Point::new(tile_rect.x + 40.0, tile_rect.y + 20.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: tile_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: tile_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 20,
        }));
        assert!(list_tapped.get());

        // 3. Interaction: Tap Chip -> toggles selected
        assert!(!chip_selected.get());
        let chip_pt = Point::new(chip_rect.x + 15.0, chip_rect.y + 15.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Down,
            position: chip_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 30,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(2),
            phase: PointerPhase::Up,
            position: chip_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 40,
        }));
        assert!(chip_selected.get());

        // 4. Interaction: Tap AppBar leading menu icon
        let menu_pt = Point::new(ab_rect.x + 20.0, ab_rect.y + 30.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(3),
            phase: PointerPhase::Down,
            position: menu_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 50,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(3),
            phase: PointerPhase::Up,
            position: menu_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 60,
        }));
        assert!(menu_tapped.get());

        // 5. Open Drawer & Paint Scene
        drawer_open.set(true);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let has_drop_shadow = scene
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::DropShadow { .. }));
        assert!(has_drop_shadow);
    }

    #[test]
    fn metrics_assigned_directly_reach_the_reactive_signal() {
        // Platform runners write `tree.metrics` before the first frame and then
        // call `layout(tree.metrics)`. `Window.*` bindings read the signal, so
        // the signal must follow even though the field already has the value.
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let root = tree.column(cx);
        tree.set_root(root);

        let seen_width = Rc::new(Cell::new(0.0f32));
        let seen = seen_width.clone();
        let signal = tree.metrics_signal;
        cx.effect(move || seen.set(signal.get().logical_size.width));

        tree.metrics = WindowMetrics {
            logical_size: Size::new(390.0, 800.0),
            ..Default::default()
        };
        tree.layout(tree.metrics);

        assert_eq!(
            tree.metrics_signal.get_untracked().logical_size.width,
            390.0
        );
        assert_eq!(seen_width.get(), 390.0);
    }

    #[test]
    fn is_visible_follows_visible_when_on_the_node_and_its_ancestors() {
        reset_runtime();
        let cx = Scope::root();
        let shown = cx.signal(true);
        let mut tree = WidgetTree::new(cx);

        let root = tree.column(cx);
        let panel = tree.column(cx);
        let label = tree.text(cx, "inside", TextStyle::default());
        tree.append_child(panel, label);
        tree.append_child(root, panel);
        tree.set_visible_when(panel, move || shown.get());
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        };
        tree.layout(metrics);
        assert!(tree.is_visible(panel));
        assert!(tree.is_visible(label));

        shown.set(false);
        tree.layout(metrics);
        assert!(tree.is_visible(root));
        assert!(!tree.is_visible(panel));
        assert!(
            !tree.is_visible(label),
            "a child of a hidden node is hidden"
        );

        shown.set(true);
        tree.layout(metrics);
        assert!(tree.is_visible(label));

        tree.remove_subtree(panel);
        assert!(!tree.is_visible(label), "a removed node is not visible");
    }

    #[test]
    fn text_fields_use_their_placeholder_as_the_accessible_name() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);

        let search = tree.text_input(cx, "", "Search notes", None);
        let body = tree.text_area(cx, "", "Start writing...", None);
        let unnamed = tree.text_input(cx, "", "", None);

        let label =
            |id: craftui_core::NodeId| tree.nodes.get(id).and_then(|n| n.a11y.label.clone());
        assert_eq!(label(search).as_deref(), Some("Search notes"));
        assert_eq!(label(body).as_deref(), Some("Start writing..."));
        assert_eq!(label(unnamed), None);
    }
}
