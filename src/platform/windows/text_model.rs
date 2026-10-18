//! UTF-16 IME offsets converted without splitting a UTF-8 scalar.
use std::ops::Range;

pub fn utf16_offset(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

pub fn utf16_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf16_offset(text, range.start);
    let end = utf16_offset(text, range.end).max(start);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_selection_is_relative_to_inserted_text() {
        let origin = "prefix中文".len();
        let r = utf16_range("你😀好", 1..3);
        let absolute = origin + r.start..origin + r.end;
        let text = "prefix中文你😀好suffix";
        assert_eq!(&text[absolute], "😀");
    }
    #[test]
    fn surrogate_offsets_never_split_utf8() {
        let s = "a😀中";
        assert_eq!(utf16_offset(s, 2), 1);
        assert_eq!(utf16_offset(s, 3), 5);
        assert_eq!(utf16_offset(s, usize::MAX), s.len());
        assert_eq!(&s[utf16_range(s, 3..2)], "");
    }
}
