"""Strict ELF manifest-section extraction for corpus classification."""
from __future__ import annotations


def extract_elf_manifest(raw: bytes) -> bytes | None:
    """Independently parse the strict ELF envelope used by the Python inspector."""
    def number(offset: int, width: int, endian: str) -> int:
        if offset < 0 or offset + width > len(raw):
            raise ValueError("truncated ELF metadata")
        return int.from_bytes(raw[offset:offset + width], endian)

    def byte_range(offset: int, size: int) -> bytes:
        if offset < 0 or size < 0 or offset + size > len(raw):
            raise ValueError("ELF range is outside the file")
        return raw[offset:offset + size]

    if len(raw) < 16 or raw[:4] != b"\x7fELF" or raw[6] != 1:
        raise ValueError("not a supported ELF")
    elf_class = raw[4]
    if raw[5] == 1:
        endian = "little"
    elif raw[5] == 2:
        endian = "big"
    else:
        raise ValueError("unsupported ELF byte order")
    if elf_class == 1:
        header_size, entry_at, entry_width = 52, 24, 4
        program_offset_at, program_width, program_entry_at, program_count_at, program_entry_size = 28, 4, 42, 44, 32
        section_offset_at, section_width, header_size_at, section_entry_at, section_count_at, strings_index_at, section_entry_size = 32, 4, 40, 46, 48, 50, 40
    elif elf_class == 2:
        header_size, entry_at, entry_width = 64, 24, 8
        program_offset_at, program_width, program_entry_at, program_count_at, program_entry_size = 32, 8, 54, 56, 56
        section_offset_at, section_width, header_size_at, section_entry_at, section_count_at, strings_index_at, section_entry_size = 40, 8, 52, 58, 60, 62, 64
    else:
        raise ValueError("unsupported ELF class")
    if len(raw) < header_size or number(20, 4, endian) != 1 or number(header_size_at, 2, endian) != header_size:
        raise ValueError("invalid ELF header")
    program_offset, program_entry = number(program_offset_at, program_width, endian), number(program_entry_at, 2, endian)
    program_count = number(program_count_at, 2, endian)
    if program_count == 0:
        if program_offset != 0:
            raise ValueError("invalid empty program table")
    else:
        if program_count == 0xffff or program_offset == 0 or program_entry != program_entry_size:
            raise ValueError("invalid program table")
        byte_range(program_offset, program_entry * program_count)
        for index in range(program_count):
            base = program_offset + index * program_entry
            kind = number(base, 4, endian)
            offset_at, size_at = (4, 16) if elf_class == 1 else (8, 32)
            offset = number(base + offset_at, program_width, endian)
            size = number(base + size_at, program_width, endian)
            if kind and size:
                byte_range(offset, size)
    number(entry_at, entry_width, endian)
    section_offset, section_entry = number(section_offset_at, section_width, endian), number(section_entry_at, 2, endian)
    section_count, strings_index = number(section_count_at, 2, endian), number(strings_index_at, 2, endian)
    if section_count == 0:
        if section_offset == 0 and strings_index == 0:
            return None
        raise ValueError("extended ELF section numbering")
    if strings_index == 0xffff or section_offset == 0 or section_entry != section_entry_size or not 0 < strings_index < section_count:
        raise ValueError("invalid ELF section table")
    byte_range(section_offset, section_entry * section_count)

    def section(index: int) -> tuple[int, int, int, int]:
        base = section_offset + index * section_entry
        name, kind = number(base, 4, endian), number(base + 4, 4, endian)
        offset_at, size_at, width = (16, 20, 4) if elf_class == 1 else (24, 32, 8)
        return name, kind, number(base + offset_at, width, endian), number(base + size_at, width, endian)

    _, strings_kind, strings_offset, strings_size = section(strings_index)
    if strings_kind != 3:
        raise ValueError("ELF section names are not a string table")
    strings, found = byte_range(strings_offset, strings_size), None
    for index in range(section_count):
        name_offset, kind, offset, size = section(index)
        if name_offset >= len(strings):
            raise ValueError("ELF section name is outside the string table")
        terminator = strings.find(b"\0", name_offset)
        if terminator < 0:
            raise ValueError("ELF section name is unterminated")
        record = b"" if kind == 8 else byte_range(offset, size)
        if strings[name_offset:terminator] == b"__ViCell_manifest":
            if found is not None:
                raise ValueError("duplicate manifest ELF sections")
            found = record
    return found
