use super::{extract_content, extract_tool_call};

fn response(content: &str) -> alloc::string::String {
    alloc::format!(r#"{{"choices":[{{"message":{{"content":{content}}}}}]}}"#)
}

#[test]
fn content_decodes_json_escapes_and_unicode() {
    let body = response(r#""line\n\"quoted\" \/ \u263a \ud83d\ude80""#);
    assert_eq!(
        extract_content(body.as_bytes()).as_deref(),
        Some("line\n\"quoted\" / ☺ 🚀")
    );
}

#[test]
fn content_rejects_malformed_trailing_and_wrong_shape() {
    for body in [
        br#"{"choices":[{"message":{"content":"cut}}]}"#.as_slice(),
        br#"{"choices":[{"message":{"content":"ok"}}]} trailing"#,
        br#"{"choices":[{"message":{"content":7}}]}"#,
        br#"{"content":"decoy","choices":[]}"#,
        &[0xff, 0xfe],
    ] {
        assert_eq!(extract_content(body), None);
    }
}

#[test]
fn tool_call_accepts_nested_arguments() {
    let call = extract_tool_call(
        r#"TOOL_CALL: {"name":"read_file","args":{"path":"/tmp/a\"b","meta":{"retry":true},"ids":[1,2]}}"#,
    )
    .expect("valid nested tool call must parse");
    assert_eq!(call.name, "read_file");
    let args: ostd::json::Value =
        ostd::json::from_slice(call.args_json.as_bytes()).expect("serialized args must be JSON");
    assert_eq!(args["path"], "/tmp/a\"b");
    assert_eq!(args["meta"]["retry"], true);
    assert_eq!(args["ids"][1], 2);
}

#[test]
fn tool_call_defaults_missing_args_and_rejects_non_object_args() {
    let call = extract_tool_call(r#"TOOL_CALL: {"name":"list_cells"}"#)
        .expect("missing args retains the empty-object compatibility contract");
    assert_eq!(call.args_json, "{}");
    assert!(extract_tool_call(r#"TOOL_CALL: {"name":"bad","args":[]}"#).is_none());
}

#[test]
fn duplicate_keys_fail_closed_at_every_object_depth() {
    for content in [
        r#"TOOL_CALL: {"name":"a","name":"b","args":{}}"#,
        r#"TOOL_CALL: {"name":"a","args":{"path":"a","path":"b"}}"#,
        r#"TOOL_CALL: {"name":"a","args":{"items":[{"id":1,"id":2}]}}"#,
        r#"TOOL_CALL: {"name":"a","\u006eame":"b","args":{}}"#,
    ] {
        assert!(extract_tool_call(content).is_none(), "accepted {content}");
    }
}

#[test]
fn duplicate_response_content_fails_closed() {
    let body = br#"{"choices":[{"message":{"content":"safe","content":"evil"}}]}"#;
    assert_eq!(extract_content(body), None);
}
