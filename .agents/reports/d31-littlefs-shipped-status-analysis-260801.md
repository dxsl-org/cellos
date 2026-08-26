# D31 — Correct littlefs status

**Status:** approved/applied 2026-08-01. No code changed.

## Finding

Spec 09 still puts littlefs at the G1 tail and describes `/data` as FAT32 transitioning
later. The VFS default feature set includes littlefs, the backend and block adapter are
present, and the mount manager uses it for `/data`; FAT32 is the interoperability backend
for `/mnt/sd`. Project history records passing QEMU VFS and power-loss suites.

That evidence is not equivalent to real-board power-cut qualification.

## Recommended ruling [FINAL]

**Approve recommendation A: mark the software path shipped and preserve the hardware gate.**

1. Document `/data` as default-enabled littlefs persistent storage and `/mnt/sd` as FAT.
2. Record the QEMU functional/power-loss evidence without calling it field qualification.
3. Keep real-board repeated power-cut testing as the remaining production/robot gate.
4. Put moving feature/test counts in generated status rather than the normative spec.
