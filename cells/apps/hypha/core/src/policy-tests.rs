use crate::preamble::system_preamble;
use crate::tool_gate::{confirmation_prompt, display_escape, is_confirmed, needs_confirmation};
use crate::tool_owner::{owner_arg, parse_caller_tid, parse_owner};
use hypha_tool_fs::workspace::WORKSPACE_ROOT;
use hypha_tool_spawn::spawn_policy::SPAWN_ALLOWLIST;

#[test]
fn side_effecting_tools_need_the_operator() {
    for name in ["spawn_cell", "kill_cell", "write_file"] {
        assert!(needs_confirmation(name), "{name}");
    }
    for name in [
        "read_file",
        "list_dir",
        "list_cells",
        "sys_info",
        "lookup_service",
    ] {
        assert!(!needs_confirmation(name), "{name}");
    }
}

#[test]
fn only_an_explicit_y_approves() {
    for answer in ["y", "Y", "y\n", " y \r\n"] {
        assert!(is_confirmed(answer), "{answer:?}");
    }
    for answer in ["", "\n", "n", "N", "yes", "yy", "ok", "y n", "\u{1b}y"] {
        assert!(!is_confirmed(answer), "{answer:?}");
    }
}

#[test]
fn the_prompt_shows_the_exact_call() {
    let prompt = confirmation_prompt("spawn_cell", r#"{"path":"/bin/tetris"}"#);
    assert!(
        prompt.contains(r#"spawn_cell {"path":"/bin/tetris"}"#),
        "{prompt}"
    );
    assert!(prompt.ends_with("[y/N] "));
}

#[test]
fn model_text_cannot_drive_the_terminal() {
    // Clear screen + cursor home + carriage return would let a reply paint a
    // harmless-looking call over the real one.
    let shown = display_escape("{\"path\":\"/x\"}\x1b[2J\x1b[H\rspawn_cell /bin/tetris");
    assert!(!shown.chars().any(|c| c.is_control()), "{shown:?}");
    assert!(shown.contains("\\u{1b}[2J") && shown.contains("\\u{d}"));
    // Bidi override and zero-width characters reorder or hide text.
    let shown = display_escape("a\u{202e}b\u{200b}c\u{2066}d\u{feff}");
    assert_eq!(shown, "a\\u{202e}b\\u{200b}c\\u{2066}d\\u{feff}");
    // Ordinary JSON, including non-ASCII text, is shown unchanged.
    let json = r#"{"path":"/data/hypha/caf\u00e9","content":"héllo"}"#;
    assert_eq!(display_escape(json), json);
}

#[test]
fn core_and_tools_agree_on_the_owner_binding() {
    // tool-sys answers `caller_tid` with exactly `{"tid":N}`.
    assert_eq!(parse_caller_tid("{\"tid\":42}"), Some(42));
    for bad in [
        "",
        "{\"tid\":0}",
        "{\"tid\":-1}",
        "{\"tid\":42,\"x\":1}",
        "{\"tid\": 42}",
        "{\"tid\":\"42\"}",
    ] {
        assert_eq!(parse_caller_tid(bad), None, "{bad:?}");
    }
    assert_eq!(parse_owner(&[owner_arg(42)]), Some(42));
}

#[test]
fn the_model_is_told_the_real_limits() {
    let preamble = system_preamble();
    assert!(preamble.contains(WORKSPACE_ROOT));
    for path in SPAWN_ALLOWLIST {
        assert!(preamble.contains(path), "{path}");
    }
    assert!(preamble.contains("TOOL_CALL: {\"name\":\"TOOL\",\"args\":{ARGS_JSON}}"));
}
