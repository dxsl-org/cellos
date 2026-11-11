// SPDX-License-Identifier: MPL-2.0
// Parser for `syscalls.toml`, shared by `libs/api/build.rs` and
// `kernel/build.rs` through `include!` (build scripts cannot depend on each
// other). It accepts exactly the subset the table uses: `[[syscall]]` and
// `[[raw]]` rows, `key = integer`, `key = "string"`, `key = '''` multi-line
// literal strings, and `#` comment lines. Anything else panics the build.

/// One `[[syscall]]` row: a `ViSyscall` variant.
#[derive(Default)]
#[allow(dead_code)] // each build script reads a different subset of fields
struct TableRow {
    name: String,
    number: Option<usize>,
    bit: Option<u8>,
    /// Kernel `Syscall` variant when its name differs from `name`.
    kernel: Option<String>,
    typed_oom: bool,
    doc: Vec<String>,
    bit_note: Vec<String>,
}

/// One `[[raw]]` row: a kernel-only opcode outside `ViSyscall`.
#[derive(Default)]
#[allow(dead_code)]
struct TableRaw {
    id: Option<usize>,
    kernel: String,
    bit: Option<u8>,
    feature: Option<String>,
    gate_as: Option<String>,
}

enum TableEntry {
    Syscall(TableRow),
    Raw(TableRaw),
    /// A kernel `Syscall` variant with no opcode (`[[internal]]`).
    Internal(String),
}

/// Parsed table: `[[syscall]]` rows, `[[raw]]` rows, `[[internal]]` variants.
type Table = (Vec<TableRow>, Vec<TableRaw>, Vec<String>);

fn parse_table(text: &str) -> Table {
    let mut entries: Vec<TableEntry> = Vec::new();
    let mut keys_in_row: Vec<String> = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[syscall]]" => {
                entries.push(TableEntry::Syscall(TableRow::default()));
                keys_in_row.clear();
                continue;
            }
            "[[raw]]" => {
                entries.push(TableEntry::Raw(TableRaw::default()));
                keys_in_row.clear();
                continue;
            }
            "[[internal]]" => {
                entries.push(TableEntry::Internal(String::new()));
                keys_in_row.clear();
                continue;
            }
            _ => {}
        }
        let (key, value) = line
            .split_once(" = ")
            .unwrap_or_else(|| panic!("syscalls.toml:{}: expected `key = value`", n + 1));
        assert!(
            !keys_in_row.iter().any(|k| k == key),
            "syscalls.toml:{}: duplicate key `{key}` in one row",
            n + 1
        );
        keys_in_row.push(key.to_string());
        let text_value = if value == "'''" {
            let mut body = Vec::new();
            loop {
                let (_, l) = lines
                    .next()
                    .unwrap_or_else(|| panic!("syscalls.toml:{}: unterminated '''", n + 1));
                if l == "'''" {
                    break;
                }
                body.push(l.to_string());
            }
            Some(body)
        } else {
            None
        };
        let string = || value.trim_matches('"').to_string();
        match entries
            .last_mut()
            .unwrap_or_else(|| panic!("syscalls.toml:{}: key before a row", n + 1))
        {
            TableEntry::Syscall(row) => match key {
                "name" => row.name = string(),
                "number" => row.number = Some(value.parse().expect("number")),
                "bit" => row.bit = Some(parse_bit(value, n)),
                "kernel" => row.kernel = Some(string()),
                "result" => {
                    assert_eq!(value, "\"typed_oom\"", "syscalls.toml:{}: result", n + 1);
                    row.typed_oom = true;
                }
                "doc" => row.doc = text_value.expect("doc is a ''' string"),
                "bit_note" => row.bit_note = text_value.expect("bit_note is a ''' string"),
                "group" | "group_note" | "comment" => {}
                other => panic!("syscalls.toml:{}: unknown syscall key `{other}`", n + 1),
            },
            TableEntry::Raw(raw) => match key {
                "id" => raw.id = Some(value.parse().expect("id")),
                "kernel" => raw.kernel = string(),
                "bit" => raw.bit = Some(parse_bit(value, n)),
                "feature" => raw.feature = Some(string()),
                "gate_as" => raw.gate_as = Some(string()),
                "comment" => {}
                other => panic!("syscalls.toml:{}: unknown raw key `{other}`", n + 1),
            },
            TableEntry::Internal(kernel) => match key {
                "kernel" => *kernel = string(),
                other => panic!("syscalls.toml:{}: unknown internal key `{other}`", n + 1),
            },
        }
    }
    let mut rows = Vec::new();
    let mut raws = Vec::new();
    let mut internal = Vec::new();
    for entry in entries {
        match entry {
            TableEntry::Syscall(row) => rows.push(row),
            TableEntry::Raw(raw) => raws.push(raw),
            TableEntry::Internal(kernel) => internal.push(kernel),
        }
    }
    validate_table(&rows, &raws, &internal);
    (rows, raws, internal)
}

/// An allowlist bit indexes a `u64` mask.
fn parse_bit(value: &str, n: usize) -> u8 {
    let bit: u8 = value.parse().expect("bit");
    assert!(bit < 64, "syscalls.toml:{}: allowlist bit {bit} is not below 64", n + 1);
    bit
}

/// Cross-row invariants the generators rely on: unique numbers and names, raw
/// ids disjoint from api numbers, and each kernel variant claimed by one row.
fn validate_table(rows: &[TableRow], raws: &[TableRaw], internal: &[String]) {
    let mut numbers = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    let mut kernel = std::collections::BTreeMap::new();
    for row in rows {
        let number = row.number.unwrap_or_else(|| panic!("{}: no number", row.name));
        assert!(numbers.insert(number), "syscalls.toml: duplicate number {number}");
        assert!(names.insert(row.name.clone()), "syscalls.toml: duplicate name {}", row.name);
        if row.name != "Unknown" {
            let variant = row.kernel.clone().unwrap_or_else(|| row.name.clone());
            assert!(
                kernel.insert(variant.clone(), number).is_none(),
                "syscalls.toml: kernel variant {variant} claimed by two api rows"
            );
        }
    }
    for raw in raws {
        let id = raw.id.unwrap_or_else(|| panic!("raw {}: no id", raw.kernel));
        assert!(!numbers.contains(&id), "syscalls.toml: raw id {id} is also an api number");
        assert!(numbers.insert(id), "syscalls.toml: duplicate raw id {id}");
        // A raw alias may share a variant with an api row (100 and `Call`), never
        // with another raw row.
        assert!(
            raws.iter().filter(|r| r.kernel == raw.kernel).count() == 1,
            "syscalls.toml: kernel variant {} claimed by two raw rows",
            raw.kernel
        );
        if let Some(gate_as) = &raw.gate_as {
            assert!(names.contains(gate_as), "syscalls.toml: raw {id} gate_as {gate_as} is no row");
        }
    }
    for variant in internal {
        assert!(
            !kernel.contains_key(variant) && raws.iter().all(|r| &r.kernel != variant),
            "syscalls.toml: internal variant {variant} also has an opcode"
        );
    }
}
