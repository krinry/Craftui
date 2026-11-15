//! Behaviour of the reactive engine through its public API.
//!
//! The runtime is thread-local and the test harness runs each test on its own
//! thread, so tests cannot affect each other.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use craftui_reactive::{Scope, batch, reset_runtime, untrack};

/// A counter that closures can share.
fn counter() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

#[test]
fn an_effect_runs_once_immediately_and_again_on_each_change() {
    reset_runtime();
    let cx = Scope::root();
    let name = cx.signal("Ada".to_string());
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));

    let log = seen.clone();
    cx.effect(move || log.borrow_mut().push(name.get()));
    assert_eq!(*seen.borrow(), ["Ada"]);

    name.set("Grace".to_string());
    name.update(|n| n.push_str(" Hopper"));
    assert_eq!(*seen.borrow(), ["Ada", "Grace", "Grace Hopper"]);
}

#[test]
fn untracked_reads_do_not_subscribe() {
    reset_runtime();
    let cx = Scope::root();
    let tracked = cx.signal(1);
    let ignored = cx.signal(10);
    let also_ignored = cx.signal(100);
    let runs = counter();
    let last = Rc::new(Cell::new(0));

    let (r, l) = (runs.clone(), last.clone());
    cx.effect(move || {
        r.set(r.get() + 1);
        l.set(tracked.get() + ignored.get_untracked() + untrack(|| also_ignored.get()));
    });
    assert_eq!((runs.get(), last.get()), (1, 111));

    ignored.set(20);
    also_ignored.set(200);
    assert_eq!(
        runs.get(),
        1,
        "untracked signals must not re-run the effect"
    );

    tracked.set(2);
    assert_eq!(runs.get(), 2);
    assert_eq!(
        last.get(),
        222,
        "the re-run sees the latest untracked values"
    );
}

#[test]
fn set_if_changed_skips_equal_values() {
    reset_runtime();
    let cx = Scope::root();
    let level = cx.signal(3);
    let runs = counter();

    let r = runs.clone();
    cx.effect(move || {
        let _ = level.get();
        r.set(r.get() + 1);
    });
    assert_eq!(runs.get(), 1);

    assert!(!level.set_if_changed(3));
    assert_eq!(runs.get(), 1, "an equal value is not a change");

    assert!(level.set_if_changed(4));
    assert_eq!(runs.get(), 2);
    assert_eq!(level.get(), 4);
}

#[test]
fn nested_batches_flush_once_at_the_outermost_end() {
    reset_runtime();
    let cx = Scope::root();
    let a = cx.signal(0);
    let b = cx.signal(0);
    let runs = counter();

    let r = runs.clone();
    cx.effect(move || {
        let _ = a.get() + b.get();
        r.set(r.get() + 1);
    });
    assert_eq!(runs.get(), 1);

    let returned = batch(|| {
        a.set(1);
        batch(|| {
            b.set(2);
            a.set(3);
        });
        assert_eq!(runs.get(), 1, "the inner batch must not flush early");
        b.set(4);
        "done"
    });

    assert_eq!(returned, "done");
    assert_eq!(runs.get(), 2);
    assert_eq!((a.get(), b.get()), (3, 4));
}

#[test]
fn a_disposed_effect_stops_running() {
    reset_runtime();
    let cx = Scope::root();
    let value = cx.signal(0);
    let runs = counter();

    let r = runs.clone();
    let effect = cx.effect(move || {
        let _ = value.get();
        r.set(r.get() + 1);
    });
    value.set(1);
    assert_eq!(runs.get(), 2);

    effect.dispose();
    value.set(2);
    assert_eq!(runs.get(), 2);
    assert_eq!(value.get(), 2, "the signal itself still works");
}

#[test]
fn disposing_a_child_scope_stops_its_effects_but_not_the_parents() {
    reset_runtime();
    let root = Scope::root();
    let shared = root.signal(0);
    let child = root.child_scope();
    let parent_runs = counter();
    let child_runs = counter();

    let p = parent_runs.clone();
    root.effect(move || {
        let _ = shared.get();
        p.set(p.get() + 1);
    });
    let c = child_runs.clone();
    child.effect(move || {
        let _ = shared.get();
        c.set(c.get() + 1);
    });

    shared.set(1);
    assert_eq!((parent_runs.get(), child_runs.get()), (2, 2));

    child.dispose();
    assert!(!child.is_alive());
    assert!(root.is_alive());

    shared.set(2);
    assert_eq!(parent_runs.get(), 3);
    assert_eq!(child_runs.get(), 2, "the child's effect must be gone");
    assert!(shared.is_alive(), "the parent's signal outlives the child");
}

#[test]
fn disposing_a_scope_disposes_its_descendants() {
    reset_runtime();
    let root = Scope::root();
    let child = root.child_scope();
    let grandchild = child.child_scope();
    let deep = grandchild.signal("x");

    child.dispose();

    assert!(!grandchild.is_alive());
    assert!(!deep.is_alive());
    assert_eq!(deep.try_get(), None);
}

#[test]
fn computed_values_chain_and_stay_consistent() {
    reset_runtime();
    let cx = Scope::root();
    let price = cx.signal(100);
    let quantity = cx.signal(2);
    let subtotal = cx.computed(move || price.get() * quantity.get());
    let tax = cx.computed(move || subtotal.get() / 10);
    let total = cx.computed(move || subtotal.get() + tax.get());
    let seen = Rc::new(RefCell::new(Vec::<i32>::new()));

    let log = seen.clone();
    cx.effect(move || log.borrow_mut().push(total.get()));
    assert_eq!(total.get(), 220);

    quantity.set(3);
    assert_eq!(total.get(), 330);

    batch(|| {
        price.set(10);
        quantity.set(1);
    });
    assert_eq!(total.get(), 11);

    // The effect must never have observed a half-updated total.
    let seen = seen.borrow();
    assert_eq!(seen.first(), Some(&220));
    assert_eq!(seen.last(), Some(&11));
    assert!(
        seen.iter().all(|t| [220, 330, 11].contains(t)),
        "inconsistent totals were observed: {seen:?}"
    );
}

#[test]
fn an_effect_can_write_to_a_signal_it_does_not_read() {
    reset_runtime();
    let cx = Scope::root();
    let celsius = cx.signal(0.0_f32);
    let fahrenheit = cx.signal(0.0_f32);

    cx.effect(move || fahrenheit.set(celsius.get() * 9.0 / 5.0 + 32.0));
    assert_eq!(fahrenheit.get(), 32.0);

    celsius.set(100.0);
    assert_eq!(fahrenheit.get(), 212.0);
}

#[test]
fn a_read_only_handle_follows_its_signal() {
    reset_runtime();
    let cx = Scope::root();
    let source = cx.signal(1);
    let view = source.read_only();
    let seen = Rc::new(Cell::new(0));

    let s = seen.clone();
    cx.effect(move || s.set(view.get()));
    assert_eq!(seen.get(), 1);

    source.set(7);
    assert_eq!(view.get(), 7);
    assert_eq!(seen.get(), 7, "reads through the handle are tracked");
}

#[test]
fn context_is_inherited_and_can_be_overridden_by_a_child() {
    reset_runtime();
    let root = Scope::root();
    root.provide_context("root theme".to_string());
    let child = root.child_scope();
    let grandchild = child.child_scope();

    assert_eq!(
        grandchild.use_context::<String>().as_deref(),
        Some("root theme")
    );
    assert_eq!(grandchild.use_context::<u64>(), None);

    child.provide_context("child theme".to_string());
    assert_eq!(
        grandchild.use_context::<String>().as_deref(),
        Some("child theme")
    );
    assert_eq!(root.use_context::<String>().as_deref(), Some("root theme"));
}

#[test]
fn cleanups_run_once_when_the_scope_is_disposed() {
    reset_runtime();
    let root = Scope::root();
    let child = root.child_scope();
    let cleanups = counter();

    let c = cleanups.clone();
    child.on_cleanup(move || c.set(c.get() + 1));
    assert_eq!(cleanups.get(), 0);

    child.dispose();
    child.dispose();
    assert_eq!(cleanups.get(), 1);
}
