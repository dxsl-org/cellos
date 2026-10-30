//! Class guard: the cell never asks for opcodes 21-24 and holds no syscall or manifest
//! authority beyond the terminal-session child set.

const SOURCES: [(&str, &str); 9] = [
    ("main.rs", include_str!("../main.rs")),
    ("armor.rs", include_str!("../armor.rs")),
    ("console.rs", include_str!("../console.rs")),
    ("delete.rs", include_str!("../delete.rs")),
    ("import.rs", include_str!("../import.rs")),
    ("kms.rs", include_str!("../kms.rs")),
    ("report.rs", include_str!("../report.rs")),
    ("secret.rs", include_str!("../secret.rs")),
    ("status.rs", include_str!("../status.rs")),
];

#[test]
fn no_source_names_a_sign_or_unlock_operation() {
    // Opcodes 21-24 and their client methods and payload types.
    const BANNED: [&str; 14] = [
        "ssh_sign_userauth",
        "ssh_unlock_begin",
        "ssh_unlock_chunk",
        "ssh_unlock_abort",
        "ssh_stage_secret",
        "SignSshUserauth",
        "BeginSshUnlock",
        "SshUnlockChunk",
        "AbortSshUnlock",
        "SshSignRequest",
        "SshSignResponse",
        "SshUnlockBeginRequest",
        "KmsOpcode::",
        "request_id: 2",
    ];
    for (file, source) in SOURCES {
        for word in BANNED {
            assert!(!source.contains(word), "{file} names {word}");
        }
    }
}

#[test]
fn the_kms_seam_is_exactly_opcodes_15_to_20() {
    let seam = include_str!("../kms.rs");
    let trait_body =
        &seam[seam.find("pub trait Kms").unwrap()..seam.find("impl Kms for KmsClient").unwrap()];
    let methods: Vec<&str> = trait_body
        .lines()
        .filter_map(|l| l.trim().strip_prefix("fn "))
        .map(|l| l.split(['(', '<']).next().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "status",
            "import_begin",
            "import_chunk",
            "import_commit",
            "import_abort",
            "delete"
        ]
    );
    // The client methods behind them are the admin ones.
    for name in [
        "ssh_key_status",
        "ssh_import_begin",
        "ssh_import_chunk",
        "ssh_import_commit",
        "ssh_import_abort",
        "ssh_delete",
    ] {
        assert!(seam.contains(name), "{name}");
    }
}

#[test]
fn the_allowlist_is_the_terminal_session_child_set() {
    let main = include_str!("../main.rs");
    let start = main.find("api::declare_syscalls![").unwrap();
    let block = &main[start..start + main[start..].find("];").unwrap()];
    let names: Vec<&str> = block
        .lines()
        .skip(1)
        .map(|l| l.trim().trim_end_matches(','))
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        names,
        [
            "Send",
            "Recv",
            "TryRecv",
            "RecvTimeout",
            "Reply",
            "Log",
            "Exit",
            "Yield",
            "Heartbeat",
            "LookupService",
            "GetTime",
            "SetTimer",
            "MemInfo"
        ]
    );
    for forbidden in [
        "GetRandom",
        "Spawn",
        "SpawnFromPath",
        "SpawnFromElf",
        "NetTx",
        "NetRx",
        "VfsMutate",
        "Open",
        "Read",
        "Write",
        "Wait",
        "StateStash",
    ] {
        assert!(!names.contains(&forbidden), "{forbidden}");
    }
    // The manifest is all false.
    let manifest = &main[main.find("api::declare_manifest!(").unwrap()..];
    let manifest = &manifest[..manifest.find(");").unwrap()];
    for field in ["block_io", "network", "spawn", "gpio", "uart", "hypervisor"] {
        assert!(
            manifest.contains(&std::format!("{field} = false")),
            "{field}"
        );
    }
    assert!(!manifest.contains("true"));
}

#[test]
fn the_cell_logs_nothing_and_uses_no_entropy_or_files() {
    for (file, source) in SOURCES {
        for word in [
            "sys_log",
            "println",
            "sys_get_random",
            "VfsClient",
            "sys_spawn",
            "NetClient",
            "format!(\"{:?}\", blob",
        ] {
            // `report.rs` formats error names only; `println`/`sys_log` are absent everywhere.
            assert!(!source.contains(word), "{file} uses {word}");
        }
    }
}
