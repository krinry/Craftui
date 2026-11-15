//! Dynamic `.cui` AST/IR interpreter enabling `< 100ms` visual hot-reload without Rust recompilation.

use craftui_core::{
    Alignment, Axis, BlendMode, Border, BoxShadow, Brush, Color, CornerRadii, CraftError,
    CursorIcon, DirtyFlags, Fit, Insets, Justify, Length, NodeId, PlatformViewId, Point, Result,
    TextureId, ThemeOverride, ThemeTokens, Transform2D, WindowMetrics,
};
use craftui_dsl::{
    AnimateSpec, AssignOp, BinOp, ChildItem, ComponentDecl, CuiFile, Diagnostic, ElementNode, Expr,
    PropBinding, Severity, Stmt, Unit, resolve_file, resolve_source,
};
use craftui_layout::LayoutStyle;
use craftui_reactive::{Scope, Signal};
use craftui_text::TextStyle;
use craftui_widgets::{
    AnimatableProp, Easing, IndexCallback, ReactiveValue, TapHandler, ToastItem, ToastSeverity,
    TransitionCurve, VirtualItemFactory, WidgetTree,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Dynamically typed value inside interpreted `.cui` components.
#[derive(Debug, Clone, PartialEq)]
pub enum DynValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Color(Color),
    Length(Length),
    Texture(Option<TextureId>),
    Brush(Brush),
    List(Vec<DynValue>),
    Struct {
        name: String,
        fields: BTreeMap<String, DynValue>,
    },
    Enum(String, String),
    Null,
}

impl DynValue {
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Float(v) => *v as f32,
            Self::Int(v) => *v as f32,
            Self::Length(Length::Dp(v) | Length::Px(v) | Length::Pct(v)) => *v,
            _ => 0.0,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Int(v) => *v,
            Self::Float(v) => *v as i64,
            Self::Bool(b) => i64::from(*b),
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::List(v) => !v.is_empty(),
            _ => false,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => {
                if (f.fract()).abs() < 1e-6 {
                    format!("{f:.1}")
                } else {
                    format!("{f}")
                }
            }
            Self::Bool(b) => b.to_string(),
            Self::Color(c) => c.to_hex(),
            Self::Enum(_, v) => v.clone(),
            Self::Struct { name, fields } => {
                let mut out = format!("{name} {{ ");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{k}: {}", v.as_str());
                }
                out.push_str(" }");
                out
            }
            Self::List(items) => {
                let joined: Vec<String> = items.iter().map(DynValue::as_str).collect();
                format!("[{}]", joined.join(", "))
            }
            Self::Length(Length::Dp(v)) => format!("{v}dp"),
            Self::Length(Length::Px(v)) => format!("{v}px"),
            Self::Length(Length::Pct(v)) => format!("{v}%"),
            Self::Length(Length::Auto) => "auto".to_string(),
            Self::Texture(Some(id)) => format!("{id:?}"),
            Self::Texture(None) | Self::Null => String::new(),
            Self::Brush(_) => "brush".to_string(),
        }
    }

    pub fn as_length(&self) -> Length {
        match self {
            Self::Length(l) => *l,
            Self::Float(v) => Length::Dp(*v as f32),
            Self::Int(v) => Length::Dp(*v as f32),
            _ => Length::Auto,
        }
    }

    pub fn as_brush(&self) -> Option<Brush> {
        match self {
            Self::Brush(b) => Some(b.clone()),
            Self::Color(c) => Some(Brush::Solid(*c)),
            Self::String(s) => Color::from_hex(s).map(Brush::Solid),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<Color> {
        match self {
            Self::Color(c) => Some(*c),
            Self::Brush(Brush::Solid(c)) => Some(*c),
            Self::String(s) => Color::from_hex(s),
            _ => None,
        }
    }

    pub fn as_string_list(&self) -> Vec<String> {
        match self {
            Self::List(items) => items.iter().map(DynValue::as_str).collect(),
            _ => Vec::new(),
        }
    }

    pub fn as_string_table(&self) -> Vec<Vec<String>> {
        match self {
            Self::List(rows) => rows
                .iter()
                .map(|row| match row {
                    Self::List(cells) => cells.iter().map(DynValue::as_str).collect(),
                    other => vec![other.as_str()],
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn key_string(&self) -> String {
        match self {
            Self::Int(i) => format!("i:{i}"),
            Self::Float(f) => format!("f:{f:.4}"),
            Self::Bool(b) => format!("b:{b}"),
            Self::String(s) => format!("s:{s}"),
            Self::Color(c) => format!("c:{}", c.to_hex()),
            Self::Enum(n, v) => format!("e:{n}.{v}"),
            Self::Struct { name, fields } => {
                let mut s = format!("st:{name}{{");
                for (k, v) in fields {
                    let _ = write!(s, "{k}:{},", v.key_string());
                }
                s.push('}');
                s
            }
            Self::List(items) => {
                let parts: Vec<String> = items.iter().map(DynValue::key_string).collect();
                format!("l:[{}]", parts.join(","))
            }
            Self::Texture(id) => format!("t:{id:?}"),
            Self::Length(l) => format!("len:{l:?}"),
            Self::Brush(_) => "brush".to_string(),
            Self::Null => "null".to_string(),
        }
    }
}

pub type DynCallback = Rc<RefCell<dyn FnMut(Vec<DynValue>)>>;

/// Live state store preserved across `.cui` hot-reloads.
pub struct DynamicStore {
    pub props: HashMap<String, Signal<DynValue>>,
    pub callbacks: HashMap<String, DynCallback>,
}

impl Default for DynamicStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicStore {
    pub fn new() -> Self {
        Self {
            props: HashMap::new(),
            callbacks: HashMap::new(),
        }
    }

    pub fn get_or_create_prop(
        &mut self,
        scope: Scope,
        name: &str,
        initial: DynValue,
    ) -> Signal<DynValue> {
        if let Some(&sig) = self.props.get(name)
            && sig.is_alive()
        {
            return sig;
        }
        let sig = scope.signal(initial);
        self.props.insert(name.to_string(), sig);
        sig
    }

    pub fn set_prop(&mut self, scope: Scope, name: &str, value: DynValue) {
        let sig = self.get_or_create_prop(scope, name, value.clone());
        sig.set(value);
    }

    pub fn register_callback(
        &mut self,
        name: impl Into<String>,
        cb: impl FnMut(Vec<DynValue>) + 'static,
    ) {
        self.callbacks
            .insert(name.into(), Rc::new(RefCell::new(cb)));
    }
}

/// Manages hot-reloading a `.cui` template into a live [`WidgetTree`].
pub struct HotReloadSession {
    pub state_scope: Scope,
    pub mount_scope: Option<Scope>,
    pub store: DynamicStore,
    pub last_source: String,
    pub last_reload_duration: Duration,
    pub last_diagnostics: Vec<Diagnostic>,
}

impl HotReloadSession {
    pub fn new(root_scope: Scope) -> Self {
        Self {
            state_scope: root_scope.child_scope(),
            mount_scope: None,
            store: DynamicStore::new(),
            last_source: String::new(),
            last_reload_duration: Duration::ZERO,
            last_diagnostics: Vec::new(),
        }
    }

    /// Parse, validate, and rebuild an inline `.cui` UI subtree while preserving live signal state.
    pub fn reload(
        &mut self,
        source: &str,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.reload_with_base(source, None, component_name, tree)
    }

    /// Parse, resolve relative `import` modules on disk, validate, and hot-reload a `.cui` file.
    pub fn reload_file(
        &mut self,
        path: impl AsRef<Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let path_ref = path.as_ref();
        let src = std::fs::read_to_string(path_ref).map_err(|e| {
            CraftError::Runtime(format!("failed reading {}: {e}", path_ref.display()))
        })?;
        let t0 = Instant::now();
        let resolved = resolve_file(path_ref);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = src;
        self.last_reload_duration = t0.elapsed();
        Ok(root)
    }

    /// Parse, resolve imports relative to `base_dir` (if provided), validate, and rebuild the subtree.
    pub fn reload_with_base(
        &mut self,
        source: &str,
        base_dir: Option<&Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let t0 = Instant::now();
        let resolved = resolve_source(source, base_dir);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = source.to_string();
        self.last_reload_duration = t0.elapsed();
        Ok(root)
    }

    fn mount_resolved(
        &mut self,
        merged: CuiFile,
        diags: Vec<Diagnostic>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.last_diagnostics = diags.clone();
        if diags.iter().any(|d| d.severity == Severity::Error) {
            let msg = diags
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CraftError::Runtime(msg));
        }

        let comp = if let Some(name) = component_name {
            merged
                .components
                .iter()
                .find(|c| c.name == name)
                .or_else(|| merged.components.last())
        } else {
            merged.components.last()
        }
        .ok_or_else(|| CraftError::Runtime("no component found in .cui file".into()))?
        .clone();

        // Capture UI focus/caret/scroll snapshot BEFORE tearing down old subtree
        let ui_snap = tree.capture_ui_state();

        // Tear down old mounted tree nodes if present
        if let Some(old_root) = tree.root.take() {
            tree.remove_subtree(old_root);
        }
        if let Some(old_scope) = self.mount_scope.take() {
            old_scope.dispose();
        }

        let mount_scope = self.state_scope.child_scope();
        self.mount_scope = Some(mount_scope);

        let file_rc = Rc::new(merged);
        let base_env = DynEnv {
            file: file_rc,
            props: self.store.props.clone(),
            signal_locals: HashMap::new(),
            callbacks: self.store.callbacks.clone(),
            locals: HashMap::new(),
            theme_sig: tree.theme_signal,
            theme_ov_sig: tree.theme_override_signal,
            metrics_sig: tree.metrics_signal,
        };

        // First pass: ensure all component properties exist in preserved DynamicStore
        for prop in &comp.properties {
            let env_cur = DynEnv {
                props: self.store.props.clone(),
                ..base_env.clone()
            };
            let init = prop
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, &env_cur))
                .unwrap_or(DynValue::Null);
            self.store
                .get_or_create_prop(self.state_scope, &prop.name, init);
        }

        let full_env = DynEnv {
            props: self.store.props.clone(),
            ..base_env.clone()
        };

        // Second pass: install reactive Window effects with full component properties map
        for prop in &comp.properties {
            if let Some(ref def_expr) = prop.default_value
                && expr_references_window(def_expr)
            {
                let sig_c = self.store.props.get(&prop.name).copied().unwrap();
                let expr_c = def_expr.clone();
                let env_c = full_env.clone();
                mount_scope.effect(move || {
                    let v = eval_expr_tracked(&expr_c, &env_c, true);
                    sig_c.set(v);
                });
            }
        }

        let root_env = DynEnv {
            props: self.store.props.clone(),
            ..base_env
        };

        let root_node = if let Some(ref root_el) = comp.root {
            build_element(mount_scope, root_el, &root_env, tree)
        } else {
            tree.column(mount_scope)
        };

        tree.set_root(root_node);
        tree.restore_ui_state(&ui_snap);
        Ok(root_node)
    }
}

#[derive(Clone)]
struct DynEnv {
    file: Rc<CuiFile>,
    props: HashMap<String, Signal<DynValue>>,
    signal_locals: HashMap<String, Signal<DynValue>>,
    callbacks: HashMap<String, DynCallback>,
    locals: HashMap<String, DynValue>,
    theme_sig: Signal<ThemeTokens>,
    theme_ov_sig: Signal<ThemeOverride>,
    metrics_sig: Signal<WindowMetrics>,
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

fn expr_references_window(expr: &Expr) -> bool {
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

fn build_element(scope: Scope, el: &ElementNode, env: &DynEnv, tree: &mut WidgetTree) -> NodeId {
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
    };

    let node = if let Some(ref root) = comp.root {
        build_element(scope, root, &sub_env, tree)
    } else {
        tree.column(scope)
    };
    build_children(scope, node, call_el, outer_env, tree);
    apply_common_props_and_events(scope, node, call_el, outer_env, tree, true);
    node
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

fn eval_expr(expr: &Expr, env: &DynEnv) -> DynValue {
    eval_expr_tracked(expr, env, false)
}

fn eval_expr_tracked(expr: &Expr, env: &DynEnv, track: bool) -> DynValue {
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

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{
        Modifiers, Point, PointerButton, PointerEvent, PointerId, PointerPhase, Size, ThemeMode,
        UiEvent, WindowMetrics,
    };
    use craftui_reactive::reset_runtime;
    use craftui_widgets::WidgetKind;
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
                assert!((*size - 26.0).abs() < 0.01);
                assert!((*thickness - 3.0).abs() < 0.01);
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
                assert!(!*striped);
                assert!((*row_height - 36.0).abs() < 0.01);
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
}
