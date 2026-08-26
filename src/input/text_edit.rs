//! UTF-8-safe single-line text editing (caret fixed at the end in v1).

/// Append printable input, dropping control characters.
pub fn push_str(s: &mut String, input: &str) {
    for ch in input.chars() {
        if !ch.is_control() {
            s.push(ch);
        }
    }
}

/// Remove the last character (whole char, never a stray byte).
pub fn backspace(s: &mut String) {
    s.pop();
}

/// Remove the trailing word plus any trailing whitespace (Ctrl+Backspace).
pub fn backspace_word(s: &mut String) {
    while s.ends_with(char::is_whitespace) {
        s.pop();
    }
    while !s.is_empty() && !s.ends_with(char::is_whitespace) {
        s.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_backspace_removes_whole_char() {
        let mut s = String::from("wörld");
        backspace(&mut s);
        assert_eq!(s, "wörl");
        let mut s = String::from("hé");
        backspace(&mut s);
        assert_eq!(s, "h");
    }

    #[test]
    fn push_filters_control_chars() {
        let mut s = String::new();
        push_str(&mut s, "ab\u{7f}\ncd");
        assert_eq!(s, "abcd");
    }

    #[test]
    fn word_backspace() {
        let mut s = String::from("hello wörld  ");
        backspace_word(&mut s);
        assert_eq!(s, "hello ");
        backspace_word(&mut s);
        assert_eq!(s, "");
    }
}
