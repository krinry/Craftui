use crate::ast::*;
use craftui_core::Color;
use std::collections::{HashMap, HashSet};

pub const BUILTIN_WIDGETS: &[&str] = &[
    "View",
    "Container",
    "Column",
    "Row",
    "Grid",
    "Stack",
    "Spacer",
    "SafeArea",
    "Divider",
    "Text",
    "Button",
    "ImageView",
    "Image",
    "Icon",
    "Canvas",
    "ScrollView",
    "VirtualList",
    "VirtualGrid",
    "TextInput",
    "TextArea",
    "Checkbox",
    "Switch",
    "Slider",
    "Tabs",
    "TabBar",
    "SegmentedControl",
    "BottomNavBar",
    "FloatingActionButton",
    "Accordion",
    "DataTable",
    "Select",
    "Dropdown",
    "RadioGroup",
    "PopupMenu",
    "Tooltip",
    "ProgressIndicator",
    "Spinner",
    "Badge",
    "SplitPane",
    "ToastHost",
    "Modal",
    "BottomSheet",
    "PlatformView",
    "HtmlView",
    "Card",
    "ElevatedCard",
    "FilledCard",
    "OutlinedCard",
    "ListTile",
    "Chip",
    "FilterChip",
    "ActionChip",
    "ChoiceChip",
    "InputChip",
    "FilledButton",
    "ElevatedButton",
    "TonalButton",
    "FilledTonalButton",
    "OutlinedButton",
    "TextButton",
    "IconButton",
    "FilledIconButton",
    "TonalIconButton",
    "OutlinedIconButton",
    "SegmentedButton",
    "CircularProgressIndicator",
    "LinearProgressIndicator",
    "HorizontalDivider",
    "VerticalDivider",
    "CheckBoxTile",
    "RadioButtonTile",
    "SnackBar",
    "Dialog",
    "AppBar",
    "Drawer",
    "Scaffold",
    "div",
    "span",
    "p",
    "h1",
    "h2",
    "h3",
    "button",
    "input",
    "nav",
    "section",
    "header",
    "footer",
    "main",
    "article",
    "aside",
];

pub const COMMON_PROPS: &[&str] = &[
    "class",
    "width",
    "height",
    "min_width",
    "max_width",
    "min_height",
    "max_height",
    "flex_grow",
    "flex_shrink",
    "padding",
    "padding_x",
    "padding_y",
    "margin",
    "top",
    "bottom",
    "left",
    "right",
    "absolute",
    "safe_area",
    "visible",
    "background",
    "border_radius",
    "border_width",
    "border_color",
    "opacity",
    "clip",
    "shadow",
    "backdrop_blur",
    "backdrop_tint",
    "blend_mode",
    "cursor",
    "scale",
    "scale_x",
    "scale_y",
    "rotate",
    "rotation",
    "translate_x",
    "translate_y",
    "skew_x",
    "skew_y",
    "disabled",
    "id",
];

pub const CONTAINER_PROPS: &[&str] = &["gap", "align", "justify"];

pub const ANIMATABLE_PROPS: &[&str] = &[
    "opacity",
    "translate_x",
    "translate_y",
    "scale",
    "scale_x",
    "scale_y",
    "rotate",
    "rotation",
    "background",
    "border_color",
];

pub const BUILTIN_PROPS: &[&str] = &[
    "width",
    "height",
    "min_width",
    "max_width",
    "min_height",
    "max_height",
    "flex_grow",
    "flex_shrink",
    "padding",
    "padding_x",
    "padding_y",
    "margin",
    "top",
    "bottom",
    "left",
    "right",
    "absolute",
    "gap",
    "align",
    "justify",
    "background",
    "foreground",
    "color",
    "border_radius",
    "border_width",
    "border_color",
    "opacity",
    "clip",
    "shadow",
    "backdrop_blur",
    "backdrop_tint",
    "blend_mode",
    "cursor",
    "scale",
    "scale_x",
    "scale_y",
    "rotate",
    "rotation",
    "translate_x",
    "translate_y",
    "skew_x",
    "skew_y",
    "text",
    "content",
    "font_size",
    "font_weight",
    "source",
    "svg",
    "icon",
    "name",
    "size",
    "fit",
    "value",
    "min",
    "max",
    "checked",
    "placeholder",
    "multiline",
    "direction",
    "thickness",
    "title",
    "active",
    "selected",
    "tabs",
    "options",
    "items",
    "columns",
    "rows",
    "striped",
    "row_height",
    "expanded",
    "model",
    "item_count",
    "count",
    "item_extent",
    "item_height",
    "label",
    "open",
    "progress",
    "ratio",
    "min_first",
    "min_second",
    "divider_size",
    "safe_area",
    "visible",
    "disabled",
    "view_id",
    "key",
    "id",
];

pub const BUILTIN_EVENTS: &[&str] = &[
    "on_tap",
    "on_click",
    "on_change",
    "on_submit",
    "on_close",
    "on_dismiss",
    "on_select",
    "on_toggle",
    "on_sort",
    "on_hover",
    "on_double_tap",
    "on_long_press",
];

const COMMON_EVENTS: &[&str] = &[
    "on_tap",
    "on_click",
    "on_double_tap",
    "on_long_press",
    "on_hover",
];

pub fn widget_allows_children(kind: &str) -> bool {
    matches!(
        kind,
        "View"
            | "Container"
            | "Column"
            | "Row"
            | "Grid"
            | "Stack"
            | "SafeArea"
            | "ScrollView"
            | "VirtualList"
            | "VirtualGrid"
            | "Modal"
            | "BottomSheet"
            | "SplitPane"
            | "Accordion"
            | "Button"
            | "FilledButton"
            | "ElevatedButton"
            | "TonalButton"
            | "FilledTonalButton"
            | "OutlinedButton"
            | "TextButton"
            | "IconButton"
            | "FilledIconButton"
            | "TonalIconButton"
            | "OutlinedIconButton"
            | "FloatingActionButton"
            | "div"
            | "section"
            | "nav"
            | "header"
            | "footer"
            | "main"
            | "article"
            | "aside"
            | "span"
            | "button"
            | "Card"
            | "ElevatedCard"
            | "FilledCard"
            | "OutlinedCard"
            | "Dialog"
            | "Drawer"
            | "Scaffold"
    )
}

pub fn props_for_widget(kind: &str) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(COMMON_PROPS.len() + 10);
    out.extend_from_slice(COMMON_PROPS);
    match kind {
        "View" | "Container" | "Column" | "Row" | "Stack" | "SafeArea" => {
            out.extend_from_slice(CONTAINER_PROPS);
        }
        "Grid" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.push("columns");
        }
        "ScrollView" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.push("direction");
        }
        "VirtualList" | "VirtualGrid" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "direction",
                "columns",
                "item_extent",
                "item_height",
                "item_count",
                "count",
                "model",
                "items",
            ]);
        }
        "Button" | "button" | "FilledButton" | "ElevatedButton" | "TonalButton"
        | "FilledTonalButton" | "OutlinedButton" | "TextButton" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "text",
                "title",
                "label",
                "foreground",
                "color",
                "font_size",
                "font_weight",
                "disabled",
            ]);
        }
        "IconButton" | "FilledIconButton" | "TonalIconButton" | "OutlinedIconButton" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "icon",
                "name",
                "svg",
                "text",
                "title",
                "label",
                "size",
                "font_size",
                "color",
                "foreground",
                "disabled",
            ]);
        }
        "Modal" | "BottomSheet" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.push("open");
        }
        "Spacer" => {}
        "Divider" | "HorizontalDivider" | "VerticalDivider" => {
            out.extend_from_slice(&["direction", "thickness", "color", "foreground"]);
        }
        "Text" => {
            out.extend_from_slice(&[
                "text",
                "title",
                "content",
                "font_size",
                "font_weight",
                "color",
                "foreground",
            ]);
        }
        "Icon" => {
            out.extend_from_slice(&[
                "svg",
                "icon",
                "name",
                "source",
                "size",
                "font_size",
                "color",
                "foreground",
            ]);
        }
        "ImageView" | "Image" => {
            out.extend_from_slice(&["source", "fit"]);
        }
        "Canvas" => {}
        "Slider" => {
            out.extend_from_slice(&["value", "min", "max", "label"]);
        }
        "TextInput" | "TextArea" => {
            out.extend_from_slice(&[
                "value",
                "text",
                "placeholder",
                "multiline",
                "font_size",
                "font_weight",
                "color",
                "foreground",
            ]);
        }
        "Checkbox" | "Switch" => {
            out.extend_from_slice(&["checked", "value", "label", "text"]);
        }
        "Tabs" | "TabBar" | "SegmentedControl" => {
            out.extend_from_slice(&["tabs", "items", "options", "active", "selected", "value"]);
        }
        "BottomNavBar" => {
            out.extend_from_slice(&["items", "options", "icons", "active", "selected", "value"]);
        }
        "FloatingActionButton" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "icon",
                "label",
                "text",
                "disabled",
                "foreground",
                "background",
                "color",
            ]);
        }
        "Accordion" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&["title", "label", "text", "expanded", "open"]);
        }
        "DataTable" => {
            out.extend_from_slice(&[
                "columns",
                "rows",
                "items",
                "selected",
                "striped",
                "row_height",
            ]);
        }
        "Select" | "Dropdown" => {
            out.extend_from_slice(&[
                "options",
                "items",
                "selected",
                "value",
                "placeholder",
                "label",
            ]);
        }
        "RadioGroup" => {
            out.extend_from_slice(&["options", "items", "selected", "value", "direction"]);
        }
        "PopupMenu" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&["open", "items", "options"]);
        }
        "Tooltip" => {
            out.extend_from_slice(&["text", "label", "content"]);
        }
        "ProgressIndicator" | "LinearProgressIndicator" => {
            out.extend_from_slice(&["progress", "value", "color", "foreground"]);
        }
        "Spinner" | "CircularProgressIndicator" => {
            out.extend_from_slice(&["size", "thickness", "color", "foreground"]);
        }
        "Badge" => {
            out.extend_from_slice(&["label", "text", "icon", "color", "foreground"]);
        }
        "SplitPane" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "direction",
                "ratio",
                "min_first",
                "min_second",
                "divider_size",
            ]);
        }
        "ToastHost" => {
            out.push("items");
        }
        "PlatformView" => {
            out.push("view_id");
        }
        "HtmlView" => {
            out.extend_from_slice(&["src", "html", "url"]);
        }
        "Card" | "ElevatedCard" | "FilledCard" | "OutlinedCard" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.push("variant");
        }
        "ListTile" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&["title", "subtitle", "leading", "trailing", "selected"]);
        }
        "Chip" | "FilterChip" | "ActionChip" | "ChoiceChip" | "InputChip" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&[
                "label",
                "text",
                "selected",
                "value",
                "icon",
                "leading_icon",
                "disabled",
            ]);
        }
        "AppBar" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&["title", "leading", "leading_icon", "elevation"]);
        }
        "Drawer" => {
            out.extend_from_slice(CONTAINER_PROPS);
            out.extend_from_slice(&["open", "width"]);
        }
        "Scaffold" => {
            out.extend_from_slice(CONTAINER_PROPS);
        }
        "div" | "section" | "nav" | "header" | "footer" | "main" | "article" | "aside" => {
            out.extend_from_slice(CONTAINER_PROPS);
        }
        "span" | "p" | "h1" | "h2" | "h3" => {
            out.extend_from_slice(&["text", "content", "color", "font_size", "font_weight"]);
        }
        "input" => {
            out.extend_from_slice(&["text", "value", "placeholder"]);
        }
        _ => {}
    }
    out
}

pub fn events_for_widget(kind: &str) -> Vec<&'static str> {
    let mut out = COMMON_EVENTS.to_vec();
    match kind {
        "Slider" | "Checkbox" | "Switch" | "SplitPane" => out.push("on_change"),
        "TextInput" | "TextArea" | "input" => out.extend_from_slice(&["on_change", "on_submit"]),
        "Tabs" | "TabBar" | "SegmentedControl" | "BottomNavBar" | "Select" | "Dropdown"
        | "RadioGroup" => {
            out.extend_from_slice(&["on_change", "on_select"]);
        }
        "Accordion" => out.extend_from_slice(&["on_change", "on_toggle"]),
        "DataTable" => out.extend_from_slice(&["on_select", "on_change", "on_sort"]),
        "PopupMenu" => out.extend_from_slice(&["on_select", "on_change", "on_close"]),
        "ToastHost" => out.push("on_dismiss"),
        "Modal" | "BottomSheet" | "Drawer" => out.push("on_close"),
        "AppBar" => out.push("on_leading_tap"),
        _ => {}
    }
    out
}

fn widget_supports_two_way(kind: &str, prop: &str) -> bool {
    match kind {
        "TextInput" | "TextArea" | "input" => matches!(prop, "value" | "text"),
        "Slider" => prop == "value",
        "SplitPane" => prop == "ratio",
        "Checkbox" | "Switch" => matches!(prop, "checked" | "value"),
        "Tabs" | "TabBar" | "SegmentedControl" | "BottomNavBar" => {
            matches!(prop, "active" | "selected" | "value")
        }
        "Accordion" => matches!(prop, "expanded" | "open"),
        "DataTable" => prop == "selected",
        "Select" | "Dropdown" | "RadioGroup" => matches!(prop, "selected" | "value"),
        "Modal" | "BottomSheet" | "PopupMenu" | "Drawer" => matches!(prop, "open" | "visible"),
        "Chip" | "FilterChip" | "ActionChip" | "ChoiceChip" | "InputChip" => {
            matches!(prop, "selected" | "value")
        }
        _ => false,
    }
}

pub fn validate(file: &CuiFile) -> Vec<Diagnostic> {
    let mut imported_symbols = HashSet::new();
    for imp in &file.imports {
        for sym in &imp.symbols {
            imported_symbols.insert(sym.clone());
        }
    }
    validate_file_with_symbols(file, file, &imported_symbols)
}

struct ScopeCtx<'a> {
    struct_map: &'a HashMap<&'a str, &'a StructDecl>,
    enum_map: &'a HashMap<&'a str, &'a EnumDecl>,
    comp_map: &'a HashMap<&'a str, &'a ComponentDecl>,
    builtin_widgets: &'a HashSet<&'a str>,
    known_components: &'a HashSet<String>,
    imported_symbols: &'a HashSet<String>,
    comp_name: &'a str,
    comp_props: &'a HashMap<&'a str, (&'a TypeExpr, PropertyDir)>,
    comp_cbs: &'a HashMap<&'a str, usize>,
    locals: HashMap<String, TypeExpr>,
    in_for_loop: bool,
}

pub fn validate_file_with_symbols(
    file: &CuiFile,
    merged: &CuiFile,
    imported_symbols: &HashSet<String>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let builtin_widgets: HashSet<&str> = BUILTIN_WIDGETS.iter().copied().collect();

    let mut known_components: HashSet<String> =
        BUILTIN_WIDGETS.iter().map(|s| (*s).to_string()).collect();
    known_components.extend(imported_symbols.iter().cloned());

    let struct_map: HashMap<&str, &StructDecl> = merged
        .structs
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();
    let enum_map: HashMap<&str, &EnumDecl> =
        merged.enums.iter().map(|e| (e.name.as_str(), e)).collect();
    let comp_map: HashMap<&str, &ComponentDecl> = merged
        .components
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    for comp in &merged.components {
        known_components.insert(comp.name.clone());
    }

    // Validate structs
    let mut seen_structs = HashSet::new();
    for st in &file.structs {
        if !seen_structs.insert(st.name.clone()) {
            diags.push(Diagnostic {
                span: st.span,
                severity: Severity::Error,
                message: format!("duplicate struct '{}'", st.name),
            });
        }
        let mut field_names = HashSet::new();
        for f in &st.fields {
            if !field_names.insert(f.name.clone()) {
                diags.push(Diagnostic {
                    span: f.span,
                    severity: Severity::Error,
                    message: format!("duplicate field '{}' in struct '{}'", f.name, st.name),
                });
            }
            if let Some(ref def) = f.default_value {
                let empty_props = HashMap::new();
                let empty_cbs = HashMap::new();
                let ctx = ScopeCtx {
                    struct_map: &struct_map,
                    enum_map: &enum_map,
                    comp_map: &comp_map,
                    builtin_widgets: &builtin_widgets,
                    known_components: &known_components,
                    imported_symbols,
                    comp_name: "",
                    comp_props: &empty_props,
                    comp_cbs: &empty_cbs,
                    locals: HashMap::new(),
                    in_for_loop: false,
                };
                validate_expr(def, None, f.span, &ctx, &mut diags);
            }
        }
    }

    // Validate enums
    let mut seen_enums = HashSet::new();
    for en in &file.enums {
        if !seen_enums.insert(en.name.clone()) {
            diags.push(Diagnostic {
                span: en.span,
                severity: Severity::Error,
                message: format!("duplicate enum '{}'", en.name),
            });
        }
        let mut var_names = HashSet::new();
        for v in &en.variants {
            if !var_names.insert(v.clone()) {
                diags.push(Diagnostic {
                    span: en.span,
                    severity: Severity::Error,
                    message: format!("duplicate variant '{}' in enum '{}'", v, en.name),
                });
            }
        }
    }

    for comp in &file.components {
        if builtin_widgets.contains(comp.name.as_str()) {
            diags.push(Diagnostic {
                span: comp.span,
                severity: Severity::Error,
                message: format!("component '{}' cannot shadow built-in widget", comp.name),
            });
        }

        let mut comp_props: HashMap<&str, (&TypeExpr, PropertyDir)> = HashMap::new();
        let empty_props = HashMap::new();
        let empty_cbs = HashMap::new();
        let init_ctx = ScopeCtx {
            struct_map: &struct_map,
            enum_map: &enum_map,
            comp_map: &comp_map,
            builtin_widgets: &builtin_widgets,
            known_components: &known_components,
            imported_symbols,
            comp_name: comp.name.as_str(),
            comp_props: &empty_props,
            comp_cbs: &empty_cbs,
            locals: HashMap::new(),
            in_for_loop: false,
        };
        for prop in &comp.properties {
            if comp_props
                .insert(prop.name.as_str(), (&prop.ty, prop.dir))
                .is_some()
            {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate property '{}' in component '{}'",
                        prop.name, comp.name
                    ),
                });
            }
            if let Some(ref expr) = prop.default_value {
                validate_expr(
                    expr,
                    Some(prop.name.as_str()),
                    prop.span,
                    &init_ctx,
                    &mut diags,
                );
            }
        }

        let mut comp_cbs: HashMap<&str, usize> = HashMap::new();
        for cb in &comp.callbacks {
            if comp_cbs.insert(cb.name.as_str(), cb.params.len()).is_some() {
                diags.push(Diagnostic {
                    span: cb.span,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate callback '{}' in component '{}'",
                        cb.name, comp.name
                    ),
                });
            }
        }

        if let Some(ref root) = comp.root {
            let ctx = ScopeCtx {
                struct_map: &struct_map,
                enum_map: &enum_map,
                comp_map: &comp_map,
                builtin_widgets: &builtin_widgets,
                known_components: &known_components,
                imported_symbols,
                comp_name: comp.name.as_str(),
                comp_props: &comp_props,
                comp_cbs: &comp_cbs,
                locals: HashMap::new(),
                in_for_loop: false,
            };
            validate_element(root, &ctx, &mut diags);
        }
    }

    diags
}

fn validate_element(el: &ElementNode, ctx: &ScopeCtx<'_>, diags: &mut Vec<Diagnostic>) {
    let is_known = ctx.known_components.contains(&el.kind);
    if !is_known {
        diags.push(Diagnostic {
            span: el.span,
            severity: Severity::Error,
            message: format!("unknown component or widget '{}'", el.kind),
        });
    }

    let is_builtin = ctx.builtin_widgets.contains(el.kind.as_str());
    let allowed_widget_props: Option<Vec<&'static str>> =
        is_builtin.then(|| props_for_widget(&el.kind));

    if is_known && !el.children.is_empty() && is_builtin && !widget_allows_children(&el.kind) {
        diags.push(Diagnostic {
            span: el.span,
            severity: Severity::Error,
            message: format!("leaf widget '{}' cannot contain child elements", el.kind),
        });
    }

    for prop in &el.props {
        if prop.name == "key" {
            if !ctx.in_for_loop {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: "'key' property is only allowed inside a 'for' loop body".into(),
                });
            }
            validate_expr(&prop.value, Some("key"), prop.span, ctx, diags);
            continue;
        }

        if let Some(ref allowed) = allowed_widget_props {
            if !allowed.contains(&prop.name.as_str()) {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!("unknown property '{}' on '{}'", prop.name, el.kind),
                });
            }
        } else if let Some(sub) = ctx.comp_map.get(el.kind.as_str()) {
            let is_declared = sub.properties.iter().any(|sp| sp.name == prop.name);
            let is_common = COMMON_PROPS.contains(&prop.name.as_str());
            if !is_declared && !is_common {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!(
                        "unknown property '{}' on component '{}'",
                        prop.name, el.kind
                    ),
                });
            }
        }

        if prop.two_way {
            match &prop.value {
                Expr::Ident(id) => match ctx.comp_props.get(id.as_str()) {
                    None => {
                        diags.push(Diagnostic {
                            span: prop.span,
                            severity: Severity::Error,
                            message: format!(
                                "two-way binding '<=>' target '{id}' must be a property of '{}'",
                                ctx.comp_name
                            ),
                        });
                    }
                    Some((_, PropertyDir::In)) => {
                        diags.push(Diagnostic {
                            span: prop.span,
                            severity: Severity::Error,
                            message: format!(
                                "cannot use two-way binding '<=>' on read-only 'in property' '{id}'"
                            ),
                        });
                    }
                    Some(_) => {}
                },
                _ => {
                    diags.push(Diagnostic {
                        span: prop.span,
                        severity: Severity::Error,
                        message: "two-way binding '<=>' requires a component property identifier"
                            .into(),
                    });
                }
            }

            if is_builtin && !widget_supports_two_way(&el.kind, &prop.name) {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!(
                        "property '{}' on '{}' does not support two-way binding '<=>'",
                        prop.name, el.kind
                    ),
                });
            } else if let Some(sub) = ctx.comp_map.get(el.kind.as_str())
                && let Some(sp) = sub.properties.iter().find(|sp| sp.name == prop.name)
                && sp.dir == PropertyDir::In
            {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!(
                        "cannot use two-way binding '<=>' with read-only 'in property' '{}' on '{}'",
                        prop.name, el.kind
                    ),
                });
            }
        }

        validate_expr(&prop.value, Some(prop.name.as_str()), prop.span, ctx, diags);
    }

    let allowed_events = is_builtin.then(|| events_for_widget(&el.kind));
    for handler in &el.handlers {
        if let Some(ref evs) = allowed_events {
            if !evs.contains(&handler.event.as_str()) {
                diags.push(Diagnostic {
                    span: handler.span,
                    severity: Severity::Error,
                    message: format!("unknown event handler '{}' on '{}'", handler.event, el.kind),
                });
            }
        } else if let Some(sub) = ctx.comp_map.get(el.kind.as_str()) {
            let is_cb = sub.callbacks.iter().any(|c| c.name == handler.event);
            let is_common_ev = COMMON_EVENTS.contains(&handler.event.as_str());
            if !is_cb && !is_common_ev {
                diags.push(Diagnostic {
                    span: handler.span,
                    severity: Severity::Error,
                    message: format!(
                        "unknown callback or event '{}' on component '{}'",
                        handler.event, el.kind
                    ),
                });
            }
        }

        let mut h_locals = ctx.locals.clone();
        populate_handler_locals(&el.kind, handler.event.as_str(), ctx, &mut h_locals);
        let h_ctx = ScopeCtx {
            locals: h_locals,
            ..*ctx
        };
        for stmt in &handler.body {
            validate_stmt(stmt, handler.span, &h_ctx, diags);
        }
    }

    for spec in &el.animations {
        validate_animate_spec(spec, ctx, diags);
    }

    for state in &el.states {
        validate_expr(&state.condition, Some(&state.name), state.span, ctx, diags);
        for prop in &state.property_overrides {
            validate_expr(&prop.value, Some(prop.name.as_str()), prop.span, ctx, diags);
        }
    }

    for tr in &el.transitions {
        for anim in &tr.animations {
            validate_animate_spec(anim, ctx, diags);
        }
    }

    for child in &el.children {
        validate_child(child, ctx, diags);
    }
}

fn populate_handler_locals(
    kind: &str,
    event: &str,
    ctx: &ScopeCtx<'_>,
    locals: &mut HashMap<String, TypeExpr>,
) {
    if event == "on_hover" {
        locals.insert("hovered".into(), TypeExpr::Named("bool".into()));
        locals.insert("value".into(), TypeExpr::Named("bool".into()));
        return;
    }
    if let Some(sub) = ctx.comp_map.get(kind)
        && let Some(cb) = sub.callbacks.iter().find(|c| c.name == event)
    {
        for (i, pt) in cb.params.iter().enumerate() {
            if i == 0 {
                locals.insert("value".into(), pt.clone());
            }
            locals.insert(format!("arg{i}"), pt.clone());
        }
        return;
    }
    match (kind, event) {
        ("Slider", "on_change") => {
            locals.insert("value".into(), TypeExpr::Named("float".into()));
        }
        ("SplitPane", "on_change") => {
            locals.insert("value".into(), TypeExpr::Named("float".into()));
            locals.insert("ratio".into(), TypeExpr::Named("float".into()));
        }
        ("ToastHost", "on_dismiss") => {
            locals.insert("id".into(), TypeExpr::Named("int".into()));
            locals.insert("value".into(), TypeExpr::Named("int".into()));
        }
        ("Checkbox" | "Switch", "on_change" | "on_tap" | "on_click") => {
            locals.insert("checked".into(), TypeExpr::Named("bool".into()));
            locals.insert("value".into(), TypeExpr::Named("bool".into()));
        }
        ("TextInput" | "TextArea", "on_change" | "on_submit") => {
            locals.insert("value".into(), TypeExpr::Named("string".into()));
            locals.insert("text".into(), TypeExpr::Named("string".into()));
        }
        (
            "Tabs" | "TabBar" | "SegmentedControl" | "BottomNavBar" | "Select" | "Dropdown"
            | "RadioGroup" | "PopupMenu",
            "on_change" | "on_select",
        ) => {
            locals.insert("index".into(), TypeExpr::Named("int".into()));
            locals.insert("idx".into(), TypeExpr::Named("int".into()));
            locals.insert("selected".into(), TypeExpr::Named("int".into()));
            locals.insert("active".into(), TypeExpr::Named("int".into()));
            locals.insert("value".into(), TypeExpr::Named("int".into()));
        }
        ("Accordion", "on_change" | "on_toggle") => {
            locals.insert("expanded".into(), TypeExpr::Named("bool".into()));
            locals.insert("open".into(), TypeExpr::Named("bool".into()));
            locals.insert("value".into(), TypeExpr::Named("bool".into()));
        }
        ("DataTable", "on_select" | "on_change") => {
            locals.insert("index".into(), TypeExpr::Named("int".into()));
            locals.insert("idx".into(), TypeExpr::Named("int".into()));
            locals.insert("selected".into(), TypeExpr::Named("int".into()));
            locals.insert("value".into(), TypeExpr::Named("int".into()));
        }
        ("DataTable", "on_sort") => {
            locals.insert("column".into(), TypeExpr::Named("int".into()));
            locals.insert("col".into(), TypeExpr::Named("int".into()));
            locals.insert("index".into(), TypeExpr::Named("int".into()));
            locals.insert("idx".into(), TypeExpr::Named("int".into()));
            locals.insert("value".into(), TypeExpr::Named("int".into()));
        }
        _ => {}
    }
}

fn validate_animate_spec(spec: &AnimateSpec, ctx: &ScopeCtx<'_>, diags: &mut Vec<Diagnostic>) {
    if !ANIMATABLE_PROPS.contains(&spec.prop.as_str()) {
        diags.push(Diagnostic {
            span: spec.span,
            severity: Severity::Error,
            message: format!(
                "property '{}' cannot be animated; supported properties: {}",
                spec.prop,
                ANIMATABLE_PROPS.join(", ")
            ),
        });
    }
    for (fname, fval) in &spec.fields {
        match fname.as_str() {
            "duration" => {
                if !matches!(fval, Expr::DurationSec(_) | Expr::Float(_) | Expr::Int(_)) {
                    diags.push(Diagnostic {
                        span: spec.span,
                        severity: Severity::Error,
                        message:
                            "animate 'duration' expects a duration (e.g. 180ms or 0.2s) or number"
                                .into(),
                    });
                }
            }
            "easing" => {
                let valid = match fval {
                    Expr::Ident(s) => {
                        matches!(s.as_str(), "Linear" | "EaseIn" | "EaseOut" | "EaseInOut")
                    }
                    Expr::Member(b, v) if matches!(b.as_ref(), Expr::Ident(r) if r == "Easing") => {
                        matches!(v.as_str(), "Linear" | "EaseIn" | "EaseOut" | "EaseInOut")
                    }
                    _ => false,
                };
                if !valid {
                    diags.push(Diagnostic {
                        span: spec.span,
                        severity: Severity::Error,
                        message: "animate 'easing' must be Linear, EaseIn, EaseOut, or EaseInOut"
                            .into(),
                    });
                }
            }
            "stiffness" | "damping" => {
                if !matches!(fval, Expr::Int(_) | Expr::Float(_)) {
                    diags.push(Diagnostic {
                        span: spec.span,
                        severity: Severity::Error,
                        message: format!("animate '{fname}' expects a numeric literal"),
                    });
                }
            }
            "curve" => match fval {
                Expr::Call(cname, args) => match cname.as_str() {
                    "snappy" | "gentle" | "instant" => {
                        if !args.is_empty() {
                            diags.push(Diagnostic {
                                span: spec.span,
                                severity: Severity::Error,
                                message: format!("animate '{cname}()' takes 0 arguments"),
                            });
                        }
                    }
                    "spring" => {
                        if args.len() != 2 {
                            diags.push(Diagnostic {
                                span: spec.span,
                                severity: Severity::Error,
                                message: "animate 'spring(stiffness, damping)' takes 2 arguments"
                                    .into(),
                            });
                        } else {
                            for a in args {
                                validate_expr(a, None, spec.span, ctx, diags);
                            }
                        }
                    }
                    "tween" => {
                        if args.is_empty() || args.len() > 2 {
                            diags.push(Diagnostic {
                                span: spec.span,
                                severity: Severity::Error,
                                message:
                                    "animate 'tween(duration, [easing])' takes 1 or 2 arguments"
                                        .into(),
                            });
                        } else {
                            validate_expr(&args[0], None, spec.span, ctx, diags);
                            if let Some(eas) = args.get(1) {
                                let ok = matches!(
                                    eas,
                                    Expr::Ident(s)
                                        if matches!(
                                            s.as_str(),
                                            "Linear" | "EaseIn" | "EaseOut" | "EaseInOut"
                                        )
                                );
                                if !ok {
                                    diags.push(Diagnostic {
                                        span: spec.span,
                                        severity: Severity::Error,
                                        message: "animate 'tween' easing must be Linear, EaseIn, EaseOut, or EaseInOut".into(),
                                    });
                                }
                            }
                        }
                    }
                    other => {
                        diags.push(Diagnostic {
                            span: spec.span,
                            severity: Severity::Error,
                            message: format!("unknown animate curve '{other}'"),
                        });
                    }
                },
                _ => {
                    diags.push(Diagnostic {
                        span: spec.span,
                        severity: Severity::Error,
                        message: "invalid 'curve' in animate block".into(),
                    });
                }
            },
            other => {
                diags.push(Diagnostic {
                    span: spec.span,
                    severity: Severity::Error,
                    message: format!("unknown animate field '{other}'"),
                });
            }
        }
    }
}

fn validate_child(child: &ChildItem, ctx: &ScopeCtx<'_>, diags: &mut Vec<Diagnostic>) {
    match child {
        ChildItem::Element(c) => validate_element(c, ctx, diags),
        ChildItem::For {
            item_var,
            index_var,
            iterable,
            key,
            body,
            span,
        } => {
            validate_expr(iterable, None, *span, ctx, diags);
            let item_ty = match infer_expr_type(iterable, ctx) {
                Some(TypeExpr::List(inner)) => *inner,
                _ => TypeExpr::Named("string".into()),
            };
            let mut loop_locals = ctx.locals.clone();
            loop_locals.insert(item_var.clone(), item_ty);
            if let Some(iv) = index_var {
                loop_locals.insert(iv.clone(), TypeExpr::Named("uint".into()));
            }
            let loop_ctx = ScopeCtx {
                locals: loop_locals,
                in_for_loop: true,
                ..*ctx
            };
            if let Some(k) = key {
                validate_expr(k, None, *span, &loop_ctx, diags);
            }
            validate_element(body, &loop_ctx, diags);
        }
        ChildItem::If {
            condition,
            then_branch,
            else_branch,
            span,
        } => {
            validate_expr(condition, None, *span, ctx, diags);
            validate_element(then_branch, ctx, diags);
            if let Some(eb) = else_branch {
                validate_element(eb, ctx, diags);
            }
        }
    }
}

fn validate_lvalue<'a>(
    target: &'a Expr,
    span: Span,
    ctx: &ScopeCtx<'a>,
    diags: &mut Vec<Diagnostic>,
) -> Option<&'a str> {
    match target {
        Expr::Ident(id) => {
            if let Some((_, dir)) = ctx.comp_props.get(id.as_str()) {
                if *dir == PropertyDir::In {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!("cannot mutate read-only 'in property' '{id}'"),
                    });
                }
                Some(id.as_str())
            } else if ctx.locals.contains_key(id.as_str()) {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("cannot assign to immutable local variable '{id}'"),
                });
                None
            } else {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("undefined property '{id}' in assignment"),
                });
                None
            }
        }
        Expr::Member(base, field) => {
            let root = validate_lvalue(base, span, ctx, diags);
            if let Some(TypeExpr::Named(sname)) = infer_expr_type(base, ctx) {
                if sname == "ToastItem" {
                    if !matches!(
                        field.as_str(),
                        "id" | "title" | "message" | "severity" | "duration_ms"
                    ) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown field '{field}' on struct 'ToastItem'"),
                        });
                    }
                } else if let Some(st) = ctx.struct_map.get(sname.as_str())
                    && !st.fields.iter().any(|f| f.name == *field)
                {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!("unknown field '{field}' on struct '{sname}'"),
                    });
                }
            }
            root
        }
        Expr::Index(base, idx) => {
            validate_expr(idx, None, span, ctx, diags);
            validate_lvalue(base, span, ctx, diags)
        }
        _ => {
            diags.push(Diagnostic {
                span,
                severity: Severity::Error,
                message: "invalid assignment target".into(),
            });
            None
        }
    }
}

fn validate_stmt(stmt: &Stmt, span: Span, ctx: &ScopeCtx<'_>, diags: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::Assign { target, op, value } => {
            let _ = validate_lvalue(target, span, ctx, diags);
            if *op != AssignOp::Set
                && let Some(TypeExpr::Named(tname)) = infer_expr_type(target, ctx)
            {
                let numeric_ok = matches!(tname.as_str(), "int" | "uint" | "usize" | "float");
                let string_ok = *op == AssignOp::Add && tname == "string";
                if !numeric_ok && !string_ok {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!(
                            "compound assignment operator cannot be applied to type '{tname}'"
                        ),
                    });
                }
            }
            validate_expr(value, None, span, ctx, diags);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expr(condition, None, span, ctx, diags);
            for s in then_branch {
                validate_stmt(s, span, ctx, diags);
            }
            for s in else_branch {
                validate_stmt(s, span, ctx, diags);
            }
        }
        Stmt::Call { callee, args } => {
            match ctx.comp_cbs.get(callee.as_str()) {
                None => {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!(
                            "unknown callback '{}' in component '{}'",
                            callee, ctx.comp_name
                        ),
                    });
                }
                Some(&arity) => {
                    if args.len() != arity {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!(
                                "callback '{}' expects {} argument(s), got {}",
                                callee,
                                arity,
                                args.len()
                            ),
                        });
                    }
                }
            }
            for a in args {
                validate_expr(a, None, span, ctx, diags);
            }
        }
        Stmt::Expr(e) => {
            validate_expr(e, None, span, ctx, diags);
        }
    }
}

fn is_prop_keyword(prop_hint: Option<&str>, ident: &str) -> bool {
    match prop_hint {
        Some("width" | "height" | "min_width" | "max_width" | "min_height" | "max_height") => {
            ident == "Auto"
        }
        Some("align") => matches!(ident, "Start" | "Center" | "End" | "Stretch"),
        Some("justify") => matches!(
            ident,
            "Start" | "Center" | "End" | "SpaceBetween" | "SpaceAround" | "SpaceEvenly"
        ),
        Some("fit") => matches!(ident, "Contain" | "Cover" | "Fill" | "None"),
        Some("direction") => matches!(ident, "Horizontal" | "Vertical"),
        Some("font_weight") => matches!(
            ident,
            "Thin"
                | "ExtraLight"
                | "Light"
                | "Normal"
                | "Regular"
                | "Medium"
                | "SemiBold"
                | "Semibold"
                | "Bold"
                | "ExtraBold"
                | "Black"
        ),
        Some("blend_mode") => matches!(
            ident,
            "Normal" | "Multiply" | "Screen" | "Overlay" | "Darken" | "Lighten" | "Plus"
        ),
        Some("cursor") => matches!(
            ident,
            "Default"
                | "Pointer"
                | "Text"
                | "Grab"
                | "Grabbing"
                | "ResizeHorizontal"
                | "ResizeVertical"
                | "NotAllowed"
                | "Crosshair"
        ),
        _ => false,
    }
}

fn infer_expr_type(expr: &Expr, ctx: &ScopeCtx<'_>) -> Option<TypeExpr> {
    match expr {
        Expr::Int(_) => Some(TypeExpr::Named("int".into())),
        Expr::Float(_) | Expr::Length(_, _) => Some(TypeExpr::Named("float".into())),
        Expr::Bool(_) => Some(TypeExpr::Named("bool".into())),
        Expr::String(_) => Some(TypeExpr::Named("string".into())),
        Expr::ColorHex(_) => Some(TypeExpr::Named("color".into())),
        Expr::DurationSec(_) => Some(TypeExpr::Named("duration".into())),
        Expr::StructLit { name, .. } => Some(TypeExpr::Named(name.clone())),
        Expr::Range { .. } => Some(TypeExpr::List(Box::new(TypeExpr::Named("int".into())))),
        Expr::Array(items) => {
            let inner = items
                .first()
                .and_then(|x| infer_expr_type(x, ctx))
                .unwrap_or_else(|| TypeExpr::Named("string".into()));
            Some(TypeExpr::List(Box::new(inner)))
        }
        Expr::Ident(id) => ctx
            .locals
            .get(id.as_str())
            .cloned()
            .or_else(|| ctx.comp_props.get(id.as_str()).map(|(t, _)| (*t).clone())),
        Expr::Member(base, field) => {
            if let Expr::Ident(root) = base.as_ref() {
                if root == "root" {
                    return ctx
                        .comp_props
                        .get(field.as_str())
                        .map(|(t, _)| (*t).clone());
                }
                if root == "Theme" {
                    return Some(TypeExpr::Named("color".into()));
                }
                if root == "ThemeMode" {
                    return Some(TypeExpr::Named("ThemeMode".into()));
                }
                if root == "ToastSeverity" {
                    return Some(TypeExpr::Named("ToastSeverity".into()));
                }
                if root == "SafeArea" {
                    return Some(TypeExpr::Named("Insets".into()));
                }
                if root == "Window" {
                    return match field.as_str() {
                        "width" | "height" | "scale_factor" => {
                            Some(TypeExpr::Named("float".into()))
                        }
                        "is_compact" | "is_medium" | "is_expanded" => {
                            Some(TypeExpr::Named("bool".into()))
                        }
                        "breakpoint" => Some(TypeExpr::Named("string".into())),
                        _ => None,
                    };
                }
                if ctx.enum_map.contains_key(root.as_str()) {
                    return Some(TypeExpr::Named(root.clone()));
                }
            }
            match infer_expr_type(base, ctx) {
                Some(TypeExpr::Named(sname)) if sname == "ToastItem" => match field.as_str() {
                    "id" | "duration_ms" => Some(TypeExpr::Named("int".into())),
                    "title" | "message" => Some(TypeExpr::Named("string".into())),
                    "severity" => Some(TypeExpr::Named("ToastSeverity".into())),
                    _ => None,
                },
                Some(TypeExpr::Named(sname)) => ctx
                    .struct_map
                    .get(sname.as_str())
                    .and_then(|st| st.fields.iter().find(|f| f.name == *field))
                    .map(|f| f.ty.clone()),
                _ => None,
            }
        }
        Expr::Index(base, _) => match infer_expr_type(base, ctx) {
            Some(TypeExpr::List(inner)) => Some(*inner),
            _ => None,
        },
        Expr::UnaryNot(_) => Some(TypeExpr::Named("bool".into())),
        Expr::UnaryNeg(inner) => infer_expr_type(inner, ctx),
        Expr::Binary(l, op, r) => match op {
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => Some(TypeExpr::Named("bool".into())),
            BinOp::Add => {
                let lt = infer_expr_type(l, ctx);
                let rt = infer_expr_type(r, ctx);
                if matches!(lt.as_ref().and_then(|t| t.base_name()), Some("string"))
                    || matches!(rt.as_ref().and_then(|t| t.base_name()), Some("string"))
                {
                    Some(TypeExpr::Named("string".into()))
                } else if matches!(lt.as_ref().and_then(|t| t.base_name()), Some("float"))
                    || matches!(rt.as_ref().and_then(|t| t.base_name()), Some("float"))
                {
                    Some(TypeExpr::Named("float".into()))
                } else {
                    lt.or(rt)
                }
            }
            _ => {
                let lt = infer_expr_type(l, ctx);
                let rt = infer_expr_type(r, ctx);
                if matches!(lt.as_ref().and_then(|t| t.base_name()), Some("float"))
                    || matches!(rt.as_ref().and_then(|t| t.base_name()), Some("float"))
                {
                    Some(TypeExpr::Named("float".into()))
                } else {
                    lt.or(rt)
                }
            }
        },
        Expr::Ternary {
            then_expr,
            else_expr,
            ..
        } => infer_expr_type(then_expr, ctx).or_else(|| infer_expr_type(else_expr, ctx)),
        Expr::MethodCall(recv, m, _) => {
            if let Expr::Ident(root) = recv.as_ref()
                && root == "Math"
            {
                return match m.as_str() {
                    "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "abs" | "round"
                    | "floor" | "ceil" | "min" | "max" | "clamp" | "pow" | "atan2" => {
                        Some(TypeExpr::Named("float".into()))
                    }
                    _ => None,
                };
            }
            match m.as_str() {
                "len" => Some(TypeExpr::Named("uint".into())),
                "is_empty" | "contains" | "starts_with" | "ends_with" => {
                    Some(TypeExpr::Named("bool".into()))
                }
                "to_lowercase" | "to_uppercase" | "trim" => Some(TypeExpr::Named("string".into())),
                "sin" | "cos" | "tan" | "sqrt" | "abs" | "round" | "floor" | "ceil" | "min"
                | "max" | "clamp" | "pow" => Some(TypeExpr::Named("float".into())),
                _ => None,
            }
        }
        Expr::Call(name, args) => match name.as_str() {
            "rgb" | "rgba" => Some(TypeExpr::Named("color".into())),
            "@linear-gradient" | "linear_gradient" | "@radial-gradient" | "radial_gradient" => {
                Some(TypeExpr::Named("brush".into()))
            }
            "sin" | "cos" | "tan" | "sqrt" | "pow" => Some(TypeExpr::Named("float".into())),
            "abs" | "round" | "floor" | "ceil" | "min" | "max" | "clamp" => {
                args.first().and_then(|a| infer_expr_type(a, ctx))
            }
            _ => None,
        },
    }
}

fn validate_expr(
    expr: &Expr,
    prop_hint: Option<&str>,
    span: Span,
    ctx: &ScopeCtx<'_>,
    diags: &mut Vec<Diagnostic>,
) {
    match expr {
        Expr::ColorHex(hex) => {
            if Color::from_hex(hex).is_none() {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("invalid hex color literal '{hex}'"),
                });
            }
        }
        Expr::Ident(id) => {
            if id == "Theme"
                || id == "ThemeMode"
                || id == "ToastSeverity"
                || id == "SafeArea"
                || id == "Math"
                || id == "Window"
            {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("'{id}' requires a member access (e.g. '{id}....')"),
                });
                return;
            }
            let in_scope = ctx.comp_props.contains_key(id.as_str())
                || ctx.locals.contains_key(id.as_str())
                || matches!(id.as_str(), "None" | "null")
                || is_prop_keyword(prop_hint, id.as_str());
            if !in_scope {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("undefined identifier '{id}'"),
                });
            }
        }
        Expr::Member(base, field) => {
            if let Expr::Ident(root) = base.as_ref() {
                if root == "root" {
                    if !ctx.comp_props.contains_key(field.as_str()) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown property '{field}' on 'root'"),
                        });
                    }
                    return;
                }
                if root == "Theme" {
                    return;
                }
                if root == "ThemeMode" {
                    if !matches!(field.as_str(), "Light" | "Dark" | "Amoled" | "HighContrast") {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown ThemeMode variant '{field}'"),
                        });
                    }
                    return;
                }
                if root == "ToastSeverity" {
                    if !matches!(field.as_str(), "Info" | "Success" | "Warning" | "Error") {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown ToastSeverity variant '{field}'"),
                        });
                    }
                    return;
                }
                if root == "SafeArea" {
                    if field != "insets" {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!(
                                "unknown SafeArea member '{field}' (expected 'insets')"
                            ),
                        });
                    }
                    return;
                }
                if root == "Window" {
                    if !matches!(
                        field.as_str(),
                        "width"
                            | "height"
                            | "scale_factor"
                            | "is_compact"
                            | "is_medium"
                            | "is_expanded"
                            | "breakpoint"
                    ) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!(
                                "unknown Window property '{field}' (expected width, height, scale_factor, is_compact, is_medium, is_expanded, breakpoint)"
                            ),
                        });
                    }
                    return;
                }
                if root == "root" {
                    if !ctx.comp_props.contains_key(field.as_str()) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown property '{field}' on 'root'"),
                        });
                    }
                    return;
                }
                if root == "Math" {
                    if !matches!(
                        field.as_str(),
                        "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "SQRT2" | "SQRT1_2"
                    ) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!(
                                "unknown Math constant '{field}' (expected PI, E, LN2, etc.)"
                            ),
                        });
                    }
                    return;
                }
                if let Some(en) = ctx.enum_map.get(root.as_str()) {
                    if !en.variants.iter().any(|v| v == field) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown variant '{field}' on enum '{root}'"),
                        });
                    }
                    return;
                }
            }

            validate_expr(base, None, span, ctx, diags);
            if let Some(TypeExpr::Named(sname)) = infer_expr_type(base, ctx) {
                if sname == "ToastItem" {
                    if !matches!(
                        field.as_str(),
                        "id" | "title" | "message" | "severity" | "duration_ms"
                    ) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown field '{field}' on struct 'ToastItem'"),
                        });
                    }
                } else if let Some(st) = ctx.struct_map.get(sname.as_str())
                    && !st.fields.iter().any(|f| f.name == *field)
                {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!("unknown field '{field}' on struct '{sname}'"),
                    });
                }
            }
        }
        Expr::Array(items) => {
            for item in items {
                validate_expr(item, prop_hint, span, ctx, diags);
            }
        }
        Expr::Binary(l, _, r) | Expr::Index(l, r) => {
            validate_expr(l, None, span, ctx, diags);
            validate_expr(r, None, span, ctx, diags);
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            validate_expr(cond, None, span, ctx, diags);
            validate_expr(then_expr, prop_hint, span, ctx, diags);
            validate_expr(else_expr, prop_hint, span, ctx, diags);
        }
        Expr::Range { start, end, .. } => {
            validate_expr(start, None, span, ctx, diags);
            validate_expr(end, None, span, ctx, diags);
        }
        Expr::StructLit { name, fields } => {
            if name == "ToastItem" {
                for (fname, _) in fields {
                    if !matches!(
                        fname.as_str(),
                        "id" | "title" | "message" | "severity" | "duration_ms"
                    ) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!(
                                "unknown field '{fname}' in struct literal 'ToastItem'"
                            ),
                        });
                    }
                }
            } else if let Some(st) = ctx.struct_map.get(name.as_str()) {
                for (fname, _) in fields {
                    if !st.fields.iter().any(|sf| sf.name == *fname) {
                        diags.push(Diagnostic {
                            span,
                            severity: Severity::Error,
                            message: format!("unknown field '{fname}' in struct literal '{name}'"),
                        });
                    }
                }
            } else if !ctx.imported_symbols.contains(name) {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("unknown struct '{name}'"),
                });
            }
            for (_, v) in fields {
                validate_expr(v, None, span, ctx, diags);
            }
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            validate_expr(inner, None, span, ctx, diags);
        }
        Expr::Call(name, args) => {
            let valid_builtin = match name.as_str() {
                "Some" => args.len() == 1,
                "min" | "max" | "pow" => args.len() == 2,
                "clamp" | "rgb" => args.len() == 3,
                "rgba" => args.len() == 4,
                "abs" | "round" | "floor" | "ceil" | "sin" | "cos" | "tan" | "sqrt" => {
                    args.len() == 1
                }
                "@linear-gradient" | "linear_gradient" | "@radial-gradient" | "radial_gradient" => {
                    args.len() >= 2
                }
                _ => false,
            };
            if !valid_builtin {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!(
                        "unknown or invalid call '{}({} args)' in expression",
                        name,
                        args.len()
                    ),
                });
            }
            for a in args {
                validate_expr(a, None, span, ctx, diags);
            }
        }
        Expr::MethodCall(recv, method, args) => {
            if let Expr::Ident(root) = recv.as_ref()
                && root == "Math"
            {
                let known_math = match method.as_str() {
                    "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "abs" | "round"
                    | "floor" | "ceil" => args.len() == 1,
                    "min" | "max" | "pow" | "atan2" => args.len() == 2,
                    "clamp" => args.len() == 3,
                    _ => false,
                };
                if !known_math {
                    diags.push(Diagnostic {
                        span,
                        severity: Severity::Error,
                        message: format!(
                            "unknown or invalid Math function 'Math.{}({} args)'",
                            method,
                            args.len()
                        ),
                    });
                }
                for a in args {
                    validate_expr(a, None, span, ctx, diags);
                }
                return;
            }
            validate_expr(recv, None, span, ctx, diags);
            let known_ok = match method.as_str() {
                "len" | "is_empty" | "to_lowercase" | "to_uppercase" | "trim" => args.is_empty(),
                "contains" | "starts_with" | "ends_with" => args.len() == 1,
                "sin" | "cos" | "tan" | "sqrt" | "abs" | "round" | "floor" | "ceil" => {
                    args.is_empty()
                }
                "min" | "max" | "pow" => args.len() == 1,
                "clamp" => args.len() == 2,
                _ => false,
            };
            if !known_ok {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!(
                        "unknown or invalid method call '.{}({} args)'",
                        method,
                        args.len()
                    ),
                });
            }
            for a in args {
                validate_expr(a, None, span, ctx, diags);
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Length(_, _)
        | Expr::DurationSec(_) => {}
    }
}
