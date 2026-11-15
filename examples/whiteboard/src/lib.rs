pub mod canvas;
pub mod export;
pub mod model;
pub mod sketch;
pub mod web_view;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/whiteboard.rs"));
}

use std::sync::Arc;

use craftui::prelude::*;

use crate::canvas::{WhiteboardController, attach_whiteboard_canvas};
use crate::export::{elements_from_json, elements_to_json, elements_to_svg};
use crate::model::Tool;

/// Mount the complete responsive Whiteboard application built 100% in CraftUI (.cui declarative DSL + reactive controller).
pub fn mount_whiteboard_app(
    cx: Scope,
    tree: &mut WidgetTree,
    services: Option<Arc<PlatformServices>>,
) -> NodeId {
    // Set crisp Excalidraw light theme by default
    tree.set_theme(ThemeTokens::light());
    cx.provide_context(tree.theme_signal);

    let ctrl = WhiteboardController::new(cx);

    // Instantiate declarative Whiteboard screen compiled from ui/whiteboard.cui
    let screen = aot_ui::WhiteboardScreen::new(cx);

    // Initial state setup
    screen.active_tool.set("Select".to_string());
    screen.zoom_level.set("100%".to_string());
    screen
        .status_text
        .set(format!("{} shapes", ctrl.elements.get().len()));
    screen.mode_name.set("Native CUI".to_string());
    screen.is_hybrid.set(false);
    screen.drawer_open.set(false);
    screen.can_undo.set(ctrl.can_undo());
    screen.can_redo.set(ctrl.can_redo());
    screen.is_dark.set(false);

    // 1. Tool Selection
    let ctrl_tool = ctrl.clone();
    let at_sig = screen.active_tool;
    screen.set_on_select_tool(move |t_name| {
        let tool = match t_name.as_str() {
            "Hand" => Tool::Hand,
            "Select" => Tool::Select,
            "Rectangle" => Tool::Rectangle,
            "Diamond" => Tool::Diamond,
            "Ellipse" => Tool::Ellipse,
            "Arrow" => Tool::Arrow,
            "Line" => Tool::Line,
            "Pen" => Tool::Pen,
            "Text" => Tool::Text,
            "Eraser" => Tool::Eraser,
            _ => Tool::Select,
        };
        ctrl_tool.select_tool(tool);
        at_sig.set(t_name);
    });

    // 2. Color Swatch Picker
    let ctrl_col = ctrl.clone();
    screen.set_on_pick_color(move |col_name| {
        let col = match col_name.as_str() {
            "red" => Color::rgb(224, 49, 49),
            "green" => Color::rgb(47, 158, 68),
            "blue" => Color::rgb(25, 113, 194),
            _ => Color::rgb(30, 30, 30),
        };
        ctrl_col.stroke_color.set(col);
    });

    // 3. Zoom Controls
    let ctrl_zin = ctrl.clone();
    let zl_sig = screen.zoom_level;
    screen.set_on_zoom_in(move || {
        ctrl_zin.zoom_in(Point::new(600.0, 400.0));
        zl_sig.set(format!("{:.0}%", ctrl_zin.view.get().zoom * 100.0));
    });

    let ctrl_zout = ctrl.clone();
    let zl_sig2 = screen.zoom_level;
    screen.set_on_zoom_out(move || {
        ctrl_zout.zoom_out(Point::new(600.0, 400.0));
        zl_sig2.set(format!("{:.0}%", ctrl_zout.view.get().zoom * 100.0));
    });

    let ctrl_zres = ctrl.clone();
    let zl_sig3 = screen.zoom_level;
    screen.set_on_zoom_reset(move || {
        ctrl_zres.zoom_reset();
        zl_sig3.set("100%".to_string());
    });

    // 4. Undo / Redo
    let ctrl_u = ctrl.clone();
    let u_sig = screen.can_undo;
    let r_sig = screen.can_redo;
    screen.set_on_undo(move || {
        ctrl_u.undo();
        u_sig.set(ctrl_u.can_undo());
        r_sig.set(ctrl_u.can_redo());
    });

    let ctrl_r = ctrl.clone();
    let u_sig2 = screen.can_undo;
    let r_sig2 = screen.can_redo;
    screen.set_on_redo(move || {
        ctrl_r.redo();
        u_sig2.set(ctrl_r.can_undo());
        r_sig2.set(ctrl_r.can_redo());
    });

    // 5. Mode Toggle (Native CUI <-> Hybrid WebView)
    let hybrid_sig = screen.is_hybrid;
    let mode_sig = screen.mode_name;
    screen.set_on_toggle_mode(move || {
        let is_h = !hybrid_sig.get();
        hybrid_sig.set(is_h);
        if is_h {
            mode_sig.set("Hybrid WebView".to_string());
        } else {
            mode_sig.set("Native CUI".to_string());
        }
    });

    // 6. Theme Toggle (Light <-> Dark)
    let dark_sig = screen.is_dark;
    let theme_sig = tree.theme_signal;
    let ctrl_theme = ctrl.clone();
    screen.set_on_toggle_theme(move || {
        let is_d = !dark_sig.get();
        dark_sig.set(is_d);
        ctrl_theme.is_dark.set(is_d);
        if is_d {
            theme_sig.set(ThemeTokens::dark());
        } else {
            theme_sig.set(ThemeTokens::light());
        }
    });

    // 7. Share Action (Exports SVG & copies to clipboard / shares natively)
    let svc_share = services.clone();
    let ctrl_share = ctrl.clone();
    screen.set_on_share(move || {
        let svg = elements_to_svg(
            &ctrl_share.elements.get(),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
        );
        if let Some(svc) = &svc_share {
            svc.write_clipboard(&svg);
            svc.share(&SharePayload {
                title: Some("CraftUI Whiteboard SVG".into()),
                text: Some(svg),
                ..Default::default()
            });
        }
    });

    // 8. New Canvas
    let ctrl_new = ctrl.clone();
    let d_sig = screen.drawer_open;
    let st_sig = screen.status_text;
    screen.set_on_new_canvas(move || {
        ctrl_new.clear();
        st_sig.set("0 shapes".to_string());
        d_sig.set(false);
    });

    // 9. Export SVG
    let ctrl_svg = ctrl.clone();
    let svc_svg = services.clone();
    let d_sig2 = screen.drawer_open;
    screen.set_on_export_svg(move || {
        let svg = elements_to_svg(&ctrl_svg.elements.get(), Rect::new(0.0, 0.0, 1200.0, 800.0));
        if let Some(svc) = &svc_svg {
            svc.write_clipboard(&svg);
        }
        d_sig2.set(false);
    });

    // 10. Export JSON
    let ctrl_json = ctrl.clone();
    let svc_json = services.clone();
    let d_sig3 = screen.drawer_open;
    screen.set_on_export_json(move || {
        if let Ok(json_str) = elements_to_json(&ctrl_json.elements.get())
            && let Some(svc) = &svc_json
        {
            svc.write_clipboard(&json_str);
        }
        d_sig3.set(false);
    });

    // 11. Import JSON from Clipboard
    let ctrl_imp = ctrl.clone();
    let svc_imp = services.clone();
    let d_sig4 = screen.drawer_open;
    let st_sig2 = screen.status_text;
    screen.set_on_import_json(move || {
        if let Some(svc) = &svc_imp
            && let Some(text) = svc.read_clipboard()
            && let Ok(elems) = elements_from_json(&text)
        {
            st_sig2.set(format!("{} shapes", elems.len()));
            ctrl_imp.elements.set(elems);
        }
        d_sig4.set(false);
    });

    // 12. Security & Platform Diagnostics
    let svc_admin = services.clone();
    screen.set_on_open_admin(move || {
        if let Some(svc) = &svc_admin {
            svc.open_admin_settings();
        }
    });

    let svc_a11y = services.clone();
    screen.set_on_open_accessibility(move || {
        if let Some(svc) = &svc_a11y {
            svc.open_accessibility_settings();
        }
    });

    let svc_sett = services.clone();
    screen.set_on_open_settings(move || {
        if let Some(svc) = &svc_sett {
            svc.open_app_settings();
        }
    });

    // 13. Drawer Toggle
    let d_sig5 = screen.drawer_open;
    screen.set_on_toggle_drawer(move || {
        d_sig5.set(!d_sig5.get());
    });

    // 14. Tool Lock Toggle
    let ctrl_lock = ctrl.clone();
    let lock_sig = screen.tool_locked;
    screen.set_on_toggle_lock(move || {
        let next = !lock_sig.get();
        lock_sig.set(next);
        ctrl_lock.tool_locked.set(next);
    });

    // 15. Reactive sync of active tool back to UI
    let at_sig2 = screen.active_tool;
    let ctrl_t_sig = ctrl.tool;
    cx.effect(move || {
        let t = ctrl_t_sig.get();
        let name = match t {
            Tool::Hand => "Hand",
            Tool::Select => "Select",
            Tool::Rectangle => "Rectangle",
            Tool::Diamond => "Diamond",
            Tool::Ellipse => "Ellipse",
            Tool::Arrow => "Arrow",
            Tool::Line => "Line",
            Tool::Pen => "Pen",
            Tool::Text => "Text",
            Tool::Eraser => "Eraser",
        };
        at_sig2.set(name.to_string());
    });

    // Build the complete widget tree from the compiled .cui component!
    let root = screen.build(cx, tree);
    tree.set_root(root);

    // Attach high-performance canvas painting & pointer interactions to the declarative Canvas node
    if let Some(canvas_node) = tree.node_by_id("whiteboard_canvas") {
        attach_whiteboard_canvas(tree, canvas_node, ctrl);
    }

    root
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    craftui_platform_web::start_web_app(
        craftui_platform_web::WebBridgeConfig::default(),
        |cx, tree, _textures, _dispatcher| mount_whiteboard_app(cx, tree, None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ElementKind, WhiteboardElement};

    #[test]
    fn test_whiteboard_element_creation_and_bounds() {
        let elem = WhiteboardElement::new(
            1,
            Point::new(100.0, 100.0),
            ElementKind::Rectangle {
                width: 200.0,
                height: 150.0,
                roundness: 0.0,
            },
            Color::rgb(0, 0, 0),
            None,
            2.0,
        );

        let bounds = elem.bounds();
        assert!(bounds.x <= 100.0);
        assert!(bounds.y <= 100.0);
        assert!(bounds.width >= 200.0);
        assert!(bounds.height >= 150.0);
        assert!(elem.contains_point(Point::new(150.0, 150.0), 4.0));
    }

    #[test]
    fn test_whiteboard_arrow_sketch_path() {
        let arrow = WhiteboardElement::new(
            2,
            Point::new(50.0, 50.0),
            ElementKind::Arrow {
                end: Point::new(250.0, 50.0),
            },
            Color::rgb(0, 0, 0),
            None,
            3.0,
        );

        let path = crate::sketch::sketch_arrow_path(
            arrow.origin,
            Point::new(250.0, 50.0),
            arrow.roughness,
            arrow.id,
        );
        assert!(!path.is_empty());
        assert!(arrow.contains_point(Point::new(150.0, 50.0), 6.0));
    }

    #[test]
    fn test_view_transform_screen_world_projection() {
        let mut view = crate::model::ViewTransform {
            pan: Point::new(100.0, 50.0),
            zoom: 2.0,
        };

        let screen_pt = Point::new(200.0, 150.0);
        let world_pt = view.screen_to_world(screen_pt);
        assert_eq!(world_pt, Point::new(50.0, 50.0));

        let back_to_screen = view.world_to_screen(world_pt);
        assert_eq!(back_to_screen, screen_pt);

        view.zoom_at(1.5, screen_pt);
        assert_eq!(view.zoom, 3.0);
    }

    #[test]
    fn test_history_manager_undo_redo() {
        let mut hist = crate::model::HistoryManager::default();
        let s0 = vec![];
        let s1 = vec![WhiteboardElement::new(
            1,
            Point::new(0.0, 0.0),
            ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                roundness: 0.0,
            },
            Color::rgb(0, 0, 0),
            None,
            1.0,
        )];

        hist.push(s0.clone());
        let undone = hist.undo(s1.clone()).unwrap();
        assert_eq!(undone.len(), 0);

        let redone = hist.redo(s0).unwrap();
        assert_eq!(redone.len(), 1);
    }

    #[test]
    fn test_export_svg_and_json() {
        let elements = vec![
            WhiteboardElement::new(
                10,
                Point::new(10.0, 20.0),
                ElementKind::Rectangle {
                    width: 100.0,
                    height: 50.0,
                    roundness: 0.0,
                },
                Color::rgb(30, 30, 30),
                None,
                2.0,
            ),
            WhiteboardElement::new(
                11,
                Point::new(150.0, 150.0),
                ElementKind::Arrow {
                    end: Point::new(300.0, 150.0),
                },
                Color::rgb(220, 0, 0),
                None,
                3.0,
            ),
        ];

        let json_str = elements_to_json(&elements).expect("json serialization failed");
        assert!(json_str.contains("Rectangle"));
        assert!(json_str.contains("Arrow"));

        let imported = elements_from_json(&json_str).expect("json deserialization failed");
        assert_eq!(imported.len(), 2);

        let svg_str = elements_to_svg(&elements, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(svg_str.contains("<svg"));
        assert!(svg_str.contains("<path"));
        assert!(svg_str.contains("</svg>"));
    }

    #[test]
    fn test_mount_whiteboard_app_hierarchy() {
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let services = Arc::new(PlatformServices::in_memory());

        let root = mount_whiteboard_app(cx, &mut tree, Some(services));
        assert!(tree.nodes.contains_key(root));
        // Verify canvas was mounted and attached
        let canvas_node = tree
            .node_by_id("whiteboard_canvas")
            .expect("canvas must exist");

        // 1. Desktop viewport (1200x800, expanded)
        let desktop_metrics = WindowMetrics {
            logical_size: Size::new(1200.0, 800.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(desktop_metrics);
        let canvas_rect = tree.layout_engine.rect(canvas_node);
        assert_eq!(canvas_rect.width, 1200.0);
        assert_eq!(canvas_rect.height, 800.0);

        let root_node = tree.nodes.get(root).unwrap();
        let top_nav = root_node.children[1];
        let top_nav_rect = tree.layout_engine.rect(top_nav);
        assert!(top_nav_rect.width <= 1200.0);

        // 2. Tablet viewport (768x1024, medium)
        let tablet_metrics = WindowMetrics {
            logical_size: Size::new(768.0, 1024.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(tablet_metrics);
        let canvas_rect_tab = tree.layout_engine.rect(canvas_node);
        assert_eq!(canvas_rect_tab.width, 768.0);
        assert_eq!(canvas_rect_tab.height, 1024.0);

        // 3. Mobile viewport (390x844, compact phone)
        let mobile_metrics = WindowMetrics {
            logical_size: Size::new(390.0, 844.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(mobile_metrics);
        let canvas_rect_mob = tree.layout_engine.rect(canvas_node);
        assert_eq!(canvas_rect_mob.width, 390.0);
        assert_eq!(canvas_rect_mob.height, 844.0);

        let top_nav_rect_mob = tree.layout_engine.rect(top_nav);
        assert!(
            top_nav_rect_mob.width <= 390.0,
            "Top nav must fit within mobile screen width"
        );
    }
}

#[cfg(target_os = "android")]
pub fn android_log(msg: &str) {
    unsafe extern "C" {
        fn __android_log_write(
            prio: std::ffi::c_int,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> std::ffi::c_int;
    }
    use std::ffi::CString;
    if let (Ok(tag), Ok(text)) = (CString::new("CraftUI_Native"), CString::new(msg)) {
        unsafe {
            __android_log_write(4, tag.as_ptr(), text.as_ptr());
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: craftui::platform_android::android_activity::AndroidApp) {
    std::panic::set_hook(Box::new(|info| {
        android_log(&format!("CRITICAL CRAFTUI PANIC: {info}"));
    }));
    android_log(">>> STARTING WHITEBOARD ANDROID_MAIN <<<");
    craftui::platform_android::android_runtime::run_android_app(
        app,
        |cx, tree, _textures, _disp| {
            android_log(">>> MOUNTING WHITEBOARD APP <<<");
            let root = mount_whiteboard_app(cx, tree, None);
            android_log(">>> MOUNTED WHITEBOARD APP ROOT OK <<<");
            root
        },
    );
    android_log(">>> EXITED WHITEBOARD ANDROID_MAIN <<<");
}
