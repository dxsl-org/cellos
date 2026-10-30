# Diagnostic CP1 approval (cancel-on-close)

Date: 2026-10-30. Approver: user, "Chạy 1 lần diag (≤2 boot) để xác nhận + positive control --timeout".
- Text: cp1-diag-draft.md SHA-256 cd27817aca60eafd010ba0074f1f8bc9106b07bbc649174da0e0589a23e8d35f, D-DIAG-1..5 as recommended in it. Authorizes the lane-only `--stage diag` closure of that draft: at most 2 stage boots; no product source change; QEMU/TCG, loopback only; no network fetch. Output is a diagnostic finding, not row evidence (no acceptance row is edited from it; claims per D-DIAG-2: CBX-23 "producer stops on consumer close" only for a cooperative close, cancel half recorded as not met plus a product finding).
- A product fix (Net cell/kernel) is a separate slice with its own CP1 and lock handoff; unchanged T3-D rerun is not authorized.
