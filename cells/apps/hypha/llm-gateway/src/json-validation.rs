//! Complete JSON validation with duplicate-key rejection.

use alloc::collections::BTreeSet;
use alloc::string::String;
use ostd::json::Value;

const MAX_JSON_DEPTH: usize = 32;

/// Parse one JSON value while rejecting duplicate decoded keys at every depth.
pub(super) fn parse_unique(input: &[u8]) -> Option<Value> {
    let value = ostd::json::from_slice(input).ok()?;
    let mut cursor = Cursor { input, at: 0 };
    cursor.value(0)?;
    cursor.whitespace();
    (cursor.at == input.len()).then_some(value)
}

struct Cursor<'a> {
    input: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn value(&mut self, depth: usize) -> Option<()> {
        self.whitespace();
        match self.current()? {
            b'{' if depth < MAX_JSON_DEPTH => self.object(depth + 1),
            b'[' if depth < MAX_JSON_DEPTH => self.array(depth + 1),
            b'"' => {
                self.string_end()?;
                Some(())
            }
            b'{' | b'[' => None,
            _ => self.scalar(),
        }
    }

    fn object(&mut self, depth: usize) -> Option<()> {
        self.at += 1;
        self.whitespace();
        if self.take(b'}') {
            return Some(());
        }
        let mut keys = BTreeSet::<String>::new();
        loop {
            self.whitespace();
            let start = self.at;
            self.string_end()?;
            let key = ostd::json::from_slice::<String>(&self.input[start..self.at]).ok()?;
            if !keys.insert(key) {
                return None;
            }
            self.whitespace();
            if !self.take(b':') {
                return None;
            }
            self.value(depth)?;
            self.whitespace();
            if self.take(b'}') {
                return Some(());
            }
            if !self.take(b',') {
                return None;
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<()> {
        self.at += 1;
        self.whitespace();
        if self.take(b']') {
            return Some(());
        }
        loop {
            self.value(depth)?;
            self.whitespace();
            if self.take(b']') {
                return Some(());
            }
            if !self.take(b',') {
                return None;
            }
        }
    }

    fn string_end(&mut self) -> Option<()> {
        if !self.take(b'"') {
            return None;
        }
        while let Some(byte) = self.current() {
            self.at += 1;
            match byte {
                b'"' => return Some(()),
                b'\\' => self.at = self.at.checked_add(1)?,
                _ => {}
            }
            if self.at > self.input.len() {
                return None;
            }
        }
        None
    }

    fn scalar(&mut self) -> Option<()> {
        let start = self.at;
        while let Some(byte) = self.current() {
            if matches!(byte, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                break;
            }
            self.at += 1;
        }
        (self.at > start).then_some(())
    }

    fn whitespace(&mut self) {
        while matches!(self.current(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn current(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.current() == Some(expected) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_unique, MAX_JSON_DEPTH};
    use alloc::string::String;

    fn nested_empty_arrays(depth: usize) -> String {
        let mut json = String::with_capacity(depth * 2);
        for _ in 0..depth {
            json.push('[');
        }
        for _ in 0..depth {
            json.push(']');
        }
        json
    }

    #[test]
    fn container_depth_limit_is_exact() {
        assert!(parse_unique(nested_empty_arrays(MAX_JSON_DEPTH).as_bytes()).is_some());
        assert!(parse_unique(nested_empty_arrays(MAX_JSON_DEPTH + 1).as_bytes()).is_none());
    }
}
