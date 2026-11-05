// Vendored from cosmonic-labs/agent-tools (crates/tool-common/src/bound.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Unmodified.

//! The output budget.
//!
//! Desktop's toolset dispatcher cuts a result's text at 32 KiB and refuses a
//! result whose structured JSON is over 32 KiB. Every tool here stays inside
//! both on its own, and says what it left out, so the cut never lands in the
//! middle of something with no explanation.

/// The most text one reply carries, the dispatcher's own cap.
pub const TEXT_MAX: usize = 32 * 1024;
/// Kept free at the end of the text for the note that says what was left out.
pub const NOTE_RESERVE: usize = 512;
/// The most structured JSON one reply carries. Half the dispatcher's cap, so
/// a summary never crowds out the text.
pub const STRUCTURED_MAX: usize = 16 * 1024;

/// A text buffer that stops growing at its limit and remembers that it did.
#[derive(Debug)]
pub struct Budget {
    buf: String,
    limit: usize,
    truncated: bool,
    note: Option<String>,
}

impl Default for Budget {
    fn default() -> Self {
        Self::new(TEXT_MAX - NOTE_RESERVE)
    }
}

impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            buf: String::new(),
            limit,
            truncated: false,
            note: None,
        }
    }

    pub fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.buf.len())
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Whether `s` fits whole.
    pub fn fits(&self, s: &str) -> bool {
        s.len() <= self.remaining()
    }

    /// Append `s` whole if it fits. Otherwise append as many whole lines of
    /// it as fit, mark the budget truncated and return false. Once truncated,
    /// nothing more is appended.
    pub fn push(&mut self, s: &str) -> bool {
        if self.truncated {
            return false;
        }
        if self.fits(s) {
            self.buf.push_str(s);
            return true;
        }
        let head = line_prefix(s, self.remaining());
        self.buf.push_str(head);
        self.truncated = true;
        false
    }

    /// Append `s` only if it fits whole; never truncates.
    pub fn push_whole(&mut self, s: &str) -> bool {
        if !self.truncated && self.fits(s) {
            self.buf.push_str(s);
            true
        } else {
            false
        }
    }

    /// Mark the budget as cut and say what was left out; this note wins over
    /// the one passed to [`Budget::finish`].
    pub fn set_note(&mut self, note: String) {
        self.truncated = true;
        self.note = Some(note);
    }

    /// Mark the budget as cut without appending.
    pub fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    /// The text, with `note` appended on its own line when anything was left
    /// out. The note is cut to [`NOTE_RESERVE`] so the total stays within
    /// `limit + NOTE_RESERVE`.
    pub fn finish(mut self, note: impl FnOnce() -> String) -> String {
        if self.truncated {
            if !self.buf.is_empty() && !self.buf.ends_with('\n') {
                self.buf.push('\n');
            }
            let note = self.note.take().unwrap_or_else(note);
            self.buf.push_str(utf8_prefix(&note, NOTE_RESERVE - 2));
            self.buf.push('\n');
        }
        self.buf
    }

    /// The text so far, without a note.
    pub fn into_string(self) -> String {
        self.buf
    }
}

/// The longest prefix of `s` within `max` bytes that ends on a char
/// boundary.
pub fn utf8_prefix(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The longest prefix of `s` within `max` bytes that ends just after a
/// newline, or on a char boundary when no newline fits.
pub fn line_prefix(s: &str, max: usize) -> &str {
    let p = utf8_prefix(s, max);
    match p.rfind('\n') {
        Some(i) => &p[..=i],
        None => p,
    }
}

/// Serialize `value` if it fits in [`STRUCTURED_MAX`]; otherwise `fallback`
/// (which must be small), so a reply never trips the dispatcher's cap.
pub fn structured(value: &serde_json::Value, fallback: serde_json::Value) -> Option<String> {
    let s = value.to_string();
    if s.len() <= STRUCTURED_MAX {
        Some(s)
    } else {
        Some(fallback.to_string())
    }
}

/// `text` cut to `max` bytes at a line end, with a note saying how much was
/// left out. Used for one file's patch, one comment's body and the like.
pub fn clip(text: &str, max: usize, what: &str) -> (String, bool) {
    if text.len() <= max {
        return (text.to_string(), false);
    }
    let head = line_prefix(text, max);
    let rest = &text[head.len()..];
    let lines = rest.lines().count();
    let mut out = head.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "[{what} truncated: {} more bytes, {lines} more line{} not shown]\n",
        rest.len(),
        if lines == 1 { "" } else { "s" }
    ));
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keeps_whole_lines_then_stops() {
        let mut b = Budget::new(10);
        assert!(b.push("abc\n"));
        assert!(!b.push("defg\nhijk\n"));
        assert!(b.truncated());
        assert!(!b.push("x"));
        let out = b.finish(|| "[cut]".into());
        assert_eq!(out, "abc\ndefg\n[cut]\n");
        assert!(out.len() <= 10 + NOTE_RESERVE);
    }

    #[test]
    fn push_whole_never_cuts() {
        let mut b = Budget::new(5);
        assert!(!b.push_whole("123456"));
        assert!(!b.truncated());
        assert!(b.push_whole("12345"));
        assert_eq!(b.finish(|| unreachable!()), "12345");
    }

    #[test]
    fn utf8_safe() {
        let s = "aé€😀";
        for max in 0..=s.len() {
            let p = utf8_prefix(s, max);
            assert!(p.len() <= max);
            assert!(s.starts_with(p));
        }
        let mut b = Budget::new(4);
        b.push("😀😀");
        assert_eq!(b.into_string(), "😀");
    }

    #[test]
    fn finish_stays_within_reserve() {
        let mut b = Budget::new(100);
        b.push(&"x".repeat(200));
        let out = b.finish(|| "n".repeat(10_000));
        assert!(out.len() <= 100 + NOTE_RESERVE, "{}", out.len());
    }

    #[test]
    fn set_note_wins() {
        let mut b = Budget::new(10);
        b.push("a\n");
        b.set_note("[specific]".into());
        assert_eq!(b.finish(|| "[generic]".into()), "a\n[specific]\n");
    }

    #[test]
    fn clip_notes_what_was_left_out() {
        let (s, cut) = clip("one\ntwo\nthree\n", 8, "patch");
        assert!(cut);
        assert_eq!(
            s,
            "one\ntwo\n[patch truncated: 6 more bytes, 1 more line not shown]\n"
        );
        let (s, cut) = clip("short", 8, "patch");
        assert!(!cut);
        assert_eq!(s, "short");
    }

    #[test]
    fn structured_falls_back_when_large() {
        let big = serde_json::json!({ "x": "y".repeat(STRUCTURED_MAX) });
        let s = structured(&big, serde_json::json!({"truncated": true})).unwrap();
        assert_eq!(s, r#"{"truncated":true}"#);
    }
}
