//! Native screen-reader accessibility bridge powered by `accesskit`.

pub use accesskit;

pub mod bridge;
pub mod types;

#[cfg(test)]
mod tests;

pub use bridge::A11yBridge;
pub use types::{A11yAction, A11yMetadata, A11yRole, A11ySnapshotNode};
