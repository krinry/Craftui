# CraftUI UI Cookbook & Recipes

Production-ready, copy-paste recipes for common user interface patterns in CraftUI.

---

## 1. Sign-In Form with Validation

A complete login card with email and password inputs, an error banner, and a loading state.

```cui
export component SignInForm {
    in-out property <string> email: "";
    in-out property <string> password: "";
    in-out property <string> error_text: "";
    in-out property <bool> is_loading: false;

    callback on_sign_in(string, string);

    SafeArea {
        Column {
            width: 100%;
            height: 100%;
            align: Center;
            justify: Center;
            background: Theme.background;
            padding: 24dp;

            Column {
                width: 100%;
                max_width: 380dp;
                background: Theme.surface;
                border_radius: 12dp;
                padding: 24dp;
                gap: 16dp;

                Text {
                    text: "Welcome Back";
                    font_size: 24;
                    font_weight: Bold;
                    color: Theme.text_primary;
                }

                if error_text != "" : Text {
                    text: error_text;
                    font_size: 14;
                    color: Theme.danger;
                }

                TextInput {
                    placeholder: "Email address";
                    value <=> email;
                }

                TextInput {
                    placeholder: "Password";
                    value <=> password;
                }

                Button {
                    text: is_loading ? "Signing in..." : "Sign In";
                    background: Theme.primary;
                    foreground: Theme.on_primary;
                    border_radius: 8dp;
                    on_tap => {
                        if email != "" && password != "" {
                            on_sign_in(email, password);
                        }
                    }
                }
            }
        }
    }
}
```

---

## 2. Responsive Master-Detail Layout

Adapts between phone and wide screens (desktop or tablet). On a phone, it shows one pane at a time with a back button. On a desktop (720dp+), it displays the sidebar list and main detail pane side by side.

```cui
export component ResponsiveLayout {
    in-out property <bool> is_wide: false;
    in-out property <bool> show_detail: false;
    in-out property <string> detail_title: "Select an item";

    callback on_select_item(string);
    callback on_back();

    SafeArea {
        Row {
            width: 100%;
            height: 100%;
            background: Theme.background;

            // List Sidebar: visible on wide screens, or when detail is hidden on phone
            if is_wide || !show_detail : Column {
                width: is_wide ? 320dp : 100%;
                height: 100%;
                padding: 16dp;
                gap: 8dp;
                background: Theme.surface;

                Text {
                    text: "Inbox";
                    font_size: 20;
                    font_weight: Bold;
                    color: Theme.text_primary;
                }

                Button {
                    text: "Item 1";
                    background: Theme.surface_variant;
                    foreground: Theme.text_primary;
                    on_tap => { on_select_item("Item 1"); }
                }

                Button {
                    text: "Item 2";
                    background: Theme.surface_variant;
                    foreground: Theme.text_primary;
                    on_tap => { on_select_item("Item 2"); }
                }
            }

            // Divider on wide screen
            if is_wide : Divider {
                direction: Vertical;
                thickness: 1;
                color: Theme.border;
            }

            // Detail View: visible on wide screens, or when active on phone
            if is_wide || show_detail : Column {
                flex_grow: 1.0;
                height: 100%;
                padding: 24dp;
                gap: 16dp;

                if !is_wide : Button {
                    text: "Back";
                    on_tap => { on_back(); }
                }

                Text {
                    text: detail_title;
                    font_size: 24;
                    font_weight: Bold;
                    color: Theme.text_primary;
                }

                Text {
                    text: "Detail body content goes here.";
                    font_size: 16;
                    color: Theme.text_secondary;
                }
            }
        }
    }
}
```

---

## 3. Search Bar with Filter Tabs

A search interface with a real-time text input and filter tab chips.

```cui
export component SearchWithFilter {
    in-out property <string> query: "";
    in-out property <int> active_tab: 0;

    callback on_tab_changed(int);

    Column {
        width: 100%;
        padding: 16dp;
        gap: 12dp;
        background: Theme.surface;

        TextInput {
            placeholder: "Search items...";
            value <=> query;
        }

        Row {
            gap: 8dp;
            align: Center;

            Button {
                text: "All";
                background: active_tab == 0 ? Theme.primary : Theme.surface_variant;
                foreground: active_tab == 0 ? Theme.on_primary : Theme.text_primary;
                border_radius: 16dp;
                on_tap => {
                    active_tab = 0;
                    on_tab_changed(0);
                }
            }

            Button {
                text: "Starred";
                background: active_tab == 1 ? Theme.primary : Theme.surface_variant;
                foreground: active_tab == 1 ? Theme.on_primary : Theme.text_primary;
                border_radius: 16dp;
                on_tap => {
                    active_tab = 1;
                    on_tab_changed(1);
                }
            }
        }
    }
}
```

---

## 4. Settings Card with Toggles

Demonstrates using `Switch`, `Checkbox`, and `Slider` in a settings panel.

```cui
export component SettingsCard {
    in-out property <bool> notifications_enabled: true;
    in-out property <bool> dark_mode: false;
    in-out property <float> volume: 75.0;

    callback on_toggle_notifications(bool);

    Column {
        width: 100%;
        max_width: 500dp;
        padding: 16dp;
        gap: 16dp;
        background: Theme.surface;
        border_radius: 12dp;

        Text {
            text: "Preferences";
            font_size: 18;
            font_weight: SemiBold;
            color: Theme.text_primary;
        }

        Row {
            width: 100%;
            align: Center;
            justify: SpaceBetween;

            Text { text: "Notifications"; color: Theme.text_primary; }
            Switch {
                checked <=> notifications_enabled;
                on_change => { on_toggle_notifications(checked); }
            }
        }

        Divider { direction: Horizontal; thickness: 1; color: Theme.border; }

        Row {
            width: 100%;
            align: Center;
            justify: SpaceBetween;

            Text { text: "Dark Mode"; color: Theme.text_primary; }
            Switch { checked <=> dark_mode; }
        }

        Divider { direction: Horizontal; thickness: 1; color: Theme.border; }

        Column {
            width: 100%;
            gap: 8dp;

            Text { text: "Volume: " + volume; color: Theme.text_primary; }
            Slider {
                value <=> volume;
                min: 0.0;
                max: 100.0;
            }
        }
    }
}
```

---

## 5. Wiring Recipes to Rust Controllers

In Rust, you connect data models to these screens using `Scope` and `Signal`:

```rust
use craftui::prelude::*;

pub struct FormController {
    email: Signal<String>,
    password: Signal<String>,
    error_text: Signal<String>,
    is_loading: Signal<bool>,
}

impl FormController {
    pub fn new(cx: Scope) -> Self {
        Self {
            email: cx.signal(String::new()),
            password: cx.signal(String::new()),
            error_text: cx.signal(String::new()),
            is_loading: cx.signal(false),
        }
    }

    pub fn handle_submit(&self, email: String, pass: String) {
        if !email.contains('@') {
            self.error_text.set("Please enter a valid email.".into());
            return;
        }
        self.error_text.set(String::new());
        self.is_loading.set(true);
        // Dispatch async task or network request...
    }
}
```
