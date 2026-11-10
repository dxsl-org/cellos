#!/usr/bin/env python3
"""Cross-check the exported kernel catalog against what images start and ship.

Inputs: `scripts/catalog.generated.json` (export of `kernel/src/catalog.rs`),
`config/services.toml` (what init starts), and every committed image manifest
under `kernel/src/embedded*/manifest.json` (what an image ships).

Each `/bin/` path there must have a catalog row, or be listed in
`OUTSIDE_CATALOG` below with the reason it may run without one. Every Ethernet
ID the kernel hands out must name a catalog row that requests PCIe authority,
and every `pcie_driver` row must map to one `catalog::PCI_DRIVERS` program.
Exit 0 when consistent, 1 otherwise.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Paths that may appear in an image or service plan without a catalog row.
# Every entry needs a reason; a new entry is a review decision, not a fix.
OUTSIDE_CATALOG = {
    "/bin/ai": "init launch edge with the empty boot ceiling (pure IPC service)",
    "/bin/intel-gpu": "service row only; no image packages it yet (config/services.toml)",
    "/bin/kill": "shell-launched tool; ceiling comes from the cellbox launch edge",
    "/bin/shutdown": "shell-launched tool; ceiling comes from the cellbox launch edge",
    "/bin/bcm57xx": "packaged but inert: no catalog row and no Ethernet ID (x86 breadth S5)",
    "/bin/i40e": "packaged but inert: no catalog row and no Ethernet ID (x86 breadth S5)",
    "/bin/igc": "packaged but inert: no catalog row and no Ethernet ID (x86 breadth S5)",
    "/bin/ixgbe": "packaged but inert: no catalog row and no Ethernet ID (x86 breadth S5)",
    "/bin/r8169": "packaged but inert: no catalog row and no Ethernet ID (x86 breadth S5)",
}


# Signed program name of an install path whose basename differs from it
# (`scripts/sign-cell.py` names the artifact; images rename only the file).
PROGRAM_NAMES = {"/bin/block": "virtio-blk"}

# `pcie_driver` holders that never query 418/424, so they need no
# `pci_drivers` row; each entry needs a reason.
PCIE_WITHOUT_QUERY = {
    "/bin/virtio-gpu": "uses virtio MMIO / boot framebuffer; never calls 418/424",
    "/bin/input": "virtio-input over MMIO; never calls 418/424",
}

# `pci_drivers` programs with no `pcie_driver` catalog row, each with a reason.
PCI_ROW_WITHOUT_CATALOG = {
    "edu-holder": "test-hooks fixture; its row is in TEST_HOOK_CELLS",
}


def main() -> int:
    catalog = json.loads((ROOT / "scripts/catalog.generated.json").read_text())
    rows = {cell["path"]: cell for cell in catalog["cells"]}
    errors: list[str] = []

    sources: dict[str, set[str]] = {}
    services = (ROOT / "config/services.toml").read_text()
    sources["config/services.toml"] = set(re.findall(r'"(/bin/[^"]+)"', services))
    for manifest in sorted(ROOT.glob("kernel/src/embedded*/manifest.json")):
        files = json.loads(manifest.read_text()).get("files", {})
        sources[str(manifest.relative_to(ROOT))] = {p for p in files if p.startswith("/bin/")}

    used = set()
    for source, paths in sources.items():
        for path in sorted(paths):
            if path in rows:
                continue
            if path in OUTSIDE_CATALOG:
                used.add(path)
                continue
            errors.append(f"{source}: {path} has no catalog row (kernel/src/catalog.rs) "
                          "and no OUTSIDE_CATALOG reason")
    for path in sorted(set(OUTSIDE_CATALOG) - used):
        errors.append(f"OUTSIDE_CATALOG lists {path}, which no image or service plan uses — remove it")
    for path in sorted(set(OUTSIDE_CATALOG) & rows.keys()):
        errors.append(f"OUTSIDE_CATALOG lists {path}, which has a catalog row — remove it")

    for entry in catalog["ethernet_driver_ids"]:
        row = rows.get(entry["path"])
        if row is None or not row["request"]["pcie_driver"]:
            errors.append(f"Ethernet ID {entry['vendor']:#06x}:{entry['device']:#06x} names "
                          f"{entry['path']}, which does not request pcie_driver in the catalog")

    pci_programs = set(catalog["pci_drivers"])
    pcie_rows = sorted(p for p, row in rows.items() if row["request"]["pcie_driver"])
    claimed = set()
    for path in pcie_rows:
        program = PROGRAM_NAMES.get(path, path.rsplit("/", 1)[-1])
        if program in pci_programs:
            claimed.add(program)
            if path in PCIE_WITHOUT_QUERY:
                errors.append(f"PCIE_WITHOUT_QUERY lists {path}, but {program} has a pci_drivers row")
        elif path not in PCIE_WITHOUT_QUERY:
            errors.append(f"{path} requests pcie_driver but program {program!r} has no "
                          "catalog::PCI_DRIVERS row (418/424 would refuse every query)")
    for program in sorted(pci_programs - claimed - set(PCI_ROW_WITHOUT_CATALOG)):
        errors.append(f"catalog::PCI_DRIVERS row {program!r} matches no pcie_driver catalog row")

    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    print(f"catalog: OK ({len(rows)} rows; {sum(len(p) for p in sources.values())} image/service "
          f"paths checked; {len(OUTSIDE_CATALOG)} outside the catalog with a reason)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
