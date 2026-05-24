use std::collections::VecDeque;

/// A bounded line buffer for received serial data.
///
/// Bytes arrive in arbitrary chunks; we split on `\n`, strip `\r`, and
/// stash up to `capacity` lines. The current (incomplete) line is kept
/// separately so partial lines render immediately rather than waiting
/// for a newline.
pub struct LineBuffer {
    lines: VecDeque<String>,
    current: String,
    capacity: usize,
}

impl LineBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity),
            current: String::new(),
            capacity,
        }
    }

    /// Push a chunk of received bytes. Non-UTF8 bytes are rendered as `\xNN`
    /// escapes so the user sees something rather than nothing — a real
    /// terminal would interpret them, but this is a console viewer, not
    /// a terminal emulator.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        // Decode lossily so invalid UTF-8 becomes the replacement char
        // instead of being dropped silently.
        let s = String::from_utf8_lossy(bytes);
        for ch in s.chars() {
            match ch {
                '\n' => self.commit_current(),
                '\r' => {} // strip; lines are split on \n only
                '\t' => self.current.push_str("    "),
                c if c.is_control() => {
                    // Render other control chars as their caret form so a
                    // misbehaving device doesn't blank the screen.
                    self.current.push('^');
                    self.current.push(((c as u8 + b'@') & 0x7F) as char);
                }
                c => self.current.push(c),
            }
        }
    }

    fn commit_current(&mut self) {
        let line = std::mem::take(&mut self.current);
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// Return the last `n` lines plus the current partial line, in order.
    pub fn tail(&self, n: usize) -> Vec<&str> {
        let total = self.lines.len() + if self.current.is_empty() { 0 } else { 1 };
        let take = n.min(total);
        let mut out = Vec::with_capacity(take);
        let skip = total.saturating_sub(take);

        let mut idx = 0;
        for line in &self.lines {
            if idx >= skip {
                out.push(line.as_str());
            }
            idx += 1;
        }
        if !self.current.is_empty() && idx >= skip {
            out.push(self.current.as_str());
        }
        out
    }

    pub fn line_count(&self) -> usize {
        self.lines.len() + if self.current.is_empty() { 0 } else { 1 }
    }
}
