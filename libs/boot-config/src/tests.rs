use super::*;
use alloc::{format, vec};

fn cells(body: &str) -> Result<CellConfig, ConfigError> {
    parse_cells(format!("version = 1\n{body}").as_bytes())
}

fn entry(name: &str) -> String {
    format!("[[cells]]\nname = '{name}'\npath = '/bin/{name}'\n")
}

fn valid(body: &str) -> CellConfig { cells(body).unwrap() }

#[test]
fn system_defaults_and_explicit_logging_are_distinct() {
    let omitted = parse_system(b"version = 1").unwrap();
    assert_eq!(omitted, SystemConfig::default());
    let explicit = parse_system(b"version = 1\n[logging]").unwrap();
    assert_eq!(explicit.logging.level, LogLevel::Info);
    assert!(explicit.logging_explicit);
    let memory_only = parse_system(b"version = 1\n[memory]\ndefault_cell_heap_mib = 1").unwrap();
    assert!(!memory_only.logging_explicit);
    assert_eq!(memory_only.memory.default_cell_heap_mib, 1);
}

#[test]
fn every_logging_level_and_heap_boundary_parses() {
    for (text, level) in [("off", LogLevel::Off), ("error", LogLevel::Error), ("warn", LogLevel::Warn),
        ("info", LogLevel::Info), ("debug", LogLevel::Debug), ("trace", LogLevel::Trace)]
    {
        let config = parse_system(format!("version=1\n[logging]\nlevel='{text}'\n[memory]\ndefault_cell_heap_mib=16").as_bytes()).unwrap();
        assert_eq!(config.logging.level, level);
        assert!(config.logging_explicit);
    }
    for heap in [0, 17, 1024] {
        assert_eq!(parse_system(format!("version=1\n[memory]\ndefault_cell_heap_mib={heap}").as_bytes()), Err(ConfigError::HeapOutOfRange));
    }
    assert!(parse_system(b"version=1\n[logging]\nlevel='verbose'").is_err());
}

#[test]
fn versions_are_required_and_unknown_fields_are_rejected_recursively() {
    for text in ["", "[memory]", "version=1\nextra=true", "version=1\n[logging]\nextra=true",
        "version=1\n[memory]\nextra=true"]
    {
        assert!(parse_system(text.as_bytes()).is_err(), "{text}");
    }
    assert_eq!(parse_system(b"version=2"), Err(ConfigError::UnsupportedVersion(2)));
    assert!(parse_cells(b"cells=[]").is_err());
    assert_eq!(parse_cells(b"version=0"), Err(ConfigError::UnsupportedVersion(0)));
    assert!(cells("other = true").is_err());
    assert!(cells(&(entry("shell") + "extra=true\n")).is_err());
}

#[test]
fn real_toml_grammar_and_strict_document_bounds() {
    let parsed = parse_system(b"# actual TOML escapes and dotted keys\nversion = 1\nlogging.level = \"in\\u0066o\"\nmemory.default_cell_heap_mib = 0x10\n").unwrap();
    assert_eq!(parsed.logging.level, LogLevel::Info);
    assert_eq!(parsed.memory.default_cell_heap_mib, 16);
    for text in ["version=1\nversion=1", "version =", "version='1'", "version=1\n[logging\n"] {
        assert!(parse_system(text.as_bytes()).is_err());
    }
    assert_eq!(parse_system(&[]), Err(ConfigError::Empty));
    assert_eq!(parse_cells(&[]), Err(ConfigError::Empty));
    assert_eq!(parse_system(&[0xff]), Err(ConfigError::InvalidUtf8));
    let mut boundary = b"version=1\n".to_vec();
    boundary.resize(MAX_CONFIG_BYTES, b' ');
    assert!(parse_system(&boundary).is_ok());
    assert!(parse_cells(&boundary).is_ok());
    boundary.push(b' ');
    assert_eq!(parse_system(&boundary), Err(ConfigError::TooLarge));
    assert_eq!(parse_cells(&boundary), Err(ConfigError::TooLarge));
}

#[test]
fn empty_plan_and_toml_argument_decoding_are_supported() {
    assert!(ordered_indices(&parse_cells(b"version=1").unwrap(), &CellConfig::default()).unwrap().is_empty());
    let full = valid(&(entry("net") + "args=['', 'two words', 'λ']\nenabled=false\nrequired=true\nrestart='always'\nservice_id=2\nregistration='self-ready'\nafter=['driver']\nready_timeout_ticks=5000\n"));
    let cell = &full.cells[0];
    assert_eq!(cell.args, vec![String::from(""), String::from("two words"), String::from("λ")]);
    assert!(!cell.enabled);
    assert!(cell.required);
    assert_eq!(cell.registration, Registration::SelfReady);
    assert_eq!(cell.restart, RestartPolicy::Always);
    assert_eq!(cell.after, vec![String::from("driver")]);
}

#[test]
fn enum_and_registration_errors_are_fail_closed() {
    for extra in ["restart='sometimes'", "registration='ready'", "registration='self-ready'",
        "service_id=0", "service_id=65536", "enabled='true'", "required=1"]
    {
        assert!(cells(&(entry("shell") + extra)).is_err(), "{extra}");
    }
    let policies = [("always", RestartPolicy::Always), ("on-failure", RestartPolicy::OnFailure), ("never", RestartPolicy::Never)];
    for (value, expected) in policies {
        assert_eq!(valid(&(entry("shell") + &format!("restart='{value}'"))).cells[0].restart, expected);
    }
    assert!(cells(&(entry("shell") + "service_id=65535")).is_ok());
}

#[test]
fn timeouts_have_exact_bounds() {
    for timeout in [1, 500, 5000] {
        assert!(cells(&(entry("shell") + &format!("ready_timeout_ticks={timeout}"))).is_ok());
    }
    for timeout in [0, 5001] {
        assert!(cells(&(entry("shell") + &format!("ready_timeout_ticks={timeout}"))).is_err());
    }
}

#[test]
fn names_paths_and_reserved_bootstrap_are_bounded() {
    let max_name = "x".repeat(MAX_NAME_BYTES);
    assert!(cells(&entry(&max_name)).is_ok());
    assert!(cells(&entry(&(max_name + "x"))).is_err());
    for name in ["", ".", "..", "bad/name", "with space", "nul\0name"] {
        assert!(cells(&entry(name)).is_err(), "{name:?}");
    }
    for path in ["shell", "/bin/", "/bin/../shell", "/bin/sub/shell", "/bin//shell", "/bin/.", "/bin/..", "/BIN/shell"] {
        assert!(cells(&format!("[[cells]]\nname='shell'\npath='{path}'")).is_err(), "{path}");
    }
    let boundary_path = format!("/bin/{}", "x".repeat(MAX_PATH_BYTES - 5));
    assert!(cells(&format!("[[cells]]\nname='x'\npath='{boundary_path}'")).is_ok());
    assert!(cells(&format!("[[cells]]\nname='x'\npath='{boundary_path}x'")).is_err());
    for body in [entry("vfs"), String::from("[[cells]]\nname='alias'\npath='/bin/VFS'"), entry("shell") + "service_id=1"] {
        assert!(cells(&body).is_err());
    }
}

#[test]
fn fixed_bootstrap_and_demand_only_registry_ownership_cannot_be_overridden() {
    for path in [
        "/bin/init", "/bin/block", "/bin/NVME", "/bin/ahci",
        "/bin/ocel-js", "/bin/OCEL-PDF",
    ] {
        assert!(cells(&format!("[[cells]]\nname='alias'\npath='{path}'\nenabled=false")).is_err(), "{path}");
    }
    for name in ["init", "block", "nvme", "ahci"] {
        assert!(cells(&format!("[[cells]]\nname='{name}'\npath='/bin/echo'")).is_err(), "{name}");
    }
    for id in [service::BLOCK_DRIVER, service::OCEL_ACTIVATOR, service::OCEL_JS, service::OCEL_PDF] {
        assert!(cells(&(entry("echo") + &format!("service_id={id}"))).is_err(), "{id}");
    }
    assert!(cells(&(entry("net") + "service_id=2")).is_ok());
}

#[test]
fn structured_argv_exact_boundary_includes_framing() {
    let largest_arg = "x".repeat(MAX_ARGV_BYTES - ARGV_PREFIX_BYTES - 1);
    assert!(cells(&(entry("shell") + &format!("args=['{largest_arg}']"))).is_ok());
    assert!(cells(&(entry("shell") + &format!("args=['{largest_arg}x']"))).is_err());
    // Empty arguments still consume separator bytes and cannot bypass the limit.
    let empty_args = "'',".repeat(MAX_ARGV_BYTES - ARGV_PREFIX_BYTES);
    assert!(cells(&(entry("shell") + &format!("args=[{empty_args}]"))).is_ok());
    assert!(cells(&(entry("shell") + &format!("args=[{empty_args}'']"))).is_err());
    assert!(cells(&(entry("shell") + "args=[\"nul\\u0000arg\"]")).is_err());
}

#[test]
fn duplicate_names_paths_and_service_ids_within_file_are_rejected() {
    for body in [entry("a") + &entry("a"), entry("a") + "[[cells]]\nname='b'\npath='/bin/A'",
        entry("a") + "service_id=2\n" + &entry("b") + "service_id=2"]
    {
        assert!(matches!(cells(&body), Err(ConfigError::Duplicate { .. })));
    }
    assert!(cells(&(entry("a") + &entry("b"))).is_ok());
}

#[test]
fn duplicate_identities_across_stages_are_rejected_even_if_disabled() {
    let services = valid(&(entry("a") + "service_id=2"));
    for body in [entry("a"), String::from("[[cells]]\nname='b'\npath='/bin/a'"), entry("b") + "service_id=2\nenabled=false"] {
        let autoload = valid(&body);
        assert!(matches!(validate_plan(&services, &autoload), Err(ConfigError::Duplicate { .. })));
        assert!(ordered_indices(&autoload, &services).is_err());
    }
}

#[test]
fn cell_limit_counts_disabled_entries_and_combined_stages() {
    let mut body = String::new();
    for index in 0..MAX_CELLS { body.push_str(&(entry(&format!("c{index}")) + "enabled=false\n")); }
    let services = valid(&body);
    assert!(validate_plan(&services, &CellConfig::default()).is_ok());
    assert_eq!(ordered_indices(&services, &CellConfig::default()).unwrap(), vec![]);
    let autoload = valid(&entry("extra"));
    assert_eq!(validate_plan(&services, &autoload), Err(ConfigError::TooManyCells));
    assert_eq!(ordered_indices(&autoload, &services), Err(ConfigError::TooManyCells));
    body.push_str(&entry("extra"));
    assert_eq!(cells(&body), Err(ConfigError::TooManyCells));
}

#[test]
fn dependency_transition_from_missing_to_ready_prior_stage() {
    let autoload = valid(&(entry("shell") + "after=['net']"));
    assert!(validate_plan(&CellConfig::default(), &autoload).is_err());
    let services = valid(&entry("net"));
    assert!(validate_plan(&services, &autoload).is_ok());
    assert_eq!(ordered_indices(&autoload, &services).unwrap(), vec![0]);
    let disabled = valid(&(entry("net") + "enabled=false"));
    assert!(validate_plan(&disabled, &autoload).is_err());
    assert!(ordered_indices(&autoload, &disabled).is_err());
}

#[test]
fn service_cannot_depend_on_autoload_even_when_it_exists() {
    let services = valid(&(entry("net") + "after=['shell']"));
    let autoload = valid(&entry("shell"));
    assert!(matches!(validate_plan(&services, &autoload), Err(ConfigError::Dependency { .. })));
}

#[test]
fn topological_order_handles_forward_edges_chains_and_disabled_entries() {
    let services = valid(&(entry("c") + "after=['b']\n" + &entry("b") + "after=['a']\n" + &entry("unused") + "enabled=false\n" + &entry("a")));
    assert_eq!(ordered_indices(&services, &CellConfig::default()).unwrap(), vec![3, 1, 0]);
    assert!(validate_plan(&services, &CellConfig::default()).is_ok());
    let independent = valid(&(entry("a") + &entry("b") + &entry("c")));
    assert_eq!(ordered_indices(&independent, &CellConfig::default()).unwrap(), vec![0, 1, 2]);
}

#[test]
fn self_cycles_and_multinode_cycles_are_rejected_in_either_stage() {
    for body in [entry("a") + "after=['a']", entry("a") + "after=['b']\n" + &entry("b") + "after=['a']"] {
        let cyclic = valid(&body);
        assert_eq!(validate_plan(&cyclic, &CellConfig::default()), Err(ConfigError::Cycle));
        assert_eq!(validate_plan(&CellConfig::default(), &cyclic), Err(ConfigError::Cycle));
        assert_eq!(ordered_indices(&cyclic, &CellConfig::default()), Err(ConfigError::Cycle));
    }
}

#[test]
fn programmatically_constructed_configs_cannot_bypass_validation() {
    let mut config = valid(&entry("shell"));
    config.cells[0].ready_timeout_ticks = 0;
    assert!(validate_plan(&config, &CellConfig::default()).is_err());
    assert!(ordered_indices(&config, &CellConfig::default()).is_err());
    let invalid_version = CellConfig { version: 2, cells: vec![] };
    assert_eq!(validate_plan(&invalid_version, &CellConfig::default()), Err(ConfigError::UnsupportedVersion(2)));
}
