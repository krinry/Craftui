use std::marker::PhantomData;

use craftui_core::{DirtyFlags, NodeId};

use crate::arena::{
    ARENA, SignalKey, SourceId, propagate_dirty, record_dependency, untrack,
};

/// Generational `Copy` handle to a reactive state cell.
pub struct Signal<T: 'static> {
    pub(crate) key: SignalKey,
    pub(crate) _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for Signal<T> {}
impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for Signal<T> {}
impl<T: 'static> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Signal").field(&self.key).finish()
    }
}

/// Read-only `Copy` view of a [`Signal<T>`].
pub struct ReadSignal<T: 'static> {
    pub(crate) key: SignalKey,
    pub(crate) _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for ReadSignal<T> {}
impl<T: 'static> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for ReadSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for ReadSignal<T> {}
impl<T: 'static> std::fmt::Debug for ReadSignal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ReadSignal").field(&self.key).finish()
    }
}

impl<T: 'static> From<Signal<T>> for ReadSignal<T> {
    fn from(s: Signal<T>) -> Self {
        s.read_only()
    }
}

impl<T: 'static> Signal<T> {
    pub fn read_only(self) -> ReadSignal<T> {
        ReadSignal {
            key: self.key,
            _marker: PhantomData,
        }
    }

    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().signals.contains_key(self.key))
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let rc = ARENA.with(|a| a.borrow().signals.get(self.key).map(|s| s.value.clone()))?;
        record_dependency(SourceId::Signal(self.key));
        let borrowed = rc.borrow();
        borrowed.downcast_ref::<T>().map(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with(f)
            .expect("attempted to read a disposed Signal")
    }

    pub fn try_get(self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(Clone::clone)
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    /// Read without tracking a reactive dependency.
    pub fn get_untracked(self) -> T
    where
        T: Clone,
    {
        untrack(|| self.get())
    }

    pub fn set(self, value: T) {
        self.update(|slot| *slot = value);
    }

    pub fn set_if_changed(self, value: T) -> bool
    where
        T: PartialEq,
    {
        let changed = self.try_with(|curr| *curr != value).unwrap_or(false);
        if changed {
            self.set(value);
        }
        changed
    }

    pub fn update<R>(self, f: impl FnOnce(&mut T) -> R) -> R {
        let (rc, subs, nodes) = ARENA.with(|a| {
            let arena = a.borrow();
            let sig = arena
                .signals
                .get(self.key)
                .expect("attempted to update a disposed Signal");
            (
                sig.value.clone(),
                sig.subscribers.iter().copied().collect::<Vec<_>>(),
                sig.node_bindings.clone(),
            )
        });
        let result = {
            let mut guard = rc.borrow_mut();
            let typed = guard
                .downcast_mut::<T>()
                .expect("Signal type mismatch in arena");
            f(typed)
        };
        propagate_dirty(subs, nodes);
        result
    }

    /// Subscribe a retained widget [`NodeId`] so updates mark it dirty directly.
    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(sig) = arena.signals.get_mut(self.key) {
                if let Some((_, existing)) = sig.node_bindings.iter_mut().find(|(n, _)| *n == node)
                {
                    *existing |= flags;
                } else {
                    sig.node_bindings.push((node, flags));
                }
            }
        });
    }
}

impl<T: 'static> ReadSignal<T> {
    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().signals.contains_key(self.key))
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .try_with(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .with(f)
    }

    pub fn try_get(self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(Clone::clone)
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .bind_node(node, flags);
    }
}
