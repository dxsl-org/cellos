#!/usr/bin/env python3
"""Strict, read-only Cell ELF inspector."""

import sys
from pathlib import Path

from elf_manifest import (
    InspectionError,
    PROTECTION_CLASSES,
    capability_labels,
    inspect_elf_bytes,
)


def inspect_path(path: Path) -> int:
    try:
        data = path.read_bytes()
        info = inspect_elf_bytes(data)
    except OSError as error:
        print(f"error: cannot read {path}: {error}", file=sys.stderr)
        return 1
    except InspectionError as error:
        print(f"error: {path}: {error}", file=sys.stderr)
        return 1

    print(f"ELF: {path}")
    print(f"ELF class: ELF{info.elf_class}")
    print(f"Byte order: {info.endian}-endian")
    print(f"Entry point: 0x{info.entry:X}")
    print("Execution tier: unknown (selected by external policy; not asserted by this ELF)")
    print("Runtime profile: unknown (not asserted by this ELF)")
    if info.manifest is None:
        print("Protection class: not asserted (manifest absent; legacy loader policy applies)")
        print("Capabilities: not asserted (manifest absent)")
        print("Evidence: not asserted (no manifest section)")
    else:
        manifest = info.manifest
        print(f"Protection class: {PROTECTION_CLASSES[manifest.protection_class]}")
        capabilities = capability_labels(manifest.flags)
        print(f"Capabilities: {', '.join(capabilities) if capabilities else 'none asserted'}")
        suffix = "; v1 upcasts to legacy protection" if manifest.version == 1 else ""
        print(
            f"Evidence: manifest v{manifest.version} section "
            f"({8 if manifest.version == 1 else 16} bytes{suffix}); "
            "signature and runtime measurement not asserted"
        )
    return 0


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check_elf.py <path>", file=sys.stderr)
        return 2
    return inspect_path(Path(argv[1]))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
