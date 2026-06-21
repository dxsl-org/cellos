//! JSON codec — thin re-export layer over `serde_json` (alloc feature).
//!
//! Re-exports provide a stable surface so callers can `use http_core::json::*`
//! without taking a direct `serde_json` dependency.  If the underlying codec
//! ever changes, only this module needs updating.

pub use serde_json::{from_slice, json, to_string, to_vec, Error, Value};

/// Walk a nested JSON object by a sequence of string keys and return the
/// innermost value as a `&str`, or `None` if any step is absent or non-string.
///
/// An empty `path` slice skips all navigation and calls `as_str()` on `value`
/// directly — returning `Some` if it is a JSON string, `None` otherwise.
///
/// # Duplicate-key behaviour
///
/// `serde_json` deserialises objects into a `Map<String, Value>` backed by an
/// insertion-order-preserving `IndexMap`.  When a JSON object contains duplicate
/// keys, **the last occurrence wins** (serde_json 1.x default; documented here
/// so callers are not surprised by silently-dropped earlier values).
///
/// # Example
///
/// ```
/// use http_core::json::{json, get_str};
///
/// let v = json!({"choices": [{"message": {"content": "hello"}}]});
/// // get_str only walks object keys, not array indices
/// assert_eq!(get_str(&v, &["choices"]), None); // value is an array, not a string
/// assert_eq!(get_str(&v["choices"][0], &["message", "content"]), Some("hello"));
/// ```
pub fn get_str<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.as_object()?.get(*key)?;
    }
    current.as_str()
}

#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
