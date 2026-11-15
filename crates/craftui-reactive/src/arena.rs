use craftui_core::{DirtyFlags, NodeId, UiDispatcher, monotonic_ms};
use slotmap::{SlotMap, new_key_type};
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

new_key_type! {
    pub(crate) struct SignalKey;
    pub(crate) struct ComputedKey;
    pub(crate) struct EffectKey;
    pub(crate) struct ScopeKey;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SourceId {
    Signal(SignalKey),
    Computed(ComputedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SubscriberId {
    Computed(ComputedKey),
    Effect(EffectKey),
}

pub(crate) struct SignalSlot {
    pub(crate) value: Rc<RefCell<Box<dyn Any>>>,
    pub(crate) subscribers: HashSet<SubscriberId>,
    pub(crate) node_bindings: Vec<(NodeId, DirtyFlags)>,
}

pub(crate) struct ComputedSlot {
    pub(crate) value: Option<Rc<RefCell<Box<dyn Any>>>>,
    pub(crate) compute: Rc<RefCell<dyn FnMut() -> Box<dyn Any>>>,
    pub(crate) dirty: bool,
    pub(crate) sources: Vec<SourceId>,
    pub(crate) subscribers: HashSet<SubscriberId>,
    pub(crate) node_bindings: Vec<(NodeId, DirtyFlags)>,
}

pub(crate) struct EffectSlot {
    pub(crate) callback: Rc<RefCell<dyn FnMut()>>,
    pub(crate) sources: Vec<SourceId>,
    pub(crate) disposed: bool,
}

pub(crate) struct ScopeSlot {
    pub(crate) parent: Option<ScopeKey>,
    pub(crate) children: Vec<ScopeKey>,
    pub(crate) signals: Vec<SignalKey>,
    pub(crate) computeds: Vec<ComputedKey>,
    pub(crate) effects: Vec<EffectKey>,
    pub(crate) cleanups: Vec<Box<dyn FnOnce()>>,
    pub(crate) contexts: HashMap<TypeId, Rc<dyn Any>>,
    pub(crate) disposed: bool,
}

pub(crate) struct UiTimer {
    pub(crate) due_ms: u64,
    pub(crate) interval_ms: Option<u64>,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) callback: Rc<RefCell<dyn FnMut()>>,
}

pub(crate) type PendingJobCallback = Box<dyn FnOnce(Box<dyn Any>)>;

pub(crate) struct ReactiveArena {
    pub(crate) scopes: SlotMap<ScopeKey, ScopeSlot>,
    pub(crate) signals: SlotMap<SignalKey, SignalSlot>,
    pub(crate) computeds: SlotMap<ComputedKey, ComputedSlot>,
    pub(crate) effects: SlotMap<EffectKey, EffectSlot>,
    pub(crate) observer_stack: Vec<SubscriberId>,
    pub(crate) pending_effects: VecDeque<EffectKey>,
    pub(crate) pending_effect_set: HashSet<EffectKey>,
    pub(crate) running_effects: HashSet<EffectKey>,
    pub(crate) dirty_nodes: HashMap<NodeId, DirtyFlags>,
    pub(crate) dispatcher: UiDispatcher,
    pub(crate) pending_jobs: HashMap<u64, PendingJobCallback>,
    pub(crate) next_job_id: u64,
    pub(crate) ui_timers: Vec<UiTimer>,
    pub(crate) batch_depth: usize,
    pub(crate) flushing: bool,
    pub(crate) root_scope: Option<ScopeKey>,
}

impl ReactiveArena {
    pub(crate) fn new() -> Self {
        let mut scopes = SlotMap::with_key();
        let root_scope = scopes.insert(ScopeSlot {
            parent: None,
            children: Vec::new(),
            signals: Vec::new(),
            computeds: Vec::new(),
            effects: Vec::new(),
            cleanups: Vec::new(),
            contexts: HashMap::new(),
            disposed: false,
        });
        Self {
            scopes,
            signals: SlotMap::with_key(),
            computeds: SlotMap::with_key(),
            effects: SlotMap::with_key(),
            observer_stack: Vec::new(),
            pending_effects: VecDeque::new(),
            pending_effect_set: HashSet::new(),
            running_effects: HashSet::new(),
            dirty_nodes: HashMap::new(),
            dispatcher: UiDispatcher::new(),
            pending_jobs: HashMap::new(),
            next_job_id: 1,
            ui_timers: Vec::new(),
            batch_depth: 0,
            flushing: false,
            root_scope: Some(root_scope),
        }
    }
}

thread_local! {
    pub(crate) static ARENA: RefCell<ReactiveArena> = RefCell::new(ReactiveArena::new());
}

/// Reset the thread-local reactive arena (intended for isolated unit tests).
pub fn reset_runtime() {
    ARENA.with(|a| {
        *a.borrow_mut() = ReactiveArena::new();
    });
}

/// Access the UI thread's default [`UiDispatcher`] (used by platform loops and async scope hooks).
pub fn ui_dispatcher() -> UiDispatcher {
    ARENA.with(|a| a.borrow().dispatcher.clone())
}

/// Replace the default [`UiDispatcher`] (e.g., installed by a platform runner with a window waker).
pub fn set_ui_dispatcher(dispatcher: UiDispatcher) {
    ARENA.with(|a| {
        a.borrow_mut().dispatcher = dispatcher;
    });
}

/// Drain due UI timers + pending dispatcher closures at explicit `now_ms`.
pub fn tick_timers_at(now_ms: u64) -> usize {
    let dispatcher = ui_dispatcher();
    let mut executed = dispatcher.drain_at(now_ms);

    let due_timers = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let mut due = Vec::new();
        let mut remaining = Vec::with_capacity(arena.ui_timers.len());
        for timer in arena.ui_timers.drain(..) {
            if timer.cancelled.load(Ordering::SeqCst) {
                continue;
            }
            if timer.due_ms <= now_ms {
                due.push(timer);
            } else {
                remaining.push(timer);
            }
        }
        arena.ui_timers = remaining;
        due
    });

    let mut requeue = Vec::new();
    for mut timer in due_timers {
        if timer.cancelled.load(Ordering::SeqCst) {
            continue;
        }
        if let Ok(mut cb) = timer.callback.try_borrow_mut() {
            cb();
        }
        executed += 1;
        if let Some(interval) = timer.interval_ms
            && !timer.cancelled.load(Ordering::SeqCst)
        {
            let mut next_due = timer.due_ms.saturating_add(interval);
            if next_due <= now_ms {
                next_due = now_ms.saturating_add(interval);
            }
            timer.due_ms = next_due;
            requeue.push(timer);
        }
    }

    if !requeue.is_empty() {
        ARENA.with(|a| {
            a.borrow_mut().ui_timers.extend(requeue);
        });
    }

    executed
}

/// Drain due UI timers and pending async completions as of [`monotonic_ms`].
pub fn tick_timers() -> usize {
    tick_timers_at(monotonic_ms())
}

/// Minimum delay in ms before the next reactive UI timer or dispatcher timer fires.
pub fn next_timer_delay_ms(now_ms: u64) -> Option<u64> {
    let ui_min = ARENA.with(|a| {
        a.borrow()
            .ui_timers
            .iter()
            .filter(|t| !t.cancelled.load(Ordering::SeqCst))
            .map(|t| t.due_ms.saturating_sub(now_ms))
            .min()
    });
    let disp_min = ui_dispatcher().next_due_delay_ms(now_ms);
    match (ui_min, disp_min) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

pub(crate) fn complete_ui_job(job_id: u64, cancelled: &AtomicBool, result: Box<dyn Any>) {
    if cancelled.load(Ordering::SeqCst) {
        ARENA.with(|a| {
            a.borrow_mut().pending_jobs.remove(&job_id);
        });
        return;
    }
    let job = ARENA.with(|a| a.borrow_mut().pending_jobs.remove(&job_id));
    if let Some(cb) = job {
        cb(result);
    }
}

/// Drain all widget [`NodeId`]s marked dirty since the last frame.
pub fn drain_dirty_nodes() -> Vec<(NodeId, DirtyFlags)> {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.dirty_nodes.drain().collect()
    })
}

/// Mark a retained node dirty outside of a signal update (e.g. direct widget mutation).
pub fn mark_node_dirty(node: NodeId, flags: DirtyFlags) {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena
            .dirty_nodes
            .entry(node)
            .and_modify(|f| *f |= flags)
            .or_insert(flags);
    });
}

/// Batch multiple signal updates, running effects once after the closure completes.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    ARENA.with(|a| a.borrow_mut().batch_depth += 1);
    let result = f();
    let should_flush = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.batch_depth = arena.batch_depth.saturating_sub(1);
        arena.batch_depth == 0
    });
    if should_flush {
        flush_effects();
    }
    result
}

/// Execute `f` without tracking any reactive signal reads as dependencies.
pub fn untrack<R>(f: impl FnOnce() -> R) -> R {
    let saved = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        std::mem::take(&mut arena.observer_stack)
    });
    let result = f();
    ARENA.with(|a| {
        a.borrow_mut().observer_stack = saved;
    });
    result
}

pub(crate) fn detach_subscriber(
    arena: &mut ReactiveArena,
    sub: SubscriberId,
    sources: &[SourceId],
) {
    for src in sources {
        match *src {
            SourceId::Signal(sk) => {
                if let Some(sig) = arena.signals.get_mut(sk) {
                    sig.subscribers.remove(&sub);
                }
            }
            SourceId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck) {
                    comp.subscribers.remove(&sub);
                }
            }
        }
    }
}

pub(crate) fn record_dependency(source: SourceId) {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let Some(&observer) = arena.observer_stack.last() else {
            return;
        };
        match source {
            SourceId::Signal(sk) => {
                if let Some(sig) = arena.signals.get_mut(sk) {
                    sig.subscribers.insert(observer);
                }
            }
            SourceId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck) {
                    comp.subscribers.insert(observer);
                }
            }
        }
        match observer {
            SubscriberId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck)
                    && !comp.sources.contains(&source)
                {
                    comp.sources.push(source);
                }
            }
            SubscriberId::Effect(ek) => {
                if let Some(eff) = arena.effects.get_mut(ek)
                    && !eff.sources.contains(&source)
                {
                    eff.sources.push(source);
                }
            }
        }
    });
}

pub(crate) fn propagate_dirty(
    subscribers: Vec<SubscriberId>,
    node_bindings: Vec<(NodeId, DirtyFlags)>,
) {
    let should_flush = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        for (node, flags) in node_bindings {
            arena
                .dirty_nodes
                .entry(node)
                .and_modify(|f| *f |= flags)
                .or_insert(flags);
        }

        let mut stack = subscribers;
        let mut visited_computeds = HashSet::new();

        while let Some(sub) = stack.pop() {
            match sub {
                SubscriberId::Effect(ek) => {
                    if arena.effects.get(ek).is_some_and(|e| !e.disposed)
                        && arena.pending_effect_set.insert(ek)
                    {
                        arena.pending_effects.push_back(ek);
                    }
                }
                SubscriberId::Computed(ck) => {
                    if !visited_computeds.insert(ck) {
                        continue;
                    }
                    let info = arena.computeds.get_mut(ck).map(|comp| {
                        comp.dirty = true;
                        let bindings = comp.node_bindings.clone();
                        let next_subs: Vec<SubscriberId> =
                            comp.subscribers.iter().copied().collect();
                        (bindings, next_subs)
                    });
                    if let Some((bindings, next_subs)) = info {
                        for (node, flags) in bindings {
                            arena
                                .dirty_nodes
                                .entry(node)
                                .and_modify(|f| *f |= flags)
                                .or_insert(flags);
                        }
                        stack.extend(next_subs);
                    }
                }
            }
        }

        arena.batch_depth == 0 && !arena.flushing
    });

    if should_flush {
        flush_effects();
    }
}

pub(crate) fn flush_effects() {
    let already_flushing = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        if arena.flushing {
            return true;
        }
        arena.flushing = true;
        false
    });
    if already_flushing {
        return;
    }

    loop {
        let next = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let ek = arena.pending_effects.pop_front()?;
            arena.pending_effect_set.remove(&ek);
            Some(ek)
        });
        let Some(ek) = next else {
            break;
        };
        run_effect(ek);
    }

    ARENA.with(|a| {
        a.borrow_mut().flushing = false;
    });
}

pub(crate) fn run_effect(ek: EffectKey) {
    let prepared = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        if arena.running_effects.contains(&ek) {
            return None;
        }
        let (old_sources, cb) = {
            let eff = arena.effects.get_mut(ek)?;
            if eff.disposed {
                return None;
            }
            (std::mem::take(&mut eff.sources), eff.callback.clone())
        };
        arena.running_effects.insert(ek);
        detach_subscriber(&mut arena, SubscriberId::Effect(ek), &old_sources);
        arena.observer_stack.push(SubscriberId::Effect(ek));
        Some(cb)
    });

    let Some(cb) = prepared else {
        return;
    };

    if let Ok(mut func) = cb.try_borrow_mut() {
        func();
    }

    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.running_effects.remove(&ek);
        if arena.observer_stack.last() == Some(&SubscriberId::Effect(ek)) {
            arena.observer_stack.pop();
        } else {
            arena
                .observer_stack
                .retain(|x| x != &SubscriberId::Effect(ek));
        }
    });
}

pub(crate) fn dispose_scope_recursive(scope_key: ScopeKey) {
    let scope_data = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let scope = arena.scopes.get_mut(scope_key)?;
        if scope.disposed {
            return None;
        }
        scope.disposed = true;
        let children = std::mem::take(&mut scope.children);
        let signals = std::mem::take(&mut scope.signals);
        let computeds = std::mem::take(&mut scope.computeds);
        let effects = std::mem::take(&mut scope.effects);
        let cleanups = std::mem::take(&mut scope.cleanups);
        let parent = scope.parent;

        if let Some(parent_key) = parent
            && let Some(parent_scope) = arena.scopes.get_mut(parent_key)
        {
            parent_scope.children.retain(|c| *c != scope_key);
        }

        Some((children, signals, computeds, effects, cleanups))
    });

    let Some((children, signals, computeds, effects, cleanups)) = scope_data else {
        return;
    };

    for child in children {
        dispose_scope_recursive(child);
    }

    for cleanup in cleanups {
        cleanup();
    }

    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        for eff_key in effects {
            let sources = arena
                .effects
                .get_mut(eff_key)
                .map(|e| {
                    e.disposed = true;
                    std::mem::take(&mut e.sources)
                })
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Effect(eff_key), &sources);
            arena.effects.remove(eff_key);
        }
        for comp_key in computeds {
            let sources = arena
                .computeds
                .get_mut(comp_key)
                .map(|c| std::mem::take(&mut c.sources))
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Computed(comp_key), &sources);
            arena.computeds.remove(comp_key);
        }
        for sig_key in signals {
            arena.signals.remove(sig_key);
        }
        arena.scopes.remove(scope_key);
    });
}
