# Reactivity and Rust

A `.cui` file describes what the screen looks like for a given state. Rust owns
the state and the logic. This page explains how the two are connected.

## The idea

State lives in **signals**. A signal is a value that remembers who read it.
When the value changes, exactly the readers are re-run: the one `Text` that
shows it, the one effect that saves it. Nothing else is rebuilt or compared.

| Type | What it is |
|---|---|
| `Signal<T>` | A value that can be read and changed. Cheap to copy; copies refer to the same value. |
| `Computed<T>` | A value calculated from other signals, recalculated only when they change. |
| Effect | A closure that runs again whenever a signal it read changes. |
| `Scope` | Owns signals and effects, and frees them together. |

All of them are in `craftui::prelude`.

## Signals in Rust

```rust
use craftui::prelude::*;

let cx = Scope::root();

let count = cx.signal(0);
let doubled = cx.computed(move || count.get() * 2);

cx.effect(move || {
    println!("count is {}, doubled is {}", count.get(), doubled.get());
});
// prints: count is 0, doubled is 0

count.set(5);
// prints: count is 5, doubled is 10

count.update(|n| *n += 1);
// prints: count is 6, doubled is 12
```

| Method | Meaning |
|---|---|
| `signal.get()` | Read the value and subscribe the current effect or binding to it. |
| `signal.get_untracked()` | Read without subscribing. |
| `signal.set(value)` | Replace the value and notify readers. |
| `signal.set_if_changed(value)` | The same, but silent if the value is equal. Returns whether it changed. |
| `signal.update(\|v\| ...)` | Change the value in place. |
| `batch(\|\| { ... })` | Make several changes and notify readers once, at the end. |
| `untrack(\|\| { ... })` | Run code without subscribing to anything it reads. |

When you run an app, the framework creates the root scope and hands it to you
as `cx`. You only call `Scope::root()` yourself in tests.

## What a component compiles to

For this component:

```cui
export struct Task {
    title: string = "",
    done: bool = false,
}

export component TaskScreen {
    in-out property <[Task]> tasks: [];
    in-out property <string> draft: "";
    in property <string> summary: "";
    callback on_add();
    callback on_toggle(int);

    Column {
        padding: 16dp;
        gap: 8dp;

        Text { text: summary; color: Theme.text_secondary; }

        Row {
            gap: 8dp;

            TextInput { placeholder: "New task"; value <=> draft; }
            Button { text: "Add"; on_tap => { on_add(); } }
        }

        for task, idx in tasks : Checkbox {
            label: task.title;
            checked: task.done;
            on_change => { on_toggle(idx); }
        }
    }
}
```

`build.rs` generates Rust equivalent to:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub title: String,
    pub done: bool,
}

#[derive(Clone)]
pub struct TaskScreen {
    pub tasks: Signal<Vec<Task>>,
    pub draft: Signal<String>,
    pub summary: Signal<String>,
    // one private slot per callback
}

impl TaskScreen {
    pub fn new(cx: Scope) -> Self;
    pub fn set_on_add(&self, f: impl FnMut() + 'static);
    pub fn set_on_toggle(&self, f: impl FnMut(i32) + 'static);
    pub fn build(&self, cx: Scope, tree: &mut WidgetTree) -> NodeId;
}
```

So every property is a signal you can read and set from Rust, every callback
has a `set_...` method that takes a closure, and `build` creates the widgets.

## Connecting logic

Include the generated code, create the component, attach your logic, then
build it:

```rust
use craftui::prelude::*;

mod ui {
    include!(concat!(env!("OUT_DIR"), "/tasks.rs"));
}
use ui::{Task, TaskScreen};

fn mount(cx: Scope, tree: &mut WidgetTree) -> NodeId {
    let screen = TaskScreen::new(cx);

    // A callback: the UI tells Rust that something happened.
    let s = screen.clone();
    screen.set_on_add(move || {
        let title = s.draft.get_untracked().trim().to_string();
        if title.is_empty() {
            return;
        }
        s.tasks.update(|tasks| tasks.push(Task { title, done: false }));
        s.draft.set(String::new());
    });

    let s = screen.clone();
    screen.set_on_toggle(move |index| {
        s.tasks.update(|tasks| {
            if let Some(task) = tasks.get_mut(index as usize) {
                task.done = !task.done;
            }
        });
    });

    // An effect: Rust keeps a derived value up to date.
    let s = screen.clone();
    cx.effect(move || {
        let tasks = s.tasks.get();
        let open = tasks.iter().filter(|t| !t.done).count();
        s.summary.set(format!("{open} of {} to do", tasks.len()));
    });

    screen.build(cx, tree)
}
```

The data flows in a loop that is easy to follow:

1. The user taps a button. The `.cui` handler calls a callback.
2. The Rust closure changes one or more signals.
3. Every binding and effect that read those signals runs again.
4. The screen shows the new state.

Starting the app is one more call, which differs per platform:

```rust
// Desktop
run_app(WindowConfig::default(), |cx, tree, _ctx| mount(cx, tree))
```

The [Notes app guide](../guides/notes-app.md) shows the Web and Android entry
points.

## Rules of thumb

- **Keep real data in Rust.** Hold your model in ordinary Rust types, and
  publish to the UI only what it needs to show (a list of rows, a label). The UI
  reports intent through callbacks. This keeps the model testable without any
  UI.
- **An effect must not write a signal that it also reads.** It would trigger
  itself forever. Read with `get_untracked()` when you only need the current
  value. `set_if_changed` helps only when the update settles on a value that is
  already stored: it skips the write, and the loop stops. An update that always
  produces a new value, such as `count + 1`, still loops forever with
  `set_if_changed`.
- **Signals belong to the UI thread.** To use the result of background work,
  send it back to the UI thread first (the platform runners provide a
  dispatcher for this).
- **Group related changes** in `batch` so dependents run once.

## Testing without a window

Because a component is plain Rust, it can be built and driven in a unit test:

```rust
use craftui::prelude::*;
use craftui::reactive::reset_runtime;

#[test]
fn adding_a_task_updates_the_summary() {
    reset_runtime();                    // tests share one thread-local runtime
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let root = mount(cx, &mut tree);
    tree.set_root(root);
    tree.layout(WindowMetrics {
        logical_size: Size::new(390.0, 800.0),
        ..Default::default()
    });

    // Find widgets by the `id` given in the .cui file, send pointer or text
    // events with `tree.handle_event`, then check signals or widget rectangles.
}
```

`examples/notes/src/lib.rs` contains a complete test harness built this way,
including taps, typing, resizing and rendering to an image.
