//! Fine-grained reactive state engine backed by generational `SlotMap` arenas.
//!
//! All signal, computed, effect, and scope handles are `Copy` regardless of `T`,
//! eliminating `.clone()` boilerplate in UI event closures while guaranteeing
//! deterministic scope-based disposal.

pub mod arena;
pub mod computed;
pub mod effect;
pub mod resource;
pub mod scope;
pub mod signal;

#[cfg(test)]
mod tests;

pub use arena::{
    batch, drain_dirty_nodes, mark_node_dirty, next_timer_delay_ms, reset_runtime,
    set_ui_dispatcher, tick_timers, tick_timers_at, ui_dispatcher, untrack,
};
pub use computed::Computed;
pub use effect::Effect;
pub use resource::ResourceState;
pub use scope::{Scope, TaskHandle};
pub use signal::{ReadSignal, Signal};
