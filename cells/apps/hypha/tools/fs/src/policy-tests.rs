use crate::tool_owner::{admits, owner_arg, parse_owner};
use crate::workspace::{confine, PathRefusal, MAX_PATH_BYTES, WORKSPACE_ROOT};
use alloc::string::String;

#[test]
fn only_the_spawning_core_is_served() {
    let owner = parse_owner(&[owner_arg(41)]).expect("core's argv binds the tool");
    assert!(admits(owner, 41));
    // Any other cell — including one that learned the tool's tid from GetProcs.
    assert!(!admits(owner, 40));
    assert!(!admits(owner, 42));
    assert!(!admits(owner, 0));
}

#[test]
fn malformed_or_missing_owner_leaves_the_tool_unbound() {
    let s = |v: &str| String::from(v);
    assert_eq!(parse_owner(&[]), None);
    assert_eq!(parse_owner(&[s("41")]), None);
    assert_eq!(parse_owner(&[s("--owner=")]), None);
    assert_eq!(parse_owner(&[s("--owner=0")]), None);
    assert_eq!(parse_owner(&[s("--owner=-1")]), None);
    assert_eq!(parse_owner(&[s("--owner=+41")]), None);
    assert_eq!(parse_owner(&[s("--owner=41 ")]), None);
    assert_eq!(parse_owner(&[s("--owner=99999999999999999999999")]), None);
    assert_eq!(parse_owner(&[owner_arg(41), owner_arg(42)]), None);
}

#[test]
fn workspace_paths_are_accepted() {
    assert_eq!(confine(WORKSPACE_ROOT), Ok(WORKSPACE_ROOT));
    assert_eq!(confine("/data/hypha/"), Ok(WORKSPACE_ROOT));
    assert_eq!(
        confine("/data/hypha/notes.txt"),
        Ok("/data/hypha/notes.txt")
    );
    assert_eq!(confine("/data/hypha/a/b c.md"), Ok("/data/hypha/a/b c.md"));
    assert_eq!(confine("/data/hypha/sub/"), Ok("/data/hypha/sub"));
}

#[test]
fn escapes_from_the_workspace_are_refused() {
    use PathRefusal::*;
    assert_eq!(confine("/data/hypha/../../bin/hypha"), Err(BadComponent));
    assert_eq!(confine("/data/hypha/.."), Err(BadComponent));
    assert_eq!(confine("/data/hypha/./x"), Err(BadComponent));
    assert_eq!(confine("/data/hypha//x"), Err(BadComponent));
    assert_eq!(confine("/data/hypha/x//"), Err(BadComponent));
    assert_eq!(confine("/data/hyphae/x"), Err(OutsideWorkspace));
    assert_eq!(confine("/data/hypha-evil"), Err(OutsideWorkspace));
    assert_eq!(confine("/data"), Err(OutsideWorkspace));
    assert_eq!(confine("/data/notes.txt"), Err(OutsideWorkspace));
    assert_eq!(confine("/bin/hypha"), Err(OutsideWorkspace));
    assert_eq!(confine("/etc/policy"), Err(OutsideWorkspace));
    assert_eq!(confine("/"), Err(BadComponent));
    assert_eq!(confine("data/hypha/x"), Err(NotAbsolute));
    assert_eq!(confine("notes.txt"), Err(NotAbsolute));
    assert_eq!(confine(""), Err(Empty));
}

#[test]
fn bytes_the_vfs_could_reinterpret_are_refused() {
    use PathRefusal::BadByte;
    // The args parser does not decode JSON escapes: `\u002e\u002e` must never
    // reach VFS as something other than what the operator saw.
    assert_eq!(confine("/data/hypha/\\u002e\\u002e/x"), Err(BadByte));
    assert_eq!(confine("/data/hypha/a\\b"), Err(BadByte));
    assert_eq!(confine("/data/hypha/a\0b"), Err(BadByte));
    assert_eq!(confine("/data/hypha/a\nb"), Err(BadByte));
    assert_eq!(confine("/data/hypha/\x1b[2Jx"), Err(BadByte));
    assert_eq!(confine("/data/hypha/caf\u{e9}"), Err(BadByte));
}

#[test]
fn overlong_paths_are_refused() {
    let mut long = String::from("/data/hypha/");
    while long.len() <= MAX_PATH_BYTES {
        long.push('a');
    }
    assert_eq!(confine(&long), Err(PathRefusal::TooLong));
}
