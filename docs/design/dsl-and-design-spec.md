# CraftUI — DSL, Reactivity, Animation & Design System Spec (`DESIGN.md`)

## 1. Declarative Authoring Model

CraftUI gives teams two first-class authoring styles backed by one reactive retained core:
1. **`.cui` Declarative DSL:** Hot-reloaded in dev, compiled strictly AOT into static Rust in release builds (`build.rs`).
2. **`cui!` Rust Proc-Macro + `#[component]`:** Pure single-language Rust UI authoring with inline reactivity, conditionals, loops, and `rust-analyzer` type safety.

---

## 2. Upgraded `.cui` Language Specification

### 2.1 Example: Animated Glassmorphic Settings Card in `.cui`

```slint
import { Button, Switch, Slider, Icon } from "std/widgets.cui"
import { Theme } from "theme/tokens.cui"

export component ControlCard {
    in property <string> title: "Studio Glass";
    in-out property <bool> active: true;
    in-out property <float> blur_amount: 16.0;
    callback on_saved();

    Column {
        width: 100%;
        padding: 20dp;
        gap: 14dp;
        border_radius: 16dp;
        background: Theme.surface;
        border_width: 1;
        border_color: active ? Theme.primary : Theme.border;
        backdrop_blur: active ? blur_amount : 0.0;
        scale: active ? 1.0 : 0.985;
        opacity: active ? 1.0 : 0.72;
        shadow: 18;

        // First-class declarative property animations
        animate background, border_color, opacity { duration: 180ms; easing: EaseOut; }
        animate scale { spring(280, 22); }

        Row {
            justify: SpaceBetween;
            align: Center;
            Text { text: title; font_size: 18; font_weight: Bold; color: Theme.text_primary; }
            Switch { checked <=> active; label: "Enabled"; }
        }

        Button {
            background: Theme.primary;
            border_radius: 10dp;
            on_tap => { on_saved(); }
            Row {
                gap: 8dp;
                align: Center;
                Icon { svg: "check"; size: 16; color: Theme.on_primary; }
                Text { text: "Save Preset"; color: Theme.on_primary; font_weight: SemiBold; }
            }
        }
    }
}
```

### 2.2 Strict Semantic & Binding Rules
1. **Per-Widget Schema & Compile Errors:** Every built-in widget defines its exact valid properties, value types, events, and whether children are permitted. Unknown widgets, wrong properties on a widget (e.g. `Divider { checked: true; }`), out-of-scope identifiers, or type mismatches emit **`Severity::Error`** and fail the build rather than silently substituting defaults or string literals.
2. **All Visual, Layout & Transform Properties Reactive:** `width`, `height`, `padding`, `margin`, `gap`, `background`, `border_radius`, `border_width`, `border_color`, `opacity`, `scale`, `translate_x`, `translate_y`, `rotate`, `backdrop_blur`, `shadow`, `font_size`, `font_weight`, and `cursor` all accept live reactive expressions (`prop * 2`, ternary/conditional or `Theme.*`).
3. **Strict `:` vs `<=>` Semantics:**
   - `prop: expr;` creates a **one-way** read-only binding (cannot mutate upstream signal inside child interactive controls unless explicitly wired via `on_change`).
   - `prop <=> signal_prop;` creates a **two-way** bidirectional Signal link and requires a writable lvalue signal (rejected with `Error` otherwise).
4. **Stable Keyed Loops:** `for item, idx in list [key: item.id] : Widget { ... }` keys reconciled nodes by explicit `key:` (or item identity without array index!), ensuring list insertions, deletions, and reorders preserve child widget state, focus, and running animations.
5. **Rich Event Handler Statements:** Supports direct prop assignments (`count = count + 1;`), struct field updates (`item.done = !item.done;`), compound assignments (`+=`, `-=`), `if / else` branches, and callback/method calls.
6. **Composable Interactive Containers:** `Button` and cards support both shortcut props (`text: "Save"`) and nested custom children (`Row { Icon ... Text ... }`).
7. **Declarative `animate` Blocks:**
   - Syntax: `animate <prop1>, <prop2> { duration: 200ms; easing: EaseOut; }` or `animate scale { spring(260, 24); }`.
   - Runtime maintains `AnimatedProperty` timelines on `WidgetNode` and requests VSync redraws only while animations are active.

---

## 3. Fine-Grained Signal API & Memory Rules (`craftui-reactive`)

* `Signal<T>` / `ReadSignal<T>` / `Computed<T>` / `Effect`: Generational `Copy` arena handles (`slotmap`).
* Modifying any visual property binding (e.g. `set_box_style`) cleans up/replaces superseded property effects so repeated updates never accumulate orphaned subscriptions.
* Scoped child lifetimes (`Scope::child_scope()`) deterministically free signals, layout nodes, animations, and timers when conditional branches or keyed list items unmount.

---

## 4. Visual Polish & Theme System (`ThemeTokens`)

* Built-in presets: `Light`, `Dark`, `Amoled`, `HighContrast`, plus extensible brand overrides (`ThemeOverride`).
* Theme-aware focus indicators (`primary`-tinted ring + high-contrast outline), layered `BoxShadow` specs (offset, blur, spread, corner radii, optional `inset`), and multi-stop gradients.
* Keyboard UX parity across OSes: `Cmd` (`super_key`) on macOS and `Ctrl` on Windows/Linux/Web, plus undo/redo and word cursor navigation in `TextInput`/`TextArea`.
