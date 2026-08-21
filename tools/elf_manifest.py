"""Strict, non-mutating ELF and Cell Manifest v1/v2 inspection."""

from dataclasses import dataclass

ELF_MAGIC = b"\x7fELF"
MANIFEST_MAGIC = 0x56494345
MANIFEST_FLAGS_MASK = 0x0FFF
MANIFEST_NAME = b"__ViCell_manifest"
CAPABILITIES = (
    (1 << 0, "block-io"),
    (1 << 1, "network"),
    (1 << 2, "spawn"),
    (1 << 3, "gpio"),
    (1 << 4, "uart"),
    (1 << 5, "hypervisor"),
    (1 << 6, "partition-data"),
    (1 << 7, "partition-lfs"),
    (1 << 8, "can"),
    (1 << 9, "adc"),
    (1 << 10, "i2c"),
    (1 << 11, "spi"),
)
PROTECTION_CLASSES = {
    0: "trusted-core",
    1: "standard",
    2: "ffi",
    3: "untrusted",
    0xFF: "legacy (no explicit class)",
}


class InspectionError(ValueError):
    """The input is not a structurally valid, supported ELF/manifest."""


@dataclass(frozen=True)
class ManifestInfo:
    version: int
    protection_class: int
    flags: int


@dataclass(frozen=True)
class ElfInfo:
    elf_class: int
    endian: str
    entry: int
    manifest: ManifestInfo | None


def _u(data: bytes, offset: int, width: int, endian: str) -> int:
    end = offset + width
    if offset < 0 or end < offset or end > len(data):
        raise InspectionError("truncated ELF metadata")
    return int.from_bytes(data[offset:end], endian)


def _range(data: bytes, offset: int, size: int, label: str) -> bytes:
    end = offset + size
    if offset < 0 or size < 0 or end < offset or end > len(data):
        raise InspectionError(f"{label} range is outside the file")
    return data[offset:end]


def _manifest(record: bytes) -> ManifestInfo:
    if len(record) not in (8, 16):
        raise InspectionError("manifest section must be exactly 8 or 16 bytes")
    if int.from_bytes(record[:4], "little") != MANIFEST_MAGIC:
        raise InspectionError("manifest magic is invalid")
    version = record[4]
    if version == 1 and len(record) == 8:
        if record[6:8] != b"\0\0":
            raise InspectionError("manifest v1 padding is non-zero")
        protection_class, flags = 0xFF, record[5]
    elif version == 2 and len(record) == 16:
        protection_class = record[5]
        flags = int.from_bytes(record[6:8], "little")
        if protection_class not in PROTECTION_CLASSES:
            raise InspectionError("manifest protection class is unknown")
        if record[8:16] != bytes(8):
            raise InspectionError("manifest reserved data is non-zero")
    else:
        raise InspectionError("manifest version does not match its record length")
    if flags & ~MANIFEST_FLAGS_MASK:
        raise InspectionError("manifest contains unknown capability flags")
    return ManifestInfo(version, protection_class, flags)


def inspect_elf_bytes(data: bytes) -> ElfInfo:
    if len(data) < 16 or data[:4] != ELF_MAGIC or data[6] != 1:
        raise InspectionError("not a supported ELF file")
    elf_class = data[4]
    if data[5] == 1:
        endian = "little"
    elif data[5] == 2:
        endian = "big"
    else:
        raise InspectionError("unsupported ELF byte order")
    if elf_class == 1:
        header_size, entry_at, entry_width = 52, 24, 4
        phoff_at, phoff_width, phentsize_at, phnum_at, expected_phent = 28, 4, 42, 44, 32
        shoff_at, shoff_width = 32, 4
        ehsize_at, shentsize_at, shnum_at, shstrndx_at, expected_shent = 40, 46, 48, 50, 40
    elif elf_class == 2:
        header_size, entry_at, entry_width = 64, 24, 8
        phoff_at, phoff_width, phentsize_at, phnum_at, expected_phent = 32, 8, 54, 56, 56
        shoff_at, shoff_width = 40, 8
        ehsize_at, shentsize_at, shnum_at, shstrndx_at, expected_shent = 52, 58, 60, 62, 64
    else:
        raise InspectionError("unsupported ELF class")
    if (
        len(data) < header_size
        or _u(data, 20, 4, endian) != 1
        or _u(data, ehsize_at, 2, endian) != header_size
    ):
        raise InspectionError("invalid ELF header")
    phoff = _u(data, phoff_at, phoff_width, endian)
    phentsize = _u(data, phentsize_at, 2, endian)
    phnum = _u(data, phnum_at, 2, endian)
    if phnum == 0:
        if phoff != 0:
            raise InspectionError("invalid empty program table")
    else:
        if phnum == 0xFFFF or phoff == 0 or phentsize != expected_phent:
            raise InspectionError("invalid or extended program table")
        _range(data, phoff, phentsize * phnum, "program table")
        for index in range(phnum):
            base = phoff + index * phentsize
            kind = _u(data, base, 4, endian)
            offset_at, size_at = (4, 16) if elf_class == 1 else (8, 32)
            offset = _u(data, base + offset_at, phoff_width, endian)
            size = _u(data, base + size_at, phoff_width, endian)
            if kind and size:
                _range(data, offset, size, "program segment")
    entry = _u(data, entry_at, entry_width, endian)
    shoff = _u(data, shoff_at, shoff_width, endian)
    shentsize = _u(data, shentsize_at, 2, endian)
    shnum = _u(data, shnum_at, 2, endian)
    shstrndx = _u(data, shstrndx_at, 2, endian)
    if shnum == 0:
        if shoff == 0 and shstrndx == 0:
            return ElfInfo(elf_class * 32, endian, entry, None)
        raise InspectionError("extended section numbering is unsupported")
    if shstrndx == 0xFFFF:
        raise InspectionError("extended section-name indexing is unsupported")
    if shoff == 0 or shentsize != expected_shent or not 0 < shstrndx < shnum:
        raise InspectionError("invalid ELF section-table metadata")
    _range(data, shoff, shentsize * shnum, "section table")

    def section(index: int) -> tuple[int, int, int, int]:
        base = shoff + index * shentsize
        name = _u(data, base, 4, endian)
        kind = _u(data, base + 4, 4, endian)
        if elf_class == 1:
            offset, size = _u(data, base + 16, 4, endian), _u(data, base + 20, 4, endian)
        else:
            offset, size = _u(data, base + 24, 8, endian), _u(data, base + 32, 8, endian)
        return name, kind, offset, size

    _, strings_kind, strings_offset, strings_size = section(shstrndx)
    if strings_kind != 3:
        raise InspectionError("section-name table is not a string table")
    strings = _range(data, strings_offset, strings_size, "section-name table")
    found = None
    for index in range(shnum):
        name_offset, kind, offset, size = section(index)
        if name_offset >= len(strings):
            raise InspectionError("section name is outside the name table")
        terminator = strings.find(b"\0", name_offset)
        if terminator < 0:
            raise InspectionError("section name is not terminated")
        if kind != 8:
            record = _range(data, offset, size, "section")
        else:
            record = b""
        if strings[name_offset:terminator] == MANIFEST_NAME:
            if found is not None:
                raise InspectionError("duplicate manifest sections")
            found = _manifest(record)
    return ElfInfo(elf_class * 32, endian, entry, found)


def capability_labels(flags: int) -> list[str]:
    return [name for bit, name in CAPABILITIES if flags & bit]
