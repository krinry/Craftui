//! Dynamic `.cui` AST/IR interpreter enabling sub-100ms visual hot-reload without Rust recompilation.

pub mod dyn_value;
pub mod hot_reload;
pub mod interpreter;

#[cfg(test)]
mod tests;

pub use dyn_value::{DynCallback, DynValue, DynamicStore};
pub use hot_reload::HotReloadSession;
