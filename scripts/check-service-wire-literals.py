#!/usr/bin/env python3
"""Service wire single source of truth: `libs/api/src/services/proto/`.

Phase 12 moved every service opcode, status and error byte into one proto
module per service. This gate keeps them there:

1. No `const` re-declares a proto-owned wire name outside its owner module.
2. No bare wire literal of a known shape comes back in the files that used to
   carry one (net error bytes, the App SDK envelope, the NIC status reply).

Exit 0 when clean, 1 otherwise. `--self-test` runs the patterns against
known-bad snippets.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCAN = ("cells", "libs", "kernel/src", "tools")

# Files that own a wire and may declare its names.
OWNERS = (
    "libs/api/src/services/proto/",
    "libs/api/src/services/ipc.rs",
    "libs/api/src/services/input.rs",
    "libs/api/src/services/display.rs",
    "libs/api/src/services/vmm_session",
    "libs/api/src/services/hostile_backend_recovery.rs",
    "libs/types/src/silo",
    "libs/types/src/kms",
    "libs/types/src/block_wire.rs",
)

# Wire names owned by `proto/`. A `const` of one of these names elsewhere is a copy.
WIRE_NAMES = {
    # NIC
    "OP_TX", "OP_RX", "OP_GETMAC",
    # supervisor and App SDK envelope
    "OP_HOTSWAP", "OP_POWER", "APP_MSG_MAGIC", "POWER_BUTTON_REQUEST",
    "OWNER_EXITED_OPCODE", "OWNER_EXITED_NOTICE", "OWNER_EXITED_RELAY",
    # TLS
    "TLS_CLOSE_OP", "TLS_CONNECT_OP", "TLS_SEND_OP", "TLS_RECV_OP",
    "TLS_CONNECT", "TLS_SEND", "TLS_RECV", "MAX_RAW_TLS_SEND", "MAX_TLS_RECV_DATA",
    # net, config
    "NET_ERR_DENIED", "NET_ERR_ADDR_IN_USE", "CONFIG_ERR_DENIED",
    # VFS
    "ERR_IO", "ERR_QUOTA", "ERR_DENIED", "ERR_HANDLE", "ERR_UNSUPPORTED",
    "VFS_ERR_IO", "VFS_ERR_QUOTA", "VFS_ERR_DENIED", "VFS_ERR_HANDLE", "VFS_ERR_UNSUPPORTED",
    # input producer frames
    "EV_ASCII", "EV_DEVICE", "INPUT_EVENT_OPCODE",
}

# Line comments describe wires; only code counts.
COMMENT = re.compile(r"^\s*//.*$", re.M)
CONST = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z0-9_]+)\s*:", re.M)

# (path prefix, pattern, what it is). Bare literals that used to be copies.
LITERALS: list[tuple[str, re.Pattern[str], str]] = [
    ("cells/services/net/src/", re.compile(r"Err\(\s*0x[Ff][B-Fb-f]\s*\)"), "net error byte"),
    ("", re.compile(r"\[\s*0x[Aa][Cc]\s*,\s*0x(?:00|[Ff][0-2Ff])\b"), "App SDK envelope header"),
    # The same header written byte by byte (`msg[0] = 0xAC;`).
    ("", re.compile(r"\[\s*0\s*\]\s*=\s*(?:0x[Aa][Cc]|172)\b"), "App SDK envelope magic"),
    ("cells/drivers/", re.compile(r"NicReply::Status\(\s*\d"), "NIC status byte"),
]


def owned(rel: str) -> bool:
    return rel.startswith(OWNERS)


def scan_text(rel: str, text: str) -> list[str]:
    errors = []
    text = COMMENT.sub("", text)
    if not owned(rel):
        for match in CONST.finditer(text):
            if match.group(1) in WIRE_NAMES:
                line = text.count("\n", 0, match.start()) + 1
                errors.append(f"{rel}:{line}: `{match.group(1)}` re-declares a proto wire name")
        for prefix, pattern, what in LITERALS:
            if not rel.startswith(prefix):
                continue
            for match in pattern.finditer(text):
                line = text.count("\n", 0, match.start()) + 1
                errors.append(f"{rel}:{line}: bare {what} `{match.group(0)}`")
    return errors


def self_test() -> int:
    cases = [
        ("cells/drivers/x/src/dispatch.rs", "const OP_TX: u8 = 0;\n"),
        ("cells/services/net/src/handlers.rs", "NetResponse::Err(0xFF)\n"),
        ("kernel/src/acpi/sci.rs", "const REQ: [u8; 4] = [0xAC, 0x00, 0x04, 0x02];\n"),
        ("kernel/src/task/syscall/authority.rs", "msg[0] = 0xAC;\n"),
        ("cells/drivers/e1000/src/dispatch.rs", "NicReply::Status(1)\n"),
    ]
    missed = [rel for rel, text in cases if not scan_text(rel, text)]
    clean = scan_text("libs/api/src/services/proto/nic.rs", "pub const OP_TX: u8 = 0;\n")
    if missed or clean:
        print(f"FAIL self-test: missed {missed}, owner flagged {clean}")
        return 1
    print("service wire literals: self-test OK")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    errors: list[str] = []
    files = 0
    for top in SCAN:
        for path in sorted((ROOT / top).rglob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            if "/target/" in rel or rel.startswith("third_party/"):
                continue
            files += 1
            errors.extend(scan_text(rel, path.read_text(errors="replace")))
    for error in errors:
        print(f"FAIL {error}")
    if errors:
        return 1
    print(f"service wire literals: OK ({files} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
