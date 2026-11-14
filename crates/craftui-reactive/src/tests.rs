use std::cell::Cell;
use std::rc::Rc;

use craftui_core::{DirtyFlags, NodeId, UiDispatcher};
use slotmap::SlotMap;

use crate::arena::{
    batch, drain_dirty_nodes, next_timer_delay_ms, reset_runtime, tick_timers_at,
};
use crate::scope::Scope;

#[test]
fn signals_are_copy_and_track_effects() {
    reset_runtime();
    let cx = Scope::root();
    let count = cx.signal(0i32);
    let label = cx.signal("hello".to_string());
    let observed = Rc::new(Cell::new(0));
    let obs = observed.clone();

    // Notice: `count` and `label` are Copy even though `String` is !Copy.
    cx.effect(move || {
        obs.set(count.get() + label.with(|s| s.len() as i32));
    });
    assert_eq!(observed.get(), 5);

    count.set(10);
    assert_eq!(observed.get(), 15);
    label.update(|s| s.push('!'));
    assert_eq!(observed.get(), 16);
}

#[test]
fn computed_is_lazy_and_diamond_consistent() {
    reset_runtime();
    let cx = Scope::root();
    let base = cx.signal(2);
    let eval_count = Rc::new(Cell::new(0));
    let ec = eval_count.clone();
    let doubled = cx.computed(move || {
        ec.set(ec.get() + 1);
        base.get() * 2
    });

    // Never evaluated until read
    assert_eq!(eval_count.get(), 0);
    assert_eq!(doubled.get(), 4);
    assert_eq!(eval_count.get(), 1);
    // Cached on repeated read
    assert_eq!(doubled.get(), 4);
    assert_eq!(eval_count.get(), 1);

    // Dirty on change, still lazy until read
    base.set(5);
    assert_eq!(eval_count.get(), 1);
    assert_eq!(doubled.get(), 10);
    assert_eq!(eval_count.get(), 2);
}

#[test]
fn batch_coalesces_effects() {
    reset_runtime();
    let cx = Scope::root();
    let a = cx.signal(1);
    let b = cx.signal(2);
    let runs = Rc::new(Cell::new(0));
    let r = runs.clone();
    cx.effect(move || {
        let _ = a.get() + b.get();
        r.set(r.get() + 1);
    });
    assert_eq!(runs.get(), 1);

    batch(|| {
        a.set(10);
        b.set(20);
    });
    assert_eq!(runs.get(), 2);
}

#[test]
fn scope_disposal_frees_signals_and_runs_cleanups() {
    reset_runtime();
    let root = Scope::root();
    let child = root.child_scope();
    let sig = child.signal(42);
    let cleaned = Rc::new(Cell::new(false));
    let c = cleaned.clone();
    child.on_cleanup(move || c.set(true));

    assert!(sig.is_alive());
    child.dispose();
    assert!(cleaned.get());
    assert!(!sig.is_alive());
    assert_eq!(sig.try_get(), None);
}

#[test]
fn node_dirty_propagation_and_context_lookup() {
    reset_runtime();
    let root = Scope::root();
    root.provide_context(123u32);
    let child = root.child_scope();
    assert_eq!(child.use_context::<u32>(), Some(123));

    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let n1 = nodes.insert(());
    let sig = child.signal(0.0f32);
    sig.bind_node(n1, DirtyFlags::PAINT);
    sig.set(0.5);

    let dirty = drain_dirty_nodes();
    assert_eq!(dirty, vec![(n1, DirtyFlags::PAINT)]);
    assert!(drain_dirty_nodes().is_empty());
}

#[test]
fn scope_timers_and_spawn_task_respect_disposal() {
    reset_runtime();
    let root = Scope::root();
    let child = root.child_scope();

    let fired = child.signal(0i32);
    child.timeout_at(100, 25, move || fired.set(fired.get() + 10));
    let iv = child.interval_at(100, 50, move || fired.set(fired.get() + 1));

    assert_eq!(next_timer_delay_ms(100), Some(25));
    assert_eq!(tick_timers_at(125), 1);
    assert_eq!(fired.get(), 10);
    assert_eq!(tick_timers_at(150), 1);
    assert_eq!(fired.get(), 11);

    iv.cancel();
    assert_eq!(tick_timers_at(300), 0);

    // Spawn async task on background thread and drain back onto UI thread
    let result_sig = child.signal(String::new());
    let (tx, rx) = std::sync::mpsc::channel();
    let disp = UiDispatcher::with_waker(move || {
        let _ = tx.send(());
    });
    child.spawn_with_dispatcher(
        &disp,
        || "loaded".to_string(),
        move |val| result_sig.set(val),
    );
    rx.recv_timeout(std::time::Duration::from_secs(2))
        .expect("worker signaled waker");
    assert_eq!(disp.drain(), 1);
    assert_eq!(result_sig.get(), "loaded");

    // Disposed scope prevents queued completion from executing even if work already finished
    let stale_ran = Rc::new(Cell::new(false));
    let stale_flag = stale_ran.clone();
    let orphan = root.child_scope();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (woke_tx, woke_rx) = std::sync::mpsc::channel();
    let disp2 = UiDispatcher::with_waker(move || {
        let _ = woke_tx.send(());
    });
    orphan.spawn_with_dispatcher(
        &disp2,
        move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            123u32
        },
        move |_| stale_flag.set(true),
    );
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("worker started");
    orphan.dispose();
    let _ = release_tx.send(());
    // Short wait ensures worker exited cleanly without posting cancelled work
    let _ = woke_rx.recv_timeout(std::time::Duration::from_millis(50));
    disp2.drain();
    assert!(!stale_ran.get());
}
