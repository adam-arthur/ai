//! Shared utilities for the harness workspace.

#![forbid(unsafe_code)]

/// A byte allowance that can be consumed without overflowing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteBudget {
    remaining: usize,
}

impl ByteBudget {
    pub const fn new(bytes: usize) -> Self {
        Self { remaining: bytes }
    }

    /// Consumes `bytes` when they fit within the remaining allowance.
    pub const fn try_consume(&mut self, bytes: usize) -> bool {
        if bytes > self.remaining {
            return false;
        }

        self.remaining -= bytes;
        true
    }

    pub const fn remaining(&self) -> usize {
        self.remaining
    }
}

/// Bounds a string to `max_bytes` without splitting a UTF-8 character.
///
/// Returns the bounded string and whether it was truncated.
pub fn bounded_str(value: &str, max_bytes: usize) -> (&str, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }

    let boundary = value.floor_char_boundary(max_bytes);
    (&value[..boundary], true)
}

/// Keeps complete strings while their combined length fits within `max_bytes`.
///
/// Returns the retained strings and whether any string was omitted.
pub fn bounded_strings(strings: Vec<String>, max_bytes: usize) -> (Vec<String>, bool) {
    let mut budget = ByteBudget::new(max_bytes);
    let mut output = Vec::new();
    for string in strings {
        if !budget.try_consume(string.len()) {
            return (output, true);
        }
        output.push(string);
    }
    (output, false)
}

#[cfg(test)]
mod tests {
    use super::{ByteBudget, bounded_str, bounded_strings};

    #[test]
    fn consumes_bytes_within_the_budget() {
        let mut budget = ByteBudget::new(5);

        assert!(budget.try_consume(3));
        assert_eq!(budget.remaining(), 2);
        assert!(!budget.try_consume(usize::MAX));
        assert_eq!(budget.remaining(), 2);
        assert!(budget.try_consume(2));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn bounds_strings_at_utf8_boundaries() {
        assert_eq!(bounded_str("aéz", 2), ("a", true));
        assert_eq!(bounded_str("aéz", 3), ("aé", true));
        assert_eq!(bounded_str("aéz", 4), ("aéz", false));
    }

    #[test]
    fn bounds_string_collections_at_complete_items() {
        let strings = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];

        assert_eq!(
            bounded_strings(strings, 6),
            (vec!["one".to_owned(), "two".to_owned()], true)
        );
    }

    #[test]
    fn reports_unbounded_string_collections() {
        let strings = vec!["one".to_owned(), "two".to_owned()];

        assert_eq!(bounded_strings(strings.clone(), 6), (strings, false));
    }
}
