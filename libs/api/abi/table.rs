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
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[syscall]]" => {
                entries.push(TableEntry::Syscall(TableRow::default()));
                continue;
            }
            "[[raw]]" => {
                entries.push(TableEntry::Raw(TableRaw::default()));
                continue;
            }
            "[[internal]]" => {
                entries.push(TableEntry::Internal(String::new()));
                continue;
            }
            _ => {}
        }
        let (key, value) = line
            .split_once(" = ")
            .unwrap_or_else(|| panic!("syscalls.toml:{}: expected `key = value`", n + 1));
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
                "bit" => row.bit = Some(value.parse().expect("bit")),
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
                "bit" => raw.bit = Some(value.parse().expect("bit")),
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
    (rows, raws, internal)
}
