use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use craftui_core::{UiDispatcher, monotonic_ms};

use crate::arena::{
    ARENA, ComputedSlot, EffectSlot, ScopeKey, ScopeSlot, SignalSlot, UiTimer,
    complete_ui_job, dispose_scope_recursive, run_effect, ui_dispatcher,
};
use crate::computed::Computed;
use crate::effect::Effect;
use crate::signal::Signal;

/// Cancellation handle for a [`Scope`]-bound async task, timeout, or interval.
#[derive(Clone, Debug)]
pub struct TaskHandle {
    pub(crate) cancelled: Arc<AtomicBool>,
}

impl TaskHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Ownership scope for signals, computed values, effects, and child components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scope {
    pub(crate) key: ScopeKey,
}

impl Scope {
    /// Returns the global root scope for the current UI thread.
    pub fn root() -> Self {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(root) = arena.root_scope
                && arena.scopes.contains_key(root)
            {
                return root;
            }
            let root = arena.scopes.insert(ScopeSlot {
                parent: None,
                children: Vec::new(),
                signals: Vec::new(),
                computeds: Vec::new(),
                effects: Vec::new(),
                cleanups: Vec::new(),
                contexts: HashMap::new(),
                disposed: false,
            });
            arena.root_scope = Some(root);
            root
        });
        Self { key }
    }

    /// Create an isolated child scope owned by this scope.
    pub fn child_scope(self) -> Self {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let child = arena.scopes.insert(ScopeSlot {
                parent: Some(self.key),
                children: Vec::new(),
                signals: Vec::new(),
                computeds: Vec::new(),
                effects: Vec::new(),
                cleanups: Vec::new(),
                contexts: HashMap::new(),
                disposed: false,
            });
            if let Some(parent) = arena.scopes.get_mut(self.key) {
                parent.children.push(child);
            }
            child
        });
        Self { key }
    }

    /// Allocate a reactive [`Signal<T>`] owned by this scope.
    pub fn signal<T: 'static>(self, initial: T) -> Signal<T> {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let sig_key = arena.signals.insert(SignalSlot {
                value: Rc::new(RefCell::new(Box::new(initial))),
                subscribers: HashSet::new(),
                node_bindings: Vec::new(),
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.signals.push(sig_key);
            }
            sig_key
        });
        Signal {
            key,
            _marker: PhantomData,
        }
    }

    /// Allocate a lazy memoized [`Computed<T>`] owned by this scope.
    pub fn computed<T: 'static>(self, mut compute: impl FnMut() -> T + 'static) -> Computed<T> {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let comp_key = arena.computeds.insert(ComputedSlot {
                value: None,
                compute: Rc::new(RefCell::new(move || Box::new(compute()) as Box<dyn Any>)),
                dirty: true,
                sources: Vec::new(),
                subscribers: HashSet::new(),
                node_bindings: Vec::new(),
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.computeds.push(comp_key);
            }
            comp_key
        });
        Computed {
            key,
            _marker: PhantomData,
        }
    }

    /// Register an auto-tracking reactive [`Effect`] owned by this scope.
    pub fn effect(self, callback: impl FnMut() + 'static) -> Effect {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let eff_key = arena.effects.insert(EffectSlot {
                callback: Rc::new(RefCell::new(callback)),
                sources: Vec::new(),
                disposed: false,
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.effects.push(eff_key);
            }
            eff_key
        });
        run_effect(key);
        Effect { key }
    }

    /// Register a cleanup closure executed when this scope is disposed.
    pub fn on_cleanup(self, cleanup: impl FnOnce() + 'static) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.cleanups.push(Box::new(cleanup));
            }
        });
    }

    /// Provide a typed context value accessible to descendant scopes.
    pub fn provide_context<T: 'static>(self, value: T) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.contexts.insert(TypeId::of::<T>(), Rc::new(value));
            }
        });
    }

    /// Look up the nearest ancestor context value of type `T`.
    pub fn use_context<T: Clone + 'static>(self) -> Option<T> {
        ARENA.with(|a| {
            let arena = a.borrow();
            let mut cursor = Some(self.key);
            while let Some(key) = cursor {
                let scope = arena.scopes.get(key)?;
                if let Some(val) = scope.contexts.get(&TypeId::of::<T>()) {
                    return val.downcast_ref::<T>().cloned();
                }
                cursor = scope.parent;
            }
            None
        })
    }

    /// Look up a `UiDispatcher` from ancestor context, falling back to the runtime dispatcher.
    pub fn dispatcher(self) -> UiDispatcher {
        self.use_context::<UiDispatcher>()
            .unwrap_or_else(ui_dispatcher)
    }

    /// Run `work` off the UI thread (on native targets) and deliver the result to `on_complete`
    /// on the UI thread if this [`Scope`] is still alive.
    pub fn spawn_task<T, W, C>(self, work: W, on_complete: C) -> TaskHandle
    where
        T: Send + 'static,
        W: FnOnce() -> T + Send + 'static,
        C: FnOnce(T) + 'static,
    {
        let dispatcher = self.dispatcher();
        self.spawn_with_dispatcher(&dispatcher, work, on_complete)
    }

    /// Run `work` asynchronously and marshal the completion back via an explicit [`UiDispatcher`].
    pub fn spawn_with_dispatcher<T, W, C>(
        self,
        dispatcher: &UiDispatcher,
        work: W,
        on_complete: C,
    ) -> TaskHandle
    where
        T: Send + 'static,
        W: FnOnce() -> T + Send + 'static,
        C: FnOnce(T) + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let job_id = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let id = arena.next_job_id;
            arena.next_job_id = arena.next_job_id.wrapping_add(1).max(1);
            arena.pending_jobs.insert(
                id,
                Box::new(move |boxed: Box<dyn Any>| {
                    if let Ok(value) = boxed.downcast::<T>() {
                        on_complete(*value);
                    }
                }),
            );
            id
        });

        let cancel_cleanup = cancelled.clone();
        self.on_cleanup(move || {
            cancel_cleanup.store(true, Ordering::SeqCst);
            ARENA.with(|a| {
                a.borrow_mut().pending_jobs.remove(&job_id);
            });
        });

        let handle = TaskHandle {
            cancelled: cancelled.clone(),
        };
        let disp = dispatcher.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || {
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                let result = work();
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                disp.post(move || {
                    complete_ui_job(job_id, &cancelled, Box::new(result));
                });
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            disp.post(move || {
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                let result = work();
                complete_ui_job(job_id, &cancelled, Box::new(result));
            });
        }

        handle
    }

    /// Schedule a one-shot UI-thread timeout automatically cancelled when this [`Scope`] is disposed.
    pub fn timeout(self, delay_ms: u64, on_fire: impl FnOnce() + 'static) -> TaskHandle {
        self.timeout_at(monotonic_ms(), delay_ms, on_fire)
    }

    /// Schedule a one-shot UI-thread timeout relative to an explicit `now_ms`.
    pub fn timeout_at(
        self,
        now_ms: u64,
        delay_ms: u64,
        on_fire: impl FnOnce() + 'static,
    ) -> TaskHandle {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut opt = Some(on_fire);
        ARENA.with(|a| {
            a.borrow_mut().ui_timers.push(UiTimer {
                due_ms: now_ms.saturating_add(delay_ms),
                interval_ms: None,
                cancelled: cancelled.clone(),
                callback: Rc::new(RefCell::new(move || {
                    if let Some(f) = opt.take() {
                        f();
                    }
                })),
            });
        });
        let cancel_cleanup = cancelled.clone();
        self.on_cleanup(move || cancel_cleanup.store(true, Ordering::SeqCst));
        TaskHandle { cancelled }
    }

    /// Schedule a repeating UI-thread interval automatically cancelled when this [`Scope`] is disposed.
    pub fn interval(self, interval_ms: u64, on_tick: impl FnMut() + 'static) -> TaskHandle {
        self.interval_at(monotonic_ms(), interval_ms, on_tick)
    }

    /// Schedule a repeating UI-thread interval relative to an explicit `now_ms`.
    pub fn interval_at(
        self,
        now_ms: u64,
        interval_ms: u64,
        on_tick: impl FnMut() + 'static,
    ) -> TaskHandle {
        let cancelled = Arc::new(AtomicBool::new(false));
        let step = interval_ms.max(1);
        ARENA.with(|a| {
            a.borrow_mut().ui_timers.push(UiTimer {
                due_ms: now_ms.saturating_add(step),
                interval_ms: Some(step),
                cancelled: cancelled.clone(),
                callback: Rc::new(RefCell::new(on_tick)),
            });
        });
        let cancel_cleanup = cancelled.clone();
        self.on_cleanup(move || cancel_cleanup.store(true, Ordering::SeqCst));
        TaskHandle { cancelled }
    }

    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().scopes.get(self.key).is_some_and(|s| !s.disposed))
    }

    /// Dispose this scope and recursively free all child scopes, signals, computeds, and effects.
    pub fn dispose(self) {
        dispose_scope_recursive(self.key);
    }
}
