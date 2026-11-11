//! UTF-8 storage with UTF-16 conversion for macOS native text input.
use std::ops::Range;
#[derive(Default, Debug, Clone)]
pub struct Text {
    pub value: String,
    pub anchor: usize,
    pub cursor: usize,
    pub marked: Option<Range<usize>>,
}
impl Text {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let end = value.len();
        Self {
            value,
            anchor: end,
            cursor: end,
            marked: None,
        }
    }
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.value.len();
    }
    pub fn utf8(&self, offset: usize) -> usize {
        let mut count = 0;
        for (i, ch) in self.value.char_indices() {
            if count >= offset {
                return i;
            }
            count += ch.len_utf16();
        }
        self.value.len()
    }
    pub fn utf16(&self, offset: usize) -> usize {
        self.value[..self.boundary(offset)].encode_utf16().count()
    }
    pub fn from_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.utf8(range.start)..self.utf8(range.end.max(range.start))
    }
    pub fn to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.utf16(range.start)..self.utf16(range.end)
    }
    pub fn boundary(&self, offset: usize) -> usize {
        let mut n = offset.min(self.value.len());
        while !self.value.is_char_boundary(n) {
            n -= 1;
        }
        n
    }
    pub fn replace(&mut self, range: Option<Range<usize>>, value: &str) {
        let r = range
            .or(self.marked.take())
            .unwrap_or_else(|| self.selection());
        let start = self.boundary(r.start);
        let end = self.boundary(r.end.max(start));
        let value = value.replace(['\r', '\n'], " ");
        self.value.replace_range(start..end, &value);
        self.cursor = start + value.len();
        self.anchor = self.cursor;
        self.marked = None;
    }
    pub fn left(&mut self, shift: bool) {
        let selection = self.selection();
        self.cursor = if !shift && !selection.is_empty() {
            selection.start
        } else {
            self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }
    pub fn right(&mut self, shift: bool) {
        let selection = self.selection();
        self.cursor = if !shift && !selection.is_empty() {
            selection.end
        } else {
            self.value[self.cursor..]
                .chars()
                .next()
                .map(|c| self.cursor + c.len_utf8())
                .unwrap_or(self.value.len())
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }
    pub fn backspace(&mut self) {
        if self.selection().is_empty() {
            self.left(true);
        }
        self.replace(None, "");
    }
    pub fn delete(&mut self) {
        if self.selection().is_empty() {
            self.right(true);
        }
        self.replace(None, "");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ranges_roundtrip_unicode() {
        let t = Text::new("a😀తెలుగుé");
        for (byte, _) in t.value.char_indices() {
            assert_eq!(t.utf8(t.utf16(byte)), byte);
        }
        assert_eq!(t.utf8(3), 5);
    }
    #[test]
    fn replace_selection_preserves_utf8() {
        let mut t = Text::new("a😀z");
        t.anchor = 1;
        t.cursor = 5;
        t.replace(None, "你好");
        assert_eq!(t.value, "a你好z");
        assert_eq!(t.cursor, 7);
    }
    #[test]
    fn unicode_delete() {
        let mut t = Text::new("hi😀");
        t.backspace();
        assert_eq!(t.value, "hi");
        t.cursor = 0;
        t.anchor = 0;
        t.delete();
        assert_eq!(t.value, "i");
    }
    #[test]
    fn selection_direction_survives_arrows() {
        let mut t = Text::new("abc");
        t.left(true);
        t.left(true);
        assert_eq!(t.selection(), 1..3);
        t.left(false);
        assert_eq!(t.cursor, 1);
        assert_eq!(t.anchor, 1);
    }
    #[test]
    fn marked_text_is_replaced() {
        let mut t = Text::new("abc");
        t.marked = Some(1..3);
        t.replace(None, "字");
        assert_eq!(t.value, "a字");
    }
}
