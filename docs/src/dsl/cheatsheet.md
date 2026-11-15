# CraftUI Cheatsheet

A quick reference for the `.cui` language, built-in widgets, layout styles, and Rust reactive primitives.

## Component Skeleton

```cui
export component CheatsheetDemo {
    in-out property <int> counter: 0;
    in-out property <string> username: "";
    in-out property <bool> is_active: true;

    callback on_submit(string);

    Column {
        width: 100%;
        height: 100%;
        padding: 16dp;
        gap: 12dp;
        background: Theme.background;

        Text {
            text: "Hello, " + username;
            font_size: 20;
            color: Theme.text_primary;
        }

        TextInput {
            placeholder: "Enter name...";
            value <=> username;
        }

        Button {
            text: "Submit";
            background: Theme.primary;
            foreground: Theme.on_primary;
            on_tap => { on_submit(username); }
        }
    }
}
```

## Types & Properties

```text
in-out property <int> count: 0;           // 64-bit integer
in-out property <float> ratio: 0.5;       // 64-bit float
in-out property <string> text: "Hello";   // UTF-8 string
in-out property <bool> enabled: true;     // Boolean
in-out property <color> bg: #1E293B;      // Hex color
in-out property <length> pad: 16dp;       // Length (dp, px, %)
in-out property <[string]> tags: [];      // Array / List
callback on_action(string, int);          // Event callback to Rust
```

## Common Layout Properties

```text
width: 100%;            // or 250dp, auto
height: 100%;           // or 48dp, auto
min_width: 0dp;         // allow flex child to shrink below content
max_width: 720dp;       // upper constraint
padding: 16dp;          // all sides
padding_x: 12dp;        // left & right
padding_y: 8dp;         // top & bottom
margin: 8dp;            // outer spacing
gap: 12dp;              // spacing between children in Row/Column
flex_grow: 1.0;         // share available space
flex_shrink: 1.0;       // shrink when overflowing
flex_basis: 0dp;        // starting size on main axis
align: Center;          // Start | Center | End | Stretch
justify: SpaceBetween;  // Start | Center | End | SpaceBetween | SpaceAround
background: Theme.surface;
border_radius: 8dp;
border_color: Theme.border;
border_width: 1dp;
opacity: 0.95;
visible: is_active;
```

## Control Flow

### If / Else
```text
if is_logged_in : Column {
    Text { text: "Welcome back!"; }
} else : Button {
    text: "Log in";
    on_tap => { on_login(); }
}
```

### For Loops
```text
for item in item_list : Row {
    gap: 8dp;
    Text { text: item.name; flex_grow: 1.0; }
    Button { text: "Remove"; on_tap => { on_remove(item.id); } }
}
```

## Widgets Summary

| Widget | Purpose | Key Properties |
|---|---|---|
| `Column` | Vertical flexbox | `gap`, `align`, `justify` |
| `Row` | Horizontal flexbox | `gap`, `align`, `justify` |
| `Stack` | Layered overlays | `top`, `bottom`, `left`, `right` |
| `Grid` | Column grid | `columns: 3;`, `gap` |
| `ScrollView` | Scrollable view | `direction: Vertical;` or `Horizontal;` |
| `SafeArea` | Inset away from notches & status bars | Standard layout properties |
| `SplitPane` | Resizable split | `direction`, `ratio <=> split;` |
| `Spacer` | Expands to push siblings | `flex_grow: 1.0;` |
| `Divider` | Separation line | `thickness`, `color`, `direction` |
| `Text` | Label text | `text`, `font_size`, `font_weight`, `color` |
| `Button` | Tap action | `text`, `background`, `foreground`, `on_tap => {}` |
| `TextInput` | Single-line input | `value <=> text;`, `placeholder` |
| `TextArea` | Multi-line input | `value <=> body;`, `flex_grow: 1.0;` |
| `Checkbox` | Toggle check | `checked <=> is_checked;`, `label` |
| `Switch` | Toggle switch | `checked <=> is_on;` |
| `Slider` | Continuous value | `value <=> val;`, `min`, `max`, `step` |
| `Icon` | SVG icon | `svg: "name";`, `size`, `color` |
| `Card` | Panel container | `variant: Elevated;` (or `Filled`, `Outlined`) |
| `Modal` | Dialog overlay | `open <=> is_open;`, `title` |

## Theme Tokens

| Token | Semantic Role |
|---|---|
| `Theme.background` | Main window background |
| `Theme.surface` | Cards, popups, and elevated panels |
| `Theme.surface_variant` | Input fields, chips, secondary surfaces |
| `Theme.primary` | Accent & active highlight color |
| `Theme.on_primary` | Text/icons drawn on `Theme.primary` |
| `Theme.text_primary` | High-contrast body & header text |
| `Theme.text_secondary` | Muted subtitles, labels, and placeholders |
| `Theme.border` | Dividers and element outlines |
| `Theme.danger` | Destructive actions and error messages |
| `Theme.on_error` | Text/icons drawn on `Theme.danger` |

## Rust Reactivity

```rust
use craftui::prelude::*;

// Create scope
let cx = Scope::root();

// State signal
let count = cx.signal(0);
count.get();                // read (tracks dependency)
count.get_untracked();      // read without tracking
count.set(10);              // write & notify
count.update(|n| *n += 1);  // mutate in-place

// Computed value
let is_positive = cx.computed(move || count.get() > 0);

// Effect (runs when read signals change)
cx.effect(move || {
    println!("Count updated to: {}", count.get());
});
```
