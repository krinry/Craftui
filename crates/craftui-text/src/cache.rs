use std::collections::HashMap;
use std::sync::Arc;

use craftui_core::Color;
use parley::Layout;

use crate::style::{TextAlign, TextStyle};

pub(crate) const DEFAULT_LAYOUT_CACHE_CAPACITY: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LayoutCacheKey {
    pub(crate) text: String,
    pub(crate) font_size_bits: u32,
    pub(crate) font_weight_bits: u32,
    pub(crate) italic: bool,
    pub(crate) font_family: Option<String>,
    pub(crate) line_height_bits: u32,
    pub(crate) align: TextAlign,
    pub(crate) underline: bool,
    pub(crate) max_width_bits: Option<u32>,
}

impl LayoutCacheKey {
    pub(crate) fn from_inputs(text: &str, style: &TextStyle, max_width: Option<f32>) -> Self {
        let norm_max_w = max_width
            .filter(|w| w.is_finite())
            .map(|w| if w == 0.0 { 0.0f32 } else { w.max(0.0) }.to_bits());
        let family = style
            .font_family
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        Self {
            text: text.to_string(),
            font_size_bits: style.font_size.to_bits(),
            font_weight_bits: style.font_weight.to_bits(),
            italic: style.italic,
            font_family: family,
            line_height_bits: style.line_height.to_bits(),
            align: style.align,
            underline: style.underline,
            max_width_bits: norm_max_w,
        }
    }
}

pub(crate) struct ShapedLayoutCache {
    pub(crate) entries: HashMap<LayoutCacheKey, (u64, Arc<Layout<Color>>)>,
    pub(crate) clock: u64,
    pub(crate) capacity: usize,
    pub(crate) hits: u64,
    pub(crate) misses: u64,
}

impl ShapedLayoutCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.min(256)),
            clock: 0,
            capacity: capacity.max(16),
            hits: 0,
            misses: 0,
        }
    }

    pub(crate) fn get(&mut self, key: &LayoutCacheKey) -> Option<Arc<Layout<Color>>> {
        if let Some((tick, layout)) = self.entries.get_mut(key) {
            self.clock = self.clock.wrapping_add(1);
            *tick = self.clock;
            self.hits = self.hits.saturating_add(1);
            Some(Arc::clone(layout))
        } else {
            self.misses = self.misses.saturating_add(1);
            None
        }
    }

    pub(crate) fn insert(&mut self, key: LayoutCacheKey, layout: Arc<Layout<Color>>) {
        if self.entries.len() >= self.capacity {
            let evict_count = (self.capacity / 4).max(1);
            let mut order: Vec<(u64, LayoutCacheKey)> = self
                .entries
                .iter()
                .map(|(k, (t, _))| (*t, k.clone()))
                .collect();
            order.select_nth_unstable_by_key(evict_count.saturating_sub(1), |(t, _)| *t);
            for (_, k) in order.into_iter().take(evict_count) {
                self.entries.remove(&k);
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(key, (self.clock, layout));
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
