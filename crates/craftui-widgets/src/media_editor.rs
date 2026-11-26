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

/// Byte-range formatting override layered over [`RichEditorController::editor`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyledSpanRange {
    pub start: usize,
    pub end: usize,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
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
        let next = self.active_bold;
        self.apply_flag_to_selection(|s| s.bold = Some(next));
    }

    pub fn toggle_italic(&mut self) {
        self.active_italic = !self.active_italic;
        let next = self.active_italic;
        self.apply_flag_to_selection(|s| s.italic = Some(next));
    }

    pub fn toggle_underline(&mut self) {
        self.active_underline = !self.active_underline;
        let next = self.active_underline;
        self.apply_flag_to_selection(|s| s.underline = Some(next));
    }

    fn apply_flag_to_selection(&mut self, set_flag: impl FnOnce(&mut StyledSpanRange)) {
        let Some((s, e)) = self.editor.selection_range() else {
            return;
        };
        if s >= e {
            return;
        }
        let mut range = StyledSpanRange {
            start: s,
            end: e,
            ..StyledSpanRange::default()
        };
        set_flag(&mut range);
        self.span_styles.push(range);
    }

    /// Insert `txt` at the current caret/selection and stamp active formatting when enabled.
    pub fn insert_str(&mut self, txt: &str) {
        if txt.is_empty() {
            return;
        }
        let prev_sel = self.editor.selection_range();
        let insert_pos = prev_sel.map(|(s, _)| s).unwrap_or(self.editor.cursor);
        let removed_len = prev_sel.map(|(s, e)| e.saturating_sub(s)).unwrap_or(0);
        self.editor.insert_str(txt);
        self.apply_edit_range(insert_pos, removed_len, txt.len());
        if self.active_bold || self.active_italic || self.active_underline {
            self.span_styles.push(StyledSpanRange {
                start: insert_pos,
                end: insert_pos + txt.len(),
                bold: self.active_bold.then_some(true),
                italic: self.active_italic.then_some(true),
                underline: self.active_underline.then_some(true),
                color: None,
            });
        }
    }

    /// Delete backward (selected range or char before caret) and keep span intervals aligned.
    pub fn backspace(&mut self) {
        let (edit_start, removed_len) = if let Some((s, e)) = self.editor.selection_range() {
            (s, e.saturating_sub(s))
        } else if self.editor.cursor > 0 {
            let cur = self.editor.cursor;
            let prev_start = self.editor.text[..cur]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            (prev_start, cur - prev_start)
        } else {
            return;
        };
        self.editor.backspace();
        self.apply_edit_range(edit_start, removed_len, 0);
    }

    /// Delete forward (selected range or char after caret) and keep span intervals aligned.
    pub fn delete_forward(&mut self) {
        let (edit_start, removed_len) = if let Some((s, e)) = self.editor.selection_range() {
            (s, e.saturating_sub(s))
        } else if self.editor.cursor < self.editor.text.len() {
            let cur = self.editor.cursor;
            let next_len = self.editor.text[cur..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0);
            (cur, next_len)
        } else {
            return;
        };
        self.editor.delete_forward();
        self.apply_edit_range(edit_start, removed_len, 0);
    }

    /// Rebase existing [`StyledSpanRange`] byte offsets cleanly across an edit of `[start, start + removed_len) -> inserted_len`.
    pub fn apply_edit_range(&mut self, start: usize, removed_len: usize, inserted_len: usize) {
        let del_end = start + removed_len;
        let max_len = self.editor.text.len();
        let shift_boundary = |idx: usize, is_end: bool| -> usize {
            if removed_len == 0 {
                if (!is_end && idx >= start) || (is_end && idx > start) {
                    idx.saturating_add(inserted_len)
                } else {
                    idx
                }
            } else if idx <= start {
                idx
            } else if idx < del_end || (is_end && idx == del_end) {
                start
            } else {
                idx - removed_len + inserted_len
            }
        };

        self.span_styles.retain_mut(|r| {
            r.start = shift_boundary(r.start, false).min(max_len);
            r.end = shift_boundary(r.end, true).min(max_len);
            r.start < r.end
        });
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
                    if let Some(bold) = r.bold {
                        st = st.with_weight(if bold { 700.0 } else { base_style.font_weight });
                    }
                    if let Some(italic) = r.italic {
                        st = st.with_italic(italic);
                    }
                    if let Some(underline) = r.underline {
                        st = st.with_underline(underline);
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

/// Geometry breakdown for `MediaPlayer` controls (`(surface, play_btn, scrub, mute_btn, show_time)`).
pub(crate) fn media_control_rects(rect: Rect, kind: MediaKind) -> (Rect, Rect, Rect, Rect, bool) {
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
        (bar.max_x() - 36.0).max(play_btn.max_x() + 6.0),
        bar.y + (bar.height - 26.0) * 0.5,
        28.0,
        26.0,
    );
    let show_time = bar.width >= 220.0;
    let time_reserve = if show_time { 78.0 } else { 8.0 };
    let scrub_x = play_btn.max_x() + 8.0;
    let scrub_max = (mute_btn.x - time_reserve).max(scrub_x);
    let scrub_w = (scrub_max - scrub_x).max(0.0);
    let scrub = Rect::new(scrub_x, bar.y + (bar.height - 8.0) * 0.5, scrub_w, 8.0);
    (surface, play_btn, scrub, mute_btn, show_time)
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
        rich.toggle_bold(); // Disable bold before typing unformatted suffix
        rich.insert_str("!");
        let rt = rich.to_rich_text(&TextStyle::default());
        assert_eq!(rt.plain_text(), "Hello CraftUI!");
        assert_eq!(rt.spans.len(), 3);
        assert_eq!(rt.spans[1].style.font_weight, 700.0);
        assert_eq!(
            rt.spans[2].style.font_weight,
            TextStyle::default().font_weight
        );

        // Toggle bold OFF across selected range `[6, 13)`
        rich.editor.set_cursor_at(6, false);
        rich.editor.set_cursor_at(13, true);
        rich.active_bold = true;
        rich.toggle_bold();
        let rt2 = rich.to_rich_text(&TextStyle::default());
        assert!(rt2.spans.iter().all(|s| s.style.font_weight < 700.0));
    }
}
