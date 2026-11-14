use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use craftui_core::{DirtyFlags, NodeId};

use crate::arena::{
    ARENA, ComputedKey, SourceId, SubscriberId, detach_subscriber, record_dependency,
};

/// Lazy memoized derived reactive handle (`Copy`).
pub struct Computed<T: 'static> {
    pub(crate) key: ComputedKey,
    pub(crate) _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for Computed<T> {}
impl<T: 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for Computed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for Computed<T> {}
impl<T: 'static> std::fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Computed").field(&self.key).finish()
    }
}

impl<T: 'static> Computed<T> {
    fn ensure_evaluated(self) -> Option<Rc<RefCell<Box<dyn Any>>>> {
        let needs_eval = ARENA.with(|a| {
            let arena = a.borrow();
            let comp = arena.computeds.get(self.key)?;
            Some(comp.dirty || comp.value.is_none())
        })?;

        if needs_eval {
            let prepared = ARENA.with(|a| {
                let mut arena = a.borrow_mut();
                let comp = arena.computeds.get_mut(self.key)?;
                let old_sources = std::mem::take(&mut comp.sources);
                let compute = comp.compute.clone();
                detach_subscriber(&mut arena, SubscriberId::Computed(self.key), &old_sources);
                arena.observer_stack.push(SubscriberId::Computed(self.key));
                Some(compute)
            })?;

            let new_val = prepared.borrow_mut()();

            ARENA.with(|a| {
                let mut arena = a.borrow_mut();
                if arena.observer_stack.last() == Some(&SubscriberId::Computed(self.key)) {
                    arena.observer_stack.pop();
                }
                if let Some(comp) = arena.computeds.get_mut(self.key) {
                    comp.value = Some(Rc::new(RefCell::new(new_val)));
                    comp.dirty = false;
                }
            });
        }

        ARENA.with(|a| {
            a.borrow()
                .computeds
                .get(self.key)
                .and_then(|c| c.value.clone())
        })
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let rc = self.ensure_evaluated()?;
        record_dependency(SourceId::Computed(self.key));
        let borrowed = rc.borrow();
        borrowed.downcast_ref::<T>().map(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with(f)
            .expect("attempted to read a disposed Computed")
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(comp) = arena.computeds.get_mut(self.key) {
                if let Some((_, existing)) = comp.node_bindings.iter_mut().find(|(n, _)| *n == node)
                {
                    *existing |= flags;
                } else {
                    comp.node_bindings.push((node, flags));
                }
            }
        });
    }
}
