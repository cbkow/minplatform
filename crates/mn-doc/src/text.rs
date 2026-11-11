//! The one map between UTF-8 byte offsets (this crate, the layout engine) and UTF-16 code-unit offsets (the file's
//! formatting offsets; the desktop's text protocols on macOS and Windows). Row 283, ruling 3.

/// UTF-16 length of `s`.
pub fn len16(s: &str) -> usize { s.chars().map(char::len_utf16).sum() }

/// The UTF-16 offset of byte offset `byte` in `s`. A `byte` inside a character counts that character as not yet
/// passed; past the end gives the length.
pub fn to16(s: &str, byte: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() { if i + c.len_utf8() > byte { break } n += c.len_utf16(); }
    n
}

/// The byte offset of UTF-16 offset `unit` in `s`. A `unit` in the middle of a surrogate pair gives the START of that
/// character (an outside tool may land there; a character is never split); past the end gives the length.
pub fn to8(s: &str, unit: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() { if n + c.len_utf16() > unit { return i } n += c.len_utf16(); }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: &str = "a\u{e9}\u{2014}\u{1F600}z";   // 1, 2, 3 and 4 bytes; the emoji is two UTF-16 units
    #[test]
    fn lengths_and_offsets_agree_with_the_standard_encoder() {
        assert_eq!(len16(S), S.encode_utf16().count());
        for (i, _) in S.char_indices().chain([(S.len(), ' ')]) {
            assert_eq!(to16(S, i), S[..i].encode_utf16().count());
            assert_eq!(to8(S, to16(S, i)), i);
        }
    }
    #[test]
    fn an_offset_inside_a_character_never_splits_it() {
        let emoji = S.find('\u{1F600}').unwrap();
        assert_eq!(to16(S, emoji + 2), to16(S, emoji));          // a byte inside the emoji
        assert_eq!(to8(S, to16(S, emoji) + 1), emoji);           // the low surrogate
        assert_eq!(to16(S, 999), len16(S));
        assert_eq!(to8(S, 999), S.len());
    }
}
