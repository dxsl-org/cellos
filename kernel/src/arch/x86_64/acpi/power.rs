// SPDX-License-Identifier: MPL-2.0
//! ACPI reset and S5 soft-off for `SystemPower` (436, CP1 §5).
//!
//! Contracts:
//! - `\_S5_` is taken only from a bounded structural scan of the DSDT, then of
//!   each SSDT in root-table order: `Name (\_S5_, Package { a, b, .. })` at
//!   namespace scope whose first two elements are integer constants of at most
//!   3 bits. A `\_S5_` that is a method, is created inside a method or
//!   conditional body, or holds anything else is refused, never guessed. There
//!   is no AML interpreter, and `\_PTS` is not evaluated.
//! - Every reset step and the sleep write is logged before it is attempted.
//! - [`record`] runs once on the BSP inside the x86 ACPI init block, before
//!   any reader; the facts never change afterwards.

use crate::acpi::FadtInfo;
use crate::acpi::Gas;
use crate::sync::Spinlock;

/// Outcome of the static `\_S5_` scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S5 {
    /// SLP_TYPa / SLP_TYPb for S5.
    Static { slp_typ_a: u8, slp_typ_b: u8 },
    /// No `\_S5_` in the parts of the tables the scan decoded.
    Absent,
    /// A `\_S5_` exists but is a method, is created inside a method or
    /// conditional body, or its package does not start with two 3-bit integer
    /// constants.
    NotStatic,
    /// The scan stopped at a namespace-level construct it does not decode
    /// before it found `\_S5_`; `offset` is from the start of that table.
    Unparsed { offset: usize, opcode: u8 },
}

impl S5 {
    /// Load order decides between tables: the first static package wins;
    /// without one, the most specific refusal is reported.
    fn rank(self) -> u8 {
        match self {
            S5::Absent => 0,
            S5::Unparsed { .. } => 1,
            S5::NotStatic => 2,
            S5::Static { .. } => 3,
        }
    }

    fn merge(self, next: S5) -> S5 {
        if next.rank() > self.rank() {
            next
        } else {
            self
        }
    }
}

const HEADER_LEN: usize = 36;
/// Nesting bound for scope-like terms; deeper firmware is reported unparsed.
const MAX_NEST: usize = 32;
const S5_NAME: [u8; 4] = *b"_S5_";

// AML opcodes (ACPI 6.5 §20.3).
const ZERO_OP: u8 = 0x00;
const ONE_OP: u8 = 0x01;
const ALIAS_OP: u8 = 0x06;
const NAME_OP: u8 = 0x08;
const BYTE_PREFIX: u8 = 0x0A;
const WORD_PREFIX: u8 = 0x0B;
const DWORD_PREFIX: u8 = 0x0C;
const STRING_PREFIX: u8 = 0x0D;
const QWORD_PREFIX: u8 = 0x0E;
const SCOPE_OP: u8 = 0x10;
const BUFFER_OP: u8 = 0x11;
const PACKAGE_OP: u8 = 0x12;
const VAR_PACKAGE_OP: u8 = 0x13;
const METHOD_OP: u8 = 0x14;
const EXTERNAL_OP: u8 = 0x15;
const DUAL_NAME_PREFIX: u8 = 0x2E;
const MULTI_NAME_PREFIX: u8 = 0x2F;
const EXT_OP_PREFIX: u8 = 0x5B;
const ROOT_CHAR: u8 = b'\\';
const PARENT_PREFIX: u8 = b'^';
const ADD_OP: u8 = 0x72;
const SUBTRACT_OP: u8 = 0x74;
const MULTIPLY_OP: u8 = 0x77;
const SHIFT_LEFT_OP: u8 = 0x79;
const SHIFT_RIGHT_OP: u8 = 0x7A;
const AND_OP: u8 = 0x7B;
const NAND_OP: u8 = 0x7C;
const OR_OP: u8 = 0x7D;
const NOR_OP: u8 = 0x7E;
const XOR_OP: u8 = 0x7F;
const NOT_OP: u8 = 0x80;
const MOD_OP: u8 = 0x85;
const CREATE_DWORD_FIELD_OP: u8 = 0x8A;
const CREATE_WORD_FIELD_OP: u8 = 0x8B;
const CREATE_BYTE_FIELD_OP: u8 = 0x8C;
const CREATE_BIT_FIELD_OP: u8 = 0x8D;
const CREATE_QWORD_FIELD_OP: u8 = 0x8F;
const IF_OP: u8 = 0xA0;
const ELSE_OP: u8 = 0xA1;
const WHILE_OP: u8 = 0xA2;
const NOOP_OP: u8 = 0xA3;
const ONES_OP: u8 = 0xFF;
// After EXT_OP_PREFIX.
const MUTEX_OP: u8 = 0x01;
const EVENT_OP: u8 = 0x02;
const CREATE_FIELD_OP: u8 = 0x13;
const REVISION_OP: u8 = 0x30;
const OP_REGION_OP: u8 = 0x80;
const FIELD_OP: u8 = 0x81;
const DEVICE_OP: u8 = 0x82;
const PROCESSOR_OP: u8 = 0x83;
const POWER_RES_OP: u8 = 0x84;
const THERMAL_ZONE_OP: u8 = 0x85;
const INDEX_FIELD_OP: u8 = 0x86;
const BANK_FIELD_OP: u8 = 0x87;
const DATA_REGION_OP: u8 = 0x88;

/// A decoded NameString: enough to resolve its depth and final segment.
#[derive(Clone, Copy)]
struct NameString {
    absolute: bool,
    parents: usize,
    segments: usize,
    last: [u8; 4],
}

impl NameString {
    /// Depth of the object this name denotes when defined in a scope at
    /// `depth` (root = 0); `None` when `^` climbs above the root.
    fn depth_from(self, depth: usize) -> Option<usize> {
        let base = if self.absolute {
            0
        } else {
            depth.checked_sub(self.parents)?
        };
        base.checked_add(self.segments)
    }

    fn is_root_s5(self, depth: usize) -> bool {
        self.segments != 0 && self.last == S5_NAME && self.depth_from(depth) == Some(1)
    }
}

/// `Err` carries the decisive outcome (found, refused or unparsed).
type Flow = Result<(), S5>;

struct Scan<'a> {
    aml: &'a [u8],
    /// `_S5_` bytes appeared inside a skipped method or conditional body.
    s5_in_body: bool,
}

impl Scan<'_> {
    fn byte(&self, pos: usize, end: usize) -> Option<u8> {
        (pos < end).then(|| self.aml.get(pos).copied()).flatten()
    }

    fn unparsed(&self, pos: usize) -> S5 {
        S5::Unparsed {
            offset: pos,
            opcode: self.aml.get(pos).copied().unwrap_or(0),
        }
    }

    /// `(package end, first byte after the PkgLength)` for a PkgLength at `pos`.
    fn pkg_length(&self, pos: usize, end: usize) -> Option<(usize, usize)> {
        let lead = self.byte(pos, end)?;
        let follow = (lead >> 6) as usize;
        let mut len = if follow == 0 {
            (lead & 0x3F) as usize
        } else {
            (lead & 0x0F) as usize
        };
        for index in 0..follow {
            len |= (self.byte(pos + 1 + index, end)? as usize) << (4 + 8 * index);
        }
        let body = pos + 1 + follow;
        let pkg_end = pos.checked_add(len)?;
        (pkg_end >= body && pkg_end <= end).then_some((pkg_end, body))
    }

    fn name_string(&self, mut pos: usize, end: usize) -> Option<(NameString, usize)> {
        let mut name = NameString {
            absolute: false,
            parents: 0,
            segments: 1,
            last: [0; 4],
        };
        if self.byte(pos, end)? == ROOT_CHAR {
            name.absolute = true;
            pos += 1;
        } else {
            while self.byte(pos, end)? == PARENT_PREFIX {
                name.parents += 1;
                pos += 1;
            }
        }
        match self.byte(pos, end)? {
            ZERO_OP => {
                name.segments = 0;
                return Some((name, pos + 1));
            }
            DUAL_NAME_PREFIX => {
                name.segments = 2;
                pos += 1;
            }
            MULTI_NAME_PREFIX => {
                name.segments = self.byte(pos + 1, end)? as usize;
                pos += 2;
            }
            _ => {}
        }
        for _ in 0..name.segments {
            let segment: [u8; 4] = self.aml.get(pos..pos + 4)?.try_into().ok()?;
            let lead_ok = segment[0].is_ascii_uppercase() || segment[0] == b'_';
            let rest_ok = segment[1..]
                .iter()
                .all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
            if !lead_ok || !rest_ok || pos + 4 > end {
                return None;
            }
            name.last = segment;
            pos += 4;
        }
        Some((name, pos))
    }

    fn starts_name(byte: u8) -> bool {
        byte.is_ascii_uppercase()
            || matches!(
                byte,
                b'_' | ROOT_CHAR | PARENT_PREFIX | DUAL_NAME_PREFIX | MULTI_NAME_PREFIX
            )
    }

    /// Integer constant at `pos`: `(value, next)`.
    fn integer(&self, pos: usize, end: usize) -> Option<(u64, usize)> {
        let width = match self.byte(pos, end)? {
            ZERO_OP => return Some((0, pos + 1)),
            ONE_OP => return Some((1, pos + 1)),
            ONES_OP => return Some((u64::MAX, pos + 1)),
            BYTE_PREFIX => 1,
            WORD_PREFIX => 2,
            DWORD_PREFIX => 4,
            QWORD_PREFIX => 8,
            _ => return None,
        };
        let bytes = self.aml.get(pos + 1..pos + 1 + width)?;
        if pos + 1 + width > end {
            return None;
        }
        let value = bytes
            .iter()
            .rev()
            .fold(0u64, |value, byte| value << 8 | *byte as u64);
        Some((value, pos + 1 + width))
    }

    /// Skip one TermArg of the kinds this scan accepts at namespace scope: a
    /// data object, a name (taken as a plain reference) or an integer
    /// expression whose result is discarded (`NullName` target), so loading the
    /// table cannot store into the namespace. `nest` bounds expression depth.
    fn skip_term(&self, pos: usize, end: usize, nest: usize) -> Option<usize> {
        let op = self.byte(pos, end)?;
        if let Some((_, next)) = self.integer(pos, end) {
            return Some(next);
        }
        let operands = match op {
            STRING_PREFIX => {
                let nul = self.aml.get(pos + 1..end)?.iter().position(|&b| b == 0)?;
                return Some(pos + 1 + nul + 1);
            }
            BUFFER_OP | PACKAGE_OP | VAR_PACKAGE_OP => {
                return Some(self.pkg_length(pos + 1, end)?.0)
            }
            EXT_OP_PREFIX if self.byte(pos + 1, end)? == REVISION_OP => return Some(pos + 2),
            byte if Self::starts_name(byte) => return Some(self.name_string(pos, end)?.1),
            ADD_OP | SUBTRACT_OP | MULTIPLY_OP | SHIFT_LEFT_OP | SHIFT_RIGHT_OP | AND_OP
            | NAND_OP | OR_OP | NOR_OP | XOR_OP | MOD_OP => 2,
            NOT_OP => 1,
            _ => return None,
        };
        if nest >= MAX_NEST {
            return None;
        }
        let mut next = pos + 1;
        for _ in 0..operands {
            next = self.skip_term(next, end, nest + 1)?;
        }
        (self.byte(next, end)? == ZERO_OP).then_some(next + 1)
    }

    fn skip_terms(&self, mut pos: usize, end: usize, count: usize) -> Option<usize> {
        for _ in 0..count {
            pos = self.skip_term(pos, end, 0)?;
        }
        Some(pos)
    }

    /// Skip a method or conditional body, remembering whether it names `_S5_`.
    fn skip_body(&mut self, from: usize, to: usize) -> usize {
        if self.aml[from..to].windows(4).any(|w| w == S5_NAME) {
            self.s5_in_body = true;
        }
        to
    }

    /// `Name (\_S5_, <object at pos>)`.
    fn s5_package(&self, pos: usize, end: usize) -> S5 {
        if self.byte(pos, end) != Some(PACKAGE_OP) {
            return S5::NotStatic;
        }
        let Some((pkg_end, mut at)) = self.pkg_length(pos + 1, end) else {
            return S5::NotStatic;
        };
        if self.byte(at, pkg_end).is_none_or(|count| count < 2) {
            return S5::NotStatic;
        }
        at += 1;
        let mut slp_typ = [0u8; 2];
        for value in slp_typ.iter_mut() {
            let Some((integer, next)) = self.integer(at, pkg_end) else {
                return S5::NotStatic;
            };
            let Ok(typ @ 0..=7) = u8::try_from(integer) else {
                return S5::NotStatic;
            };
            *value = typ;
            at = next;
        }
        S5::Static {
            slp_typ_a: slp_typ[0],
            slp_typ_b: slp_typ[1],
        }
    }

    /// Scope-like term (the opcode at `op`, its PkgLength at `pkg`), then
    /// NameString, `fixed` bytes and a TermList. Returns the end of the term.
    fn scope(
        &mut self,
        op: usize,
        pkg: usize,
        end: usize,
        depth: usize,
        fixed: usize,
        nest: usize,
    ) -> Result<usize, S5> {
        let bad = self.unparsed(op);
        let (pkg_end, body) = self.pkg_length(pkg, end).ok_or(bad)?;
        let (name, list) = self.name_string(body, pkg_end).ok_or(bad)?;
        let inner = name.depth_from(depth).ok_or(bad)?;
        if list + fixed > pkg_end {
            return Err(bad);
        }
        self.walk(list + fixed, pkg_end, inner, nest + 1)?;
        Ok(pkg_end)
    }

    fn walk(&mut self, mut pos: usize, end: usize, depth: usize, nest: usize) -> Flow {
        if nest > MAX_NEST {
            return Err(self.unparsed(pos));
        }
        while pos < end {
            let op = self.aml[pos];
            let bad = self.unparsed(pos);
            pos = match op {
                SCOPE_OP => self.scope(pos, pos + 1, end, depth, 0, nest)?,
                EXT_OP_PREFIX => {
                    let ext = self.byte(pos + 1, end).ok_or(bad)?;
                    let at = pos + 2;
                    match ext {
                        DEVICE_OP | THERMAL_ZONE_OP | PROCESSOR_OP | POWER_RES_OP => {
                            let fixed = match ext {
                                PROCESSOR_OP => 6,
                                POWER_RES_OP => 3,
                                _ => 0,
                            };
                            self.scope(pos, at, end, depth, fixed, nest)?
                        }
                        FIELD_OP | INDEX_FIELD_OP | BANK_FIELD_OP => {
                            self.pkg_length(at, end).ok_or(bad)?.0
                        }
                        OP_REGION_OP => {
                            let (_, next) = self.name_string(at, end).ok_or(bad)?;
                            self.skip_terms(next + 1, end, 2).ok_or(bad)?
                        }
                        DATA_REGION_OP => {
                            let (_, next) = self.name_string(at, end).ok_or(bad)?;
                            self.skip_terms(next, end, 3).ok_or(bad)?
                        }
                        MUTEX_OP => self.name_string(at, end).ok_or(bad)?.1 + 1,
                        EVENT_OP => self.name_string(at, end).ok_or(bad)?.1,
                        CREATE_FIELD_OP => {
                            let next = self.skip_terms(at, end, 3).ok_or(bad)?;
                            self.name_string(next, end).ok_or(bad)?.1
                        }
                        _ => return Err(bad),
                    }
                }
                METHOD_OP => {
                    let (pkg_end, body) = self.pkg_length(pos + 1, end).ok_or(bad)?;
                    let (name, after_name) = self.name_string(body, pkg_end).ok_or(bad)?;
                    if name.is_root_s5(depth) {
                        return Err(S5::NotStatic);
                    }
                    self.skip_body(after_name, pkg_end)
                }
                IF_OP | ELSE_OP | WHILE_OP => {
                    let (pkg_end, body) = self.pkg_length(pos + 1, end).ok_or(bad)?;
                    self.skip_body(body, pkg_end)
                }
                NAME_OP => {
                    let (name, next) = self.name_string(pos + 1, end).ok_or(bad)?;
                    if name.is_root_s5(depth) {
                        return Err(self.s5_package(next, end));
                    }
                    self.skip_term(next, end, 0).ok_or(bad)?
                }
                ALIAS_OP => {
                    let (_, next) = self.name_string(pos + 1, end).ok_or(bad)?;
                    let (alias, next) = self.name_string(next, end).ok_or(bad)?;
                    if alias.is_root_s5(depth) {
                        return Err(S5::NotStatic);
                    }
                    next
                }
                EXTERNAL_OP => self.name_string(pos + 1, end).ok_or(bad)?.1 + 2,
                CREATE_DWORD_FIELD_OP
                | CREATE_WORD_FIELD_OP
                | CREATE_BYTE_FIELD_OP
                | CREATE_BIT_FIELD_OP
                | CREATE_QWORD_FIELD_OP => {
                    let next = self.skip_terms(pos + 1, end, 2).ok_or(bad)?;
                    self.name_string(next, end).ok_or(bad)?.1
                }
                NOOP_OP => pos + 1,
                _ => return Err(bad),
            };
            if pos > end {
                return Err(bad);
            }
        }
        Ok(())
    }
}

/// Scan one definition block (DSDT or SSDT, header included) for a static
/// `\_S5_` package.
pub fn scan_definition_block(table: &[u8]) -> S5 {
    if table.len() < HEADER_LEN {
        return S5::Unparsed {
            offset: 0,
            opcode: 0,
        };
    }
    let mut scan = Scan {
        aml: table,
        s5_in_body: false,
    };
    match scan.walk(HEADER_LEN, table.len(), 0, 0) {
        Err(outcome) => outcome,
        Ok(()) if scan.s5_in_body => S5::NotStatic,
        Ok(()) => S5::Absent,
    }
}

/// An I/O-port GPE register block: `len` bytes, status half then enable half.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpeBlock {
    pub port: u16,
    pub len: u8,
}

/// GPE0/GPE1 blocks of a raw FADT (ACPI 6.5 Table 5.9: `GPE0_BLK` 80,
/// `GPE1_BLK` 84, lengths 92/93, `X_GPE0_BLK` 220, `X_GPE1_BLK` 232). A block
/// outside I/O space is left out: the SCI path only drives port I/O. So is one
/// whose ports run past `0xFFFF`: the SCI path writes `port + index`, which
/// must never wrap to the low legacy ports (8237 DMA, PIC).
fn gpe_blocks(facp: &[u8]) -> [GpeBlock; 2] {
    let u8_at = |off: usize| facp.get(off).copied().unwrap_or(0);
    let u64_at = |off: usize, width: usize| {
        facp.get(off..off + width).map_or(0, |bytes| {
            bytes
                .iter()
                .rev()
                .fold(0u64, |value, byte| value << 8 | *byte as u64)
        })
    };
    let block = |legacy: usize, len_at: usize, x_gas: usize| {
        let x_address = u64_at(x_gas + 4, 8);
        let (space, address) = if x_address != 0 {
            (u8_at(x_gas), x_address)
        } else {
            (Gas::SYSTEM_IO, u64_at(legacy, 4))
        };
        match (space, u16::try_from(address)) {
            (Gas::SYSTEM_IO, Ok(port))
                if port != 0 && port.checked_add(u16::from(u8_at(len_at))).is_some() =>
            {
                GpeBlock {
                    port,
                    len: u8_at(len_at),
                }
            }
            _ => GpeBlock::default(),
        }
    };
    [block(80, 92, 220), block(84, 93, 232)]
}

/// Everything reset, poweroff and the SCI need from the firmware tables.
#[derive(Clone, Copy, Debug)]
pub struct PowerFacts {
    pub fadt: FadtInfo,
    pub s5: S5,
    /// Kernel address of a memory-space reset register, when admitted.
    pub reset_virt: Option<usize>,
    /// Kernel address of a memory-space `SLEEP_CONTROL_REG`, when admitted.
    pub sleep_control_virt: Option<usize>,
    pub gpe: [GpeBlock; 2],
}

/// Read the facts from the DSDT named by `fadt` and the tables the root table
/// lists. Stores nothing.
pub fn discover(
    fadt: &FadtInfo,
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
) -> PowerFacts {
    let mut s5 = crate::acpi::with_table(fadt.dsdt, map_physical, |dsdt| {
        if dsdt.starts_with(b"DSDT") {
            scan_definition_block(dsdt)
        } else {
            S5::Absent
        }
    })
    .unwrap_or_else(|| {
        log::warn!("[acpi] power: DSDT at {:#x} unreadable", fadt.dsdt);
        S5::Absent
    });
    let mut gpe = [GpeBlock::default(); 2];
    crate::acpi::for_each_listed_table(rsdp_phys, map_physical, |table| {
        match &table[..4] {
            b"SSDT" => s5 = s5.merge(scan_definition_block(table)),
            b"FACP" => gpe = gpe_blocks(table),
            _ => {}
        }
        true
    });
    let mut map_register = |reg: &Gas, name: &str| {
        if reg.space_id != Gas::SYSTEM_MEMORY || !reg.is_present() {
            return None;
        }
        let virt = usize::try_from(reg.address)
            .ok()
            .and_then(|phys| map_physical(phys, 1));
        if virt.is_none() {
            log::warn!(
                "[acpi] power: {} at {:#x} not admitted; that step is skipped",
                name,
                reg.address
            );
        }
        virt
    };
    let reset_virt = map_register(&fadt.reset_reg, "reset register");
    let sleep_control_virt = map_register(&fadt.sleep_control, "SLEEP_CONTROL_REG");
    PowerFacts {
        fadt: *fadt,
        s5,
        reset_virt,
        sleep_control_virt,
        gpe,
    }
}

/// Written once by [`record`] before any reader; leaf lock.
static FACTS: Spinlock<Option<PowerFacts>> =
    Spinlock::ranked(None, crate::sync::rank::ACPI_POWER_FACTS);

/// Publish the facts 436 acts on. Called once, from `sci::init`.
pub fn record(facts: PowerFacts) {
    match facts.s5 {
        S5::Static {
            slp_typ_a,
            slp_typ_b,
        } => log::info!(
            "[acpi] power: \\_S5_ static SLP_TYPa={} SLP_TYPb={}",
            slp_typ_a,
            slp_typ_b
        ),
        S5::Absent => log::warn!("[acpi] power: no \\_S5_; poweroff refused"),
        S5::NotStatic => log::warn!("[acpi] power: \\_S5_ is not static; poweroff refused"),
        S5::Unparsed { offset, opcode } => log::warn!(
            "[acpi] power: \\_S5_ scan stopped at opcode {:#04x} offset {:#x}; poweroff refused",
            opcode,
            offset
        ),
    }
    let reset = facts.fadt.reset_reg;
    log::info!(
        "[acpi] power: reset_reg={} space={} addr={:#x} value={:#x}",
        facts.fadt.reset_supported(),
        reset.space_id,
        reset.address,
        facts.fadt.reset_value
    );
    *FACTS.lock() = Some(facts);
}

const PCI_CONFIG_SPACE: u8 = 2;
const PM1_SLP_TYP_SHIFT: u16 = 10;
const PM1_SLP_TYP_MASK: u16 = 0b111 << PM1_SLP_TYP_SHIFT;
const PM1_SLP_EN: u16 = 1 << 13;
const PM1_WAK_STS: u16 = 1 << 15;
const SLEEP_CONTROL_SLP_TYP_SHIFT: u8 = 2;
const SLEEP_CONTROL_SLP_EN: u8 = 1 << 5;
/// Port reads between a reset or sleep write and the next step: at least
/// ~0.2 s on hardware (≥ 1 µs per read), long enough for QEMU to act on it.
const SETTLE_READS: usize = 200_000;
const POWEROFF_SETTLE_ROUNDS: usize = 25;
const I8042_STATUS: u16 = 0x64;
const I8042_INPUT_FULL: u8 = 1 << 1;
const I8042_RESET_PULSE: u8 = 0xFE;
const RST_CNT: u16 = 0xCF9;

pub(super) fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: ring-0 port read; no memory is touched.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack));
    }
    value
}

pub(super) fn outb(port: u16, value: u8) {
    // SAFETY: ring-0 port write; no memory is touched.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
    }
}

pub(super) fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: ring-0 port read; no memory is touched.
    unsafe {
        core::arch::asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack));
    }
    value
}

pub(super) fn outw(port: u16, value: u16) {
    // SAFETY: ring-0 port write; no memory is touched.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack));
    }
}

fn outl(port: u16, value: u32) {
    // SAFETY: ring-0 port write; no memory is touched.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack));
    }
}

/// An I/O-space register of `reg`, if it has one.
pub(super) fn io_port(reg: &Gas) -> Option<u16> {
    (reg.space_id == Gas::SYSTEM_IO && reg.is_present())
        .then(|| u16::try_from(reg.address).ok())
        .flatten()
}

fn settle(rounds: usize) {
    for _ in 0..rounds * SETTLE_READS {
        let _ = inb(0x80);
    }
}

/// Write the 8-bit `value` to a FADT register in I/O, memory (admitted at
/// init) or bus-0 PCI configuration space. `false` when it cannot be reached.
fn write_register(reg: &Gas, mapped: Option<usize>, value: u8) -> bool {
    match reg.space_id {
        Gas::SYSTEM_IO => io_port(reg).map(|port| outb(port, value)).is_some(),
        Gas::SYSTEM_MEMORY => mapped
            // SAFETY: `discover` had the firmware mapper admit this byte.
            .map(|virt| unsafe { core::ptr::write_volatile(virt as *mut u8, value) })
            .is_some(),
        PCI_CONFIG_SPACE => {
            // ACPI 6.5 §5.2.3.2: segment 0, bus 0; device in bits 32..48,
            // function in 16..32, register offset in 0..16.
            let device = (reg.address >> 32) & 0xFFFF;
            let function = (reg.address >> 16) & 0xFFFF;
            let offset = reg.address & 0xFFFF;
            if device > 31 || function > 7 || offset > 0xFF {
                return false;
            }
            let address = 0x8000_0000 | (device << 11 | function << 8 | (offset & 0xFC)) as u32;
            outl(0xCF8, address);
            outb(0xCFC + (offset & 3) as u16, value);
            true
        }
        _ => false,
    }
}

/// Reset the machine. Each step is logged, then tried, then given time to act.
pub fn reboot() -> ! {
    let facts = *FACTS.lock();
    match facts.filter(|facts| facts.fadt.reset_supported()) {
        Some(facts) => {
            let reg = facts.fadt.reset_reg;
            log::warn!(
                "[power] reset: FADT reset register space={} addr={:#x} value={:#x}",
                reg.space_id,
                reg.address,
                facts.fadt.reset_value
            );
            if write_register(&reg, facts.reset_virt, facts.fadt.reset_value) {
                settle(1);
            } else {
                log::warn!("[power] reset: FADT reset register unreachable");
            }
        }
        None => log::warn!("[power] reset: no FADT reset register"),
    }
    log::warn!("[power] reset: port 0xcf9");
    outb(RST_CNT, 0x02);
    outb(RST_CNT, 0x06);
    settle(1);
    log::warn!("[power] reset: 8042 pulse");
    for _ in 0..SETTLE_READS {
        if inb(I8042_STATUS) & I8042_INPUT_FULL == 0 {
            break;
        }
    }
    outb(I8042_STATUS, I8042_RESET_PULSE);
    settle(1);
    log::warn!("[power] reset: triple fault");
    let empty_idt = [0u8; 10];
    // SAFETY: deliberately loads a zero-limit IDT and raises #BP: the CPU
    // cannot deliver it, double-faults the same way and shuts down, which
    // resets the platform. Nothing runs afterwards.
    unsafe {
        core::arch::asm!("lidt [{}]", "int3", in(reg) &empty_idt, options(noreturn));
    }
}

/// Enter S5. Returns only when it is refused (no static `\_S5_` or no usable
/// control register) or the machine still runs after the write; the caller
/// reports `NotSupported`.
pub fn poweroff() {
    let Some(facts) = *FACTS.lock() else {
        log::warn!("[power] poweroff refused: no ACPI facts");
        return;
    };
    let S5::Static {
        slp_typ_a,
        slp_typ_b,
    } = facts.s5
    else {
        log::warn!("[power] poweroff refused: \\_S5_ {:?}", facts.s5);
        return;
    };
    let fadt = facts.fadt;
    if fadt.hw_reduced() {
        let reg = fadt.sleep_control;
        log::warn!(
            "[power] poweroff: SLEEP_CONTROL_REG space={} addr={:#x} SLP_TYP={}",
            reg.space_id,
            reg.address,
            slp_typ_a
        );
        let value = slp_typ_a << SLEEP_CONTROL_SLP_TYP_SHIFT | SLEEP_CONTROL_SLP_EN;
        if !write_register(&reg, facts.sleep_control_virt, value) {
            log::warn!("[power] poweroff refused: SLEEP_CONTROL_REG unreachable");
            return;
        }
    } else {
        let Some(pm1a) = io_port(&fadt.pm1a_cnt) else {
            log::warn!("[power] poweroff refused: no I/O PM1a control block");
            return;
        };
        let pm1b = io_port(&fadt.pm1b_cnt);
        for status in [io_port(&fadt.pm1a_evt), io_port(&fadt.pm1b_evt)]
            .into_iter()
            .flatten()
        {
            outw(status, PM1_WAK_STS);
        }
        log::warn!(
            "[power] poweroff: PM1a_CNT {:#x} SLP_TYPa={} PM1b_CNT {:#x} SLP_TYPb={}",
            pm1a,
            slp_typ_a,
            pm1b.unwrap_or(0),
            slp_typ_b
        );
        let blocks = [(Some(pm1a), slp_typ_a), (pm1b, slp_typ_b)];
        // ACPI 6.5 §7.4.2: SLP_TYP first, then SLP_TYP with SLP_EN.
        for (port, typ) in blocks {
            if let Some(port) = port {
                let value = inw(port) & !(PM1_SLP_TYP_MASK | PM1_SLP_EN)
                    | (typ as u16) << PM1_SLP_TYP_SHIFT;
                outw(port, value);
            }
        }
        for (port, _) in blocks {
            if let Some(port) = port {
                outw(port, inw(port) | PM1_SLP_EN);
            }
        }
    }
    settle(POWEROFF_SETTLE_ROUNDS);
    log::error!("[power] poweroff: machine still running after the sleep write");
}
