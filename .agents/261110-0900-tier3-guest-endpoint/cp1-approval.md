# Tier-3 guest endpoint CP1 approval

Date: 2026-10-30. Approver: user, "Duyệt D-T3-1..11 khuyến nghị + A-NET".
- Text: cp1-draft.md SHA-256 2f7ae851e6c8f80fd8960ff8598cc717d3be0a6b822afe7994c09314a12ba8dc accepted as the final CP1. D-T3-1..11 as recommended (ceiling statement of section 1; Alpine OpenSSH apks, sha256-pinned and installed offline; route R1 hostfwd loop; lane-only build, no candidate import; core scenario set; budget 2 boots + 2 runs, 4 h cap, no fix-and-rerun after marker FAIL; D-T3-8 = A; acknowledgement text of section 9; command-catalog unchanged; on STOP record a no-go dossier).
- A-NET (section 8): exactly one HTTPS GET session to dl-cdn.alpinelinux.org for the OpenSSH apk set, into a scratch dir, hashes recorded; nothing else fetched.
- Not authorized: Intel VMX, Net-cell hairpin (R2), L11/L13/L18 or protected-path edits, canonical promotion, publickey to the guest, push.

## Amendment 2026-10-30 (user): second A-NET session
The first session fetched only APKINDEX.tar.gz (the offline resolver aborted on an ambiguous /bin/sh provider before any .apk GET). A second GET-only session to dl-cdn.alpinelinux.org is authorized for exactly: openssh-server, openssh-server-common, openssh-keygen, busybox-binsh, busybox, musl, libcrypto3, zlib (Alpine v3.21 main x86_64, ~3.4 MB, index reused from disk), into the scratch dir; hashes recorded. No other host or file.
