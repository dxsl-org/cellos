// SPDX-License-Identifier: MPL-2.0
// Parser for `syscalls.toml`, shared by `libs/abi/build.rs` and
// `kernel/build.rs` through `include!` (build scripts cannot depend on each
// other). It accepts exactly the subset the table uses: `[[syscall]]`,
// `[[raw]]` and `[[reserved]]` rows, `key = integer`, `key = "string"`,
// `key = '''` multi-line literal strings, and `#` comment lines. Anything else
// panics the build.

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

/// One `[[reserved]]` row: a retired number and allowlist bit, never reused.
#[derive(Default)]
#[allow(dead_code)]
struct TableReserved {
    number: Option<usize>,
    bit: Option<u8>,
    /// The `ViSyscall` name the number had.
    former: String,
}

enum TableEntry {
    Syscall(TableRow),
    Raw(TableRaw),
    Reserved(TableReserved),
}

/// Parsed table: `[[syscall]]` rows, `[[raw]]` rows, `[[reserved]]` rows.
type Table = (Vec<TableRow>, Vec<TableRaw>, Vec<TableReserved>);

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
            "[[reserved]]" => {
                entries.push(TableEntry::Reserved(TableReserved::default()));
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
            TableEntry::Reserved(reserved) => match key {
                "number" => reserved.number = Some(value.parse().expect("number")),
                "bit" => reserved.bit = Some(parse_bit(value, n)),
                "former" => reserved.former = string(),
                "comment" => {}
                other => panic!("syscalls.toml:{}: unknown reserved key `{other}`", n + 1),
            },
        }
    }
    let mut rows = Vec::new();
    let mut raws = Vec::new();
    let mut reserved = Vec::new();
    for entry in entries {
        match entry {
            TableEntry::Syscall(row) => rows.push(row),
            TableEntry::Raw(raw) => raws.push(raw),
            TableEntry::Reserved(row) => reserved.push(row),
        }
    }
    validate_table(&rows, &raws, &reserved);
    (rows, raws, reserved)
}

/// An allowlist bit indexes a `u64` mask.
fn parse_bit(value: &str, n: usize) -> u8 {
    let bit: u8 = value.parse().expect("bit");
    assert!(bit < 64, "syscalls.toml:{}: allowlist bit {bit} is not below 64", n + 1);
    bit
}

/// Cross-row invariants the generators rely on: unique numbers and names, raw
/// ids disjoint from api numbers, each kernel variant claimed by one row, and
/// a reserved number or bit used by no live row.
fn validate_table(rows: &[TableRow], raws: &[TableRaw], reserved: &[TableReserved]) {
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
    let live_bits: std::collections::BTreeSet<u8> = rows
        .iter()
        .filter_map(|r| r.bit)
        .chain(raws.iter().filter_map(|r| r.bit))
        .collect();
    for row in reserved {
        let number = row.number.unwrap_or_else(|| panic!("reserved {}: no number", row.former));
        assert!(numbers.insert(number), "syscalls.toml: reserved number {number} is in use");
        assert!(!names.contains(&row.former), "syscalls.toml: reserved {} is a live row", row.former);
        if let Some(bit) = row.bit {
            assert!(!live_bits.contains(&bit), "syscalls.toml: reserved bit {bit} is in use");
        }
    }
}
