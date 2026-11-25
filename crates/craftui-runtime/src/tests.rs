use super::*;
use craftui_core::{
    Brush, CornerRadii, Insets, Modifiers, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, Size, ThemeMode, ThemeTokens, UiEvent, WindowMetrics,
};
use craftui_reactive::{Scope, reset_runtime};
use craftui_widgets::{WidgetKind, WidgetTree};
use std::collections::BTreeMap;

#[test]
fn hot_reload_interprets_cui_under_100ms_and_preserves_state() {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);

    let src_v1 = r#"
component TagChip {
    in property <string> title: "Chip";
    Button { text: title; }
}
export component Demo {
    in-out property <int> count: 0;
    Column {
        width: 100%;
        height: 100%;
        padding: 16dp;
        for name, i in ["A", "B"] : TagChip { title: name; }
        Button { text: "Save"; }
    }
}
"#;
    let root1 = session.reload(src_v1, Some("Demo"), &mut tree).unwrap();
    assert!(session.last_reload_duration.as_millis() < 100);
    session
        .store
        .set_prop(session.state_scope, "count", DynValue::Int(42));
    tree.layout(WindowMetrics {
        logical_size: Size::new(400.0, 300.0),
        ..Default::default()
    });
    assert_eq!(tree.rect(root1).width, 400.0);

    // Hot-reload modified template -> count=42 must persist!
    let src_v2 = src_v1.replace("16dp", "24dp");
    session.reload(&src_v2, Some("Demo"), &mut tree).unwrap();
    assert_eq!(
        session.store.props.get("count").unwrap().get(),
        DynValue::Int(42)
    );
}

#[test]
fn reactive_if_else_for_two_way_subcomponents_theme_and_imports_work() {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    tree.set_theme_mode(ThemeMode::Dark);
    let mut session = HotReloadSession::new(cx);

    let tmp = std::env::temp_dir().join(format!(
        "craftui_runtime_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("shared.cui"),
        r#"
export struct TaskEntry {
    title: string = "",
}
export component ToggleChip {
    in property <string> label: "Pin";
    in-out property <bool> active: false;
    callback toggled();
    Button {
        text: label;
        on_tap => {
            active = !active;
            toggled();
        }
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("app.cui"),
        r#"
import { TaskEntry, ToggleChip } from "./shared.cui"

export component MainApp {
    in-out property <bool> pinned: false;
    in-out property <bool> show_list: false;
    in-out property <[string]> tags: ["Rust", "UI"];
    in-out property <int> selected_tab: 0;
    in-out property <string> query: "hello";

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        border_width: 1dp;
        border_color: Theme.border;

        ToggleChip {
            label: "Toggle";
            active <=> pinned;
        }
        Tabs {
            tabs: ["One", "Two", "Three"];
            active <=> selected_tab;
        }
        TextInput {
            placeholder: "Search";
            value <=> query;
        }
        Divider { thickness: 1dp; }
        Icon { svg: "check"; size: 18dp; }

        if show_list : Column {
            for item, idx in tags : Text {
                text: item;
                color: Theme.text_primary;
            }
        } else : Text {
            text: "Empty";
            color: Theme.text_secondary;
        }
    }
}
"#,
    )
    .unwrap();

    let root = session
        .reload_file(tmp.join("app.cui"), Some("MainApp"), &mut tree)
        .unwrap();
    tree.layout(WindowMetrics {
        logical_size: Size::new(500.0, 400.0),
        ..Default::default()
    });

    // Initially else-branch "Empty" is mounted and theme background is Dark
    let root_bg = tree
        .nodes
        .get(root)
        .unwrap()
        .box_style
        .background
        .as_ref()
        .unwrap()
        .get();
    assert_eq!(
        root_bg,
        craftui_core::Brush::Solid(ThemeTokens::dark().background)
    );

    // Toggle Theme live -> background updates smoothly without hot-reload
    tree.set_theme_mode(ThemeMode::Light);
    for _ in 0..240 {
        if !tree.tick_animations(0.016) {
            break;
        }
    }
    let root_bg_light = tree
        .nodes
        .get(root)
        .unwrap()
        .box_style
        .background
        .as_ref()
        .unwrap()
        .get();
    assert_eq!(
        root_bg_light,
        craftui_core::Brush::Solid(ThemeTokens::light().background)
    );

    // Tap ToggleChip -> toggles `pinned` via `<=>`
    let mut btn_rect = None;
    for id in tree.nodes.keys() {
        if let Some(n) = tree.nodes.get(id)
            && matches!(n.kind, WidgetKind::Button { .. })
        {
            btn_rect = Some(tree.rect(id));
            break;
        }
    }
    let br = btn_rect.unwrap();
    let pt = Point::new(br.x + br.width * 0.5, br.y + br.height * 0.5);
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Down,
        position: pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 0,
    }));
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Up,
        position: pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 20,
    }));
    assert_eq!(
        session.store.props.get("pinned").unwrap().get(),
        DynValue::Bool(true)
    );

    // Switch show_list -> dynamic_slot swaps from "Empty" to reactive_for tags
    session
        .store
        .set_prop(session.state_scope, "show_list", DynValue::Bool(true));
    tree.layout(WindowMetrics {
        logical_size: Size::new(500.0, 400.0),
        ..Default::default()
    });
    let has_rust = tree.nodes.values().any(|n| match &n.kind {
        WidgetKind::Text { content, .. } => content.get() == "Rust",
        _ => false,
    });
    assert!(has_rust);

    // Mutate tags signal live -> reactive_for adds 3rd item
    session.store.set_prop(
        session.state_scope,
        "tags",
        DynValue::List(vec![
            DynValue::String("Rust".into()),
            DynValue::String("UI".into()),
            DynValue::String("Framework".into()),
        ]),
    );
    tree.layout(WindowMetrics {
        logical_size: Size::new(500.0, 400.0),
        ..Default::default()
    });
    let has_fw = tree.nodes.values().any(|n| match &n.kind {
        WidgetKind::Text { content, .. } => content.get() == "Framework",
        _ => false,
    });
    assert!(has_fw);

    let _ = std::fs::remove_dir_all(&tmp);
    let _ = BTreeMap::<String, DynValue>::new();
}

#[test]
fn hot_reload_preserves_focus_caret_and_scroll_snapshot() {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);

    let src1 = r#"export component Editor {
    in-out property <string> query: "hello world";
    Column {
        width: 100%;
        height: 100%;
        padding: 8dp;
        TextInput { id: "search_box"; value <=> query; }
        ScrollView {
            id: "list_sv";
            height: 80dp;
            Column {
                for n in 0..20 : Text { text: n; }
            }
        }
    }
}"#;
    session.reload(src1, Some("Editor"), &mut tree).unwrap();
    let metrics = WindowMetrics {
        logical_size: Size::new(400.0, 300.0),
        ..Default::default()
    };
    tree.layout(metrics);

    let input_id = tree.node_by_id("search_box").unwrap();
    let sv_id = tree.node_by_id("list_sv").unwrap();
    tree.focused = Some(input_id);
    if let Some(n) = tree.nodes.get_mut(input_id)
        && let WidgetKind::TextInput { editor, .. } = &mut n.kind
    {
        editor.cursor = 5;
        editor.anchor = 2;
    }
    if let Some(n) = tree.nodes.get_mut(sv_id)
        && let WidgetKind::ScrollView { offset, .. } = &mut n.kind
    {
        *offset = 42.0;
    }

    // Hot reload with layout tweak: focus, caret, and scroll offset survive!
    let src2 = src1.replace("8dp", "16dp");
    session.reload(&src2, Some("Editor"), &mut tree).unwrap();
    let input_after = tree.node_by_id("search_box").unwrap();
    let sv_after = tree.node_by_id("list_sv").unwrap();
    assert_eq!(tree.focused, Some(input_after));
    match &tree.nodes.get(input_after).unwrap().kind {
        WidgetKind::TextInput { editor, .. } => {
            assert_eq!(editor.cursor, 5);
            assert_eq!(editor.anchor, 2);
        }
        _ => unreachable!(),
    }
    match &tree.nodes.get(sv_after).unwrap().kind {
        WidgetKind::ScrollView { offset, .. } => {
            assert!((*offset - 42.0).abs() < 0.5, "got offset {offset}");
        }
        _ => unreachable!(),
    }
}

#[test]
fn runtime_supports_animate_ternary_nested_assign_keyed_for_and_strict_bindings() {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);

    let src = r#"export struct TaskItem {
    id: int = 0,
    title: string = "",
    done: bool = false,
}

export component ShowcasePhase3 {
    in-out property <[TaskItem]> items: [];
    in-out property <bool> active: false;
    in-out property <bool> one_way_flag: false;
    in-out property <int> points: 10;

    Column {
        width: 100%;
        height: 100%;
        Row {
            id: "card";
            opacity: active ? 1.0 : 0.4;
            scale: active ? 1.2 : 1.0;
            backdrop_blur: 14dp;
            cursor: Pointer;
            animate opacity { duration: 150ms; easing: EaseOut; }
            animate scale { curve: snappy(); }
            Text { text: active ? "ON" : "OFF"; }
        }
        Checkbox { id: "one_way_cb"; checked: one_way_flag; }
        Button {
            id: "mutate_btn";
            text: "Run";
            on_tap => {
                active = true;
                points += 5;
                if points >= 15 {
                    items[0].done = true;
                }
            }
        }
        Column {
            id: "list_col";
            for it, idx in items key: it.id : Row {
                Text { text: idx + ": " + it.title + (it.done ? "!" : ""); }
            }
        }
    }
}"#;
    session
        .reload(src, Some("ShowcasePhase3"), &mut tree)
        .unwrap();
    let mut m0 = BTreeMap::new();
    m0.insert("id".into(), DynValue::Int(101));
    m0.insert("title".into(), DynValue::String("Build".into()));
    m0.insert("done".into(), DynValue::Bool(false));
    let mut m1 = BTreeMap::new();
    m1.insert("id".into(), DynValue::Int(102));
    m1.insert("title".into(), DynValue::String("Ship".into()));
    m1.insert("done".into(), DynValue::Bool(false));
    session.store.set_prop(
        session.state_scope,
        "items",
        DynValue::List(vec![
            DynValue::Struct {
                name: "TaskItem".into(),
                fields: m0,
            },
            DynValue::Struct {
                name: "TaskItem".into(),
                fields: m1,
            },
        ]),
    );

    let metrics = WindowMetrics {
        logical_size: Size::new(500.0, 400.0),
        ..Default::default()
    };
    tree.layout(metrics);
    assert!(
        !tree.has_active_animations(),
        "mount values must snap without animating"
    );
    let card = tree.node_by_id("card").unwrap();
    assert!((tree.nodes.get(card).unwrap().box_style.opacity - 0.4).abs() < 1e-4);

    // One-way `checked: one_way_flag` must NOT mutate parent `one_way_flag` when toggled locally
    let cb_rect = tree.rect(tree.node_by_id("one_way_cb").unwrap());
    let cb_pt = cb_rect.center();
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(1),
        phase: PointerPhase::Down,
        position: cb_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 0,
    }));
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(1),
        phase: PointerPhase::Up,
        position: cb_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 15,
    }));
    assert_eq!(
        session.store.props.get("one_way_flag").unwrap().get(),
        DynValue::Bool(false)
    );

    // Capture existing Row NodeIds in keyed loop
    let list_col = tree.node_by_id("list_col").unwrap();
    let for_host = tree.nodes.get(list_col).unwrap().children[0];
    let initial_rows = tree.nodes.get(for_host).unwrap().children.clone();
    assert_eq!(initial_rows.len(), 2);

    // Click mutate_btn -> flips `active`, adds `points += 5`, mutates `items[0].done = true`
    let btn_pt = tree.rect(tree.node_by_id("mutate_btn").unwrap()).center();
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(2),
        phase: PointerPhase::Down,
        position: btn_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 30,
    }));
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(2),
        phase: PointerPhase::Up,
        position: btn_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 45,
    }));

    tree.layout(metrics);
    assert_eq!(
        session.store.props.get("points").unwrap().get(),
        DynValue::Int(15)
    );
    assert!(
        tree.has_active_animations(),
        "updating reactive animated opacity/scale must trigger animations"
    );
    let after_rows = tree.nodes.get(for_host).unwrap().children.clone();
    assert_eq!(
        after_rows, initial_rows,
        "keyed loop rows must reuse NodeIds when payload mutates"
    );
    let row0_txt_id = tree.nodes.get(after_rows[0]).unwrap().children[0];
    match &tree.nodes.get(row0_txt_id).unwrap().kind {
        WidgetKind::Text { content, .. } => assert_eq!(content.get(), "0: Build!"),
        _ => unreachable!(),
    }

    // Swap the two items in the reactive items signal
    let cur_items = match session.store.props.get("items").unwrap().get() {
        DynValue::List(list) => list,
        _ => unreachable!(),
    };
    session.store.set_prop(
        session.state_scope,
        "items",
        DynValue::List(vec![cur_items[1].clone(), cur_items[0].clone()]),
    );

    tree.layout(metrics);
    let swapped_rows = tree.nodes.get(for_host).unwrap().children.clone();
    assert_eq!(
        swapped_rows,
        vec![initial_rows[1], initial_rows[0]],
        "keyed loop rows must reorder existing NodeIds when items are swapped"
    );

    // Row 0 now has Ship at index 0
    let row0_new_txt = tree.nodes.get(swapped_rows[0]).unwrap().children[0];
    match &tree.nodes.get(row0_new_txt).unwrap().kind {
        WidgetKind::Text { content, .. } => assert_eq!(content.get(), "0: Ship"),
        _ => unreachable!(),
    }

    // Row 1 now has Build! at index 1
    let row1_new_txt = tree.nodes.get(swapped_rows[1]).unwrap().children[0];
    match &tree.nodes.get(row1_new_txt).unwrap().kind {
        WidgetKind::Text { content, .. } => assert_eq!(content.get(), "1: Build!"),
        _ => unreachable!(),
    }
}

#[test]
fn runtime_supports_phase4_spinner_badge_split_pane_and_toast_host() {
    reset_runtime();
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);

    let src = r#"export component Phase4Widgets {
    in-out property <float> split_ratio: 0.35;
    in-out property <int> dismissed_id: 0;
    in-out property <[ToastItem]> notifications: [
        ToastItem {
            id: 9,
            title: "Build",
            message: "Artifact ready",
            severity: ToastSeverity.Success,
            duration_ms: 0,
        }
    ];

    Stack {
        width: 100%;
        height: 100%;

        SplitPane {
            id: "main_split";
            direction: Horizontal;
            ratio <=> split_ratio;
            min_first: 80dp;
            min_second: 80dp;
            divider_size: 10dp;

            Column {
                id: "left_pane";
                padding: 8dp;
                gap: 8dp;
                Spinner {
                    id: "load_spin";
                    size: 26dp;
                    thickness: 3dp;
                    color: Theme.primary;
                }
                Row {
                    Badge {
                        id: "status_badge";
                        label: "Ready";
                        icon: "check";
                        background: #10B981;
                        color: #FFFFFF;
                    }
                }
            }

            Column {
                id: "right_pane";
                Text { text: "Preview"; }
            }
        }

        ToastHost {
            id: "toasts";
            items: notifications;
            on_dismiss => {
                dismissed_id = id;
            }
        }
    }
}"#;
    session
        .reload(src, Some("Phase4Widgets"), &mut tree)
        .unwrap();
    let metrics = WindowMetrics {
        logical_size: Size::new(500.0, 320.0),
        ..Default::default()
    };
    tree.layout(metrics);

    let badge_id = tree.node_by_id("status_badge").unwrap();
    let badge_node = tree.nodes.get(badge_id).unwrap();
    assert!(badge_node.box_style.background.is_none());
    match &badge_node.kind {
        WidgetKind::Badge {
            label,
            foreground,
            background,
            icon,
        } => {
            assert_eq!(label.get(), "Ready");
            assert!(foreground.is_some());
            assert!(background.is_some());
            assert_eq!(icon.as_ref().unwrap().get(), "check");
        }
        _ => panic!("expected Badge widget"),
    }

    let spin_id = tree.node_by_id("load_spin").unwrap();
    match &tree.nodes.get(spin_id).unwrap().kind {
        WidgetKind::Spinner {
            size, thickness, ..
        } => {
            assert!((size - 26.0).abs() < 0.01);
            assert!((thickness - 3.0).abs() < 0.01);
        }
        _ => panic!("expected Spinner widget"),
    }

    let left_r = tree.rect(tree.node_by_id("left_pane").unwrap());
    let right_r = tree.rect(tree.node_by_id("right_pane").unwrap());
    assert!(left_r.width < right_r.width);

    // Click the close button of the toast card in ToastHost (500x320 -> close center (465, 259)) -> fires `on_dismiss` setting `dismissed_id = 9`
    let close_pt = Point::new(465.0, 259.0);
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Down,
        position: close_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 0,
    }));
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Up,
        position: close_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 10,
    }));
    assert_eq!(
        session.store.props.get("dismissed_id").unwrap().get(),
        DynValue::Int(9)
    );
}

#[test]
fn runtime_supports_phase5_safe_area_segmented_accordion_and_datatable() {
    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);
    let src = r#"export component Phase5Mobile {
    in-out property <int> segment: 0;
    in-out property <bool> open_acc: false;
    in-out property <int> sel_row: 0;
    in-out property <int> sorted_col: -1;
    in property <[[string]]> rows_data: [["Camera", "Granted"], ["Mic", "Denied"]];

    SafeArea {
        id: "safe_root";
        padding: 10;
        Column {
            id: "safe_content";
            gap: 8;
            SegmentedControl {
                id: "seg";
                items: ["All", "Granted", "Denied"];
                selected <=> segment;
                on_change => { segment = idx; }
            }
            Accordion {
                id: "acc";
                title: "Android Manifest Details";
                expanded <=> open_acc;
                on_toggle => { open_acc = expanded; }
                Text { id: "acc_body"; text: "Uses android.permission.CAMERA"; }
            }
            DataTable {
                id: "tbl";
                columns: ["Permission", "State"];
                rows: rows_data;
                selected <=> sel_row;
                striped: false;
                row_height: 36;
                on_select => { sel_row = idx; }
                on_sort => { sorted_col = col; }
            }
        }
    }
}"#;
    session
        .reload(src, Some("Phase5Mobile"), &mut tree)
        .unwrap();
    let m1 = WindowMetrics {
        logical_size: Size::new(400.0, 800.0),
        safe_area: Insets::new(48.0, 0.0, 34.0, 0.0),
        ..Default::default()
    };
    tree.layout(m1);
    let content_r1 = tree.rect(tree.node_by_id("safe_content").unwrap());
    assert!((content_r1.y - 58.0).abs() < 0.5, "got {}", content_r1.y);

    // Dynamic safe area + keyboard inset update moves SafeArea content smoothly
    let m2 = WindowMetrics {
        logical_size: Size::new(400.0, 800.0),
        safe_area: Insets::new(20.0, 12.0, 24.0, 12.0),
        keyboard_inset: 280.0,
        ..Default::default()
    };
    tree.layout(m2);
    let content_r2 = tree.rect(tree.node_by_id("safe_content").unwrap());
    assert!((content_r2.y - 30.0).abs() < 0.5, "got {}", content_r2.y);
    assert!((content_r2.x - 22.0).abs() < 0.5, "got {}", content_r2.x);

    let seg_node = tree.nodes.get(tree.node_by_id("seg").unwrap()).unwrap();
    assert!(matches!(seg_node.kind, WidgetKind::SegmentedControl { .. }));

    let acc_id = tree.node_by_id("acc").unwrap();
    let acc_rect_closed = tree.rect(acc_id);
    let acc_hdr_pt = Point::new(acc_rect_closed.x + 24.0, acc_rect_closed.y + 16.0);
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Down,
        position: acc_hdr_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 0,
    }));
    tree.handle_event(&UiEvent::Pointer(PointerEvent {
        id: PointerId(0),
        phase: PointerPhase::Up,
        position: acc_hdr_pt,
        button: Some(PointerButton::Primary),
        modifiers: Modifiers::default(),
        is_touch: false,
        timestamp_ms: 10,
    }));
    assert_eq!(
        session.store.props.get("open_acc").unwrap().get(),
        DynValue::Bool(true)
    );
    tree.layout(m2);
    assert!(tree.rect(acc_id).height > acc_rect_closed.height);

    let tbl_id = tree.node_by_id("tbl").unwrap();
    match &tree.nodes.get(tbl_id).unwrap().kind {
        WidgetKind::DataTable {
            columns,
            rows,
            striped,
            row_height,
            ..
        } => {
            assert_eq!(columns.get(), vec!["Permission", "State"]);
            assert_eq!(rows.get().len(), 2);
            assert!(!striped);
            assert!((row_height - 36.0).abs() < 0.01);
        }
        _ => panic!("expected DataTable widget"),
    }
}

#[test]
fn hybrid_html_and_tailwind_elements_in_cui() {
    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);
    let src = r#"export component HybridApp {
    in-out property <string> username: "alice";
    div {
        id: "main_card";
        class: "flex flex-col p-4 gap-4 bg-slate-900 rounded-xl shadow-md w-full";
        h1 {
            id: "title";
            text: "Welcome to Hybrid CraftUI";
            class: "text-2xl font-bold text-white";
        }
        p {
            id: "desc";
            text: "Fast like Rust, easy like HTML";
            class: "text-sm text-slate-400";
        }
        input {
            id: "user_input";
            value <=> username;
            placeholder: "Enter username";
        }
        HtmlView {
            id: "embedded_web";
            src: "https://tauri.app";
        }
    }
}
"#;
    session.reload(src, Some("HybridApp"), &mut tree).unwrap();
    let metrics = WindowMetrics {
        logical_size: Size::new(400.0, 800.0),
        ..Default::default()
    };
    tree.layout(metrics);

    // Verify div element has Tailwind padding, background, radius, shadow
    let card_id = tree.node_by_id("main_card").unwrap();
    let card_node = tree.nodes.get(card_id).unwrap();
    assert_eq!(card_node.layout.padding, Insets::all(16.0));
    assert_eq!(card_node.layout.gap_x, 16.0);
    assert_eq!(card_node.layout.gap_y, 16.0);
    assert_eq!(card_node.box_style.radius, CornerRadii::all(16.0));
    assert!(card_node.box_style.background.is_some());
    assert!(card_node.box_style.shadow.is_some());

    // Verify h1 and p text widgets exist
    let title_id = tree.node_by_id("title").unwrap();
    let title_node = tree.nodes.get(title_id).unwrap();
    assert!(matches!(title_node.kind, WidgetKind::Text { .. }));

    // Verify input widget exists and is bound to username
    let input_id = tree.node_by_id("user_input").unwrap();
    let input_node = tree.nodes.get(input_id).unwrap();
    assert!(matches!(input_node.kind, WidgetKind::TextInput { .. }));

    // Verify HtmlView widget exists with src
    let web_id = tree.node_by_id("embedded_web").unwrap();
    let web_node = tree.nodes.get(web_id).unwrap();
    match &web_node.kind {
        WidgetKind::HtmlView { src, .. } => {
            assert_eq!(src.get(), "https://tauri.app");
        }
        _ => panic!("expected HtmlView widget"),
    }
}

#[test]
fn test_runtime_math_and_gradient_interpretation() {
    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);
    let src = r#"export component RuntimeMathAndGradients {
    in property <float> angle: 90.0;
    in property <float> half_pi: Math.PI / 2.0;
    in property <float> calc_sin: Math.sin(0.0);
    in property <float> calc_sqrt: Math.sqrt(25.0);

    Container {
        id: "grad_box";
        width: 300dp;
        height: 200dp;
        background: @linear-gradient(90deg, #6750A4, #D0BCFF);
        border_radius: 12dp;

        Container {
            id: "radial_box";
            width: 100dp;
            height: 100dp;
            background: @radial-gradient(50.0, #FFD8E4, #6750A4);
        }
    }
}"#;
    session
        .reload(src, Some("RuntimeMathAndGradients"), &mut tree)
        .unwrap();

    // Check props in dynamic store
    let half_pi = session.store.props.get("half_pi").unwrap().get().as_f32();
    assert!((half_pi - std::f32::consts::FRAC_PI_2).abs() < 1e-4);

    let calc_sin = session.store.props.get("calc_sin").unwrap().get().as_f32();
    assert!(calc_sin.abs() < 1e-4);

    let calc_sqrt = session.store.props.get("calc_sqrt").unwrap().get().as_f32();
    assert!((calc_sqrt - 5.0).abs() < 1e-4);

    // Check grad_box has Brush::LinearGradient
    let grad_id = tree.node_by_id("grad_box").unwrap();
    let grad_node = tree.nodes.get(grad_id).unwrap();
    let bg = grad_node.box_style.background.as_ref().unwrap().get();
    match bg {
        Brush::LinearGradient { start, end, stops } => {
            assert_eq!(stops.len(), 2);
            assert!((start.x - 0.0).abs() < 0.01);
            assert!((end.x - 1.0).abs() < 0.01);
        }
        _ => panic!("expected LinearGradient, got {:?}", bg),
    }

    // Check radial_box has Brush::RadialGradient
    let rad_id = tree.node_by_id("radial_box").unwrap();
    let rad_node = tree.nodes.get(rad_id).unwrap();
    let bg_rad = rad_node.box_style.background.as_ref().unwrap().get();
    match bg_rad {
        Brush::RadialGradient { radius, stops, .. } => {
            assert_eq!(stops.len(), 2);
            assert!((radius - 50.0).abs() < 0.01);
        }
        _ => panic!("expected RadialGradient, got {:?}", bg_rad),
    }
}

#[test]
fn test_runtime_std_material_suite_with_children() {
    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);
    let src = r#"
import { Card, ElevatedCard, SearchBar, Chip, FloatingActionButton } from "std/material.cui"

export component RuntimeMaterialDemo {
    in-out property <string> search_term: "Initial Query";
    in-out property <bool> is_filtered: true;

    ElevatedCard {
        id: "main_card";
        width: 360dp;
        padding: 16dp;

        SearchBar {
            id: "search_bar";
            text <=> search_term;
        }

        Chip {
            id: "chip_filter";
            label: "Material 3 Tag";
            selected <=> is_filtered;
        }

        FloatingActionButton {
            id: "fab_btn";
            icon: "add";
            label: "Create New";
        }
    }
}"#;
    session
        .reload(src, Some("RuntimeMaterialDemo"), &mut tree)
        .unwrap();

    let card_id = tree.node_by_id("main_card").unwrap();
    let card_node = tree.nodes.get(card_id).unwrap();
    assert!(card_node.box_style.background.is_some());
    assert!(card_node.box_style.shadow.is_some() || !card_node.box_style.shadows.is_empty());
    // Verify children were mounted into the subcomponent root!
    assert!(
        !card_node.children.is_empty(),
        "Card must have child elements"
    );

    let search_id = tree.node_by_id("search_bar").unwrap();
    assert!(tree.nodes.contains_key(search_id));

    let chip_id = tree.node_by_id("chip_filter").unwrap();
    assert!(tree.nodes.contains_key(chip_id));

    let fab_id = tree.node_by_id("fab_btn").unwrap();
    assert!(tree.nodes.contains_key(fab_id));
}

#[test]
fn test_runtime_window_responsive_breakpoints() {
    use craftui_core::Size;
    let root_scope = Scope::root();
    let mut session = HotReloadSession::new(root_scope);
    let mut tree = WidgetTree::new(session.state_scope);

    let src = r#"
            component ResponsiveApp {
                in-out property <bool> mobile: Window.is_compact;
                in-out property <bool> desktop: Window.is_expanded;
                Column {
                    Text { text: root.mobile ? "Mobile Layout" : "Desktop Layout"; }
                }
            }
        "#;

    // 1. Initial layout at mobile width (400x800)
    tree.set_metrics(WindowMetrics {
        logical_size: Size::new(400.0, 800.0),
        ..Default::default()
    });
    let root = session
        .reload(src, Some("ResponsiveApp"), &mut tree)
        .unwrap();
    tree.set_root(root);
    tree.layout(tree.metrics);

    let mobile_sig = session.store.props.get("mobile").unwrap();
    let desktop_sig = session.store.props.get("desktop").unwrap();
    assert_eq!(mobile_sig.get(), DynValue::Bool(true));
    assert_eq!(desktop_sig.get(), DynValue::Bool(false));

    // 2. Resize to desktop (1280x800)
    tree.layout(WindowMetrics {
        logical_size: Size::new(1280.0, 800.0),
        scale_factor: 1.0,
        safe_area: Insets::ZERO,
        keyboard_inset: 0.0,
    });

    assert_eq!(mobile_sig.get(), DynValue::Bool(false));
    assert_eq!(desktop_sig.get(), DynValue::Bool(true));
}

#[test]
fn hot_reload_supports_children_slot_and_pickers_tree_view() {
    reset_runtime();
    let state_scope = Scope::root();
    let mut tree = WidgetTree::new(state_scope);
    let mut session = HotReloadSession::new(state_scope);

    let src = r#"
        component CardWrapper {
            in property <string> heading: "Section";
            Column {
                Text { text: root.heading; }
                Row {
                    @children;
                }
            }
        }

        component App {
            in-out property <string> date_val: "2026-11-25";
            in-out property <string> time_val: "14:15";
            in-out property <int> tree_idx: 1;

            CardWrapper {
                heading: "Planner";
                DatePicker { value <=> date_val; }
                TimePicker { value <=> time_val; }
                TreeView {
                    items: ["src", "src/lib.rs"];
                    selected <=> tree_idx;
                }
                ReorderableList {
                    items: ["First", "Second"];
                    selected <=> tree_idx;
                }
                MenuBar {
                    menus: ["File:New,Save"];
                }
                Draggable {
                    payload: "note:1";
                    Text { text: "Card"; }
                }
                DropTarget {
                    accept_prefix: "note:";
                    Text { text: "Inbox"; }
                }
            }
        }
    "#;

    let root = session.reload(src, Some("App"), &mut tree).unwrap();
    tree.set_root(root);
    tree.layout(WindowMetrics::default());

    let root_node = tree.nodes.get(root).unwrap();
    assert_eq!(root_node.children.len(), 2);
    let row_slot = tree.nodes.get(root_node.children[1]).unwrap();
    assert_eq!(row_slot.children.len(), 7);
}

#[test]
fn interpreter_matches_every_builtin_widget_and_material_variant() {
    reset_runtime();
    let state_scope = Scope::root();
    let mut tree = WidgetTree::new(state_scope);
    let mut session = HotReloadSession::new(state_scope);

    for widget in craftui_dsl::BUILTIN_WIDGETS {
        let src = format!("component Host {{ Column {{ {widget} {{ }} }} }}");
        let root = session
            .reload(&src, Some("Host"), &mut tree)
            .unwrap_or_else(|e| panic!("interpreter failed on `{widget}`: {e}"));
        tree.set_root(root);
        tree.layout(WindowMetrics::default());
        let col = tree.nodes.get(root).unwrap();
        assert_eq!(col.children.len(), 1, "missing child for `{widget}`");
    }

    // Verify specific Material & alias widget kinds match AOT codegen
    let src = r#"
        component FullSuite {
            in-out property <bool> drawer_open: false;
            in-out property <bool> chip_sel: true;
            Column {
                AppBar { title: "Title"; }
                ElevatedCard { ListTile { title: "Row"; } }
                FilledButton { text: "Go"; }
                FilterChip { label: "Rust"; selected <=> chip_sel; }
                VerticalDivider { }
                LinearProgressIndicator { progress: 0.5; }
                CircularProgressIndicator { size: 20.0; }
                Drawer { open <=> drawer_open; }
            }
        }
    "#;
    let root = session.reload(src, Some("FullSuite"), &mut tree).unwrap();
    let col = tree.nodes.get(root).unwrap();
    assert_eq!(col.children.len(), 8);
    assert!(matches!(
        tree.nodes.get(col.children[0]).unwrap().kind,
        craftui_widgets::WidgetKind::AppBar { .. }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[1]).unwrap().kind,
        craftui_widgets::WidgetKind::Card {
            variant: craftui_widgets::CardVariant::Elevated
        }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[3]).unwrap().kind,
        craftui_widgets::WidgetKind::Chip { .. }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[4]).unwrap().kind,
        craftui_widgets::WidgetKind::Divider {
            axis: craftui_core::Axis::Vertical,
            ..
        }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[5]).unwrap().kind,
        craftui_widgets::WidgetKind::ProgressIndicator { .. }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[6]).unwrap().kind,
        craftui_widgets::WidgetKind::Spinner { .. }
    ));
    assert!(matches!(
        tree.nodes.get(col.children[7]).unwrap().kind,
        craftui_widgets::WidgetKind::Drawer { .. }
    ));
}
