use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityStatus {
    Selected,
    NotSelected,
    Error,
}

impl UtilityStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Selected => 0,
            Self::NotSelected => 1,
            Self::Error => 2,
        }
    }
}

pub struct ArgCursor<'a> {
    args: &'a [String],
    index: usize,
}

impl<'a> ArgCursor<'a> {
    pub fn new(args: &'a [String]) -> Self {
        Self { args, index: 0 }
    }

    pub fn next_owned(&mut self) -> Option<String> {
        self.next().map(String::from)
    }
}

impl<'a> Iterator for ArgCursor<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.args.get(self.index)?;
        self.index += 1;
        Some(value.as_str())
    }
}

pub struct LegacyArgs<'a> {
    args: core::slice::Iter<'a, String>,
}

impl<'a> Iterator for LegacyArgs<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.args.next().map(String::as_str)
    }
}

pub fn with_legacy_parts<R>(args: &[String], f: impl FnOnce(LegacyArgs<'_>) -> R) -> R {
    f(LegacyArgs { args: args.iter() })
}

#[cfg(test)]
mod tests {
    use super::{with_legacy_parts, ArgCursor};
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn legacy_parts_preserve_argv_boundaries_and_empty_values() {
        let args = vec![
            String::from("/tmp/path with spaces"),
            String::new(),
            String::from("tail"),
        ];
        // Collect owned copies: `LegacyArgs` borrows the slice for an anonymous
        // lifetime that cannot escape the callback.
        let values = with_legacy_parts(&args, |parts| {
            parts.map(str::to_string).collect::<Vec<String>>()
        });
        assert_eq!(values, ["/tmp/path with spaces", "", "tail"]);
    }

    #[test]
    fn arg_cursor_walks_once_and_then_reports_exhaustion() {
        let args = vec![String::from("-e"), String::from("a b")];
        let mut cursor = ArgCursor::new(&args);
        assert_eq!(cursor.next(), Some("-e"));
        assert_eq!(cursor.next_owned().as_deref(), Some("a b"));
        assert_eq!(cursor.next(), None);
    }
}
