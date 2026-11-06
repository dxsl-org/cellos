use crate::spawn_policy::{spawn_allowed, SpawnedCells, MAX_SPAWNED, SPAWN_ALLOWLIST};
use crate::tool_owner::{admits, owner_arg, parse_owner};
use alloc::string::String;

#[test]
fn a_cell_that_did_not_spawn_the_tool_cannot_invoke_it() {
    let owner = parse_owner(&[owner_arg(7)]).expect("bound");
    assert!(admits(owner, 7));
    for intruder in [0, 6, 8, usize::MAX] {
        assert!(
            !admits(owner, intruder),
            "sender {intruder} must be dropped"
        );
    }
    // Spawned without the owner argv (e.g. by a shell): unbound, serves nobody.
    assert_eq!(parse_owner(&[]), None);
    assert_eq!(parse_owner(&[String::from("7")]), None);
}

#[test]
fn only_allowlisted_paths_spawn() {
    for path in SPAWN_ALLOWLIST {
        assert!(spawn_allowed(path), "{path}");
    }
    for path in [
        "/bin/nc",
        "/bin/httpd",
        "/bin/curl",
        "/bin/tool-spawn",
        "/bin/hypha",
        "/bin/cellbox",
        "/bin/hotswap",
        "/bin/shutdown",
        "/bin/../bin/tetris",
        "/bin//tetris",
        "/bin/tetris/",
        "/bin/tetris ",
        "bin/tetris",
        "tetris",
        "/data/bin/tetris",
        "/data/hypha/tetris",
        "/BIN/TETRIS",
        "",
    ] {
        assert!(!spawn_allowed(path), "{path:?} must be refused");
    }
}

#[test]
fn kill_reaches_only_cells_this_session_spawned() {
    let mut cells = SpawnedCells::new();
    assert!(!cells.contains(5), "nothing spawned yet: no kill target");
    assert!(cells.record(5));
    assert!(cells.record(9));
    assert!(cells.contains(5) && cells.contains(9));
    // A system cell, core itself, or any other tid the LLM names.
    assert!(!cells.contains(1));
    assert!(!cells.contains(6));
    assert!(!cells.contains(0));
    assert!(cells.release(5));
    assert!(!cells.contains(5), "a killed tid cannot be killed again");
    assert!(!cells.release(5));
    assert!(cells.contains(9));
}

#[test]
fn tracking_is_bounded_and_rejects_bogus_tids() {
    let mut cells = SpawnedCells::new();
    assert!(!cells.record(0));
    for tid in 1..=MAX_SPAWNED {
        assert!(cells.record(tid));
    }
    assert!(cells.is_full());
    assert!(!cells.record(MAX_SPAWNED + 1));
    assert!(!cells.contains(MAX_SPAWNED + 1));
    assert!(cells.release(3));
    assert!(!cells.is_full());
    assert!(!cells.record(4), "duplicates are not tracked twice");
    assert!(cells.record(100));
    for tid in (1..=MAX_SPAWNED).filter(|t| *t != 3) {
        assert!(cells.contains(tid));
    }
    assert!(cells.contains(100));
}
