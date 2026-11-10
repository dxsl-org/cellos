#!/usr/bin/env python3
"""Cross-check the exported kernel catalog against what images start and ship.

Inputs: `scripts/catalog.generated.json` (export of `kernel/src/catalog.rs`),
`config/services.toml` (what init starts), and every committed image manifest
under `kernel/src/embedded*/manifest.json` (what an image ships).

Each `/bin/` path there must have a catalog row, or be listed in
`OUTSIDE_CATALOG` below with the reason it may run without one. Every Ethernet
ID the kernel hands out must name a catalog row that requests PCIe authority.
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

    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    print(f"catalog: OK ({len(rows)} rows; {sum(len(p) for p in sources.values())} image/service "
          f"paths checked; {len(OUTSIDE_CATALOG)} outside the catalog with a reason)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
