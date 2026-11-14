use crate::arena::{ARENA, EffectKey, SubscriberId, detach_subscriber};

/// Handle to a reactive side-effect bound to a [`Scope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Effect {
    pub(crate) key: EffectKey,
}

impl Effect {
    pub fn dispose(self) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let sources = arena
                .effects
                .get_mut(self.key)
                .map(|e| {
                    e.disposed = true;
                    std::mem::take(&mut e.sources)
                })
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Effect(self.key), &sources);
            arena.effects.remove(self.key);
        });
    }
}
