#!/usr/bin/env python3
"""S10-3 CBX-01/02 audit (produced by code): the five candidate documents against the reconciled
delta index (catalog = the S10-0 revision), every delta-index row carries CP1 and CP2 (or a recorded
exception), and the recorded scoped cargo-deny result (log produced by the run in this directory)."""
import hashlib, json, re, sys
from pathlib import Path

P = Path("/home/dmin/cellos/.agents/261020-1750-cellbox-cli")
out = Path(sys.argv[1])
expected = {
    "spec.md": "f9c550f7a75ce4d79ac0aa6940dd21b529ba1232d0a7de081f2b105e4eba2988",  # S10-3a revision
    "abi-session-launch.md": "422df04c1f39d41aa37f4975caba359be65c48bffc2a004e7fec0f6c26822b44",
    "abi-vfs-telemetry.md": "0edda7171a424619ac49ec2f2d7569a91e45490e9f77adf793bd1ca25d7cec50",
    "command-catalog.md": "2a4c3e6652c90b5dad840c5a403276a3a8fa4236b8bab12ca1b319bc237cbd70",
    "command-contracts.md": "4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d",
}
docs = {}
for name, want in expected.items():
    got = hashlib.sha256((P / name).read_bytes()).hexdigest()
    docs[name] = {"expected": want, "actual": got, "equal": got == want}
text = (P / "contract-deltas.md").read_text()
index = text.split("### Delta index", 1)[1].split("**Recorded exceptions", 1)[0]
rows = []
for line in index.splitlines():
    if not line.startswith("| ") or line.startswith("| Delta") or line.startswith("|---"):
        continue
    cells = [c.strip() for c in line.strip("|").split("|")]
    delta, cp1, cp2 = cells[0], cells[3], cells[4]
    recorded_exception = cp1.startswith("none") or cp2 in ("—", "-")
    rows.append({"delta": delta[:80], "cp1": cp1[:60], "cp2": cp2[:60],
                 "has_cp1_and_cp2": bool(re.search(r"20\d\d-\d\d-\d\d", cp1)) and bool(re.search(r"20\d\d-\d\d-\d\d", cp2)),
                 "recorded_exception": recorded_exception})
deny_log = (out / "cargo-deny-cellbox.log").read_text()
deny_ok = "advisories ok, bans ok, licenses ok, sources ok" in deny_log and deny_log.rstrip().endswith("exit 0")
ws_log = (out / "cargo-deny-workspace.log").read_text()
ws_summary = [l.strip() for l in ws_log.splitlines() if re.search(r"(FAILED|ok): \d+ errors", l)]
result = {
    "documents": docs,
    "delta_rows": rows,
    "rows_missing_cp": [r["delta"] for r in rows if not (r["has_cp1_and_cp2"] or r["recorded_exception"])],
    "cargo_deny_cellbox_scoped": {"command": "cargo deny --offline --locked --exclude-dev --manifest-path "
                                  "cells/tools/cellbox/Cargo.toml check advisories licenses bans sources",
                                  "all_ok": deny_ok},
    "cargo_deny_workspace_summary": ws_summary,
}
result["verdict"] = "PASS" if all(d["equal"] for d in docs.values()) and not result["rows_missing_cp"] and deny_ok else "FAIL"
(out / "audit.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps({k: v for k, v in result.items() if k != "delta_rows"}, indent=1))
