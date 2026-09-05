# Phase 01 — Package Net Service

Status: completed

## Change

- Treat a nonzero `service-net` Cargo build as fatal before binary collection,
  so a stale target ELF cannot satisfy packaging.
- Package it as `/bin/net`, make it a required x86 image input, and assert the
  generated FAT layout contains `/bin/net`.
- Keep `/bin/virtio-net` optional; this lane selects the already-packaged e1000 Driver Cell.

## Acceptance

- The x86 cell build succeeds and reports `service-net -> /bin/net`.
- A fresh kernel rebuild embeds the refreshed image before ISO packaging.
- Existing required x86 cells remain present.
- A failed fresh `service-net` build cannot package an older binary.
- FAT inspection proves `/bin/net` is present, not merely available in `target/`.

## Evidence

- Fresh x86 cell build succeeded with `service-net` required as `/bin/net`.
- The builder invalidates both the prior service ELF and packaged image before
  the mandatory net build, then FAT-inspects the resulting `/bin/net` entry.
