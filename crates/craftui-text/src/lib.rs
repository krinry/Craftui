//! Complex script shaping, font discovery, layout caching, metrics, and IME editing powered by `parley`.

pub mod cache;
pub mod editor;
pub mod engine;
pub mod style;

#[cfg(test)]
mod tests;

pub use editor::TextEditorState;
pub use engine::TextEngine;
pub use style::{TextAlign, TextOverflow, TextStyle};
