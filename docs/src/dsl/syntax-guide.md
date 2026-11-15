# The `.cui` language

A `.cui` file describes a piece of user interface: what is on screen, how it is
laid out, and what happens when the user interacts with it. `build.rs` compiles
each file to plain Rust before your crate builds, so mistakes are reported at
compile time and nothing is interpreted at run time.

> Every example tagged `cui` in this book is compiled by the test suite
> (`crates/craftui-dsl/tests/docs_examples.rs`). If an example stops matching
> the language, CI fails.

## A complete file

```cui
export component Counter {
    in-out property <int> count: 0;
    callback on_reset();

    Column {
        width: 100%;
        height: 100%;
        align: Center;
        justify: Center;
        gap: 16dp;
        background: Theme.background;

        Text {
            text: "Count: " + count;
            font_size: 28;
            font_weight: Bold;
            color: Theme.text_primary;
        }

        Row {
            gap: 12dp;

            Button {
                text: "Add one";
                background: Theme.primary;
                foreground: Theme.on_primary;
                on_tap => { count += 1; }
            }
            Button {
                text: "Reset";
                on_tap => {
                    count = 0;
                    on_reset();
                }
            }
        }
    }
}
```

A file can contain four kinds of top-level items, in any order:

| Item | Purpose |
|---|---|
| `import { A, B } from "path.cui"` | Use components, structs or enums from another file. |
| `struct Name { field: type = default, }` | A record type, usually for list items. |
| `enum Name { A, B }` | A set of named choices. |
| `component Name { ... }` | A reusable piece of UI. |

Put `export` in front of a struct, enum or component to make it visible to
other `.cui` files and to give the generated Rust type `pub` visibility.

Comments are `// to the end of the line` and `/* between markers */`.

## Components

A component has three parts: **properties** (its state), **callbacks** (how it
tells Rust that something happened) and exactly one **root element**.

```text
component Name {
    <direction> property <Type> name: default;
    callback name(Type, Type);

    RootElement { ... }
}
```

### Properties

```text
in property <string> title: "Untitled";
in-out property <bool> expanded: false;
property <int> clicks: 0;
```

The direction says who may change the property, and the compiler enforces it:

| Direction | Meaning |
|---|---|
| `in` | Set by whoever uses the component. The component itself cannot assign to it or bind it with `<=>`. |
| `out` | Set by the component, read by its user. |
| `in-out` | Both sides read and write it. Needed for two-way bindings. |
| *(none)* | Internal state. |

Every property becomes a reactive `Signal<T>` field on the generated Rust
struct, whatever its direction, so Rust code can always read and set it. The
default after the colon is optional; without one, the type's default value is
used.

### Types

| `.cui` type | Rust type | Example literal |
|---|---|---|
| `int` | `i32` | `42` |
| `uint`, `usize` | `usize` | `3` |
| `float` | `f32` | `0.5` |
| `bool` | `bool` | `true` |
| `string` | `String` | `"hello"` |
| `color` | `Color` | `#4f46e5` |
| `[T]` | `Vec<T>` | `["a", "b"]` |
| `Option<T>` | `Option<T>` | |
| `Map<K, V>` | `HashMap<K, V>` | |
| `TextureHandle` | `Option<TextureId>` | |
| a `struct` or `enum` name | the generated type | `Priority.High` |

### Callbacks

A callback is a function the Rust side provides. Declare it with its parameter
types and call it from an event handler like any function:

```text
callback on_save();
callback on_select(int);
```

In Rust the generated struct gets a `set_on_save(closure)` method for each one.
See [Reactivity and Rust](reactivity.md) for the Rust half.

## Elements and properties

An element is a widget name followed by a block. Inside the block you set
properties, handle events, and nest child elements:

```text
Row {
    gap: 8dp;                      // property: value;
    on_tap => { count += 1; }      // event => handler
    Text { text: "child"; }        // child element
}
```

Each widget accepts a fixed set of properties, listed in the
[widget catalog](../widgets/catalog.md). Using a property that a widget does not
have is a compile error.

### Layout properties

These work on every element:

| Property | Value | Notes |
|---|---|---|
| `width`, `height` | length | `100%` fills the parent. |
| `min_width`, `max_width`, `min_height`, `max_height` | length | |
| `flex_grow`, `flex_shrink` | number | `flex_grow: 1.0` takes the remaining space. |
| `padding` | number | All four sides. `padding: SafeArea.insets` pads by the device's safe area. |
| `padding_x`, `padding_y` | number | Horizontal and vertical padding. |
| `margin` | number | All four sides. |
| `top`, `bottom`, `left`, `right` | number | Positions the element absolutely inside its parent. |
| `visible` | bool | |

Containers (`Column`, `Row`, `Stack`, `Container`, `ScrollView` and others) also
take:

| Property | Values |
|---|---|
| `gap` | number: space between children |
| `align` | `Start`, `Center`, `End`, `Stretch`: cross-axis alignment |
| `justify` | `Start`, `Center`, `End`, `SpaceBetween`, `SpaceAround`, `SpaceEvenly`: main-axis distribution |

### Appearance properties

These also work on every element:

| Property | Value |
|---|---|
| `background` | a colour or a gradient |
| `border_radius`, `border_width` | number |
| `border_color` | colour |
| `opacity` | `0.0` to `1.0` |
| `shadow` | `true` for the default shadow |
| `clip` | bool: clip children to this element's bounds |
| `backdrop_blur` | number: blur what is behind the element (glass effect) |
| `backdrop_tint` | colour laid over the blurred backdrop |
| `scale`, `scale_x`, `scale_y`, `rotate`, `translate_x`, `translate_y`, `skew_x`, `skew_y` | number |
| `cursor` | `Pointer`, `Text`, ... (desktop and web) |
| `disabled` | bool |
| `id` | string: lets Rust find the node with `tree.node_by_id("...")` |

### Units and literals

| Literal | Meaning |
|---|---|
| `12dp` | Density-independent pixels. A bare number means the same. |
| `12px` | Physical pixels. |
| `50%` | Percentage of the parent. |
| `90deg`, `1.57rad` | Angles. |
| `200ms`, `0.2s` | Durations. |
| `#rgb`, `#rrggbb`, `#rrggbbaa` | Colours. |
| `@linear-gradient(90deg, #6750A4, #D0BCFF)` | Linear gradient. |
| `@radial-gradient(#ffffff, #000000)` | Radial gradient. |

## Expressions

Property values and conditions are expressions:

| Kind | Examples |
|---|---|
| Arithmetic | `a + b`, `a - b`, `a * b`, `a / b`, `-a` |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| Logic | `&&`, `\|\|`, `!flag` |
| Choice | `flag ? 320dp : 220dp` |
| Text | `"Hello, " + name` (numbers are converted when joined to a string) |
| Member access | `task.title`, `Theme.primary`, `Priority.High` |
| Indexing | `items[0]` |
| Text methods | `name.is_empty()`, `name.len()`, `name.trim()`, `name.to_lowercase()`, `name.to_uppercase()`, `name.contains("x")`, `name.starts_with("x")`, `name.ends_with("x")` |
| List methods | `items.len()`, `items.is_empty()` |
| Maths | `min(a, b)`, `max(a, b)`, `clamp(x, lo, hi)`, `abs(x)`, `round(x)`, `floor(x)`, `ceil(x)`, `sqrt(x)`, `pow(x, y)`, `sin(x)`, `cos(x)`, `tan(x)`; also as `Math.min(a, b)` and so on |
| Colour | `rgb(79, 70, 229)`, `rgba(79, 70, 229, 128)` |
| Range | `0..10` |
| List | `["All", "Active", "Done"]` |
| Struct | `Task { title: "New", done: false }` |

### Values that are always available

| Name | What it gives you |
|---|---|
| `Theme.<token>` | Theme colours such as `background`, `surface`, `surface_variant`, `primary`, `on_primary`, `text_primary`, `text_secondary`, `border`, `danger`, `on_error`; sizes `radius_md` and `spacing_xs` to `spacing_xxl`; and `Theme.mode`. |
| `ThemeMode.<mode>` | `Light`, `Dark`, `Amoled`, `HighContrast`, for comparing with `Theme.mode`. |
| `Window.width`, `Window.height`, `Window.scale_factor` | Window size in dp and the display scale. |
| `Window.is_compact`, `Window.is_medium`, `Window.is_expanded` | Width breakpoints: under 600dp, 600 to 1024dp, over 1024dp. |
| `Window.breakpoint` | The breakpoint as text. |
| `SafeArea.insets` | Used as `padding: SafeArea.insets`. |
| `Math.PI`, `Math.E`, `Math.SQRT2`, ... | Constants. |
| `root.<property>` | A property of the enclosing component, for when a local name hides it. |

`Theme` and `Window` values are reactive: an expression that reads them is
re-evaluated when the theme or the window size changes.

## Bindings

A property set from an expression is a **one-way binding**. When any property
the expression reads changes, the value is recalculated and only the affected
widget is updated. There is nothing to subscribe to and nothing to refresh by
hand.

`<=>` makes a **two-way binding** between a widget and a component property: the
widget shows the property, and when the user edits the widget the property is
updated.

```cui
export component ProfileForm {
    in-out property <string> name: "";
    in-out property <bool> newsletter: false;
    in-out property <float> volume: 0.5;

    Column {
        padding: 16dp;
        gap: 12dp;

        TextInput {
            placeholder: "Your name";
            value <=> name;
        }
        Switch {
            label: "Send me the newsletter";
            checked <=> newsletter;
        }
        Slider {
            min: 0.0;
            max: 1.0;
            value <=> volume;
        }
        Text {
            text: name.is_empty() ? "Hello!" : "Hello, " + name + "!";
            color: Theme.text_primary;
        }
    }
}
```

Two-way bindings are available on these widget properties:

| Widget | Properties |
|---|---|
| `TextInput`, `TextArea` | `value`, `text` |
| `Slider` | `value` |
| `Checkbox`, `Switch` | `checked`, `value` |
| `Tabs`, `TabBar`, `SegmentedControl`, `BottomNavBar` | `active`, `selected`, `value` |
| `Select`, `Dropdown`, `RadioGroup` | `selected`, `value` |
| `Accordion` | `expanded`, `open` |
| `Modal`, `BottomSheet`, `PopupMenu`, `Drawer` | `open`, `visible` |
| `SplitPane` | `ratio` |
| `DataTable` | `selected` |
| Chips | `selected`, `value` |
| Your own components | any `in-out` property |

## Events

An event handler is the event name, `=>`, and one statement or a block:

```text
on_tap => count += 1;
on_tap => {
    count = 0;
    on_reset();
}
```

Statements can assign (`=`, `+=`, `-=`, `*=`, `/=`), branch with
`if cond { } else { }`, and call callbacks.

| Event | Widgets | Names available in the handler |
|---|---|---|
| `on_tap`, `on_double_tap`, `on_long_press` | all | |
| `on_hover` | all | `hovered` (bool) |
| `on_change` | `Slider` | `value` (float) |
| `on_change` | `Checkbox`, `Switch` | `checked` (bool) |
| `on_change`, `on_submit` | `TextInput`, `TextArea` | `text` (string) |
| `on_change`, `on_select` | `Tabs`, `TabBar`, `SegmentedControl`, `BottomNavBar`, `Select`, `Dropdown`, `RadioGroup` | `index` (int) |
| `on_toggle` | `Accordion` | `expanded` (bool) |
| `on_select`, `on_sort` | `DataTable` | `index`, `column` (int) |
| `on_select`, `on_close` | `PopupMenu` | `index` (int) |
| `on_close` | `Modal`, `BottomSheet`, `Drawer` | |
| `on_dismiss` | `ToastHost` | `id` (int) |
| `on_leading_tap` | `AppBar` | |
| a callback of your own component | that component | `value` (first argument), `arg0`, `arg1`, ... |

In every handler that receives something, `value` is also available as a second
name for it.

```cui
export component Player {
    in-out property <int> tab: 0;
    in-out property <string> status: "Stopped";

    Column {
        padding: 16dp;
        gap: 12dp;

        Tabs {
            tabs: ["Library", "Queue"];
            active: tab;
            on_change => { tab = index; }
        }
        Button {
            text: "Play";
            on_tap => {
                if tab == 0 {
                    status = "Playing the library";
                } else {
                    status = "Playing the queue";
                }
            }
        }
        Text {
            text: status;
            color: Theme.text_secondary;
        }
    }
}
```

## Conditions

`if` mounts an element only while a condition is true. Note the colon between
the condition and the element. `else` and `else if` are optional.

```cui
export component Inbox {
    in-out property <int> unread: 0;

    Column {
        padding: 16dp;
        gap: 8dp;

        if unread == 0 : Text {
            text: "You are all caught up";
            color: Theme.text_secondary;
        } else if unread == 1 : Text {
            text: "1 unread message";
            color: Theme.text_primary;
        } else : Text {
            text: unread + " unread messages";
            color: Theme.text_primary;
        }

        Button {
            text: "Receive a message";
            on_tap => { unread += 1; }
        }
    }
}
```

The two forms behave differently, and it is worth knowing how:

| Form | What happens when the condition changes |
|---|---|
| `if cond : A { }` | `A` is built once and stays in the tree. While the condition is false it is hidden and takes no space. Its state (scroll position, text being typed) survives. |
| `if cond : A { } else : B { }` | Only one branch exists at a time. Switching removes one and builds the other from scratch. |

The `if ... else` form places the active branch inside an invisible wrapper.
The wrapper copies a literal `width`, `height` or `flex_grow` from the branches,
so a branch that fills its parent keeps doing so. Give both branches the same
sizing; if they differ, the first branch that sets a value wins.

For something that only needs to look different, use a `? :` expression on a
property instead of two elements.

## Lists

`for` repeats an element for every item of a list. The optional second name is
the item's index.

```cui
export struct Task {
    title: string = "",
    done: bool = false,
}

export component TaskList {
    in-out property <[Task]> tasks: [];
    callback on_toggle(int);

    ScrollView {
        width: 100%;
        height: 100%;

        Column {
            width: 100%;
            gap: 8dp;
            padding: 16dp;

            for task, idx in tasks : Row {
                width: 100%;
                gap: 10dp;
                align: Center;

                Checkbox {
                    checked: task.done;
                    label: "";
                    on_change => { on_toggle(idx); }
                }
                Text {
                    text: task.title;
                    color: Theme.text_primary;
                }
            }
        }
    }
}
```

You can also loop over a literal list (`for label in ["A", "B"] : ...`) or a
range (`for n in 0..5 : ...`).

A common pattern, used in the example above, is to keep the real data in Rust,
publish the items to show through a list property, and report user actions back
with a callback that carries the index. The
[notes example](../guides/notes-app.md) is built this way.

For lists with thousands of rows use `VirtualList`, which only builds the rows
that are on screen.

## Animation

`animate` makes changes to a property smooth instead of instant. Name the
properties, then choose a curve:

```cui
export component HoverCard {
    in-out property <bool> hovered: false;

    Container {
        padding: 16dp;
        border_radius: 12dp;
        background: hovered ? Theme.primary : Theme.surface_variant;
        scale: hovered ? 1.04 : 1.0;
        animate scale { curve: snappy(); }
        animate background { duration: 0.2; easing: EaseOut; }
        on_hover => { hovered = value; }

        Text {
            text: "Hover me";
            color: Theme.text_primary;
        }
    }
}
```

| Curve | Meaning |
|---|---|
| `curve: snappy();` | A quick spring. |
| `curve: gentle();` | A soft spring. |
| `curve: instant();` | No animation. |
| `curve: spring(380.0, 24.0);` | A spring with the given stiffness and damping. |
| `stiffness: 280.0; damping: 26.0;` | The same, written as fields. |
| `duration: 0.2; easing: EaseOut;` | A timed tween. Easings: `Linear`, `EaseIn`, `EaseOut`, `EaseInOut`. |

Properties that can be animated: `opacity`, `scale`, `scale_x`, `scale_y`,
`rotate`, `translate_x`, `translate_y`, `background` and `border_color`.

## Splitting UI across files

Any exported component, struct or enum can be imported by path, relative to the
importing file:

```text
import { SectionHeader, TagChip } from "./tokens.cui"
```

Use an imported component like a built-in widget. Set its `in` properties, bind
its `in-out` properties with `<=>`, and handle its callbacks as events:

```cui
component Stepper {
    in-out property <int> value: 0;
    in property <string> label: "Value";
    callback on_changed(int);

    Row {
        gap: 8dp;
        align: Center;

        Text {
            text: label + ": " + value;
            color: Theme.text_primary;
        }
        Button {
            text: "+";
            on_tap => {
                value += 1;
                on_changed(value);
            }
        }
    }
}

export component Cart {
    in-out property <int> quantity: 1;
    in-out property <string> note: "";

    Column {
        padding: 16dp;
        gap: 12dp;

        Stepper {
            label: "Quantity";
            value <=> quantity;
            on_changed => { note = "Quantity is now " + value; }
        }
        Text {
            text: note;
            color: Theme.text_secondary;
        }
    }
}
```

Two libraries are built in and need no files of their own:

| Import from | Contents |
|---|---|
| `"std/material.cui"` | Material 3 components: cards, chips, search bar, navigation bar and more. See the [widget catalog](../widgets/catalog.md#material-3-components). |
| `"std/icons.cui"` | Icon components such as `IconSearch`, `IconAdd`, `IconSettings`. |

## What is not supported yet

So that you do not lose time on them:

- **`states [ ... ]` and `transitions [ ... ]` blocks are parsed and checked, but
  the code generator ignores them.** They have no effect on the running app.
  Use `? :` expressions together with `animate`, as shown above.
- A component cannot take child elements from its user (there is no slot or
  `@children` mechanism), except for the built-in containers.
- There are no local variables inside a component other than its properties.
- There are no functions in `.cui`. Put logic in Rust and call it through a
  callback.

If you have seen older CraftUI material, note that the following never worked
and are not part of the language: `state { let x = 0; }` blocks,
`on_click: { ... }` handlers (use `on_tap => { ... }`), `if x { ... }` without
the colon, colours written as strings (`"#fff"`), and the property names
`background_color`, `corner_radius`, `align_items` and `justify_content` (use
`background`, `border_radius`, `align` and `justify`).

## Checking a file without building

```bash
craftui check ui/main.cui            # parse and validate
craftui check ui/main.cui --emit-rust   # also print the generated Rust
```

The VS Code extension in `editors/vscode/` runs the same checks as you type.
