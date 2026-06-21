use super::*;

#[test]
fn get_str_nested_hit() {
    let v = json!({"a": {"b": {"c": "leaf"}}});
    assert_eq!(get_str(&v, &["a", "b", "c"]), Some("leaf"));
}

#[test]
fn get_str_miss() {
    let v = json!({"a": {"b": "val"}});
    assert_eq!(get_str(&v, &["a", "x"]), None);
}

#[test]
fn get_str_non_string_leaf() {
    let v = json!({"a": 42});
    assert_eq!(get_str(&v, &["a"]), None);
}

#[test]
fn get_str_empty_path() {
    // Empty path: zero loop iterations → returns current.as_str() directly.
    // String Value → Some; Object Value → None (it is not a string).
    let str_val = json!("just a string");
    assert_eq!(get_str(&str_val, &[]), Some("just a string"));

    let obj_val = json!({"k": "v"});
    assert_eq!(get_str(&obj_val, &[]), None);
}

#[test]
fn get_str_root_key() {
    let v = json!({"key": "value"});
    assert_eq!(get_str(&v, &["key"]), Some("value"));
}

#[test]
fn round_trip_parse_build() {
    let original = json!({"model": "gpt-4", "temperature": 0.7});
    let serialised = to_string(&original).unwrap();
    let parsed: Value = from_slice(serialised.as_bytes()).unwrap();
    assert_eq!(parsed["model"], json!("gpt-4"));
}

#[test]
fn duplicate_key_last_wins() {
    // serde_json default: last occurrence of a duplicate key survives.
    let raw = br#"{"x": 1, "x": 2}"#;
    let v: Value = from_slice(raw).unwrap();
    assert_eq!(v["x"], json!(2));
}

#[test]
fn parse_nested_object() {
    let raw = br#"{"choices":[{"message":{"content":"hello world"}}]}"#;
    let v: Value = from_slice(raw).unwrap();
    assert_eq!(
        get_str(&v["choices"][0], &["message", "content"]),
        Some("hello world")
    );
}
