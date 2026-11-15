use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSnapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

const MAX_UNDO_HISTORY: usize = 128;

/// Interactive single/multi-line text buffer state supporting selection, word navigation, undo/redo, and IME preedit.
#[derive(Debug, Clone, Default)]
pub struct TextEditorState {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub preedit: Option<String>,
    pub preedit_cursor: Option<(usize, usize)>,
    undo_stack: Vec<TextSnapshot>,
    redo_stack: Vec<TextSnapshot>,
}

impl TextEditorState {
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let len = text.len();
        Self {
            text,
            cursor: len,
            anchor: len,
            preedit: None,
            preedit_cursor: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn push_undo(&mut self) {
        let snap = TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        };
        if self.undo_stack.last() == Some(&snap) {
            return;
        }
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear();
    }

    /// Synchronize external signal updates while preserving cursor/selection clamped on valid UTF-8 boundaries.
    pub fn sync_external(&mut self, next: &str) -> bool {
        if self.text == next {
            return false;
        }
        self.push_undo();
        self.text.clear();
        self.text.push_str(next);
        self.cursor = self.clamp_boundary(self.cursor);
        self.anchor = self.clamp_boundary(self.anchor);
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = prev.text;
        self.cursor = self.clamp_boundary(prev.cursor);
        self.anchor = self.clamp_boundary(prev.anchor);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = next.text;
        self.cursor = self.clamp_boundary(next.cursor);
        self.anchor = self.clamp_boundary(next.anchor);
        true
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        if self.cursor == self.anchor {
            None
        } else {
            Some((self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
        }
    }

    pub fn selected_text(&self) -> &str {
        if let Some((start, end)) = self.selection_range() {
            &self.text[start..end]
        } else {
            ""
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.push_undo();
        self.delete_selection_inner();
        let idx = self.clamp_boundary(self.cursor);
        self.text.insert_str(idx, s);
        self.cursor = idx + s.len();
        self.anchor = self.cursor;
    }

    fn delete_selection_inner(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            self.text.replace_range(start..end, "");
            self.cursor = start;
            self.anchor = start;
            true
        } else {
            false
        }
    }

    pub fn delete_selection(&mut self) -> bool {
        if self.selection_range().is_some() {
            self.push_undo();
            self.delete_selection_inner()
        } else {
            false
        }
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur == 0 {
            return;
        }
        self.push_undo();
        let prev = self.prev_char_boundary(cur);
        self.text.replace_range(prev..cur, "");
        self.cursor = prev;
        self.anchor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur >= self.text.len() {
            return;
        }
        self.push_undo();
        let next = self.next_char_boundary(cur);
        self.text.replace_range(cur..next, "");
        self.anchor = self.cursor;
    }

    pub fn delete_word_backward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur == 0 {
            return;
        }
        self.push_undo();
        let prev = self.prev_word_boundary(cur);
        self.text.replace_range(prev..cur, "");
        self.cursor = prev;
        self.anchor = prev;
    }

    pub fn delete_word_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur >= self.text.len() {
            return;
        }
        self.push_undo();
        let next = self.next_word_boundary(cur);
        self.text.replace_range(cur..next, "");
        self.anchor = self.cursor;
    }

    pub fn move_left(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.prev_char_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_right(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.next_char_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_word_left(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.prev_word_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_word_right(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.next_word_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_home(&mut self, extend_selection: bool) {
        self.cursor = 0;
        if !extend_selection {
            self.anchor = 0;
        }
    }

    pub fn move_end(&mut self, extend_selection: bool) {
        self.cursor = self.text.len();
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    /// Move cursor vertically up one logical `\n`-separated line keeping approximate column offset.
    pub fn move_up(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        let before = &self.text[..cur];
        let Some(curr_line_start_nl) = before.rfind('\n') else {
            self.move_home(extend_selection);
            return;
        };
        let col_chars = self.text[curr_line_start_nl + 1..cur].chars().count();
        let prev_line_start = self.text[..curr_line_start_nl]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let target_byte = prev_line_start
            + self.text[prev_line_start..curr_line_start_nl]
                .char_indices()
                .nth(col_chars)
                .map(|(off, _)| off)
                .unwrap_or(curr_line_start_nl - prev_line_start);
        self.set_cursor_at(target_byte, extend_selection);
    }

    /// Move cursor vertically down one logical `\n`-separated line keeping approximate column offset.
    pub fn move_down(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        let curr_line_start = self.text[..cur].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_chars = self.text[curr_line_start..cur].chars().count();
        let Some(rel_nl) = self.text[cur..].find('\n') else {
            self.move_end(extend_selection);
            return;
        };
        let next_line_start = cur + rel_nl + 1;
        let next_line_end = self.text[next_line_start..]
            .find('\n')
            .map(|off| next_line_start + off)
            .unwrap_or(self.text.len());
        let target_byte = next_line_start
            + self.text[next_line_start..next_line_end]
                .char_indices()
                .nth(col_chars)
                .map(|(off, _)| off)
                .unwrap_or(next_line_end - next_line_start);
        self.set_cursor_at(target_byte, extend_selection);
    }

    pub fn set_cursor_at(&mut self, byte_index: usize, extend_selection: bool) {
        self.cursor = self.clamp_boundary(byte_index);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    /// String displayed with active IME preedit inserted at cursor.
    pub fn display_text(&self) -> Cow<'_, str> {
        match &self.preedit {
            Some(pre) if !pre.is_empty() => {
                let cur = self.clamp_boundary(self.cursor);
                let mut out = String::with_capacity(self.text.len() + pre.len());
                out.push_str(&self.text[..cur]);
                out.push_str(pre);
                out.push_str(&self.text[cur..]);
                Cow::Owned(out)
            }
            _ => Cow::Borrowed(&self.text),
        }
    }

    fn clamp_boundary(&self, idx: usize) -> usize {
        let mut i = idx.min(self.text.len());
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn prev_char_boundary(&self, idx: usize) -> usize {
        if idx == 0 {
            return 0;
        }
        let mut i = idx - 1;
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        if idx >= self.text.len() {
            return self.text.len();
        }
        let mut i = idx + 1;
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    fn prev_word_boundary(&self, idx: usize) -> usize {
        let mut cur = idx.min(self.text.len());
        let mut iter = self.text[..cur].char_indices().rev().peekable();
        // Skip trailing whitespace / punctuation before cursor
        while let Some(&(pos, ch)) = iter.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                break;
            }
            cur = pos;
            iter.next();
        }
        // Walk backwards across word characters
        for (pos, ch) in iter {
            if !(ch.is_alphanumeric() || ch == '_') {
                break;
            }
            cur = pos;
        }
        cur
    }

    fn next_word_boundary(&self, idx: usize) -> usize {
        let start = idx.min(self.text.len());
        let mut iter = self.text[start..].char_indices().peekable();
        while let Some(&(_, ch)) = iter.peek() {
            if !(ch.is_alphanumeric() || ch == '_') {
                break;
            }
            iter.next();
        }
        while let Some(&(off, ch)) = iter.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                return start + off;
            }
            iter.next();
        }
        self.text.len()
    }
}
