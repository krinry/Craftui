# Widget catalog

Every widget that can be used in a `.cui` file, grouped by purpose, with the
properties and events each one accepts.

All widgets also accept the common layout and appearance properties (`width`,
`height`, `padding`, `background`, `border_radius`, `opacity`, `visible`, `id`
and the rest) and the common events (`on_tap`, `on_double_tap`,
`on_long_press`, `on_hover`). Those are described once in
[The `.cui` language](../dsl/syntax-guide.md#elements-and-properties) and are
not repeated here.

> The examples on this page are compiled by the test suite, so they stay in
> step with the framework. The list of widgets and properties comes from the
> validator in `crates/craftui-dsl/src/validator.rs`, which is the final
> authority.

## Layout

| Widget | What it does | Own properties |
|---|---|---|
| `Column` | Lays children out top to bottom. | `gap`, `align`, `justify` |
| `Row` | Lays children out left to right. | `gap`, `align`, `justify` |
| `Stack` | Layers children on top of each other. Position them with `top`, `left`, `right`, `bottom`. | `gap`, `align`, `justify` |
| `Grid` | Places children in a grid. | `columns`, `gap`, `align`, `justify` |
| `Container`, `View` | A plain box, for backgrounds, borders and padding. | `gap`, `align`, `justify` |
| `SafeArea` | Keeps children clear of notches, status bars and the on-screen keyboard. | `gap`, `align`, `justify` |
| `ScrollView` | Scrolls content that is larger than itself. | `direction` (`Vertical`, `Horizontal`), `gap`, `align`, `justify` |
| `SplitPane` | Two children with a draggable divider between them. | `direction`, `ratio` (two-way), `min_first`, `min_second`, `divider_size`; event `on_change` (`ratio`) |
| `Spacer` | Empty space. Give it `flex_grow: 1.0` to push siblings apart. | |
| `Divider` | A thin line. | `direction`, `thickness`, `color` |

```cui
export component LayoutDemo {
    in-out property <float> split: 0.35;

    SafeArea {
        Column {
            width: 100%;
            height: 100%;
            background: Theme.background;
            padding: 16dp;
            gap: 12dp;

            Row {
                width: 100%;
                align: Center;
                justify: SpaceBetween;

                Text { text: "Left"; color: Theme.text_primary; }
                Spacer { flex_grow: 1.0; }
                Text { text: "Right"; color: Theme.text_primary; }
            }

            Divider {
                direction: Horizontal;
                thickness: 1;
                color: Theme.border;
            }

            Grid {
                columns: 2;
                gap: 8dp;

                Container { height: 48dp; background: Theme.surface_variant; border_radius: 8dp; }
                Container { height: 48dp; background: Theme.surface_variant; border_radius: 8dp; }
                Container { height: 48dp; background: Theme.surface_variant; border_radius: 8dp; }
                Container { height: 48dp; background: Theme.surface_variant; border_radius: 8dp; }
            }

            SplitPane {
                width: 100%;
                height: 160dp;
                ratio <=> split;

                Column { Text { text: "First pane"; color: Theme.text_primary; } }
                Column { Text { text: "Second pane"; color: Theme.text_primary; } }
            }

            ScrollView {
                width: 100%;
                flex_grow: 1.0;

                Column {
                    width: 100%;
                    gap: 8dp;

                    for n in 0..30 : Text {
                        text: "Row " + n;
                        color: Theme.text_secondary;
                    }
                }
            }
        }
    }
}
```

### Making a list fill the rest of the screen

`ScrollView { flex_grow: 1.0; }` takes the space its parent has left and scrolls
its content inside it. This only works when the parent's own height is limited.
A parent that is sized by its content is as tall as the whole list, so nothing
needs to scroll and the end of the list is off the screen.

Give every container between the screen and the `ScrollView` a limited height
in one of these ways:

- `height: 100%;` (or a fixed height), or
- `flex_grow: 1.0;` together with `min_height: 0dp;`.

The second form is needed because a container that grows is never made smaller
than its content unless you set a minimum. `examples/notes/ui/notes.cui` uses
both: the row that holds the panes has `flex_grow: 1.0; min_height: 0dp;` and
each pane has `height: 100%;`.

## Text, icons and images

| Widget | What it does | Own properties |
|---|---|---|
| `Text` | Shows text. Wraps to the available width. | `text`, `font_size`, `font_weight` (`Light`, `Regular`, `Medium`, `SemiBold`, `Bold`), `color` |
| `Icon` | Shows a built-in icon by name, or inline SVG markup. | `svg` (or `icon`, `name`), `size`, `color` |
| `ImageView`, `Image` | Shows a texture supplied from Rust. | `source`, `fit` (`Contain`, `Cover`, `Fill`) |
| `Badge` | A small pill with a label and an optional icon. | `label`, `icon`, `color` |
| `Canvas` | An empty surface that Rust code draws on. See `examples/canvas_paint`. | |

Built-in icon names include `plus`, `minus`, `check`, `close`, `search`,
`settings`, `trash`, `edit`, `menu`, `home`, `folder`, `file`, `sun`, `moon`,
`info`, `warning`, `star`, `copy`, `download`, `upload`, `camera`, `image`,
`user`, `more_vert`, `share`, `lock`, `unlock`, `undo`, `redo`, `help`, `note`,
`list`, `grid`, `sort`, and `chevron_up`, `chevron_down`, `chevron_left`,
`chevron_right`. The full list is in `crates/craftui-render/src/svg.rs`.

```cui
export component TextDemo {
    in property <TextureHandle> photo;

    Column {
        padding: 16dp;
        gap: 12dp;

        Text {
            text: "Heading";
            font_size: 24;
            font_weight: Bold;
            color: Theme.text_primary;
        }
        Text {
            text: "Body text wraps when it reaches the edge of its parent.";
            font_size: 14;
            color: Theme.text_secondary;
        }

        Row {
            gap: 8dp;
            align: Center;

            Icon { svg: "star"; size: 20; color: Theme.primary; }
            Badge { label: "New"; icon: "check"; background: Theme.primary; color: Theme.on_primary; }
        }

        ImageView {
            width: 100%;
            height: 180dp;
            source: photo;
            fit: Cover;
            border_radius: 12dp;
        }
    }
}
```

## Buttons

| Widget | What it does | Own properties |
|---|---|---|
| `Button` | A push button. If `text` is the name of a built-in icon, the icon is shown instead. Can also hold child elements. | `text`, `foreground`, `font_size`, `font_weight`, `disabled` |
| `FilledButton`, `ElevatedButton`, `TonalButton`, `OutlinedButton`, `TextButton` | Material 3 button styles. | same as `Button` |
| `IconButton`, `FilledIconButton`, `TonalIconButton`, `OutlinedIconButton` | A round button showing one icon. | `icon`, `size`, `color`, `disabled` |
| `FloatingActionButton` | The prominent action of a screen. | `icon`, `label`, `foreground`, `disabled` |

React to a press with `on_tap`.

```cui
export component ButtonDemo {
    in-out property <int> presses: 0;

    Column {
        padding: 16dp;
        gap: 12dp;

        Row {
            gap: 8dp;

            Button {
                text: "Save";
                background: Theme.primary;
                foreground: Theme.on_primary;
                border_radius: 10dp;
                on_tap => { presses += 1; }
            }
            OutlinedButton { text: "Cancel"; on_tap => { presses = 0; } }
            TextButton { text: "Later"; }
            Button { text: "Disabled"; disabled: true; }
        }

        Row {
            gap: 8dp;

            IconButton { icon: "settings"; }
            FilledIconButton { icon: "plus"; on_tap => { presses += 1; } }
            FloatingActionButton { icon: "edit"; label: "Compose"; }
        }

        Text { text: "Pressed " + presses + " times"; color: Theme.text_secondary; }
    }
}
```

## Text entry and controls

| Widget | What it does | Own properties | Events |
|---|---|---|---|
| `TextInput` | One line of editable text. | `value` (two-way), `placeholder`, `font_size`, `font_weight`, `color` | `on_change`, `on_submit` (`text`) |
| `TextArea` | Several lines of editable text. | same as `TextInput` | `on_change` (`text`) |
| `Checkbox` | A tick box with a label. | `checked` (two-way), `label` | `on_change` (`checked`) |
| `Switch` | An on/off toggle with a label. | `checked` (two-way), `label` | `on_change` (`checked`) |
| `Slider` | Picks a number in a range. | `value` (two-way), `min`, `max`, `label` | `on_change` (`value`) |
| `Select`, `Dropdown` | Picks one option from a pop-up list. | `options`, `selected` (two-way), `placeholder` | `on_change` (`index`) |
| `RadioGroup` | Picks one option from a visible list. | `options`, `selected` (two-way), `direction` | `on_change` (`index`) |

The placeholder of a text field is also its name for screen readers, so always
set one.

```cui
export component FormDemo {
    in-out property <string> name: "";
    in-out property <string> bio: "";
    in-out property <bool> agreed: false;
    in-out property <bool> dark: false;
    in-out property <float> volume: 0.5;
    in-out property <int> language: 0;
    in-out property <int> density: 1;
    in-out property <string> status: "";

    Column {
        padding: 16dp;
        gap: 12dp;

        TextInput {
            width: 100%;
            placeholder: "Your name";
            value <=> name;
            on_submit => { status = "Submitted " + text; }
        }
        TextArea {
            width: 100%;
            height: 120dp;
            placeholder: "About you";
            value <=> bio;
        }
        Checkbox { label: "I agree to the terms"; checked <=> agreed; }
        Switch { label: "Dark mode"; checked <=> dark; }
        Slider { min: 0.0; max: 1.0; label: "Volume"; value <=> volume; }
        Select {
            options: ["English", "Hindi", "Japanese"];
            selected: language;
            on_change => { language = index; }
        }
        RadioGroup {
            direction: Horizontal;
            options: ["Compact", "Comfortable", "Spacious"];
            selected: density;
            on_change => { density = index; }
        }
        Text { text: status; color: Theme.text_secondary; }
    }
}
```

## Choosing between views

| Widget | What it does | Own properties | Events |
|---|---|---|---|
| `Tabs`, `TabBar` | A row of tabs. | `tabs`, `active` (two-way) | `on_change` (`index`) |
| `SegmentedControl` | A compact set of mutually exclusive choices. | `items`, `selected` (two-way) | `on_change` (`index`) |
| `BottomNavBar` | The bar at the bottom of a phone app. | `items`, `icons`, `active` (two-way) | `on_change` (`index`) |
| `Accordion` | A titled section that opens and closes. Holds child elements. | `title`, `expanded` (two-way) | `on_toggle` (`expanded`) |

These widgets only report which item is selected. Showing the matching content
is up to you, usually with `if`:

```cui
export component TabsDemo {
    in-out property <int> tab: 0;
    in-out property <int> section: 0;
    in-out property <bool> details_open: false;

    Column {
        width: 100%;
        height: 100%;
        gap: 12dp;

        Tabs {
            width: 100%;
            tabs: ["Inbox", "Sent", "Drafts"];
            active <=> tab;
        }

        if tab == 0 : Text { text: "Your inbox is empty"; color: Theme.text_primary; }
        if tab == 1 : Text { text: "Nothing sent yet"; color: Theme.text_primary; }
        if tab == 2 : Text { text: "No drafts"; color: Theme.text_primary; }

        Accordion {
            title: "Details";
            expanded <=> details_open;

            Text { text: "Shown only while the section is open."; color: Theme.text_secondary; }
        }

        Spacer { flex_grow: 1.0; }

        BottomNavBar {
            items: ["Home", "Search", "Profile"];
            icons: ["home", "search", "user"];
            active <=> section;
        }
    }
}
```

## Lists and tables

| Widget | What it does | Own properties | Events |
|---|---|---|---|
| `for` in a `Column` | Builds every item. Right for short lists. | | |
| `VirtualList` | Builds only the rows on screen. Right for long lists. | `item_extent` (row height), `direction` | |
| `VirtualGrid` | The same, in columns. | `columns`, `item_extent` | |
| `DataTable` | A table with a header, selectable rows and sortable columns. | `columns`, `rows` (a list of rows of text), `selected` (two-way), `striped`, `row_height` | `on_select` (`index`), `on_sort` (`column`) |
| `ListTile` | A standard list row with a title, subtitle and icons. | `title`, `subtitle`, `leading`, `trailing`, `selected` | |

```cui
export component ListDemo {
    in property <[[string]]> rows: [
        ["Camera", "Granted"],
        ["Location", "Denied"]
    ];
    in-out property <int> selected_row: 0;
    in-out property <int> sorted_by: 0;

    Column {
        width: 100%;
        height: 100%;
        padding: 16dp;
        gap: 12dp;

        DataTable {
            columns: ["Permission", "Status"];
            rows: rows;
            selected <=> selected_row;
            striped: true;
            row_height: 40;
            on_sort => { sorted_by = column; }
        }

        ListTile {
            title: "Export";
            subtitle: "Save a copy of this list";
            leading: "download";
        }

        VirtualList {
            width: 100%;
            flex_grow: 1.0;
            item_extent: 42;

            for n in 0..5000 : Row {
                width: 100%;
                padding_x: 12dp;
                align: Center;

                Text { text: "Item " + n; color: Theme.text_primary; }
            }
        }
    }
}
```

## Overlays and feedback

| Widget | What it does | Own properties | Events |
|---|---|---|---|
| `Modal` | Content shown above the page with a dimmed backdrop. | `open` (two-way) | `on_close` |
| `BottomSheet` | A panel that slides up from the bottom edge. | `open` (two-way) | `on_close` |
| `Drawer` | A panel that slides in from the side. | `open` (two-way), `width` | `on_close` |
| `PopupMenu` | A floating list of actions. | `open`, `items` | `on_select` (`index`), `on_close` |
| `Tooltip` | A hint shown when the pointer rests on its parent. | `text` | |
| `ToastHost` | Shows short messages supplied from Rust as a list of `ToastItem`. | `items` | `on_dismiss` (`id`) |
| `ProgressIndicator`, `LinearProgressIndicator` | A progress bar. | `progress` (`0.0` to `1.0`), `color` | |
| `Spinner`, `CircularProgressIndicator` | A spinning "busy" indicator. | `size`, `thickness`, `color` | |

Put overlays as the last children of a `Stack` that fills the window, so that
they cover everything else.

```cui
export component OverlayDemo {
    in-out property <bool> confirm_open: false;
    in-out property <bool> menu_open: false;
    in-out property <float> progress: 0.4;
    in-out property <string> last_action: "Nothing yet";

    Stack {
        width: 100%;
        height: 100%;
        background: Theme.background;

        Column {
            width: 100%;
            height: 100%;
            padding: 16dp;
            gap: 12dp;

            Button {
                text: "Delete";
                on_tap => { confirm_open = true; }

                Tooltip { text: "Removes the item for good"; }
            }
            Button { text: "More"; on_tap => { menu_open = !menu_open; } }
            ProgressIndicator { progress: progress; }
            Spinner { size: 20dp; thickness: 3dp; color: Theme.primary; }
            Text { text: last_action; color: Theme.text_secondary; }
        }

        PopupMenu {
            open: menu_open;
            items: ["Rename", "Duplicate", "Archive"];
            on_select => {
                menu_open = false;
                last_action = "Chose menu item " + index;
            }
        }

        Modal {
            open <=> confirm_open;

            Container {
                width: 300dp;
                padding: 20dp;
                background: Theme.surface;
                border_radius: 14dp;

                Column {
                    gap: 12dp;

                    Text { text: "Delete this item?"; font_size: 18; color: Theme.text_primary; }
                    Row {
                        gap: 8dp;
                        justify: End;

                        Button { text: "Cancel"; on_tap => { confirm_open = false; } }
                        Button {
                            text: "Delete";
                            background: Theme.danger;
                            foreground: Theme.on_error;
                            on_tap => {
                                confirm_open = false;
                                last_action = "Deleted";
                            }
                        }
                    }
                }
            }
        }
    }
}
```

## App structure

| Widget | What it does | Own properties | Events |
|---|---|---|---|
| `AppBar` | The bar at the top of a page. | `title`, `leading_icon`, `elevation` | `on_leading_tap` |
| `Card`, `ElevatedCard`, `FilledCard`, `OutlinedCard` | A surface that groups related content. | `gap`, `align`, `justify` | |
| `Chip`, `FilterChip`, `ActionChip`, `ChoiceChip`, `InputChip` | A compact, selectable label. | `label`, `selected` (two-way), `icon`, `disabled` | |
| `HorizontalDivider`, `VerticalDivider` | Dividers with a fixed direction. | `thickness`, `color` | |

```cui
export component PageDemo {
    in-out property <bool> drawer_open: false;
    in-out property <bool> only_unread: false;

    Stack {
        width: 100%;
        height: 100%;

        Column {
            width: 100%;
            height: 100%;
            background: Theme.background;

            AppBar {
                title: "Messages";
                leading_icon: "menu";
                on_leading_tap => { drawer_open = true; }
            }

            Column {
                width: 100%;
                padding: 16dp;
                gap: 12dp;

                Chip { label: "Unread only"; selected <=> only_unread; }

                Card {
                    width: 100%;
                    padding: 16dp;
                    gap: 6dp;

                    Text { text: "Asha"; font_weight: SemiBold; color: Theme.text_primary; }
                    Text { text: "See you at five."; color: Theme.text_secondary; }
                }
            }
        }

        Drawer {
            open <=> drawer_open;
            width: 280dp;

            Column {
                width: 100%;
                height: 100%;
                padding: 16dp;
                background: Theme.surface;

                Text { text: "Folders"; font_size: 18; color: Theme.text_primary; }
            }
        }
    }
}
```

## Material 3 components

`"std/material.cui"` is a library of ready-made components written in `.cui`
itself. Import the ones you need:

```cui
import { SearchBar, FilledButton, ElevatedCard } from "std/material.cui"
import { IconSearch } from "std/icons.cui"

export component SearchPage {
    in-out property <string> query: "";

    ElevatedCard {
        width: 360dp;
        padding: 16dp;

        IconSearch { }
        SearchBar {
            text <=> query;
            placeholder: "Search";
        }
        FilledButton { text: "Search"; }
    }
}
```

The library exports: `Card`, `ElevatedCard`, `FilledCard`, `OutlinedCard`,
`FilledButton`, `ElevatedButton`, `TonalButton`, `OutlinedButton`, `TextButton`,
`IconButton`, `FilledIconButton`, `TonalIconButton`, `OutlinedIconButton`,
`FloatingActionButton`, `Chip`, `FilterChip`, `ActionChip`, `ChoiceChip`,
`InputChip`, `SearchBar`, `NavigationBar`, `Switch`, `CheckBoxTile`,
`RadioButtonTile`, `SnackBar`, `Dialog`, `CircularProgressIndicator`,
`LinearProgressIndicator`, `HorizontalDivider`, `VerticalDivider`,
`BottomSheet`, `StateLayer` and `Ripple`. Their source, with every property, is
in `crates/craftui-dsl/src/std_material.rs`.

`"std/icons.cui"` exports icon components such as `IconSearch`, `IconClose`,
`IconArrowBack`, `IconArrowForward`, `IconMenu`, `IconCheck`, `IconAdd`,
`IconSettings`, `IconFavorite`, `IconShare`, `IconRefresh` and `IconMoreVert`,
each with `size` and `color` properties.

## Embedding native and web content

| Widget | What it does | Own properties |
|---|---|---|
| `HtmlView` | Shows an HTML page in a platform web view. | `src`, `html`, `url` |
| `PlatformView` | Reserves a rectangle for a native view that Rust code attaches by id. | `view_id` |

Both depend on the platform: check the platform guides before relying on them.

## HTML-style names

For people coming from the web, these names are accepted as aliases:

| Alias | Behaves like |
|---|---|
| `div`, `section`, `nav`, `header`, `footer`, `main`, `article`, `aside` | `Container` |
| `span`, `p`, `h1`, `h2`, `h3` | `Text` |
| `button` | `Button` |
| `input` | `TextInput` |

## Names that are accepted but not built yet

The validator accepts `Scaffold`, `Dialog`, `SnackBar`, `CheckBoxTile`,
`RadioButtonTile` and `SegmentedButton` as built-in names, but the code
generator has no dedicated widget for them: used without an import, each one
becomes a plain `Column`. `Dialog`, `SnackBar`, `CheckBoxTile` and
`RadioButtonTile` do exist as real components in `"std/material.cui"`, so import
them from there. For a dialog you can also use `Modal`; for a page frame use a
`Column` with an `AppBar` as its first child.
