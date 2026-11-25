use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use craftui_core::{
    Alignment, Axis, BlendMode, Border, BoxShadow, Brush, Color, CornerRadii, CursorIcon,
    DirtyFlags, Fit, Insets, Justify, Length, NodeId, PlatformViewId, Point, ThemeOverride,
    ThemeTokens, Transform2D, WindowMetrics,
};
use craftui_dsl::{
    AnimateSpec, AssignOp, BinOp, ChildItem, ComponentDecl, CuiFile, ElementNode, Expr,
    PropBinding, Stmt, Unit,
};
use craftui_layout::LayoutStyle;
use craftui_reactive::{Scope, Signal};
use craftui_text::TextStyle;
use craftui_widgets::{
    AnimatableProp, Easing, IndexCallback, ReactiveValue, TapHandler, ToastItem, ToastSeverity,
    TransitionCurve, VirtualItemFactory, WidgetTree,
};

use crate::dyn_value::{DynCallback, DynValue};

#[derive(Clone)]
pub(crate) struct DynEnv {
    pub(crate) file: Rc<CuiFile>,
    pub(crate) props: HashMap<String, Signal<DynValue>>,
    pub(crate) signal_locals: HashMap<String, Signal<DynValue>>,
    pub(crate) callbacks: HashMap<String, DynCallback>,
    pub(crate) locals: HashMap<String, DynValue>,
    pub(crate) theme_sig: Signal<ThemeTokens>,
    pub(crate) theme_ov_sig: Signal<ThemeOverride>,
    pub(crate) metrics_sig: Signal<WindowMetrics>,
    pub(crate) children_slot: Option<Rc<dyn Fn(Scope, &mut WidgetTree, NodeId)>>,
}

impl DynEnv {
    fn find_prop_signal(&self, expr: &Expr) -> Option<Signal<DynValue>> {
        if let Expr::Ident(name) = expr {
            self.signal_locals
                .get(name)
                .copied()
                .or_else(|| self.props.get(name).copied())
        } else {
            None
        }
    }
}

pub(crate) fn expr_references_window(expr: &Expr) -> bool {
    match expr {
        Expr::Member(base, _) => {
            if let Expr::Ident(root) = base.as_ref() {
                root == "Window"
            } else {
                expr_references_window(base)
            }
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => expr_references_window(inner),
        Expr::Binary(l, _, r) => expr_references_window(l) || expr_references_window(r),
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_references_window(cond)
                || expr_references_window(then_expr)
                || expr_references_window(else_expr)
        }
        Expr::Call(_, args) => args.iter().any(expr_references_window),
        Expr::MethodCall(base, _, args) => {
            expr_references_window(base) || args.iter().any(expr_references_window)
        }
        _ => false,
    }
}

fn find_prop<'a>(el: &'a ElementNode, names: &[&str]) -> Option<&'a PropBinding> {
    el.props.iter().find(|p| names.iter().any(|n| p.name == *n))
}

pub(crate) fn build_element(
    scope: Scope,
    el: &ElementNode,
    env: &DynEnv,
    tree: &mut WidgetTree,
) -> NodeId {
    if let Some(sub_comp) = env
        .file
        .components
        .iter()
        .find(|c| c.name == el.kind)
        .cloned()
    {
        return instantiate_subcomponent(scope, &sub_comp, el, env, tree);
    }

    let eval_prop = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, env))
    };

    let node = match el.kind.as_str() {
        "Spacer" => tree.spacer(scope),
        "SafeArea" => tree.safe_area(scope),
        "Row" => tree.row(scope),
        "Grid" => {
            let cols = eval_prop("columns")
                .map(|v| v.as_i64().max(1) as u16)
                .unwrap_or(2);
            let gap = eval_prop("gap").map(|v| v.as_f32()).unwrap_or(0.0);
            tree.grid(scope, cols, gap)
        }
        "Stack" => tree.stack(scope),
        "Divider" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Vertical" => Axis::Vertical,
                _ => Axis::Horizontal,
            };
            let thickness = eval_prop("thickness").map(|v| v.as_f32()).unwrap_or(1.0);
            let n = tree.divider(scope, axis, thickness);
            if let Some(cp) = find_prop(el, &["color", "foreground"]) {
                let rv = expr_to_reactive_color(&cp.value, env, Color::TRANSPARENT);
                tree.set_divider_color(n, rv);
            }
            n
        }
        "Icon" => {
            let svg_rv = find_prop(el, &["svg", "icon", "name", "source"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static("info".to_string()));
            let size = find_prop(el, &["size", "font_size"])
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(20.0);
            let col_opt = find_prop(el, &["color", "foreground"]).map(|p| {
                expr_to_reactive_color(&p.value, env, env.theme_sig.get_untracked().text_primary)
            });
            tree.icon(scope, svg_rv, size, col_opt)
        }
        "Text" => {
            let text_rv = find_prop(el, &["text", "title", "content"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let size_prop = find_prop(el, &["font_size"]).map(|p| &p.value);
            let weight_prop = find_prop(el, &["font_weight"]).map(|p| &p.value);
            let color_expr = find_prop(el, &["color", "foreground"])
                .map(|p| p.value.clone())
                .unwrap_or_else(|| {
                    Expr::Member(Box::new(Expr::Ident("Theme".into())), "text_primary".into())
                });
            let style_rv = emit_reactive_text_style(size_prop, weight_prop, &color_expr, env);
            tree.text(scope, text_rv, style_rv)
        }
        "Button" => {
            let text_rv = find_prop(el, &["text", "title", "label"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| {
                    if el.children.is_empty() {
                        ReactiveValue::Static("Button".to_string())
                    } else {
                        ReactiveValue::Static(String::new())
                    }
                });
            let tap_handler = make_handler_opt(el, &["on_tap", "on_click"], env, None);
            let n = tree.button(scope, text_rv, move || {
                if let Some(ref h) = tap_handler
                    && let Ok(mut cb) = h.try_borrow_mut()
                {
                    cb();
                }
            });
            if let Some(fg) = find_prop(el, &["foreground", "color"]) {
                let rv = expr_to_reactive_color(
                    &fg.value,
                    env,
                    env.theme_sig.get_untracked().on_primary,
                );
                tree.set_button_foreground(n, rv);
            }
            n
        }
        "ImageView" | "Image" => {
            let tex_rv = match find_prop(el, &["source"]) {
                Some(p) => {
                    let expr = p.value.clone();
                    if expr_has_reactivity(&expr, env) {
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::Texture(t) => t,
                                _ => None,
                            }
                        }))
                    } else {
                        let t = match eval_expr(&expr, env) {
                            DynValue::Texture(t) => t,
                            _ => None,
                        };
                        ReactiveValue::Static(t)
                    }
                }
                None => ReactiveValue::Static(None),
            };
            let fit = match find_prop(el, &["fit"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Cover" => Fit::Cover,
                Some(Expr::Ident(s)) if s == "Fill" => Fit::Fill,
                Some(Expr::Ident(s)) if s == "None" => Fit::None,
                _ => Fit::Contain,
            };
            let rad = eval_prop("border_radius")
                .map(|v| CornerRadii::all(v.as_f32()))
                .unwrap_or(CornerRadii::ZERO);
            tree.image(scope, tex_rv, fit, rad)
        }
        "Canvas" => tree.canvas(scope, LayoutStyle::column(), |_, _| {}, None),
        "ScrollView" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let base_layout = match axis {
                Axis::Horizontal => LayoutStyle::row(),
                Axis::Vertical => LayoutStyle::column(),
            };
            tree.scroll_view(scope, axis, base_layout)
        }
        "VirtualList" | "VirtualGrid" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let cols = eval_prop("columns")
                .map(|v| v.as_i64().max(1) as usize)
                .unwrap_or(if el.kind == "VirtualGrid" { 2 } else { 1 });
            let extent = eval_prop("item_extent")
                .or_else(|| eval_prop("item_height"))
                .map(|v| v.as_f32())
                .unwrap_or(48.0);

            if let Some(ChildItem::For {
                item_var,
                index_var,
                iterable,
                body,
                ..
            }) = el.children.first()
            {
                let iter_expr = iterable.clone();
                let env_cnt = env.clone();
                let count_rv = ReactiveValue::Dynamic(Rc::new(move || {
                    eval_iterable(&iter_expr, &env_cnt, true).len()
                }));
                let iter_fac = iterable.clone();
                let env_fac = env.clone();
                let item_name = item_var.clone();
                let idx_name = index_var.clone();
                let body_el = (**body).clone();
                let factory: VirtualItemFactory = Rc::new(move |v_cx, v_idx, v_tree| {
                    let items = eval_iterable(&iter_fac, &env_fac, false);
                    let Some(item_val) = items.get(v_idx).cloned() else {
                        return v_tree.spacer(v_cx);
                    };
                    let mut child_env = env_fac.clone();
                    child_env.locals.insert(item_name.clone(), item_val);
                    if let Some(ref iv) = idx_name {
                        child_env
                            .locals
                            .insert(iv.clone(), DynValue::Int(v_idx as i64));
                    }
                    build_element(v_cx, &body_el, &child_env, v_tree)
                });
                let n = tree.virtual_list(
                    scope,
                    axis,
                    cols,
                    extent,
                    count_rv,
                    LayoutStyle::column(),
                    factory,
                );
                apply_common_props_and_events(scope, n, el, env, tree, false);
                return n;
            } else {
                let count_rv = match find_prop(el, &["item_count", "count", "model", "items"]) {
                    Some(p) => {
                        let expr = p.value.clone();
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::List(v) => v.len(),
                                other => other.as_i64().max(0) as usize,
                            }
                        }))
                    }
                    None => ReactiveValue::Static(0usize),
                };
                tree.virtual_list(
                    scope,
                    axis,
                    cols,
                    extent,
                    count_rv,
                    LayoutStyle::column(),
                    Rc::new(|cx, idx, t| t.text(cx, format!("Item {idx}"), TextStyle::default())),
                )
            }
        }
        "Slider" => {
            let min = eval_prop("min").map(|v| v.as_f32()).unwrap_or(0.0);
            let max = eval_prop("max").map(|v| v.as_f32()).unwrap_or(100.0);
            let val_prop = find_prop(el, &["value"]);
            let sig = bind_f32_signal(scope, val_prop, env, 0.0);
            let lbl = eval_prop("label").map(|v| v.as_str()).unwrap_or_default();
            let n = tree.slider(scope, sig, min, max, lbl);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_float_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::Float(value as f64));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "TextInput" | "TextArea" => {
            let is_multiline = el.kind == "TextArea"
                || matches!(
                    find_prop(el, &["multiline"]).map(|p| &p.value),
                    Some(Expr::Bool(true))
                );
            let placeholder = eval_prop("placeholder")
                .map(|v| v.as_str())
                .unwrap_or_default();
            let val_prop = find_prop(el, &["value", "text"]);
            let (init_s, sig_opt) = bind_string_signal(scope, val_prop, env);
            let n = if is_multiline {
                tree.text_area(scope, init_s, placeholder, sig_opt)
            } else {
                tree.text_input(scope, init_s, placeholder, sig_opt)
            };
            let sz_prop = find_prop(el, &["font_size"]);
            let wt_prop = find_prop(el, &["font_weight"]);
            if sz_prop.is_some() || wt_prop.is_some() {
                let sz_e = sz_prop.map(|p| p.value.clone());
                let wt_e = wt_prop.map(|p| p.value.clone());
                let reactive_st = sz_e.as_ref().is_some_and(|e| expr_has_reactivity(e, env))
                    || wt_e.as_ref().is_some_and(|e| expr_has_reactivity(e, env));
                if reactive_st {
                    let sz_tr = sz_e.clone();
                    let wt_tr = wt_e.clone();
                    let env_tr = env.clone();
                    let env_ap = env.clone();
                    tree.bind_dynamic_prop(
                        n,
                        DirtyFlags::LAYOUT | DirtyFlags::PAINT,
                        move || {
                            if let Some(ref e) = sz_tr {
                                let _ = eval_expr_tracked(e, &env_tr, true);
                            }
                            if let Some(ref e) = wt_tr {
                                let _ = eval_expr_tracked(e, &env_tr, true);
                            }
                        },
                        move |id, tr| {
                            let sz = sz_e
                                .as_ref()
                                .map(|e| eval_expr(e, &env_ap).as_f32())
                                .unwrap_or(14.0);
                            let wt = wt_e
                                .as_ref()
                                .map(|e| eval_font_weight(e, &env_ap))
                                .unwrap_or(400.0);
                            tr.set_text_input_style(
                                id,
                                TextStyle::default().with_size(sz).with_weight(wt),
                            );
                        },
                    );
                } else {
                    let sz = eval_prop("font_size").map(|v| v.as_f32()).unwrap_or(14.0);
                    let wt = find_prop(el, &["font_weight"])
                        .map(|p| eval_font_weight(&p.value, env))
                        .unwrap_or(400.0);
                    tree.set_text_input_style(
                        n,
                        TextStyle::default().with_size(sz).with_weight(wt),
                    );
                }
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_submit") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_submit(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Checkbox" | "Switch" => {
            let checked_prop = find_prop(el, &["checked", "value"]);
            let sig = bind_bool_signal(scope, checked_prop, env, false);
            let lbl = eval_prop("label")
                .or_else(|| eval_prop("text"))
                .map(|v| v.as_str())
                .unwrap_or_default();
            let n = if el.kind == "Checkbox" {
                tree.checkbox(scope, sig, lbl)
            } else {
                tree.switch(scope, sig, lbl)
            };
            if let Some(h) = el
                .handlers
                .iter()
                .find(|h| h.event == "on_change" || h.event == "on_tap")
            {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_bool_change(n, move |checked| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("checked".into(), DynValue::Bool(checked));
                    h_env.locals.insert("value".into(), DynValue::Bool(checked));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Tabs" | "TabBar" => {
            let tabs_rv = find_prop(el, &["tabs", "items", "options"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(vec!["Tab 1".to_string()]));
            let idx_prop = find_prop(el, &["active", "selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.tabs(scope, tabs_rv, idx_sig);
            wire_index_change(n, el, env, tree);
            n
        }
        "SegmentedControl" => {
            let items_rv = find_prop(el, &["items", "options", "tabs"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let idx_prop = find_prop(el, &["selected", "active", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.segmented_control(scope, items_rv, idx_sig);
            wire_index_change(n, el, env, tree);
            n
        }
        "BottomNavBar" => {
            let items_rv = find_prop(el, &["items", "options", "tabs"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let icons_rv = find_prop(el, &["icons"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let idx_prop = find_prop(el, &["selected", "active", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.bottom_nav_bar(scope, items_rv, icons_rv, idx_sig);
            wire_index_change(n, el, env, tree);
            n
        }
        "FloatingActionButton" => {
            let icon_rv = find_prop(el, &["icon"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let label_opt =
                find_prop(el, &["label", "text"]).map(|p| expr_to_reactive_string(&p.value, env));
            tree.floating_action_button(scope, icon_rv, label_opt)
        }
        "Accordion" => {
            let title_rv = find_prop(el, &["title", "label", "text"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let exp_prop = find_prop(el, &["expanded", "open"]);
            let exp_sig = bind_bool_signal(scope, exp_prop, env, false);
            let n = tree.accordion(scope, title_rv, exp_sig);
            if let Some(h) = el
                .handlers
                .iter()
                .find(|h| h.event == "on_change" || h.event == "on_toggle")
            {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_bool_change(n, move |expanded| {
                    let mut h_env = env_c.clone();
                    let v = DynValue::Bool(expanded);
                    h_env.locals.insert("expanded".into(), v.clone());
                    h_env.locals.insert("open".into(), v.clone());
                    h_env.locals.insert("value".into(), v);
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "DataTable" => {
            let cols_rv = find_prop(el, &["columns"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let rows_rv = find_prop(el, &["rows", "items"])
                .map(|p| expr_to_reactive_string_table(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let sel_prop = find_prop(el, &["selected"]);
            let sel_sig = bind_usize_signal(scope, sel_prop, env, 0);
            let n = tree.data_table(scope, cols_rv, rows_rv, sel_sig);
            let striped_opt = eval_prop("striped").map(|v| v.as_bool());
            let row_height_opt = eval_prop("row_height").map(|v| v.as_f32());
            if striped_opt.is_some() || row_height_opt.is_some() {
                tree.set_data_table_options(n, striped_opt, row_height_opt);
            }
            wire_index_change(n, el, env, tree);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_sort") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_sort(n, move |col| {
                    let mut h_env = env_c.clone();
                    let v = DynValue::Int(col as i64);
                    h_env.locals.insert("column".into(), v.clone());
                    h_env.locals.insert("col".into(), v.clone());
                    h_env.locals.insert("index".into(), v.clone());
                    h_env.locals.insert("idx".into(), v.clone());
                    h_env.locals.insert("value".into(), v);
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Select" | "Dropdown" => {
            let opts_rv = find_prop(el, &["options", "items"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let placeholder = eval_prop("placeholder")
                .or_else(|| eval_prop("label"))
                .map(|v| v.as_str())
                .unwrap_or_else(|| "Select...".to_string());
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.select(scope, opts_rv, idx_sig, placeholder);
            wire_index_change(n, el, env, tree);
            n
        }
        "RadioGroup" => {
            let opts_rv = find_prop(el, &["options", "items"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.radio_group(scope, opts_rv, idx_sig, axis);
            wire_index_change(n, el, env, tree);
            n
        }
        "DatePicker" => {
            let val_prop = find_prop(el, &["value"]);
            let (init_s, sig_opt) = bind_string_signal(scope, val_prop, env);
            let val_sig = sig_opt.unwrap_or_else(|| {
                let initial = if init_s.trim().is_empty() {
                    "2026-01-15".to_string()
                } else {
                    init_s
                };
                scope.signal(initial)
            });
            let n = tree.date_picker(scope, val_sig, None::<fn(String)>);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "TimePicker" => {
            let val_prop = find_prop(el, &["value"]);
            let (init_s, sig_opt) = bind_string_signal(scope, val_prop, env);
            let val_sig = sig_opt.unwrap_or_else(|| {
                let initial = if init_s.trim().is_empty() {
                    "09:30".to_string()
                } else {
                    init_s
                };
                scope.signal(initial)
            });
            let use_24h = eval_prop("use_24h").map(|v| v.as_bool()).unwrap_or(true);
            let n = tree.time_picker(scope, val_sig, use_24h, None::<fn(String)>);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "TreeView" => {
            let items_rv = find_prop(el, &["items", "options"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.tree_view(scope, items_rv, idx_sig, None::<fn(usize)>);
            wire_index_change(n, el, env, tree);
            n
        }
        "ReorderableList" => {
            let items_prop = find_prop(el, &["items", "options"]);
            let items_sig = bind_string_list_signal(scope, items_prop, env);
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.reorderable_list(scope, items_sig, idx_sig, None::<fn(usize, usize)>);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_reorder") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_reorder(n, move |from, to| {
                    let mut h_env = env_c.clone();
                    h_env.locals.insert("from".into(), DynValue::Int(from as i64));
                    h_env.locals.insert("to".into(), DynValue::Int(to as i64));
                    run_stmts(&stmts, &h_env);
                });
            }
            wire_index_change(n, el, env, tree);
            n
        }
        "MenuBar" => {
            let menus_rv = find_prop(el, &["menus", "items"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let n = tree.menu_bar(scope, menus_rv, None::<fn(String)>);
            if let Some(h) = el
                .handlers
                .iter()
                .find(|h| h.event == "on_select" || h.event == "on_change")
            {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_menu_select(n, move |item| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(item.clone()));
                    h_env.locals.insert("item".into(), DynValue::String(item));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Draggable" => {
            let payload_rv = find_prop(el, &["payload", "value"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            tree.draggable(scope, payload_rv)
        }
        "DropTarget" => {
            let prefix = eval_prop("accept_prefix")
                .or_else(|| eval_prop("accept"))
                .map(|v| v.as_str())
                .unwrap_or_default();
            let n = tree.drop_target(scope, prefix, None::<fn(String)>);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_drop") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_drop(n, move |payload| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("payload".into(), DynValue::String(payload.clone()));
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(payload));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "PopupMenu" => {
            let open_prop = find_prop(el, &["open", "visible"]);
            let open_rv = open_prop
                .map(|p| expr_to_reactive_bool(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(false));
            let items_rv = find_prop(el, &["items", "options"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let on_sel: Option<IndexCallback> = el
                .handlers
                .iter()
                .find(|h| h.event == "on_select" || h.event == "on_change")
                .map(|h| {
                    let stmts = h.body.clone();
                    let env_c = env.clone();
                    Rc::new(RefCell::new(move |idx: usize| {
                        let mut h_env = env_c.clone();
                        h_env.locals.insert("idx".into(), DynValue::Int(idx as i64));
                        h_env
                            .locals
                            .insert("index".into(), DynValue::Int(idx as i64));
                        run_stmts(&stmts, &h_env);
                    })) as IndexCallback
                });
            let on_cls = build_overlay_close_handler(el, open_prop, env);
            tree.popup_menu(scope, open_rv, items_rv, None, on_sel, on_cls)
        }
        "Tooltip" => {
            let txt_rv = find_prop(el, &["text", "label", "content"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            tree.tooltip(scope, txt_rv)
        }
        "ProgressIndicator" => {
            let prog_rv = match find_prop(el, &["progress", "value"]) {
                Some(p) => {
                    let expr = p.value.clone();
                    if expr_has_reactivity(&expr, env) {
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::Null => None,
                                v => Some(v.as_f32()),
                            }
                        }))
                    } else {
                        match eval_expr(&expr, env) {
                            DynValue::Null => ReactiveValue::Static(None),
                            v => ReactiveValue::Static(Some(v.as_f32())),
                        }
                    }
                }
                None => ReactiveValue::Static(None),
            };
            tree.progress_indicator(scope, prog_rv)
        }
        "Spinner" => {
            let sz_p = find_prop(el, &["size"]);
            let th_p = find_prop(el, &["thickness"]);
            let col_p = find_prop(el, &["color", "foreground"]);
            let init_sz = sz_p
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(24.0);
            let n = tree.spinner(scope, init_sz);
            let th_opt = th_p.map(|p| eval_expr(&p.value, env).as_f32());
            let col_opt = col_p.map(|p| {
                expr_to_reactive_color(&p.value, env, env.theme_sig.get_untracked().primary)
            });
            if th_opt.is_some() || col_opt.is_some() {
                tree.set_spinner_style(n, None, th_opt, col_opt);
            }
            let sz_dyn = sz_p.is_some_and(|p| expr_has_reactivity(&p.value, env));
            let th_dyn = th_p.is_some_and(|p| expr_has_reactivity(&p.value, env));
            if sz_dyn || th_dyn {
                let sz_tr = sz_p.map(|p| p.value.clone());
                let th_tr = th_p.map(|p| p.value.clone());
                let sz_ap = sz_tr.clone();
                let th_ap = th_tr.clone();
                let env_tr = env.clone();
                let env_ap = env.clone();
                tree.bind_dynamic_prop(
                    n,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT,
                    move || {
                        if let Some(ref e) = sz_tr {
                            let _ = eval_expr_tracked(e, &env_tr, true);
                        }
                        if let Some(ref e) = th_tr {
                            let _ = eval_expr_tracked(e, &env_tr, true);
                        }
                    },
                    move |id, tr| {
                        let sz = sz_ap.as_ref().map(|e| eval_expr(e, &env_ap).as_f32());
                        let th = th_ap.as_ref().map(|e| eval_expr(e, &env_ap).as_f32());
                        tr.set_spinner_style(id, sz, th, None);
                    },
                );
            }
            n
        }
        "Badge" => {
            let lbl_rv = find_prop(el, &["label", "text"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let n = tree.badge(scope, lbl_rv);
            let fg_opt = find_prop(el, &["color", "foreground"]).map(|p| {
                expr_to_reactive_color(&p.value, env, env.theme_sig.get_untracked().text_primary)
            });
            let bg_opt = find_prop(el, &["background"]).map(|p| {
                expr_to_reactive_color(&p.value, env, env.theme_sig.get_untracked().surface_variant)
            });
            if fg_opt.is_some() || bg_opt.is_some() {
                tree.set_badge_colors(n, fg_opt, bg_opt);
            }
            if let Some(ip) = find_prop(el, &["icon"]) {
                let icon_rv = expr_to_reactive_string(&ip.value, env);
                tree.set_badge_icon(n, Some(icon_rv));
            }
            n
        }
        "SplitPane" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Vertical" => Axis::Vertical,
                _ => Axis::Horizontal,
            };
            let ratio_prop = find_prop(el, &["ratio"]);
            let ratio_sig = bind_f32_signal(scope, ratio_prop, env, 0.5);
            let n = tree.split_pane(scope, axis, ratio_sig);
            let mf = find_prop(el, &["min_first"]);
            let ms = find_prop(el, &["min_second"]);
            let ds = find_prop(el, &["divider_size"]);
            if mf.is_some() || ms.is_some() || ds.is_some() {
                let mf_v = mf
                    .map(|p| eval_expr(&p.value, env).as_f32())
                    .unwrap_or(80.0);
                let ms_v = ms
                    .map(|p| eval_expr(&p.value, env).as_f32())
                    .unwrap_or(80.0);
                let ds_v = ds.map(|p| eval_expr(&p.value, env).as_f32()).unwrap_or(8.0);
                tree.set_split_pane_config(n, mf_v, ms_v, ds_v);
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_float_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::Float(value as f64));
                    h_env
                        .locals
                        .insert("ratio".into(), DynValue::Float(value as f64));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "ToastHost" => {
            let items_rv = match find_prop(el, &["items"]) {
                Some(p) => {
                    if expr_has_reactivity(&p.value, env) {
                        let expr = p.value.clone();
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            dyn_value_to_toast_list(&eval_expr_tracked(&expr, &env_c, true))
                        }))
                    } else {
                        ReactiveValue::Static(dyn_value_to_toast_list(&eval_expr(&p.value, env)))
                    }
                }
                None => ReactiveValue::Static(Vec::new()),
            };
            let n = tree.toast_host_custom(scope, items_rv, None);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_dismiss") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_dismiss(n, move |tid| {
                    let mut h_env = env_c.clone();
                    h_env.locals.insert("id".into(), DynValue::Int(tid as i64));
                    h_env
                        .locals
                        .insert("value".into(), DynValue::Int(tid as i64));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Modal" | "BottomSheet" => {
            let is_sheet = el.kind == "BottomSheet";
            let open_prop = find_prop(el, &["open", "visible"]);
            let open_rv = open_prop
                .map(|p| expr_to_reactive_bool(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(false));
            let on_cls = build_overlay_close_handler(el, open_prop, env);
            tree.overlay(scope, open_rv, is_sheet, on_cls)
        }
        "PlatformView" => {
            let vid = eval_prop("view_id")
                .map(|v| PlatformViewId::from_raw(v.as_i64().max(0) as u64))
                .unwrap_or_default();
            tree.platform_view(scope, vid, LayoutStyle::column())
        }
        "HtmlView" => {
            let src_rv = find_prop(el, &["src", "url"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let html_rv = find_prop(el, &["html"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            tree.html_view(scope, src_rv, html_rv, None)
        }
        "div" | "section" | "nav" => {
            let class_str = find_prop(el, &["class"])
                .map(|p| match eval_expr(&p.value, env) {
                    DynValue::String(s) => s,
                    _ => String::new(),
                })
                .unwrap_or_default();
            if class_str.contains("flex-row") {
                tree.row(scope)
            } else {
                tree.column(scope)
            }
        }
        "span" | "p" | "h1" | "h2" | "h3" => {
            let (sz, wt) = match el.kind.as_str() {
                "h1" => (28.0f32, 700.0f32),
                "h2" => (24.0f32, 700.0f32),
                "h3" => (20.0f32, 600.0f32),
                _ => (15.0f32, 400.0f32),
            };
            let content_rv = find_prop(el, &["text", "content"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let st = TextStyle::default().with_size(sz).with_weight(wt);
            tree.text(scope, content_rv, st)
        }
        "button" => {
            let text_rv = find_prop(el, &["text", "title", "label"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static("Button".into()));
            tree.button(scope, text_rv, || {})
        }
        "input" => {
            let text_prop = find_prop(el, &["text", "value"]);
            let (init_s, sig_opt) = bind_string_signal(scope, text_prop, env);
            let ph = eval_prop("placeholder")
                .map(|v| v.as_str())
                .unwrap_or_default();
            let n = tree.text_input(scope, init_s, ph, sig_opt);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_submit") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_submit(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        _ => tree.column(scope),
    };

    apply_common_props_and_events(scope, node, el, env, tree, false);
    build_children(scope, node, el, env, tree);
    node
}

fn apply_node_layout_props(
    layout: &mut craftui_layout::LayoutStyle,
    el: &ElementNode,
    env: &DynEnv,
) -> bool {
    let mut dirty = false;
    let eval_p = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, env))
    };
    if let Some(v) = eval_p("class") {
        let tw = craftui_core::TailwindTokens::parse(&v.as_str());
        if tw.is_column {
            layout.mode = craftui_layout::LayoutMode::Column;
            dirty = true;
        } else if tw.is_row {
            layout.mode = craftui_layout::LayoutMode::Row;
            dirty = true;
        }
        if let Some(p) = tw.padding {
            layout.padding = p;
            dirty = true;
        }
        if let Some(g) = tw.gap {
            layout.gap_x = g;
            layout.gap_y = g;
            dirty = true;
        }
        if let Some(w) = tw.width {
            layout.width = w;
            dirty = true;
        }
        if let Some(h) = tw.height {
            layout.height = h;
            dirty = true;
        }
    }
    if let Some(v) = eval_p("width") {
        layout.width = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("height") {
        layout.height = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("min_width") {
        layout.min_width = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("max_width") {
        layout.max_width = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("min_height") {
        layout.min_height = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("max_height") {
        layout.max_height = v.as_length();
        dirty = true;
    }
    if let Some(v) = eval_p("flex_grow") {
        layout.flex_grow = v.as_f32();
        dirty = true;
    }
    if let Some(v) = eval_p("flex_shrink") {
        layout.flex_shrink = v.as_f32();
        dirty = true;
    }
    if let Some(v) = eval_p("gap") {
        let g = v.as_f32();
        layout.gap_x = g;
        layout.gap_y = g;
        dirty = true;
    }
    if let Some(v) = eval_p("safe_area") {
        layout.safe_area = v.as_bool();
        dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "padding") {
        if let Expr::Member(base, field) = &p_binding.value
            && matches!(base.as_ref(), Expr::Ident(s) if s == "SafeArea")
            && field == "insets"
        {
            layout.safe_area = true;
        } else {
            let v = eval_expr(&p_binding.value, env).as_f32();
            layout.padding = Insets::all(v);
        }
        dirty = true;
    }
    if let Some(v) = eval_p("padding_x") {
        let px = v.as_f32();
        layout.padding.left = px;
        layout.padding.right = px;
        dirty = true;
    }
    if let Some(v) = eval_p("padding_y") {
        let py = v.as_f32();
        layout.padding.top = py;
        layout.padding.bottom = py;
        dirty = true;
    }
    if let Some(v) = eval_p("margin") {
        layout.margin = Insets::all(v.as_f32());
        dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "justify")
        && let Expr::Ident(s) = &p_binding.value
    {
        layout.justify_content = match s.as_str() {
            "Center" => Justify::Center,
            "End" => Justify::End,
            "SpaceBetween" => Justify::SpaceBetween,
            "SpaceAround" => Justify::SpaceAround,
            "SpaceEvenly" => Justify::SpaceEvenly,
            _ => Justify::Start,
        };
        dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "align")
        && let Expr::Ident(s) = &p_binding.value
    {
        layout.align_items = match s.as_str() {
            "Center" => Alignment::Center,
            "End" => Alignment::End,
            "Stretch" => Alignment::Stretch,
            _ => Alignment::Start,
        };
        dirty = true;
    }
    if let Some(v) = eval_p("clip") {
        layout.clip_overflow = v.as_bool();
        dirty = true;
    }
    dirty
}

fn eval_shadows_dyn(v: &DynValue) -> Vec<BoxShadow> {
    match v {
        DynValue::Bool(true) => vec![BoxShadow::new(0.0, 4.0, 12.0, Color::rgba(0, 0, 0, 80))],
        DynValue::Bool(false) | DynValue::Null => Vec::new(),
        DynValue::Struct { name, fields } if name == "BoxShadow" => {
            let ox = fields.get("offset_x").map(|v| v.as_f32()).unwrap_or(0.0);
            let oy = fields.get("offset_y").map(|v| v.as_f32()).unwrap_or(0.0);
            let blur = fields.get("blur").map(|v| v.as_f32()).unwrap_or(0.0);
            let spread = fields.get("spread").map(|v| v.as_f32()).unwrap_or(0.0);
            let col = fields
                .get("color")
                .and_then(|v| v.as_color())
                .unwrap_or(Color::rgba(0, 0, 0, 80));
            let inset = fields.get("inset").map(|v| v.as_bool()).unwrap_or(false);
            vec![BoxShadow {
                offset_x: ox,
                offset_y: oy,
                blur_radius: blur,
                spread,
                color: col,
                inset,
            }]
        }
        DynValue::List(items) => {
            let mut out = Vec::new();
            for chunk in items.as_chunks::<4>().0 {
                let ox = chunk[0].as_f32();
                let oy = chunk[1].as_f32();
                let blur = chunk[2].as_f32();
                let col = chunk[3].as_color().unwrap_or(Color::rgba(0, 0, 0, 80));
                out.push(BoxShadow::new(ox, oy, blur, col));
            }
            out
        }
        other => {
            let blur = other.as_f32();
            if blur > 0.0 {
                vec![BoxShadow::new(0.0, 4.0, blur, Color::rgba(0, 0, 0, 80))]
            } else {
                Vec::new()
            }
        }
    }
}

fn parse_blend_mode_expr(expr: &Expr) -> Option<BlendMode> {
    let s = match expr {
        Expr::Ident(s) => s.as_str(),
        Expr::Member(_, f) => f.as_str(),
        _ => return None,
    };
    Some(match s {
        "Multiply" => BlendMode::Multiply,
        "Screen" => BlendMode::Screen,
        "Overlay" => BlendMode::Overlay,
        "Darken" => BlendMode::Darken,
        "Lighten" => BlendMode::Lighten,
        "Plus" => BlendMode::Plus,
        _ => BlendMode::Normal,
    })
}

fn parse_cursor_expr(expr: &Expr) -> Option<CursorIcon> {
    let s = match expr {
        Expr::Ident(s) => s.as_str(),
        Expr::Member(_, f) => f.as_str(),
        _ => return None,
    };
    Some(match s {
        "Pointer" => CursorIcon::Pointer,
        "Text" => CursorIcon::Text,
        "Grab" => CursorIcon::Grab,
        "Grabbing" => CursorIcon::Grabbing,
        "Crosshair" => CursorIcon::Crosshair,
        "NotAllowed" => CursorIcon::NotAllowed,
        "ResizeHorizontal" => CursorIcon::ResizeHorizontal,
        "ResizeVertical" => CursorIcon::ResizeVertical,
        _ => CursorIcon::Default,
    })
}

fn parse_easing_expr(expr: &Expr) -> Easing {
    let s = match expr {
        Expr::Ident(s) => s.as_str(),
        Expr::Member(_, f) => f.as_str(),
        _ => return Easing::EaseOut,
    };
    match s {
        "Linear" => Easing::Linear,
        "EaseIn" => Easing::EaseIn,
        "EaseInOut" => Easing::EaseInOut,
        _ => Easing::EaseOut,
    }
}

fn eval_animate_curve(spec: &AnimateSpec, env: &DynEnv) -> TransitionCurve {
    if let Some((_, curve_expr)) = spec.fields.iter().find(|(k, _)| k == "curve")
        && let Expr::Call(name, args) = curve_expr
    {
        return match name.as_str() {
            "snappy" => TransitionCurve::snappy(),
            "gentle" => TransitionCurve::gentle(),
            "instant" => TransitionCurve::instant(),
            "spring" if args.len() == 2 => TransitionCurve::spring(
                eval_expr(&args[0], env).as_f32(),
                eval_expr(&args[1], env).as_f32(),
            ),
            "tween" if !args.is_empty() => {
                let dur = eval_expr(&args[0], env).as_f32();
                let eas = args
                    .get(1)
                    .map(parse_easing_expr)
                    .unwrap_or(Easing::EaseOut);
                TransitionCurve::tween(dur, eas)
            }
            _ => TransitionCurve::snappy(),
        };
    }
    let stiff = spec
        .fields
        .iter()
        .find(|(k, _)| k == "stiffness")
        .map(|(_, e)| eval_expr(e, env).as_f32());
    let damp = spec
        .fields
        .iter()
        .find(|(k, _)| k == "damping")
        .map(|(_, e)| eval_expr(e, env).as_f32());
    if stiff.is_some() || damp.is_some() {
        return TransitionCurve::spring(stiff.unwrap_or(320.0), damp.unwrap_or(28.0));
    }
    let dur = spec
        .fields
        .iter()
        .find(|(k, _)| k == "duration")
        .map(|(_, e)| eval_expr(e, env).as_f32())
        .unwrap_or(0.2);
    let eas = spec
        .fields
        .iter()
        .find(|(k, _)| k == "easing")
        .map(|(_, e)| parse_easing_expr(e))
        .unwrap_or(Easing::EaseOut);
    TransitionCurve::tween(dur, eas)
}

fn animatable_props_for_name(name: &str) -> Vec<AnimatableProp> {
    match name {
        "opacity" => vec![AnimatableProp::Opacity],
        "translate_x" => vec![AnimatableProp::TranslateX],
        "translate_y" => vec![AnimatableProp::TranslateY],
        "scale" => vec![AnimatableProp::ScaleX, AnimatableProp::ScaleY],
        "scale_x" => vec![AnimatableProp::ScaleX],
        "scale_y" => vec![AnimatableProp::ScaleY],
        "rotate" | "rotation" => vec![AnimatableProp::Rotation],
        "background" => vec![AnimatableProp::BackgroundColor],
        "border_color" => vec![AnimatableProp::BorderColor],
        _ => Vec::new(),
    }
}

fn apply_common_props_and_events(
    _scope: Scope,
    node: NodeId,
    el: &ElementNode,
    env: &DynEnv,
    tree: &mut WidgetTree,
    is_subcomponent: bool,
) {
    let eval_prop = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, env))
    };

    if let Some(id_val) = eval_prop("id") {
        let s = id_val.as_str();
        if !s.is_empty() {
            tree.register_node_id(node, s);
        }
    }

    const LAYOUT_NAMES: &[&str] = &[
        "width",
        "height",
        "min_width",
        "max_width",
        "min_height",
        "max_height",
        "flex_grow",
        "flex_shrink",
        "gap",
        "safe_area",
        "padding",
        "padding_x",
        "padding_y",
        "margin",
        "clip",
    ];
    let reactive_layout_exprs: Vec<Expr> = el
        .props
        .iter()
        .filter(|p| LAYOUT_NAMES.contains(&p.name.as_str()) && expr_has_reactivity(&p.value, env))
        .map(|p| p.value.clone())
        .collect();

    if !reactive_layout_exprs.is_empty() {
        let env_tr = env.clone();
        let el_c = el.clone();
        let env_ap = env.clone();
        tree.bind_dynamic_prop(
            node,
            DirtyFlags::LAYOUT | DirtyFlags::PAINT,
            move || {
                for expr in &reactive_layout_exprs {
                    let _ = eval_expr_tracked(expr, &env_tr, true);
                }
            },
            move |id, tr| {
                let mut l = tr
                    .nodes
                    .get(id)
                    .map(|n| n.layout.clone())
                    .unwrap_or_default();
                if apply_node_layout_props(&mut l, &el_c, &env_ap) {
                    tr.set_layout(id, l);
                }
            },
        );
    } else {
        let mut layout = tree
            .nodes
            .get(node)
            .map(|n| n.layout.clone())
            .unwrap_or_default();
        if apply_node_layout_props(&mut layout, el, env) {
            tree.set_layout(node, layout);
        }
    }

    let mut box_style = tree
        .nodes
        .get(node)
        .map(|n| n.box_style.clone())
        .unwrap_or_default();
    let mut box_dirty = false;

    if let Some(cls_prop) = find_prop(el, &["class"]) {
        let tw = craftui_core::TailwindTokens::parse(&eval_expr(&cls_prop.value, env).as_str());
        if let Some(bg) = tw.background
            && box_style.background.is_none()
        {
            box_style.background = Some(ReactiveValue::Static(Brush::Solid(bg)));
            box_dirty = true;
        }
        if let Some(r) = tw.radius
            && box_style.radius == CornerRadii::ZERO
        {
            box_style.radius = r;
            if let Some(ref mut br) = box_style.border {
                br.radius = r;
            }
            tree.set_image_radius(node, r);
            box_dirty = true;
        }
        if let Some(sh) = tw.shadow
            && box_style.shadow.is_none()
        {
            box_style.shadow = Some(sh);
            box_dirty = true;
        }
    }

    if el.kind != "Badge"
        && let Some(bg_prop) = find_prop(el, &["background"])
    {
        if expr_has_reactivity(&bg_prop.value, env) {
            let expr = bg_prop.value.clone();
            let env_c = env.clone();
            box_style.background = Some(ReactiveValue::Dynamic(Rc::new(move || {
                let v = eval_expr_tracked(&expr, &env_c, true);
                v.as_brush().unwrap_or(Brush::Solid(Color::TRANSPARENT))
            })));
            box_dirty = true;
        } else {
            let v = eval_expr(&bg_prop.value, env);
            if let Some(b) = v.as_brush() {
                box_style.background = Some(ReactiveValue::Static(b));
                box_dirty = true;
            }
        }
    }
    if let Some(br_prop) = find_prop(el, &["border_radius"]) {
        let rad = CornerRadii::all(eval_expr(&br_prop.value, env).as_f32());
        box_style.radius = rad;
        if let Some(ref mut br) = box_style.border {
            br.radius = rad;
        }
        tree.set_image_radius(node, rad);
        box_dirty = true;
    }
    if let Some(bw_prop) = find_prop(el, &["border_width"]) {
        let bw = eval_expr(&bw_prop.value, env).as_f32();
        let def_c = env.theme_sig.get_untracked().border;
        let cur = box_style.border.unwrap_or(Border {
            width: bw,
            color: def_c,
            radius: box_style.radius,
        });
        box_style.border = Some(Border {
            width: bw,
            color: cur.color,
            radius: box_style.radius,
        });
        if find_prop(el, &["border_color"]).is_none() && box_style.border_color.is_none() {
            let tsig = env.theme_sig;
            box_style.border_color =
                Some(ReactiveValue::Dynamic(Rc::new(move || tsig.get().border)));
        }
        box_dirty = true;
    }
    if let Some(bc_prop) = find_prop(el, &["border_color"]) {
        let col_rv =
            expr_to_reactive_color(&bc_prop.value, env, env.theme_sig.get_untracked().border);
        box_style.border_color = Some(col_rv);
        if box_style.border.is_none() {
            box_style.border = Some(Border {
                width: 1.0,
                color: Color::TRANSPARENT,
                radius: box_style.radius,
            });
        }
        box_dirty = true;
    }
    if let Some(cp) = find_prop(el, &["clip"]) {
        box_style.clip = eval_expr(&cp.value, env).as_bool();
        box_dirty = true;
    }
    if let Some(bm_prop) = find_prop(el, &["blend_mode"])
        && let Some(bm) = parse_blend_mode_expr(&bm_prop.value)
    {
        box_style.blend_mode = bm;
        box_dirty = true;
    }
    if let Some(cur_prop) = find_prop(el, &["cursor"])
        && let Some(c) = parse_cursor_expr(&cur_prop.value)
    {
        box_style.cursor = Some(c);
        box_dirty = true;
    }
    if box_dirty {
        tree.set_box_style(node, box_style);
    }

    // Reactive border_radius / border_width / clip updates
    let br_expr = find_prop(el, &["border_radius"])
        .filter(|p| expr_has_reactivity(&p.value, env))
        .map(|p| p.value.clone());
    let bw_expr = find_prop(el, &["border_width"])
        .filter(|p| expr_has_reactivity(&p.value, env))
        .map(|p| p.value.clone());
    let cl_expr = find_prop(el, &["clip"])
        .filter(|p| expr_has_reactivity(&p.value, env))
        .map(|p| p.value.clone());
    if br_expr.is_some() || bw_expr.is_some() || cl_expr.is_some() {
        let br_tr = br_expr.clone();
        let bw_tr = bw_expr.clone();
        let cl_tr = cl_expr.clone();
        let env_tr = env.clone();
        let env_ap = env.clone();
        tree.bind_dynamic_prop(
            node,
            DirtyFlags::PAINT,
            move || {
                for expr in [&br_tr, &bw_tr, &cl_tr].into_iter().flatten() {
                    let _ = eval_expr_tracked(expr, &env_tr, true);
                }
            },
            move |id, tr| {
                let br_v = br_expr
                    .as_ref()
                    .map(|e| CornerRadii::all(eval_expr(e, &env_ap).as_f32()));
                let bw_v = bw_expr.as_ref().map(|e| eval_expr(e, &env_ap).as_f32());
                let cl_v = cl_expr.as_ref().map(|e| eval_expr(e, &env_ap).as_bool());
                if let Some(rad) = br_v {
                    tr.set_image_radius(id, rad);
                }
                if let Some(n) = tr.nodes.get_mut(id) {
                    if let Some(rad) = br_v {
                        n.box_style.radius = rad;
                        if let Some(ref mut br) = n.box_style.border {
                            br.radius = rad;
                        }
                    }
                    if let Some(bw) = bw_v
                        && let Some(ref mut br) = n.box_style.border
                    {
                        br.width = bw;
                    }
                    if let Some(c) = cl_v {
                        n.box_style.clip = c;
                    }
                    n.dirty |= DirtyFlags::PAINT;
                }
            },
        );
    }

    // Shadows (reactive or static, single or multi-shadow array)
    if let Some(sp) = find_prop(el, &["shadow"]) {
        if expr_has_reactivity(&sp.value, env) {
            let expr = sp.value.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            let expr_ap = expr.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    let _ = eval_expr_tracked(&expr, &env_tr, true);
                },
                move |id, tr| {
                    let shadows = eval_shadows_dyn(&eval_expr(&expr_ap, &env_ap));
                    tr.set_shadows(id, shadows);
                },
            );
        } else {
            let shadows = eval_shadows_dyn(&eval_expr(&sp.value, env));
            tree.set_shadows(node, shadows);
        }
    }

    // Backdrop blur + tint
    let bb_prop = find_prop(el, &["backdrop_blur"]);
    let bt_prop = find_prop(el, &["backdrop_tint"]);
    if bb_prop.is_some() || bt_prop.is_some() {
        let reactive = bb_prop.is_some_and(|p| expr_has_reactivity(&p.value, env))
            || bt_prop.is_some_and(|p| expr_has_reactivity(&p.value, env));
        if reactive {
            let b_tr = bb_prop.map(|p| p.value.clone());
            let t_tr = bt_prop.map(|p| p.value.clone());
            let b_ap = b_tr.clone();
            let t_ap = t_tr.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    if let Some(ref b) = b_tr {
                        let _ = eval_expr_tracked(b, &env_tr, true);
                    }
                    if let Some(ref t) = t_tr {
                        let _ = eval_expr_tracked(t, &env_tr, true);
                    }
                },
                move |id, tr| {
                    let blur = b_ap
                        .as_ref()
                        .map(|e| eval_expr(e, &env_ap).as_f32())
                        .unwrap_or(0.0);
                    let tint = t_ap
                        .as_ref()
                        .and_then(|e| eval_expr(e, &env_ap).as_color())
                        .unwrap_or(Color::TRANSPARENT);
                    tr.set_backdrop_blur(id, blur, tint);
                },
            );
        } else {
            let blur = bb_prop
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(0.0);
            let tint = bt_prop
                .and_then(|p| eval_expr(&p.value, env).as_color())
                .unwrap_or(Color::TRANSPARENT);
            tree.set_backdrop_blur(node, blur, tint);
        }
    }

    // Opacity (set via set_opacity before set_transition!)
    if let Some(op_prop) = find_prop(el, &["opacity"]) {
        if expr_has_reactivity(&op_prop.value, env) {
            let expr = op_prop.value.clone();
            let expr_ap = expr.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    let _ = eval_expr_tracked(&expr, &env_tr, true);
                },
                move |id, tr| {
                    tr.set_opacity(id, eval_expr(&expr_ap, &env_ap).as_f32());
                },
            );
        } else {
            tree.set_opacity(node, eval_expr(&op_prop.value, env).as_f32());
        }
    }

    // Transforms (scale, translate, rotate, skew) BEFORE set_transition
    let sc_p = find_prop(el, &["scale"]);
    let scx_p = find_prop(el, &["scale_x"]);
    let scy_p = find_prop(el, &["scale_y"]);
    if sc_p.is_some() || scx_p.is_some() || scy_p.is_some() {
        let reactive = [sc_p, scx_p, scy_p]
            .into_iter()
            .flatten()
            .any(|p| expr_has_reactivity(&p.value, env));
        if reactive {
            let e_sc = sc_p.map(|p| p.value.clone());
            let e_sx = scx_p.map(|p| p.value.clone());
            let e_sy = scy_p.map(|p| p.value.clone());
            let tr_sc = e_sc.clone();
            let tr_sx = e_sx.clone();
            let tr_sy = e_sy.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    for e in [&tr_sc, &tr_sx, &tr_sy].into_iter().flatten() {
                        let _ = eval_expr_tracked(e, &env_tr, true);
                    }
                },
                move |id, tr| {
                    let base = e_sc.as_ref().map(|e| eval_expr(e, &env_ap).as_f32());
                    let sx = match (base, &e_sx) {
                        (Some(b), Some(x)) => b * eval_expr(x, &env_ap).as_f32(),
                        (Some(b), None) => b,
                        (None, Some(x)) => eval_expr(x, &env_ap).as_f32(),
                        (None, None) => 1.0,
                    };
                    let sy = match (base, &e_sy) {
                        (Some(b), Some(y)) => b * eval_expr(y, &env_ap).as_f32(),
                        (Some(b), None) => b,
                        (None, Some(y)) => eval_expr(y, &env_ap).as_f32(),
                        (None, None) => 1.0,
                    };
                    tr.set_scale(id, sx, sy);
                },
            );
        } else {
            let base = sc_p.map(|p| eval_expr(&p.value, env).as_f32());
            let sx = match (base, scx_p) {
                (Some(b), Some(x)) => b * eval_expr(&x.value, env).as_f32(),
                (Some(b), None) => b,
                (None, Some(x)) => eval_expr(&x.value, env).as_f32(),
                (None, None) => 1.0,
            };
            let sy = match (base, scy_p) {
                (Some(b), Some(y)) => b * eval_expr(&y.value, env).as_f32(),
                (Some(b), None) => b,
                (None, Some(y)) => eval_expr(&y.value, env).as_f32(),
                (None, None) => 1.0,
            };
            tree.set_scale(node, sx, sy);
        }
    }

    let tx_p = find_prop(el, &["translate_x"]);
    let ty_p = find_prop(el, &["translate_y"]);
    if tx_p.is_some() || ty_p.is_some() {
        let reactive = [tx_p, ty_p]
            .into_iter()
            .flatten()
            .any(|p| expr_has_reactivity(&p.value, env));
        if reactive {
            let e_tx = tx_p.map(|p| p.value.clone());
            let e_ty = ty_p.map(|p| p.value.clone());
            let tr_tx = e_tx.clone();
            let tr_ty = e_ty.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    for e in [&tr_tx, &tr_ty].into_iter().flatten() {
                        let _ = eval_expr_tracked(e, &env_tr, true);
                    }
                },
                move |id, tr| {
                    let dx = e_tx
                        .as_ref()
                        .map(|e| eval_expr(e, &env_ap).as_f32())
                        .unwrap_or(0.0);
                    let dy = e_ty
                        .as_ref()
                        .map(|e| eval_expr(e, &env_ap).as_f32())
                        .unwrap_or(0.0);
                    tr.set_translate(id, dx, dy);
                },
            );
        } else {
            let dx = tx_p
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(0.0);
            let dy = ty_p
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(0.0);
            tree.set_translate(node, dx, dy);
        }
    }

    if let Some(rot_p) = find_prop(el, &["rotate", "rotation"]) {
        if expr_has_reactivity(&rot_p.value, env) {
            let expr = rot_p.value.clone();
            let expr_ap = expr.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    let _ = eval_expr_tracked(&expr, &env_tr, true);
                },
                move |id, tr| {
                    tr.set_rotation(id, eval_expr(&expr_ap, &env_ap).as_f32());
                },
            );
        } else {
            tree.set_rotation(node, eval_expr(&rot_p.value, env).as_f32());
        }
    }

    let kx_p = find_prop(el, &["skew_x"]);
    let ky_p = find_prop(el, &["skew_y"]);
    if kx_p.is_some() || ky_p.is_some() {
        let reactive = [kx_p, ky_p]
            .into_iter()
            .flatten()
            .any(|p| expr_has_reactivity(&p.value, env));
        if reactive {
            let e_kx = kx_p.map(|p| p.value.clone());
            let e_ky = ky_p.map(|p| p.value.clone());
            let tr_kx = e_kx.clone();
            let tr_ky = e_ky.clone();
            let env_tr = env.clone();
            let env_ap = env.clone();
            tree.bind_dynamic_prop(
                node,
                DirtyFlags::PAINT,
                move || {
                    for e in [&tr_kx, &tr_ky].into_iter().flatten() {
                        let _ = eval_expr_tracked(e, &env_tr, true);
                    }
                },
                move |id, tr| {
                    let kx = e_kx
                        .as_ref()
                        .map(|e| eval_expr(e, &env_ap).as_f32())
                        .unwrap_or(0.0);
                    let ky = e_ky
                        .as_ref()
                        .map(|e| eval_expr(e, &env_ap).as_f32())
                        .unwrap_or(0.0);
                    tr.set_transform(id, Transform2D::skew(kx, ky));
                },
            );
        } else {
            let kx = kx_p
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(0.0);
            let ky = ky_p
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(0.0);
            tree.set_transform(node, Transform2D::skew(kx, ky));
        }
    }

    if let Some(vp) = find_prop(el, &["visible"])
        && !matches!(el.kind.as_str(), "Modal" | "BottomSheet" | "PopupMenu")
    {
        let expr = vp.value.clone();
        let env_c = env.clone();
        tree.set_visible_when(node, move || {
            eval_expr_tracked(&expr, &env_c, true).as_bool()
        });
    }

    if let Some(dp) = find_prop(el, &["disabled"]) {
        let rv = expr_to_reactive_bool(&dp.value, env);
        tree.set_disabled(node, rv);
    }

    let is_sub_cb = is_subcomponent
        && env
            .file
            .components
            .iter()
            .find(|c| c.name == el.kind)
            .is_some_and(|c| c.callbacks.iter().any(|cb| cb.name == "on_tap"));

    if (is_subcomponent || el.kind != "Button")
        && !is_sub_cb
        && let Some(h) = make_handler_opt(el, &["on_tap", "on_click"], env, None)
    {
        tree.set_on_tap(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = make_handler_opt(el, &["on_double_tap"], env, None) {
        tree.set_on_double_tap(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = make_handler_opt(el, &["on_long_press"], env, None) {
        tree.set_on_long_press(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = el.handlers.iter().find(|h| h.event == "on_hover") {
        let stmts = h.body.clone();
        let env_c = env.clone();
        tree.set_on_hover(node, move |hovered| {
            let mut h_env = env_c.clone();
            h_env
                .locals
                .insert("hovered".into(), DynValue::Bool(hovered));
            h_env.locals.insert("value".into(), DynValue::Bool(hovered));
            run_stmts(&stmts, &h_env);
        });
    }

    // Declarative `animate` blocks LAST so initial values don't trigger mount animations
    for spec in &el.animations {
        let curve = eval_animate_curve(spec, env);
        for anim_prop in animatable_props_for_name(&spec.prop) {
            tree.set_transition(node, anim_prop, curve);
        }
    }
}

fn build_children(
    scope: Scope,
    parent_node: NodeId,
    el: &ElementNode,
    env: &DynEnv,
    tree: &mut WidgetTree,
) {
    let slot_layout = {
        let mut base = match el.kind.as_str() {
            "Row" => LayoutStyle::row(),
            "Grid" => {
                let cols = find_prop(el, &["columns"])
                    .map(|p| eval_expr(&p.value, env).as_i64().max(1) as u16)
                    .unwrap_or(2);
                LayoutStyle::grid(cols)
            }
            "Stack" => LayoutStyle::stack(),
            _ => LayoutStyle::column(),
        };
        if let Some(g) = find_prop(el, &["gap"]) {
            base = base.with_gap(eval_expr(&g.value, env).as_f32());
        }
        base
    };

    for child in &el.children {
        match child {
            ChildItem::Element(c) => {
                let cnode = build_element(scope.child_scope(), c, env, tree);
                tree.append_child(parent_node, cnode);
            }
            ChildItem::For {
                item_var,
                index_var,
                iterable,
                key,
                body,
                ..
            } => {
                let iter_model = iterable.clone();
                let env_model = env.clone();
                let item_name = item_var.clone();
                let idx_name = index_var.clone();
                let body_el = (**body).clone();
                let key_expr = key
                    .clone()
                    .or_else(|| find_prop(body, &["key"]).map(|p| p.value.clone()));
                let env_key = env.clone();
                let item_key_var = item_var.clone();
                let idx_key_var = index_var.clone();
                let env_fac = env.clone();

                let for_host = tree.reactive_for_keyed(
                    scope,
                    slot_layout.clone(),
                    move || eval_iterable(&iter_model, &env_model, true),
                    move |idx, val| {
                        if let Some(ref ke) = key_expr {
                            let mut k_env = env_key.clone();
                            k_env.locals.insert(item_key_var.clone(), val.clone());
                            if let Some(ref iv) = idx_key_var {
                                k_env.locals.insert(iv.clone(), DynValue::Int(idx as i64));
                            }
                            eval_expr(ke, &k_env).key_string()
                        } else {
                            val.key_string()
                        }
                    },
                    move |for_cx, item_sig, idx_sig, for_tree| {
                        let mut child_env = env_fac.clone();
                        child_env.signal_locals.insert(item_name.clone(), item_sig);
                        if let Some(ref iv) = idx_name {
                            let idx_dyn =
                                for_cx.signal(DynValue::Int(idx_sig.get_untracked() as i64));
                            for_cx.effect(move || {
                                let nv = DynValue::Int(idx_sig.get() as i64);
                                if idx_dyn.get_untracked() != nv {
                                    idx_dyn.set(nv);
                                }
                            });
                            child_env.signal_locals.insert(iv.clone(), idx_dyn);
                        }
                        build_element(for_cx, &body_el, &child_env, for_tree)
                    },
                );
                tree.append_child(parent_node, for_host);
            }
            ChildItem::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if let Some(eb) = else_branch {
                    let cond_expr = condition.clone();
                    let env_cond = env.clone();
                    let then_el = (**then_branch).clone();
                    let else_el = (**eb).clone();
                    let env_body = env.clone();

                    let if_host = tree.dynamic_slot(
                        scope,
                        slot_layout.clone(),
                        move || eval_expr_tracked(&cond_expr, &env_cond, true).as_bool(),
                        move |if_cx, &is_then, if_tree| {
                            if is_then {
                                build_element(if_cx, &then_el, &env_body, if_tree)
                            } else {
                                build_element(if_cx, &else_el, &env_body, if_tree)
                            }
                        },
                    );
                    tree.append_child(parent_node, if_host);
                } else {
                    let cnode = build_element(scope.child_scope(), then_branch, env, tree);
                    let cond_expr = condition.clone();
                    let env_cond = env.clone();
                    tree.set_visible_when(cnode, move || {
                        eval_expr_tracked(&cond_expr, &env_cond, true).as_bool()
                    });
                    tree.append_child(parent_node, cnode);
                }
            }
            ChildItem::ChildrenSlot { .. } => {
                if let Some(ref slot_fn) = env.children_slot {
                    slot_fn(scope, tree, parent_node);
                }
            }
        }
    }
}

fn instantiate_subcomponent(
    scope: Scope,
    comp: &ComponentDecl,
    call_el: &ElementNode,
    outer_env: &DynEnv,
    tree: &mut WidgetTree,
) -> NodeId {
    let mut sub_props = HashMap::new();

    for p in &comp.properties {
        if let Some(bound) = call_el.props.iter().find(|bp| bp.name == p.name) {
            if bound.two_way
                && let Some(outer_sig) = outer_env.find_prop_signal(&bound.value)
            {
                // True bidirectional aliasing across parent and child components
                sub_props.insert(p.name.clone(), outer_sig);
                continue;
            }
            let init = eval_expr(&bound.value, outer_env);
            let child_sig = scope.signal(init);
            if expr_has_reactivity(&bound.value, outer_env) {
                let expr_c = bound.value.clone();
                let outer_c = outer_env.clone();
                scope.effect(move || {
                    let nv = eval_expr_tracked(&expr_c, &outer_c, true);
                    if child_sig.get_untracked() != nv {
                        child_sig.set(nv);
                    }
                });
            }
            sub_props.insert(p.name.clone(), child_sig);
        } else {
            let v = p
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, outer_env))
                .unwrap_or(DynValue::Null);
            let child_sig = scope.signal(v);
            if let Some(ref def_expr) = p.default_value
                && expr_references_window(def_expr)
            {
                let expr_c = def_expr.clone();
                let outer_c = outer_env.clone();
                scope.effect(move || {
                    let nv = eval_expr_tracked(&expr_c, &outer_c, true);
                    if child_sig.get_untracked() != nv {
                        child_sig.set(nv);
                    }
                });
            }
            sub_props.insert(p.name.clone(), child_sig);
        }
    }

    let mut sub_callbacks = outer_env.callbacks.clone();
    for cb_decl in &comp.callbacks {
        if let Some(handler) = call_el.handlers.iter().find(|h| h.event == cb_decl.name) {
            let stmts = handler.body.clone();
            let env_c = outer_env.clone();
            let dyn_cb: DynCallback = Rc::new(RefCell::new(move |args: Vec<DynValue>| {
                let mut h_env = env_c.clone();
                for (i, a) in args.into_iter().enumerate() {
                    if i == 0 {
                        h_env.locals.insert("value".into(), a.clone());
                    }
                    h_env.locals.insert(format!("arg{i}"), a);
                }
                run_stmts(&stmts, &h_env);
            }));
            sub_callbacks.insert(cb_decl.name.clone(), dyn_cb);
        }
    }

    let sub_env = DynEnv {
        file: outer_env.file.clone(),
        props: sub_props,
        signal_locals: HashMap::new(),
        callbacks: sub_callbacks,
        locals: HashMap::new(),
        theme_sig: outer_env.theme_sig,
        theme_ov_sig: outer_env.theme_ov_sig,
        metrics_sig: outer_env.metrics_sig,
        children_slot: if component_has_children_slot(comp) {
            let call_children = call_el.children.clone();
            let slot_env = outer_env.clone();
            Some(Rc::new(move |slot_scope, slot_tree, slot_parent| {
                let fake_el = ElementNode {
                    kind: "Column".into(),
                    props: Vec::new(),
                    handlers: Vec::new(),
                    animations: Vec::new(),
                    states: Vec::new(),
                    transitions: Vec::new(),
                    children: call_children.clone(),
                    span: craftui_dsl::ast::Span::default(),
                };
                build_children(slot_scope, slot_parent, &fake_el, &slot_env, slot_tree);
            }))
        } else {
            None
        },
    };

    let has_slot = component_has_children_slot(comp);
    let node = if let Some(ref root) = comp.root {
        build_element(scope, root, &sub_env, tree)
    } else {
        tree.column(scope)
    };
    if !has_slot {
        build_children(scope, node, call_el, outer_env, tree);
    }
    apply_common_props_and_events(scope, node, call_el, outer_env, tree, true);
    node
}

fn element_has_children_slot(el: &ElementNode) -> bool {
    el.children.iter().any(|c| match c {
        ChildItem::ChildrenSlot { .. } => true,
        ChildItem::Element(inner) => element_has_children_slot(inner),
        ChildItem::For { body, .. } => element_has_children_slot(body),
        ChildItem::If {
            then_branch,
            else_branch,
            ..
        } => {
            element_has_children_slot(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| element_has_children_slot(eb))
        }
    })
}

fn component_has_children_slot(comp: &ComponentDecl) -> bool {
    comp.root.as_ref().is_some_and(element_has_children_slot)
}

fn bind_f32_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: f32,
) -> Signal<f32> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_f32())
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_f32());
            scope.effect(move || {
                let nv = dyn_sig.get().as_f32();
                if (sig.get_untracked() - nv).abs() > 1e-4 {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if (dyn_sig.get_untracked().as_f32() - nv).abs() > 1e-4 {
                    dyn_sig.set(DynValue::Float(nv as f64));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_f32();
                if (sig.get_untracked() - nv).abs() > 1e-4 {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_bool_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: bool,
) -> Signal<bool> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_bool())
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_bool());
            scope.effect(move || {
                let nv = dyn_sig.get().as_bool();
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if dyn_sig.get_untracked().as_bool() != nv {
                    dyn_sig.set(DynValue::Bool(nv));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_bool();
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_usize_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: usize,
) -> Signal<usize> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_i64().max(0) as usize)
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_i64().max(0) as usize);
            scope.effect(move || {
                let nv = dyn_sig.get().as_i64().max(0) as usize;
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if (dyn_sig.get_untracked().as_i64().max(0) as usize) != nv {
                    dyn_sig.set(DynValue::Int(nv as i64));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_i64().max(0) as usize;
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_string_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
) -> (String, Option<Signal<String>>) {
    let Some(p) = prop else {
        return (String::new(), None);
    };
    let init = eval_expr(&p.value, env).as_str();
    if let Some(dyn_sig) = env.find_prop_signal(&p.value) {
        let str_sig = scope.signal(dyn_sig.get_untracked().as_str());
        scope.effect(move || {
            let nv = dyn_sig.get().as_str();
            if str_sig.get_untracked() != nv {
                str_sig.set(nv);
            }
        });
        if p.two_way {
            scope.effect(move || {
                let nv = str_sig.get();
                if dyn_sig.get_untracked().as_str() != nv {
                    dyn_sig.set(DynValue::String(nv));
                }
            });
        }
        return (String::new(), Some(str_sig));
    }
    if expr_has_reactivity(&p.value, env) {
        let str_sig = scope.signal(init.clone());
        let expr = p.value.clone();
        let env_c = env.clone();
        scope.effect(move || {
            let nv = eval_expr_tracked(&expr, &env_c, true).as_str();
            if str_sig.get_untracked() != nv {
                str_sig.set(nv);
            }
        });
        return (init, Some(str_sig));
    }
    (init, None)
}

fn bind_string_list_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
) -> Signal<Vec<String>> {
    let to_vec = |dv: DynValue| -> Vec<String> {
        match dv {
            DynValue::List(items) => items.into_iter().map(|v| v.as_str()).collect(),
            _ => Vec::new(),
        }
    };
    let Some(p) = prop else {
        return scope.signal(Vec::new());
    };
    let init = to_vec(eval_expr(&p.value, env));
    let list_sig = scope.signal(init);
    if let Some(dyn_sig) = env.find_prop_signal(&p.value) {
        list_sig.set(to_vec(dyn_sig.get_untracked()));
        scope.effect(move || {
            let nv = to_vec(dyn_sig.get());
            if list_sig.get_untracked() != nv {
                list_sig.set(nv);
            }
        });
        if p.two_way {
            scope.effect(move || {
                let nv = list_sig.get();
                let cur = to_vec(dyn_sig.get_untracked());
                if cur != nv {
                    dyn_sig.set(DynValue::List(
                        nv.into_iter().map(DynValue::String).collect(),
                    ));
                }
            });
        }
    } else if expr_has_reactivity(&p.value, env) {
        let expr = p.value.clone();
        let env_c = env.clone();
        scope.effect(move || {
            let nv = to_vec(eval_expr_tracked(&expr, &env_c, true));
            if list_sig.get_untracked() != nv {
                list_sig.set(nv);
            }
        });
    }
    list_sig
}

fn wire_index_change(node: NodeId, el: &ElementNode, env: &DynEnv, tree: &mut WidgetTree) {
    if let Some(h) = el
        .handlers
        .iter()
        .find(|h| h.event == "on_change" || h.event == "on_select")
    {
        let stmts = h.body.clone();
        let env_c = env.clone();
        tree.set_on_index_change(node, move |idx| {
            let mut h_env = env_c.clone();
            let v = DynValue::Int(idx as i64);
            h_env.locals.insert("index".into(), v.clone());
            h_env.locals.insert("idx".into(), v.clone());
            h_env.locals.insert("selected".into(), v.clone());
            h_env.locals.insert("active".into(), v.clone());
            h_env.locals.insert("value".into(), v);
            run_stmts(&stmts, &h_env);
        });
    }
}

fn build_overlay_close_handler(
    el: &ElementNode,
    open_prop: Option<&PropBinding>,
    env: &DynEnv,
) -> Option<TapHandler> {
    if let Some(h) = make_handler_opt(el, &["on_close"], env, None) {
        return Some(h);
    }
    if let Some(op) = open_prop
        && op.two_way
        && let Some(dyn_sig) = env.find_prop_signal(&op.value)
    {
        return Some(Rc::new(RefCell::new(move || {
            dyn_sig.set(DynValue::Bool(false));
        })));
    }
    None
}

fn make_handler_opt(
    el: &ElementNode,
    event_names: &[&str],
    env: &DynEnv,
    extra_cb: Option<DynCallback>,
) -> Option<TapHandler> {
    let handler = el
        .handlers
        .iter()
        .find(|h| event_names.iter().any(|e| h.event == *e))?;
    let stmts = handler.body.clone();
    let env_c = env.clone();
    Some(Rc::new(RefCell::new(move || {
        run_stmts(&stmts, &env_c);
        if let Some(ref c) = extra_cb
            && let Ok(mut f) = c.try_borrow_mut()
        {
            f(Vec::new());
        }
    })))
}

enum DynLValueStep {
    Field(String),
    Index(usize),
}

fn extract_dyn_lvalue<'a>(
    expr: &'a Expr,
    env: &DynEnv,
    steps: &mut Vec<DynLValueStep>,
) -> Option<&'a str> {
    match expr {
        Expr::Ident(id) => Some(id.as_str()),
        Expr::Member(base, field) => {
            let root = extract_dyn_lvalue(base, env, steps)?;
            steps.push(DynLValueStep::Field(field.clone()));
            Some(root)
        }
        Expr::Index(base, idx_expr) => {
            let root = extract_dyn_lvalue(base, env, steps)?;
            let idx_val = eval_expr(idx_expr, env).as_i64();
            if idx_val < 0 {
                return None;
            }
            steps.push(DynLValueStep::Index(idx_val as usize));
            Some(root)
        }
        _ => None,
    }
}

fn apply_dyn_assign_leaf(slot: &mut DynValue, op: AssignOp, rhs: DynValue) {
    match op {
        AssignOp::Set => {
            *slot = rhs;
        }
        AssignOp::Add => match slot {
            DynValue::String(s) => s.push_str(&rhs.as_str()),
            DynValue::Int(a) => {
                if let DynValue::Int(b) = rhs {
                    *a += b;
                } else {
                    *slot = DynValue::Float((*a as f64) + (rhs.as_f32() as f64));
                }
            }
            DynValue::Float(a) => *a += rhs.as_f32() as f64,
            _ => *slot = rhs,
        },
        AssignOp::Sub => match slot {
            DynValue::Int(a) => {
                if let DynValue::Int(b) = rhs {
                    *a -= b;
                } else {
                    *slot = DynValue::Float((*a as f64) - (rhs.as_f32() as f64));
                }
            }
            DynValue::Float(a) => *a -= rhs.as_f32() as f64,
            _ => {}
        },
        AssignOp::Mul => match slot {
            DynValue::Int(a) => {
                if let DynValue::Int(b) = rhs {
                    *a *= b;
                } else {
                    *slot = DynValue::Float((*a as f64) * (rhs.as_f32() as f64));
                }
            }
            DynValue::Float(a) => *a *= rhs.as_f32() as f64,
            _ => {}
        },
        AssignOp::Div => match slot {
            DynValue::Int(a) => {
                let d = rhs.as_i64();
                if d != 0 {
                    *a /= d;
                }
            }
            DynValue::Float(a) => {
                let d = rhs.as_f32() as f64;
                if d.abs() > 1e-6 {
                    *a /= d;
                }
            }
            _ => {}
        },
    }
}

fn apply_dyn_nested(slot: &mut DynValue, steps: &[DynLValueStep], op: AssignOp, rhs: DynValue) {
    if steps.is_empty() {
        apply_dyn_assign_leaf(slot, op, rhs);
        return;
    }
    match &steps[0] {
        DynLValueStep::Field(f) => {
            if let DynValue::Struct { fields, .. } = slot
                && let Some(next) = fields.get_mut(f)
            {
                apply_dyn_nested(next, &steps[1..], op, rhs);
            }
        }
        DynLValueStep::Index(i) => {
            if let DynValue::List(items) = slot
                && let Some(next) = items.get_mut(*i)
            {
                apply_dyn_nested(next, &steps[1..], op, rhs);
            }
        }
    }
}

fn run_stmts(stmts: &[Stmt], env: &DynEnv) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { target, op, value } => {
                let mut steps = Vec::new();
                if let Some(root) = extract_dyn_lvalue(target, env, &mut steps)
                    && let Some(&sig) = env.props.get(root)
                {
                    let rhs = eval_expr(value, env);
                    sig.update(|slot| apply_dyn_nested(slot, &steps, *op, rhs));
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if eval_expr(condition, env).as_bool() {
                    run_stmts(then_branch, env);
                } else {
                    run_stmts(else_branch, env);
                }
            }
            Stmt::Call { callee, args } => {
                let eval_args: Vec<DynValue> = args.iter().map(|a| eval_expr(a, env)).collect();
                if let Some(cb) = env.callbacks.get(callee)
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(eval_args);
                }
            }
            Stmt::Expr(_) => {}
        }
    }
}

fn eval_iterable(expr: &Expr, env: &DynEnv, track: bool) -> Vec<DynValue> {
    match eval_expr_tracked(expr, env, track) {
        DynValue::List(items) => items,
        DynValue::Int(n) if n > 0 => (0..n.min(10_000)).map(DynValue::Int).collect(),
        _ => Vec::new(),
    }
}

fn expr_has_reactivity(expr: &Expr, env: &DynEnv) -> bool {
    match expr {
        Expr::Ident(name) => env.props.contains_key(name) || env.signal_locals.contains_key(name),
        Expr::Member(base, field) => {
            if let Expr::Ident(r) = base.as_ref() {
                if r == "Theme" || r == "Window" {
                    return true;
                }
                if r == "root" {
                    return env.props.contains_key(field) || env.signal_locals.contains_key(field);
                }
            }
            expr_has_reactivity(base, env)
        }
        Expr::Index(a, b) | Expr::Binary(a, _, b) => {
            expr_has_reactivity(a, env) || expr_has_reactivity(b, env)
        }
        Expr::Range { start, end, .. } => {
            expr_has_reactivity(start, env) || expr_has_reactivity(end, env)
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_reactivity(cond, env)
                || expr_has_reactivity(then_expr, env)
                || expr_has_reactivity(else_expr, env)
        }
        Expr::UnaryNot(i) | Expr::UnaryNeg(i) => expr_has_reactivity(i, env),
        Expr::Array(items) | Expr::Call(_, items) => {
            items.iter().any(|x| expr_has_reactivity(x, env))
        }
        Expr::MethodCall(recv, _, args) => {
            expr_has_reactivity(recv, env) || args.iter().any(|x| expr_has_reactivity(x, env))
        }
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| expr_has_reactivity(v, env)),
        _ => false,
    }
}

fn expr_to_reactive_string(expr: &Expr, env: &DynEnv) -> ReactiveValue<String> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_str()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_str())
    }
}

fn expr_to_reactive_bool(expr: &Expr, env: &DynEnv) -> ReactiveValue<bool> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_bool()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_bool())
    }
}

fn expr_to_reactive_string_list(expr: &Expr, env: &DynEnv) -> ReactiveValue<Vec<String>> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_string_list()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_string_list())
    }
}

fn expr_to_reactive_string_table(expr: &Expr, env: &DynEnv) -> ReactiveValue<Vec<Vec<String>>> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_string_table()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_string_table())
    }
}

fn expr_to_reactive_color(expr: &Expr, env: &DynEnv, fallback: Color) -> ReactiveValue<Color> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true)
                .as_color()
                .unwrap_or(fallback)
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_color().unwrap_or(fallback))
    }
}

fn dyn_value_to_toast_item(v: &DynValue) -> Option<ToastItem> {
    let DynValue::Struct { fields, .. } = v else {
        return None;
    };
    let id = fields
        .get("id")
        .map(|x| x.as_i64().max(0) as u64)
        .unwrap_or(0);
    let title = fields
        .get("title")
        .map(DynValue::as_str)
        .unwrap_or_default();
    let message = fields
        .get("message")
        .map(DynValue::as_str)
        .unwrap_or_default();
    let severity = match fields.get("severity") {
        Some(DynValue::Enum(_, s) | DynValue::String(s)) => match s.as_str() {
            "Success" => ToastSeverity::Success,
            "Warning" => ToastSeverity::Warning,
            "Error" => ToastSeverity::Error,
            _ => ToastSeverity::Info,
        },
        _ => ToastSeverity::Info,
    };
    let duration_ms = fields
        .get("duration_ms")
        .map(|x| x.as_i64().max(0) as u64)
        .unwrap_or(0);
    Some(ToastItem::new(id, title, message, severity, duration_ms))
}

fn dyn_value_to_toast_list(v: &DynValue) -> Vec<ToastItem> {
    match v {
        DynValue::List(items) => items.iter().filter_map(dyn_value_to_toast_item).collect(),
        _ => Vec::new(),
    }
}

fn emit_reactive_text_style(
    size_prop: Option<&Expr>,
    weight_prop: Option<&Expr>,
    color_expr: &Expr,
    env: &DynEnv,
) -> ReactiveValue<TextStyle> {
    let reactive = size_prop.is_some_and(|e| expr_has_reactivity(e, env))
        || weight_prop.is_some_and(|e| expr_has_reactivity(e, env))
        || expr_has_reactivity(color_expr, env);
    if reactive {
        let se = size_prop.cloned();
        let we = weight_prop.cloned();
        let ce = color_expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            let sz = se
                .as_ref()
                .map(|e| eval_expr_tracked(e, &env_c, true).as_f32())
                .unwrap_or(16.0);
            let wt = we
                .as_ref()
                .map(|e| eval_font_weight_tracked(e, &env_c, true))
                .unwrap_or(400.0);
            let col = eval_expr_tracked(&ce, &env_c, true)
                .as_color()
                .unwrap_or_else(|| env_c.theme_sig.get().text_primary);
            TextStyle::default()
                .with_size(sz)
                .with_weight(wt)
                .with_color(col)
        }))
    } else {
        let sz = size_prop
            .map(|e| eval_expr(e, env).as_f32())
            .unwrap_or(16.0);
        let wt = weight_prop
            .map(|e| eval_font_weight(e, env))
            .unwrap_or(400.0);
        let col = eval_expr(color_expr, env)
            .as_color()
            .unwrap_or_else(|| env.theme_sig.get_untracked().text_primary);
        ReactiveValue::Static(
            TextStyle::default()
                .with_size(sz)
                .with_weight(wt)
                .with_color(col),
        )
    }
}

fn eval_font_weight(expr: &Expr, env: &DynEnv) -> f32 {
    eval_font_weight_tracked(expr, env, false)
}

fn eval_font_weight_tracked(expr: &Expr, env: &DynEnv, track: bool) -> f32 {
    match expr {
        Expr::Ident(s) => match s.as_str() {
            "Thin" => 100.0,
            "ExtraLight" => 200.0,
            "Light" => 300.0,
            "Normal" | "Regular" => 400.0,
            "Medium" => 500.0,
            "SemiBold" | "Semibold" => 600.0,
            "Bold" => 700.0,
            "ExtraBold" => 800.0,
            "Black" => 900.0,
            _ => {
                let v = eval_expr_tracked(expr, env, track).as_f32();
                if v > 0.0 { v } else { 400.0 }
            }
        },
        other => eval_expr_tracked(other, env, track).as_f32().max(100.0),
    }
}

pub(crate) fn eval_expr(expr: &Expr, env: &DynEnv) -> DynValue {
    eval_expr_tracked(expr, env, false)
}

pub(crate) fn eval_expr_tracked(expr: &Expr, env: &DynEnv, track: bool) -> DynValue {
    match expr {
        Expr::Int(i) => DynValue::Int(*i),
        Expr::Float(f) => DynValue::Float(*f),
        Expr::Bool(b) => DynValue::Bool(*b),
        Expr::String(s) => DynValue::String(s.clone()),
        Expr::ColorHex(h) => DynValue::Color(Color::from_hex(h).unwrap_or(Color::rgb(255, 0, 255))),
        Expr::DurationSec(s) => DynValue::Float(*s as f64),
        Expr::Length(v, unit) => DynValue::Length(match unit {
            Unit::Dp => Length::Dp(*v),
            Unit::Px => Length::Px(*v),
            Unit::Pct => Length::Pct(*v),
            Unit::Deg | Unit::Rad => Length::Dp(*v),
        }),
        Expr::Array(items) => DynValue::List(
            items
                .iter()
                .map(|i| eval_expr_tracked(i, env, track))
                .collect(),
        ),
        Expr::Range {
            start,
            end,
            inclusive,
        } => {
            let s = eval_expr_tracked(start, env, track).as_i64();
            let e = eval_expr_tracked(end, env, track).as_i64();
            let items: Vec<DynValue> = if *inclusive {
                (s..=e).take(10_000).map(DynValue::Int).collect()
            } else {
                (s..e).take(10_000).map(DynValue::Int).collect()
            };
            DynValue::List(items)
        }
        Expr::StructLit { name, fields } => {
            let mut map = BTreeMap::new();
            if name == "ToastItem" {
                map.insert("id".into(), DynValue::Int(0));
                map.insert("title".into(), DynValue::String(String::new()));
                map.insert("message".into(), DynValue::String(String::new()));
                map.insert(
                    "severity".into(),
                    DynValue::Enum("ToastSeverity".into(), "Info".into()),
                );
                map.insert("duration_ms".into(), DynValue::Int(0));
            } else if let Some(st) = env.file.structs.iter().find(|s| s.name == *name) {
                for f in &st.fields {
                    let def = f
                        .default_value
                        .as_ref()
                        .map(|e| eval_expr_tracked(e, env, track))
                        .unwrap_or(DynValue::Null);
                    map.insert(f.name.clone(), def);
                }
            }
            for (k, v) in fields {
                map.insert(k.clone(), eval_expr_tracked(v, env, track));
            }
            DynValue::Struct {
                name: name.clone(),
                fields: map,
            }
        }
        Expr::Ident(name) => {
            if let Some(sig) = env.signal_locals.get(name).or_else(|| env.props.get(name)) {
                if track {
                    sig.get()
                } else {
                    sig.get_untracked()
                }
            } else if let Some(val) = env.locals.get(name) {
                val.clone()
            } else if name == "None" || name == "null" {
                DynValue::Null
            } else {
                DynValue::String(name.clone())
            }
        }
        Expr::Member(base, field) => {
            if let Expr::Ident(root) = base.as_ref() {
                if root == "root"
                    && let Some(sig) = env.props.get(field)
                {
                    return if track {
                        sig.get()
                    } else {
                        sig.get_untracked()
                    };
                }
                if root == "Math" {
                    return match field.as_str() {
                        "PI" => DynValue::Float(std::f64::consts::PI),
                        "E" => DynValue::Float(std::f64::consts::E),
                        "LN2" => DynValue::Float(std::f64::consts::LN_2),
                        "LN10" => DynValue::Float(std::f64::consts::LN_10),
                        "LOG2E" => DynValue::Float(std::f64::consts::LOG2_E),
                        "LOG10E" => DynValue::Float(std::f64::consts::LOG10_E),
                        "SQRT2" => DynValue::Float(std::f64::consts::SQRT_2),
                        _ => DynValue::Null,
                    };
                }
                if root == "Window" {
                    let m = if track {
                        env.metrics_sig.get()
                    } else {
                        env.metrics_sig.get_untracked()
                    };
                    return match field.as_str() {
                        "width" => DynValue::Float(m.logical_size.width as f64),
                        "height" => DynValue::Float(m.logical_size.height as f64),
                        "scale_factor" => DynValue::Float(m.scale_factor as f64),
                        "is_compact" => DynValue::Bool(m.logical_size.width < 600.0),
                        "is_medium" => DynValue::Bool(
                            m.logical_size.width >= 600.0 && m.logical_size.width <= 1024.0,
                        ),
                        "is_expanded" => DynValue::Bool(m.logical_size.width > 1024.0),
                        "breakpoint" => DynValue::String(format!("{:?}", m.breakpoint())),
                        _ => DynValue::Null,
                    };
                }
                if root == "Theme" {
                    let theme = if track {
                        env.theme_sig.get()
                    } else {
                        env.theme_sig.get_untracked()
                    };
                    if let Some(c) = theme.token_color(field) {
                        return DynValue::Color(c);
                    }
                    let ov = if track {
                        env.theme_ov_sig.get()
                    } else {
                        env.theme_ov_sig.get_untracked()
                    };
                    return DynValue::Color(ov.color(field).unwrap_or(theme.primary));
                }
                if root == "ThemeMode"
                    || root == "ToastSeverity"
                    || env.file.enums.iter().any(|e| e.name == *root)
                {
                    return DynValue::Enum(root.clone(), field.clone());
                }
            }
            match eval_expr_tracked(base, env, track) {
                DynValue::Struct { fields, .. } => {
                    fields.get(field).cloned().unwrap_or(DynValue::Null)
                }
                DynValue::List(v) if field == "len" => DynValue::Int(v.len() as i64),
                _ => DynValue::Null,
            }
        }
        Expr::Index(base, idx) => {
            let b = eval_expr_tracked(base, env, track);
            let i = eval_expr_tracked(idx, env, track).as_i64();
            if i < 0 {
                return DynValue::Null;
            }
            let iu = i as usize;
            match b {
                DynValue::List(items) => items.get(iu).cloned().unwrap_or(DynValue::Null),
                DynValue::String(s) => s
                    .chars()
                    .nth(iu)
                    .map(|c| DynValue::String(c.to_string()))
                    .unwrap_or(DynValue::Null),
                _ => DynValue::Null,
            }
        }
        Expr::MethodCall(recv, method, args) => {
            if let Expr::Ident(id) = recv.as_ref()
                && id == "Math"
            {
                let eval_args: Vec<DynValue> = args
                    .iter()
                    .map(|a| eval_expr_tracked(a, env, track))
                    .collect();
                return match method.as_str() {
                    "sin" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().sin() as f64)
                    }
                    "cos" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().cos() as f64)
                    }
                    "tan" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().tan() as f64)
                    }
                    "sqrt" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().sqrt() as f64)
                    }
                    "abs" if eval_args.len() == 1 => match eval_args[0] {
                        DynValue::Int(v) => DynValue::Int(v.abs()),
                        _ => DynValue::Float(eval_args[0].as_f32().abs() as f64),
                    },
                    "round" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().round() as f64)
                    }
                    "floor" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().floor() as f64)
                    }
                    "ceil" if eval_args.len() == 1 => {
                        DynValue::Float(eval_args[0].as_f32().ceil() as f64)
                    }
                    "min" if eval_args.len() == 2 => {
                        if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1])
                        {
                            DynValue::Int((*a).min(*b))
                        } else {
                            DynValue::Float(eval_args[0].as_f32().min(eval_args[1].as_f32()) as f64)
                        }
                    }
                    "max" if eval_args.len() == 2 => {
                        if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1])
                        {
                            DynValue::Int((*a).max(*b))
                        } else {
                            DynValue::Float(eval_args[0].as_f32().max(eval_args[1].as_f32()) as f64)
                        }
                    }
                    "pow" if eval_args.len() == 2 => {
                        DynValue::Float(eval_args[0].as_f32().powf(eval_args[1].as_f32()) as f64)
                    }
                    "clamp" if eval_args.len() == 3 => {
                        let lo = eval_args[1].as_f32();
                        let hi = eval_args[2].as_f32().max(lo);
                        DynValue::Float(eval_args[0].as_f32().clamp(lo, hi) as f64)
                    }
                    _ => DynValue::Null,
                };
            }
            let r = eval_expr_tracked(recv, env, track);
            let eval_args: Vec<DynValue> = args
                .iter()
                .map(|a| eval_expr_tracked(a, env, track))
                .collect();
            match (r, method.as_str()) {
                (DynValue::List(v), "len") => DynValue::Int(v.len() as i64),
                (DynValue::List(v), "is_empty") => DynValue::Bool(v.is_empty()),
                (DynValue::List(v), "contains") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| v.iter().any(|x| x == needle)),
                ),
                (DynValue::String(s), "len") => DynValue::Int(s.chars().count() as i64),
                (DynValue::String(s), "is_empty") => DynValue::Bool(s.is_empty()),
                (DynValue::String(s), "to_uppercase") => DynValue::String(s.to_uppercase()),
                (DynValue::String(s), "to_lowercase") => DynValue::String(s.to_lowercase()),
                (DynValue::String(s), "trim") => DynValue::String(s.trim().to_string()),
                (DynValue::String(s), "starts_with") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| s.starts_with(&needle.as_str())),
                ),
                (DynValue::String(s), "ends_with") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| s.ends_with(&needle.as_str())),
                ),
                (DynValue::String(s), "contains") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| s.contains(&needle.as_str())),
                ),
                (DynValue::Float(v), "sin") => DynValue::Float((v as f32).sin() as f64),
                (DynValue::Float(v), "cos") => DynValue::Float((v as f32).cos() as f64),
                (DynValue::Float(v), "tan") => DynValue::Float((v as f32).tan() as f64),
                (DynValue::Float(v), "sqrt") => DynValue::Float((v as f32).sqrt() as f64),
                (DynValue::Float(v), "abs") => DynValue::Float(v.abs()),
                (DynValue::Float(v), "round") => DynValue::Float(v.round()),
                (DynValue::Float(v), "floor") => DynValue::Float(v.floor()),
                (DynValue::Float(v), "ceil") => DynValue::Float(v.ceil()),
                (DynValue::Float(v), "min") if eval_args.len() == 1 => {
                    DynValue::Float(v.min(eval_args[0].as_f32() as f64))
                }
                (DynValue::Float(v), "max") if eval_args.len() == 1 => {
                    DynValue::Float(v.max(eval_args[0].as_f32() as f64))
                }
                (DynValue::Float(v), "pow") if eval_args.len() == 1 => {
                    DynValue::Float(v.powf(eval_args[0].as_f32() as f64))
                }
                (DynValue::Int(v), "abs") => DynValue::Int(v.abs()),
                _ => DynValue::Null,
            }
        }
        Expr::Call(name, args) => {
            let eval_args: Vec<DynValue> = args
                .iter()
                .map(|a| eval_expr_tracked(a, env, track))
                .collect();
            match name.as_str() {
                "Some" => eval_args.into_iter().next().unwrap_or(DynValue::Null),
                "min" if eval_args.len() == 2 => {
                    if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1]) {
                        DynValue::Int((*a).min(*b))
                    } else {
                        DynValue::Float((eval_args[0].as_f32().min(eval_args[1].as_f32())) as f64)
                    }
                }
                "max" if eval_args.len() == 2 => {
                    if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1]) {
                        DynValue::Int((*a).max(*b))
                    } else {
                        DynValue::Float((eval_args[0].as_f32().max(eval_args[1].as_f32())) as f64)
                    }
                }
                "clamp" if eval_args.len() == 3 => {
                    if let (DynValue::Int(v), DynValue::Int(lo), DynValue::Int(hi)) =
                        (&eval_args[0], &eval_args[1], &eval_args[2])
                    {
                        let hi_clamped = (*hi).max(*lo);
                        DynValue::Int((*v).clamp(*lo, hi_clamped))
                    } else {
                        let lo = eval_args[1].as_f32();
                        let hi = eval_args[2].as_f32().max(lo);
                        DynValue::Float(eval_args[0].as_f32().clamp(lo, hi) as f64)
                    }
                }
                "abs" if eval_args.len() == 1 => match eval_args[0] {
                    DynValue::Int(v) => DynValue::Int(v.abs()),
                    _ => DynValue::Float(eval_args[0].as_f32().abs() as f64),
                },
                "round" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().round() as f64)
                }
                "floor" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().floor() as f64)
                }
                "ceil" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().ceil() as f64)
                }
                "sin" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().sin() as f64)
                }
                "cos" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().cos() as f64)
                }
                "tan" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().tan() as f64)
                }
                "sqrt" if eval_args.len() == 1 => {
                    DynValue::Float(eval_args[0].as_f32().sqrt() as f64)
                }
                "pow" if eval_args.len() == 2 => {
                    DynValue::Float(eval_args[0].as_f32().powf(eval_args[1].as_f32()) as f64)
                }
                "@linear-gradient" | "linear_gradient" if eval_args.len() >= 2 => {
                    let angle_deg = eval_args[0].as_f32();
                    let rad = (angle_deg - 90.0).to_radians();
                    let dx = rad.cos();
                    let dy = rad.sin();
                    let start = Point::new(0.5 - 0.5 * dx, 0.5 - 0.5 * dy);
                    let end = Point::new(0.5 + 0.5 * dx, 0.5 + 0.5 * dy);
                    let color_args = &eval_args[1..];
                    let count = color_args.len();
                    let stops: Vec<craftui_core::ColorStop> = color_args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let offset = if count > 1 {
                                i as f32 / (count - 1) as f32
                            } else {
                                0.0
                            };
                            let color = a.as_color().unwrap_or(Color::BLACK);
                            craftui_core::ColorStop { offset, color }
                        })
                        .collect();
                    DynValue::Brush(Brush::LinearGradient { start, end, stops })
                }
                "@radial-gradient" | "radial_gradient" if eval_args.len() >= 2 => {
                    let radius = eval_args[0].as_f32().max(0.01);
                    let center = Point::new(0.5, 0.5);
                    let color_args = &eval_args[1..];
                    let count = color_args.len();
                    let stops: Vec<craftui_core::ColorStop> = color_args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let offset = if count > 1 {
                                i as f32 / (count - 1) as f32
                            } else {
                                0.0
                            };
                            let color = a.as_color().unwrap_or(Color::BLACK);
                            craftui_core::ColorStop { offset, color }
                        })
                        .collect();
                    DynValue::Brush(Brush::RadialGradient {
                        center,
                        radius,
                        stops,
                    })
                }
                "rgb" if eval_args.len() == 3 => DynValue::Color(Color::rgb(
                    eval_args[0].as_i64().clamp(0, 255) as u8,
                    eval_args[1].as_i64().clamp(0, 255) as u8,
                    eval_args[2].as_i64().clamp(0, 255) as u8,
                )),
                "rgba" if eval_args.len() == 4 => DynValue::Color(Color::rgba(
                    eval_args[0].as_i64().clamp(0, 255) as u8,
                    eval_args[1].as_i64().clamp(0, 255) as u8,
                    eval_args[2].as_i64().clamp(0, 255) as u8,
                    eval_args[3].as_i64().clamp(0, 255) as u8,
                )),
                _ => DynValue::Null,
            }
        }
        Expr::UnaryNot(inner) => DynValue::Bool(!eval_expr_tracked(inner, env, track).as_bool()),
        Expr::UnaryNeg(inner) => match eval_expr_tracked(inner, env, track) {
            DynValue::Int(v) => DynValue::Int(-v),
            DynValue::Float(v) => DynValue::Float(-v),
            DynValue::Length(Length::Dp(v)) => DynValue::Length(Length::Dp(-v)),
            DynValue::Length(Length::Px(v)) => DynValue::Length(Length::Px(-v)),
            DynValue::Length(Length::Pct(v)) => DynValue::Length(Length::Pct(-v)),
            other => DynValue::Float(-(other.as_f32() as f64)),
        },
        Expr::Binary(l, op, r) => match op {
            BinOp::And => {
                let lb = eval_expr_tracked(l, env, track).as_bool();
                if !lb {
                    DynValue::Bool(false)
                } else {
                    DynValue::Bool(eval_expr_tracked(r, env, track).as_bool())
                }
            }
            BinOp::Or => {
                let lb = eval_expr_tracked(l, env, track).as_bool();
                if lb {
                    DynValue::Bool(true)
                } else {
                    DynValue::Bool(eval_expr_tracked(r, env, track).as_bool())
                }
            }
            _ => {
                let lv = eval_expr_tracked(l, env, track);
                let rv = eval_expr_tracked(r, env, track);
                match op {
                    BinOp::Eq => DynValue::Bool(lv == rv),
                    BinOp::Ne => DynValue::Bool(lv != rv),
                    BinOp::Lt => DynValue::Bool(lv.as_f32() < rv.as_f32()),
                    BinOp::Gt => DynValue::Bool(lv.as_f32() > rv.as_f32()),
                    BinOp::Le => DynValue::Bool(lv.as_f32() <= rv.as_f32()),
                    BinOp::Ge => DynValue::Bool(lv.as_f32() >= rv.as_f32()),
                    BinOp::Add => match (&lv, &rv) {
                        (DynValue::String(a), _) => DynValue::String(format!("{a}{}", rv.as_str())),
                        (_, DynValue::String(b)) => DynValue::String(format!("{}{b}", lv.as_str())),
                        (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a + b),
                        _ => DynValue::Float((lv.as_f32() + rv.as_f32()) as f64),
                    },
                    BinOp::Sub => match (&lv, &rv) {
                        (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a - b),
                        _ => DynValue::Float((lv.as_f32() - rv.as_f32()) as f64),
                    },
                    BinOp::Mul => match (&lv, &rv) {
                        (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a * b),
                        _ => DynValue::Float((lv.as_f32() * rv.as_f32()) as f64),
                    },
                    BinOp::Div => {
                        let d = rv.as_f32();
                        DynValue::Float(if d.abs() > 1e-6 {
                            (lv.as_f32() / d) as f64
                        } else {
                            0.0
                        })
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                }
            }
        },
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            if eval_expr_tracked(cond, env, track).as_bool() {
                eval_expr_tracked(then_expr, env, track)
            } else {
                eval_expr_tracked(else_expr, env, track)
            }
        }
    }
}
