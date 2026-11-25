//! Playback controller and rich-text editing state for Media and RichTextEditor widgets.

use craftui_core::{Color, PlatformViewId, Point, Rect};
use craftui_text::{RichText, TextEditorState, TextSpan, TextStyle};

/// Media modality rendered by [`WidgetKind::MediaPlayer`](crate::WidgetKind::MediaPlayer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
}

/// Interactive transport state for audio/video playback, optionally paired with a native `PlatformViewId`.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaController {
    pub playing: bool,
    pub muted: bool,
    pub position_secs: f32,
    pub duration_secs: f32,
    pub volume: f32,
    pub platform_view_id: Option<PlatformViewId>,
}

impl Default for MediaController {
    fn default() -> Self {
        Self {
            playing: false,
            muted: false,
            position_secs: 0.0,
            duration_secs: 120.0,
            volume: 1.0,
            platform_view_id: None,
        }
    }
}

impl MediaController {
    pub fn new(duration_secs: f32) -> Self {
        Self {
            duration_secs: duration_secs.max(0.0),
            ..Self::default()
        }
    }

    pub fn with_platform_view(mut self, id: PlatformViewId) -> Self {
        self.platform_view_id = Some(id);
        self
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    pub fn seek_fraction(&mut self, frac: f32) {
        let clamped = frac.clamp(0.0, 1.0);
        self.position_secs = self.duration_secs.max(0.0) * clamped;
    }

    pub fn progress_fraction(&self) -> f32 {
        if self.duration_secs <= 0.0 {
            0.0
        } else {
            (self.position_secs / self.duration_secs).clamp(0.0, 1.0)
        }
    }

    pub fn format_timecode(secs: f32) -> String {
        let total = secs.max(0.0).round() as u32;
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

/// Byte-range formatting run layered over [`RichEditorController::editor`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyledSpanRange {
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<Color>,
}

/// Interactive rich-text controller combining cursor/selection editing with styled span runs.
#[derive(Debug, Clone, Default)]
pub struct RichEditorController {
    pub editor: TextEditorState,
    pub active_bold: bool,
    pub active_italic: bool,
    pub active_underline: bool,
    pub span_styles: Vec<StyledSpanRange>,
}

impl RichEditorController {
    pub fn with_text(text: impl Into<String>) -> Self {
        Self {
            editor: TextEditorState::new(text),
            active_bold: false,
            active_italic: false,
            active_underline: false,
            span_styles: Vec::new(),
        }
    }

    pub fn toggle_bold(&mut self) {
        self.active_bold = !self.active_bold;
        self.apply_flag_to_selection(|s, val| s.bold = val, self.active_bold);
    }

    pub fn toggle_italic(&mut self) {
        self.active_italic = !self.active_italic;
        self.apply_flag_to_selection(|s, val| s.italic = val, self.active_italic);
    }

    pub fn toggle_underline(&mut self) {
        self.active_underline = !self.active_underline;
        self.apply_flag_to_selection(|s, val| s.underline = val, self.active_underline);
    }

    fn apply_flag_to_selection(
        &mut self,
        setter: impl Fn(&mut StyledSpanRange, bool),
        value: bool,
    ) {
        let Some((s, e)) = self.editor.selection_range() else {
            return;
        };
        if s >= e {
            return;
        }
        let mut range = StyledSpanRange {
            start: s,
            end: e,
            bold: self.active_bold,
            italic: self.active_italic,
            underline: self.active_underline,
            color: None,
        };
        setter(&mut range, value);
        self.span_styles.push(range);
    }

    /// Insert `txt` at the current caret/selection and stamp active formatting if enabled.
    pub fn insert_str(&mut self, txt: &str) {
        if txt.is_empty() {
            return;
        }
        let prev_sel = self.editor.selection_range();
        let insert_pos = prev_sel.map(|(s, _)| s).unwrap_or(self.editor.cursor);
        let removed_len = prev_sel.map(|(s, e)| e.saturating_sub(s)).unwrap_or(0);
        self.editor.insert_str(txt);
        let delta = txt.len() as isize - removed_len as isize;
        self.shift_spans_after(insert_pos, delta);
        if self.active_bold || self.active_italic || self.active_underline {
            self.span_styles.push(StyledSpanRange {
                start: insert_pos,
                end: insert_pos + txt.len(),
                bold: self.active_bold,
                italic: self.active_italic,
                underline: self.active_underline,
                color: None,
            });
        }
    }

    /// Update spans cleanly when text bytes are removed or replaced inside `editor`.
    pub fn sync_span_bounds(&mut self) {
        let max_len = self.editor.text.len();
        self.span_styles.retain_mut(|r| {
            r.start = r.start.min(max_len);
            r.end = r.end.min(max_len);
            r.start < r.end
        });
    }

    fn shift_spans_after(&mut self, pos: usize, delta: isize) {
        for r in &mut self.span_styles {
            if r.start >= pos {
                r.start = (r.start as isize + delta).max(pos as isize) as usize;
            }
            if r.end >= pos {
                r.end = (r.end as isize + delta).max(pos as isize) as usize;
            }
        }
        self.sync_span_bounds();
    }

    /// Flatten the plain text + overlaid [`StyledSpanRange`] entries into a [`RichText`] paragraph.
    pub fn to_rich_text(&self, base_style: &TextStyle) -> RichText {
        let text = &self.editor.text;
        if text.is_empty() {
            return RichText::new();
        }
        if self.span_styles.is_empty() {
            return RichText::from_spans([TextSpan::new(text.clone(), base_style.clone())])
                .with_align(base_style.align);
        }

        let mut boundaries = vec![0usize, text.len()];
        for r in &self.span_styles {
            let s = r.start.min(text.len());
            let e = r.end.min(text.len());
            if s < e && text.is_char_boundary(s) && text.is_char_boundary(e) {
                boundaries.push(s);
                boundaries.push(e);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        let mut spans = Vec::new();
        for win in boundaries.windows(2) {
            let (a, b) = (win[0], win[1]);
            if a >= b {
                continue;
            }
            let mut st = base_style.clone();
            for r in &self.span_styles {
                if r.start <= a && r.end >= b {
                    if r.bold {
                        st = st.with_weight(700.0);
                    }
                    if r.italic {
                        st = st.with_italic(true);
                    }
                    if r.underline {
                        st = st.with_underline(true);
                    }
                    if let Some(c) = r.color {
                        st = st.with_color(c);
                    }
                }
            }
            spans.push(TextSpan::new(&text[a..b], st));
        }

        RichText::from_spans(spans).with_align(base_style.align)
    }
}

/// Geometry breakdown for `MediaPlayer` controls (play button, scrub bar, mute button).
pub(crate) fn media_control_rects(rect: Rect, kind: MediaKind) -> (Rect, Rect, Rect, Rect) {
    let bar_h = 36.0;
    let bar = match kind {
        MediaKind::Video => Rect::new(
            rect.x,
            (rect.max_y() - bar_h).max(rect.y),
            rect.width,
            bar_h.min(rect.height),
        ),
        MediaKind::Audio => rect,
    };
    let surface = match kind {
        MediaKind::Video => Rect::new(rect.x, rect.y, rect.width, (rect.height - bar_h).max(0.0)),
        MediaKind::Audio => Rect::ZERO,
    };
    let play_btn = Rect::new(bar.x + 8.0, bar.y + (bar.height - 26.0) * 0.5, 28.0, 26.0);
    let mute_btn = Rect::new(
        (bar.max_x() - 36.0).max(play_btn.max_x() + 8.0),
        bar.y + (bar.height - 26.0) * 0.5,
        28.0,
        26.0,
    );
    let time_reserve = 78.0;
    let scrub_x = play_btn.max_x() + 10.0;
    let scrub_w = (mute_btn.x - time_reserve - scrub_x).max(24.0);
    let scrub = Rect::new(scrub_x, bar.y + (bar.height - 8.0) * 0.5, scrub_w, 8.0);
    (surface, play_btn, scrub, mute_btn)
}

/// Geometry breakdown for `RichTextEditor` toolbar pills (`B`, `I`, `U`) and inner text area.
pub(crate) fn rich_editor_layout(rect: Rect) -> ([Rect; 3], Rect) {
    let tb_h = 32.0;
    let btn_w = 28.0;
    let y = rect.y + 4.0;
    let btns = [
        Rect::new(rect.x + 8.0, y, btn_w, 24.0),
        Rect::new(rect.x + 8.0 + 34.0, y, btn_w, 24.0),
        Rect::new(rect.x + 8.0 + 68.0, y, btn_w, 24.0),
    ];
    let text_rect = Rect::new(
        rect.x + 10.0,
        rect.y + tb_h + 6.0,
        (rect.width - 20.0).max(20.0),
        (rect.height - tb_h - 12.0).max(20.0),
    );
    (btns, text_rect)
}

/// Convenience helper for hit-testing toolbar button `0 = B, 1 = I, 2 = U`.
pub(crate) fn rich_editor_hit_toolbar(rect: Rect, pos: Point) -> Option<usize> {
    let (btns, _) = rich_editor_layout(rect);
    btns.iter().position(|r| r.contains(pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_controller_and_rich_editor_state_work() {
        let mut media = MediaController::new(200.0);
        assert!(!media.playing);
        media.toggle_play();
        assert!(media.playing);
        media.seek_fraction(0.25);
        assert_eq!(media.position_secs, 50.0);
        assert_eq!(
            MediaController::format_timecode(media.position_secs),
            "00:50"
        );

        let mut rich = RichEditorController::with_text("Hello ");
        rich.toggle_bold();
        rich.insert_str("CraftUI");
        assert_eq!(rich.editor.text, "Hello CraftUI");
        let rt = rich.to_rich_text(&TextStyle::default());
        assert_eq!(rt.plain_text(), "Hello CraftUI");
        assert_eq!(rt.spans.len(), 2);
        assert_eq!(rt.spans[1].style.font_weight, 700.0);
    }
}
