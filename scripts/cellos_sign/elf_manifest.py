"""Read the manifest class of a cell ELF the way the kernel loader reads it.

The tier-class gate reads a crate's manifest class from source, but the kernel
picks the tier from the `__ViCell_manifest` section of the ELF it loads
(`kernel/src/loader/governed_spawn.rs`): FFI or UNTRUSTED gives Tier 2,
anything else — a tier-less record, STANDARD, TRUSTED_CORE or no section at
all — gives Tier 1. Source and binary can disagree (a `#[cfg]`-gated or never
expanded `declare_manifest!`, an autobin without one), so before signing,
`cellos-sign` reads each target here and holds the binary to the same rule.

This mirrors, byte for byte, the kernel's structural classification:
`kernel/src/loader/elf_section.rs` (`ElfSections::parse`, `names`, `name`,
`bytes`), `kernel/src/loader/manifest_section.rs` (`classify`) and
`libs/abi/src/manifest_parse.rs` (`CellManifest::from_bytes`). A file the
kernel would classify as malformed reads `MALFORMED` (the loader refuses it).
"""

from __future__ import annotations

import struct

MANIFEST_SECTION = b"__ViCell_manifest"
MANIFEST_MAGIC = 0x5649_4345
MANIFEST_VERSION_V1 = 1
MANIFEST_VERSION = 2
# `MANIFEST_FLAGS_MASK` in libs/abi/src/manifest_flags.rs: the 12 defined flag bits.
MANIFEST_FLAGS_MASK = 0x0FFF
SHT_STRTAB = 3
SHT_NOBITS = 8
SHN_XINDEX = 0xFFFF

# Protection-class byte → the name `tier_class` and `[[tier_exception]]` use.
CLASS_NAMES = {0: "TRUSTED_CORE", 1: "STANDARD", 2: "FFI", 3: "UNTRUSTED", 0xFF: "LEGACY"}
TIER2_CLASSES = frozenset({"FFI", "UNTRUSTED"})
ABSENT, MALFORMED = "ABSENT", "MALFORMED"


class _Malformed(Exception):
    pass


def _get(data: bytes, offset: int, size: int) -> bytes:
    if offset < 0 or size < 0 or offset + size > len(data):
        raise _Malformed
    return data[offset : offset + size]


def _read(data: bytes, offset: int, width: int, little: bool) -> int:
    return int.from_bytes(_get(data, offset, width), "little" if little else "big")


def _sections(data: bytes) -> tuple[list[tuple[int, int, int, int]], int]:
    """`ElfSections::parse` + every `section(i)`: ([(name, kind, offset, size)], shstrndx)."""
    if _get(data, 0, 4) != b"\x7fELF" or _get(data, 6, 1)[0] != 1:
        raise _Malformed
    klass = _get(data, 4, 1)[0]
    endian = _get(data, 5, 1)[0]
    if endian not in (1, 2):
        raise _Malformed
    little = endian == 1
    if klass == 1:
        layout = (52, 28, 42, 44, 32, 40, 46, 48, 50, 32, 40)
    elif klass == 2:
        layout = (64, 32, 54, 56, 40, 52, 58, 60, 62, 56, 64)
    else:
        raise _Malformed
    (header_size, phoff_at, phentsize_at, phnum_at, shoff_at, ehsize_at,
     shentsize_at, shnum_at, shstrndx_at, expected_phent, expected_shent) = layout
    if (len(data) < header_size or _read(data, 20, 4, little) != 1
            or _read(data, ehsize_at, 2, little) != header_size):
        raise _Malformed
    width = 4 if klass == 1 else 8
    phoff = _read(data, phoff_at, width, little)
    phentsize = _read(data, phentsize_at, 2, little)
    phnum = _read(data, phnum_at, 2, little)
    if phnum == 0:
        if phoff != 0:
            raise _Malformed
    else:
        if phnum == 0xFFFF or phoff == 0 or phentsize != expected_phent:
            raise _Malformed
        _get(data, phoff, phentsize * phnum)
        offset_at, size_at = (4, 16) if klass == 1 else (8, 32)
        for index in range(phnum):
            base = phoff + phentsize * index
            kind = _read(data, base, 4, little)
            offset = _read(data, base + offset_at, width, little)
            size = _read(data, base + size_at, width, little)
            if kind != 0 and size != 0:
                _get(data, offset, size)
    shoff = _read(data, shoff_at, width, little)
    shentsize = _read(data, shentsize_at, 2, little)
    shnum = _read(data, shnum_at, 2, little)
    shstrndx = _read(data, shstrndx_at, 2, little)
    if shnum == 0:
        if shoff == 0 and shstrndx == 0:
            return [], 0
        raise _Malformed
    if (shstrndx == SHN_XINDEX or shoff == 0 or shentsize != expected_shent
            or shstrndx == 0 or shstrndx >= shnum):
        raise _Malformed
    _get(data, shoff, shentsize * shnum)
    field_at = (16, 20) if klass == 1 else (24, 32)
    sections = []
    for index in range(shnum):
        base = shoff + shentsize * index
        sections.append((
            _read(data, base, 4, little),
            _read(data, base + 4, 4, little),
            _read(data, base + field_at[0], width, little),
            _read(data, base + field_at[1], width, little),
        ))
    return sections, shstrndx


def _manifest_class(record: bytes) -> str:
    """`CellManifest::from_bytes` reduced to the class name it yields."""
    if len(record) not in (8, 16) or struct.unpack_from("<I", record)[0] != MANIFEST_MAGIC:
        raise _Malformed
    if record[4] == MANIFEST_VERSION_V1:
        if len(record) != 8 or record[6] or record[7] or record[5] & ~MANIFEST_FLAGS_MASK:
            raise _Malformed
        return "LEGACY"  # a v1 record upcasts to PROTECTION_CLASS_LEGACY
    if record[4] == MANIFEST_VERSION:
        tier = record[5]
        flags = struct.unpack_from("<H", record, 6)[0]
        if (len(record) != 16 or tier not in CLASS_NAMES or flags & ~MANIFEST_FLAGS_MASK
                or record[8:16] != bytes(8)):
            raise _Malformed
        return CLASS_NAMES[tier]
    raise _Malformed


def classify(data: bytes) -> str:
    """`ABSENT`, `MALFORMED`, or the manifest class name (`FFI`, `LEGACY`, …)."""
    try:
        sections, shstrndx = _sections(data)
        if not sections:
            return ABSENT
        _, strtab_kind, strtab_offset, strtab_size = sections[shstrndx]
        if strtab_kind != SHT_STRTAB:
            raise _Malformed
        names = _get(data, strtab_offset, strtab_size)
        found = None
        for name_at, kind, offset, size in sections:
            if name_at > len(names):
                raise _Malformed
            end = names.find(b"\0", name_at)
            if end < 0:
                raise _Malformed
            if kind != SHT_NOBITS:
                _get(data, offset, size)
            if names[name_at:end] != MANIFEST_SECTION:
                continue
            if kind == SHT_NOBITS or found is not None:
                raise _Malformed
            record = _get(data, offset, size)
            expected = {8: MANIFEST_VERSION_V1, 16: MANIFEST_VERSION}
            if len(record) not in expected or record[4] != expected[len(record)]:
                raise _Malformed
            found = _manifest_class(record)
        return found or ABSENT
    except _Malformed:
        return MALFORMED


def manifest_record(klass: int, flags: int = 0) -> bytes:
    """A 16-byte v2 manifest record (negative controls and tests)."""
    return struct.pack("<IBBHII", MANIFEST_MAGIC, MANIFEST_VERSION, klass, flags, 0, 0)


def minimal_elf(manifest: bytes | None) -> bytes:
    """A section-only ELF64 with an optional `__ViCell_manifest` section.

    For negative controls and tests: the kernel's classification needs only the
    header and the section table, so this is the smallest file it reads exactly
    like a built cell.
    """
    names = b"\0.shstrtab\0" + (MANIFEST_SECTION + b"\0" if manifest is not None else b"")
    body = names + (manifest or b"")
    shoff = 64 + len(body)
    shoff += -shoff % 8
    sections = [(0, 0, 0, 0), (1, SHT_STRTAB, 64, len(names))]
    if manifest is not None:
        sections.append((11, 1, 64 + len(names), len(manifest)))
    header = bytearray(64)
    header[0:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQQIHHHHHH", header, 16, 2, 0x3E, 1, 0, 0, shoff, 0, 64, 0, 0, 64,
                     len(sections), 1)
    out = bytearray(header) + body
    out += bytes(shoff - len(out))
    for name, kind, offset, size in sections:
        out += struct.pack("<IIQQQQIIQQ", name, kind, 0, 0, offset, size, 0, 0, 1, 0)
    return bytes(out)
