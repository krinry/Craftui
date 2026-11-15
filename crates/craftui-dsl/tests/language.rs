//! End-to-end tests for the `.cui` language: what the parser reads, what the
//! validator rejects (and how it says so), and what the code generator emits.
//!
//! These go through the same public entry points that `build.rs`, the CLI and
//! the language server use.

use craftui_dsl::{
    MemoryModuleLoader, PropertyDir, Severity, TypeExpr, generate_rust, parse, resolve_source,
    resolve_source_with_loader,
};

/// Error messages produced for `source` by parsing, import resolution and validation.
fn errors(source: &str) -> Vec<String> {
    resolve_source(source, None)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

/// Compiles `source`, failing the test if it is not valid, and returns the generated Rust.
fn compile(source: &str) -> String {
    let unit = resolve_source(source, None);
    let problems: Vec<&str> = unit
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(problems.is_empty(), "unexpected errors: {problems:#?}");
    generate_rust(&unit.merged)
}

/// Wraps element source in a component that has a few properties and callbacks to refer to.
fn in_component(body: &str) -> String {
    format!(
        r#"
struct Task {{
    title: string = "",
    done: bool = false,
}}

component Probe {{
    in property <string> title: "Title";
    in-out property <string> name: "";
    in-out property <int> count: 0;
    in-out property <bool> flag: false;
    in-out property <[Task]> tasks: [];
    callback on_pick(int);
    callback on_done();

    Column {{
{body}
    }}
}}
"#
    )
}

#[track_caller]
fn assert_rejected(body: &str, expected: &str) {
    let found = errors(&in_component(body));
    assert!(
        found.iter().any(|message| message.contains(expected)),
        "expected an error containing {expected:?} for:\n{body}\n\ngot: {found:#?}"
    );
}

// --- Parsing -----------------------------------------------------------------

#[test]
fn the_parser_reads_every_top_level_item() {
    let parsed = parse(
        r#"
import { Card, FilledButton } from "std/material.cui"
import { Helper } from "./helper.cui"

export struct Task {
    title: string = "",
    done: bool = false,
}

enum Filter { All, Active, Done }

export component Board {
    in property <string> heading: "Tasks";
    out property <int> total;
    in-out property <[Task]> tasks: [];
    property <bool> busy: false;
    callback on_open(int, string);
    callback on_refresh();

    Column { }
}
"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let file = parsed.file;

    assert_eq!(file.imports.len(), 2);
    assert_eq!(file.imports[0].symbols, ["Card", "FilledButton"]);
    assert_eq!(file.imports[0].from_path, "std/material.cui");
    assert_eq!(file.imports[1].from_path, "./helper.cui");

    assert_eq!(file.structs.len(), 1);
    assert!(file.structs[0].exported);
    assert_eq!(file.structs[0].name, "Task");
    assert_eq!(file.structs[0].fields.len(), 2);

    assert_eq!(file.enums.len(), 1);
    assert!(!file.enums[0].exported);
    assert_eq!(file.enums[0].variants, ["All", "Active", "Done"]);

    assert_eq!(file.components.len(), 1);
    let board = &file.components[0];
    assert!(board.exported);
    assert_eq!(board.name, "Board");

    let directions: Vec<(&str, PropertyDir)> = board
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p.dir))
        .collect();
    assert_eq!(
        directions,
        [
            ("heading", PropertyDir::In),
            ("total", PropertyDir::Out),
            ("tasks", PropertyDir::InOut),
            ("busy", PropertyDir::Private),
        ]
    );
    assert!(board.properties[0].default_value.is_some());
    assert!(board.properties[1].default_value.is_none());
    assert_eq!(
        board.properties[2].ty,
        TypeExpr::List(Box::new(TypeExpr::Named("Task".into())))
    );

    assert_eq!(board.callbacks.len(), 2);
    assert_eq!(board.callbacks[0].name, "on_open");
    assert_eq!(board.callbacks[0].params.len(), 2);
    assert!(board.callbacks[1].params.is_empty());

    assert_eq!(board.root.as_ref().map(|r| r.kind.as_str()), Some("Column"));
}

#[test]
fn the_parser_reads_bindings_handlers_children_and_control_flow() {
    let parsed = parse(&in_component(
        r#"
        gap: 8dp;
        TextInput { value <=> name; }
        Button {
            text: "Go";
            scale: flag ? 1.1 : 1.0;
            animate scale { curve: snappy(); }
            on_tap => { count += 1; on_done(); }
        }
        if flag : Text { text: "on"; } else : Text { text: "off"; }
        for task, idx in tasks : Text { text: task.title; }
"#,
    ));
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let root = parsed.file.components[0]
        .root
        .clone()
        .expect("root element");

    assert_eq!(root.props.len(), 1);
    assert_eq!(root.props[0].name, "gap");
    // TextInput, Button, the `if` and the `for`.
    assert_eq!(root.children.len(), 4);

    let debug = format!("{:?}", root.children);
    assert!(debug.contains("If {"), "the `if` is a control-flow child");
    assert!(debug.contains("For {"), "the `for` is a control-flow child");
}

#[test]
fn comments_are_ignored() {
    let rust = compile(
        r#"
// A line comment.
export component Commented {
    /* A block
       comment. */
    Column {
        Text { text: "visible"; } // trailing
    }
}
"#,
    );
    assert!(rust.contains("pub struct Commented"));
}

#[test]
fn the_parser_recovers_and_reports_instead_of_panicking() {
    // Truncated and nonsensical inputs must produce diagnostics, never a panic
    // or a hang: the language server feeds the parser half-typed files.
    let inputs = [
        "",
        "component",
        "component A {",
        "component A { Column {",
        "component A { Column { text: ; } }",
        "component A { property <int count; Column { } }",
        "import { A, from",
        "}}}}",
        "component A { Column { if : Text { } } }",
        "component A { Column { for in : Text { } } }",
        "component A { Column { on_tap => } }",
        "struct { }",
        "enum E { , , }",
        "component A { Column { width: 100%%; } }",
        "\u{0}\u{1}@@##",
    ];
    for input in inputs {
        let _ = parse(input);
        let _ = errors(input);
    }

    for clearly_broken in ["}}}}", "component A { Column { text: ; } }"] {
        assert!(
            !parse(clearly_broken).diagnostics.is_empty(),
            "expected a diagnostic for {clearly_broken:?}"
        );
    }
}

// --- Validation --------------------------------------------------------------

#[test]
fn a_valid_component_has_no_errors() {
    let found = errors(&in_component(
        r#"
        Text { text: title + " (" + count + ")"; color: Theme.text_primary; }
        TextInput { placeholder: "Name"; value <=> name; }
        Checkbox { label: "Flag"; checked <=> flag; }
        Button { text: "Pick"; on_tap => { on_pick(count); } }
        for task, idx in tasks : Text { text: task.title; }
        if flag : Text { text: "flag is on"; }
"#,
    ));
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn unknown_names_are_rejected_with_the_name_in_the_message() {
    assert_rejected(
        r#"Buton { text: "x"; }"#,
        "unknown component or widget 'Buton'",
    );
    assert_rejected(
        r#"Button { text: "x"; background_color: #ffffff; }"#,
        "unknown property 'background_color' on 'Button'",
    );
    assert_rejected(
        r#"Button { text: "x"; on_press => { count += 1; } }"#,
        "unknown event handler 'on_press' on 'Button'",
    );
    assert_rejected(
        r#"Text { text: missing; }"#,
        "undefined identifier 'missing'",
    );
    assert_rejected(
        r#"for task in tasks : Text { text: task.nam; }"#,
        "unknown field 'nam' on struct 'Task'",
    );
    assert_rejected(
        r#"Text { text: "w"; width: Window.size; }"#,
        "unknown Window property 'size'",
    );
}

#[test]
fn property_directions_are_enforced() {
    assert_rejected(
        r#"Button { text: "x"; on_tap => { title = "changed"; } }"#,
        "cannot mutate read-only 'in property' 'title'",
    );
    assert_rejected(
        r#"TextInput { value <=> title; }"#,
        "read-only 'in property'",
    );
}

#[test]
fn two_way_bindings_are_limited_to_properties_that_support_them() {
    assert_rejected(
        r#"Text { text <=> name; }"#,
        "does not support two-way binding",
    );
}

#[test]
fn callbacks_are_checked_for_existence_and_argument_count() {
    assert_rejected(
        r#"Button { text: "x"; on_tap => { on_pick(); } }"#,
        "callback 'on_pick' expects 1 argument(s), got 0",
    );
    assert_rejected(
        r#"Button { text: "x"; on_tap => { on_done(1); } }"#,
        "callback 'on_done' expects 0 argument(s), got 1",
    );
}

#[test]
fn structure_rules_are_enforced() {
    assert_rejected(
        r#"Text { text: "x"; Text { text: "nested"; } }"#,
        "leaf widget 'Text' cannot contain child elements",
    );
    assert_rejected(
        r#"if flag Text { text: "x"; }"#,
        "expected ':' after if condition",
    );
}

#[test]
fn animate_blocks_are_checked() {
    assert_rejected(
        r#"Text { text: "x"; animate font_size { curve: snappy(); } }"#,
        "cannot be animated",
    );
    assert_rejected(
        r#"Text { text: "x"; animate opacity { curve: bouncy(); } }"#,
        "unknown animate curve 'bouncy'",
    );
}

#[test]
fn declarations_cannot_be_duplicated_or_shadow_builtins() {
    let duplicate = errors(
        r#"
component Twice {
    property <int> a: 0;
    property <int> a: 1;
    Column { }
}
"#,
    );
    assert!(
        duplicate
            .iter()
            .any(|m| m.contains("duplicate property 'a'")),
        "{duplicate:#?}"
    );

    let shadow = errors("component Button { Column { } }");
    assert!(
        shadow
            .iter()
            .any(|m| m.contains("cannot shadow built-in widget")),
        "{shadow:#?}"
    );
}

#[test]
fn syntax_from_outdated_material_is_rejected() {
    // These forms appeared in earlier documentation but were never part of the
    // language. They must stay errors so that nobody is misled by silence.
    let outdated = [
        r#"component A { state { let count: int = 0; } Column { } }"#,
        r#"component A { property <int> n: 0; Column { Button { text: "x"; on_click: { n += 1; } } } }"#,
        r#"component A { Column { Text { text: "x"; corner_radius: 8.0; } } }"#,
        r#"component A { Column { align_items: "center"; } }"#,
    ];
    for source in outdated {
        assert!(
            !errors(source).is_empty(),
            "expected an error for outdated syntax: {source}"
        );
    }
}

// --- Imports -----------------------------------------------------------------

#[test]
fn built_in_libraries_can_be_imported() {
    let rust = compile(
        r#"
import { FilledButton, SearchBar } from "std/material.cui"
import { IconSearch } from "std/icons.cui"

export component Shop {
    in-out property <string> query: "";

    Column {
        IconSearch { }
        SearchBar { text <=> query; }
        FilledButton { text: "Buy"; }
    }
}
"#,
    );
    assert!(rust.contains("pub struct Shop"));
}

#[test]
fn local_imports_are_resolved_through_the_module_loader() {
    let mut loader = MemoryModuleLoader::new();
    loader.insert(
        "ui/parts.cui",
        r#"
export component Badge {
    in property <string> label: "";
    Text { text: label; }
}

component Hidden {
    Text { text: "private"; }
}
"#,
    );

    let ok = resolve_source_with_loader(
        r#"
import { Badge } from "./parts.cui"
export component Page { Column { Badge { label: "New"; } } }
"#,
        "ui/page.cui",
        &loader,
    );
    let problems: Vec<_> = ok
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(problems.is_empty(), "{problems:#?}");
    let names: Vec<&str> = ok
        .merged
        .components
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        names.contains(&"Badge") && names.contains(&"Page"),
        "{names:?}"
    );

    let private = resolve_source_with_loader(
        r#"
import { Hidden } from "./parts.cui"
export component Page { Column { Hidden { } } }
"#,
        "ui/page.cui",
        &loader,
    );
    assert!(
        private
            .diagnostics
            .iter()
            .any(|d| d.message.contains("must be marked 'export'")),
        "{:#?}",
        private.diagnostics
    );

    let missing = resolve_source_with_loader(
        r#"
import { Nothing } from "./nowhere.cui"
export component Page { Column { } }
"#,
        "ui/page.cui",
        &loader,
    );
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|d| d.message.contains("cannot resolve import './nowhere.cui'")),
        "{:#?}",
        missing.diagnostics
    );
}

#[test]
fn circular_imports_are_reported() {
    let mut loader = MemoryModuleLoader::new();
    loader.insert(
        "ui/a.cui",
        r#"
import { B } from "./b.cui"
export component A { Column { B { } } }
"#,
    );
    loader.insert(
        "ui/b.cui",
        r#"
import { A } from "./a.cui"
export component B { Column { A { } } }
"#,
    );
    let source = r#"
import { A } from "./a.cui"
export component Page { Column { A { } } }
"#;
    let unit = resolve_source_with_loader(source, "ui/page.cui", &loader);
    assert!(
        unit.diagnostics
            .iter()
            .any(|d| d.message.contains("circular import")),
        "{:#?}",
        unit.diagnostics
    );
}

// --- Code generation ---------------------------------------------------------

#[test]
fn a_component_becomes_a_struct_with_signals_callbacks_and_a_builder() {
    let rust = compile(
        r#"
export component Counter {
    in-out property <int> count: 0;
    in property <float> step: 1.0;
    property <bool> busy: false;
    in property <string> label: "Count";
    in property <[string]> tags: [];
    callback on_reset();
    callback on_pick(int, string);

    Column {
        Text { text: label + ": " + count; }
        Button { text: "Reset"; on_tap => { count = 0; on_reset(); } }
    }
}
"#,
    );

    assert!(rust.contains("pub struct Counter"), "{rust}");
    for field in [
        "pub count: Signal<i32>",
        "pub step: Signal<f32>",
        "pub busy: Signal<bool>",
        "pub label: Signal<String>",
        "pub tags: Signal<Vec<String>>",
    ] {
        assert!(rust.contains(field), "missing `{field}` in:\n{rust}");
    }
    assert!(rust.contains("pub fn new(_cx: Scope) -> Self"));
    assert!(rust.contains("pub fn set_on_reset(&self, f: impl FnMut() + 'static)"));
    assert!(rust.contains("pub fn set_on_pick(&self, f: impl FnMut(i32, String) + 'static)"));
    assert!(rust.contains("pub fn build(&self, cx: Scope, tree: &mut WidgetTree) -> NodeId"));
}

#[test]
fn generated_code_opts_out_of_style_lints() {
    // Apps build with `clippy -D warnings`; machine-written code must not trip it.
    let rust = compile("component Tiny { Column { } }");
    let attribute = rust.find("#[allow(clippy::all)]").expect("lint opt-out");
    let implementation = rust.find("impl Tiny").expect("impl block");
    assert!(attribute < implementation);
}

#[test]
fn if_without_else_hides_the_element_and_if_else_swaps_it() {
    let hidden = compile(&in_component(r#"if flag : Text { text: "on"; }"#));
    assert!(hidden.contains(".set_visible_when("), "{hidden}");
    assert!(!hidden.contains(".dynamic_slot("), "{hidden}");

    let swapped = compile(&in_component(
        r#"if flag : Text { text: "on"; } else : Text { text: "off"; }"#,
    ));
    assert!(swapped.contains(".dynamic_slot("), "{swapped}");
}

#[test]
fn the_if_else_wrapper_takes_its_size_from_the_branches() {
    // Without this a branch with `flex_grow` or `height: 100%` could not fill
    // its parent, because the wrapper around it would be only content-sized.
    let rust = compile(&in_component(
        r#"
        if flag : Column {
            width: 100%;
            flex_grow: 1.0;
            Text { text: "on"; }
        } else : ScrollView {
            width: 100%;
            height: 320dp;
            flex_grow: 1.0;
        }
"#,
    ));
    let line = rust
        .lines()
        .find(|line| line.contains(".dynamic_slot("))
        .expect("an if/else slot");
    assert!(line.contains(".with_width(Length::Pct(100.00))"), "{line}");
    // One branch scrolls, so the wrapper takes the space that is left instead
    // of growing from the full height of the scrolled content.
    assert!(line.contains(".fill_remaining(1.00)"), "{line}");
    assert!(!line.contains(".with_flex_grow("), "{line}");
    // Taken from the second branch, because the first does not set a height.
    assert!(line.contains(".with_height(Length::Dp(320.00))"), "{line}");

    // Without a scrolling branch the wrapper grows in the ordinary way.
    let growing = compile(&in_component(
        r#"
        if flag : Column { flex_grow: 2.0; }
        else : Column { flex_grow: 2.0; }
"#,
    ));
    let line = growing
        .lines()
        .find(|line| line.contains(".dynamic_slot("))
        .expect("an if/else slot");
    assert!(line.contains(".with_flex_grow(2.00)"), "{line}");
    assert!(!line.contains(".fill_remaining("), "{line}");

    // Branches without explicit sizing leave the wrapper alone.
    let plain = compile(&in_component(
        r#"if flag : Text { text: "on"; } else : Text { text: "off"; }"#,
    ));
    let line = plain
        .lines()
        .find(|line| line.contains(".dynamic_slot("))
        .expect("an if/else slot");
    assert!(!line.contains(".with_width("), "{line}");
    assert!(!line.contains(".with_height("), "{line}");
    assert!(!line.contains(".with_flex_grow("), "{line}");

    // A size that depends on state cannot be copied once, so it is not copied at all.
    let dynamic = compile(&in_component(
        r#"
        if flag : Column { width: count > 3 ? 100% : 50%; }
        else : Column { }
"#,
    ));
    let line = dynamic
        .lines()
        .find(|line| line.contains(".dynamic_slot("))
        .expect("an if/else slot");
    assert!(!line.contains(".with_width("), "{line}");
}

#[test]
fn for_loops_build_one_element_per_item() {
    let rust = compile(&in_component(
        r#"for task, idx in tasks : Text { text: task.title; }"#,
    ));
    assert!(rust.contains(".reactive_for"), "{rust}");
}

#[test]
fn every_built_in_widget_can_be_generated() {
    // One element of each kind the validator knows. Widgets that need children
    // or data get the minimum; the point is that codegen has a branch for each
    // and none of them panics.
    for widget in craftui_dsl::BUILTIN_WIDGETS {
        let source = format!("component Host {{ Column {{ {widget} {{ }} }} }}");
        let unit = resolve_source(&source, None);
        let problems: Vec<&str> = unit
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            problems.is_empty(),
            "`{widget} {{ }}` is rejected: {problems:#?}"
        );
        let rust = generate_rust(&unit.merged);
        assert!(
            rust.contains("pub fn build"),
            "no builder generated for `{widget}`"
        );
    }
}

#[test]
fn the_property_and_event_tables_cover_every_widget() {
    for widget in craftui_dsl::BUILTIN_WIDGETS {
        let props = craftui_dsl::props_for_widget(widget);
        assert!(
            props.contains(&"width") && props.contains(&"id"),
            "`{widget}` is missing the common properties"
        );
        let events = craftui_dsl::events_for_widget(widget);
        assert!(
            events.contains(&"on_tap"),
            "`{widget}` is missing the common events"
        );
    }
}
