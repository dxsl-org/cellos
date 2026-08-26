# Phase 05 — glibc guest = root-on-blk boot rework (Debian minimal)

**Cửa sổ:** post-window (coding) · **Priority:** P1 · **Status:** pending · **Tier:** thinking · **Effort:** XL · **Law 1:** no
**Depends:** P01, **P04 (edge cứng)**

> ⚠️ **Re-scoped M→XL sau Red Team (F1/F2/B6).** "Thêm build script" là SAI. VMM hiện chỉ boot được initramfs→shell; Debian cần một *đường boot khác* mà chưa phase nào xây.

## Context Links
- Dossier: `reports/dossier-glibc-guest.md` (P03) · Research §4 · Red-Team F1/F2/B6
- Bằng chứng hiện trạng: `dtb.rs:22` (`rdinit=/bin/sh`, không `root=`), `main.rs:55` (RAM 128MiB), `main.rs:59-60` (load /vmlinuz+/initrd.gz), `virtio_blk.rs:15,33` (16MiB Vec volatile)

## Overview
Chạy binary glibc/proprietary — rào cản độ phủ lớn nhất. Nhưng đây KHÔNG phải script; là làm mới đường boot: từ initramfs-to-shell (đủ cho Alpine 5MiB) sang **root-on-block-device + init system** (bắt buộc cho Debian ~150-250MiB).

## Key Insights (từ Red Team, đã xác minh code)
- **Boot model hiện tại vật lý bất khả cho Debian:** initramfs unpack vào tmpfs, RAM 128MiB < rootfs Debian. Phải chuyển sang `root=/dev/vda` + `init=/sbin/init` trên rootfs thật qua virtio-blk.
- **Backing 16MiB volatile Vec** (`virtio_blk.rs:33`) không chứa nổi rootfs và không persist → phụ thuộc P04 (image-file backing) + phải resize.
- **systemd/init cần root ghi được** (`/etc/machine-id`, `/var`, journal) → P05→P04 là edge cứng, không phải song song như bản plan cũ.
- **B6 — tiền đề chạy phần mềm thật (không chỉ boot):** glibc TLS cần **RTC** (cert validity) + **virtio-rng/entropy** (`/dev/random` block). P05 exists để chạy Python wheel/.NET → tất cả TLS. Boot tới shell mà thiếu 2 cái này = FAIL mục tiêu.

## Requirements
**Functional:**
- Đường boot root-on-blk: bootargs `root=/dev/vda rw init=/sbin/init` (thay `rdinit=/bin/sh`, `dtb.rs:22`); DTB/loader cấu hình tương ứng.
- Disk backing từ **image-file thật ≥150MiB** (qua P04 image-file backing, không phải Vec).
- Guest-RAM bump (>128MiB; số theo dossier + đo).
- Init system tới shell/login (Debian dùng systemd; cân nhắc `init=/bin/sh` cho MVP trước khi systemd).
- **Guest RTC** wired vào device model (Goldfish RTC đã có trong project — nối vào guest).
- **virtio-rng** device thêm vào guest (host GetRandom đã có).
- Build script `scripts/build-glibc-guest.sh` (debootstrap/mmdebstrap theo dossier).
- **Chọn rootfs khi `vm create` (user chốt GIỮ CẢ Alpine + Debian, B4)** → 2 lane CI phải xanh, 2 kernel config. Ghi rõ chủ sở hữu bảo trì 2 guest trong dossier/CI.

**Non-functional:** không regression Alpine (nếu giữ); số RAM/boot-time thật ghi vào doc (thay ước lượng).

## Related Code Files
- Modify: `dtb.rs` (bootargs + device nodes RTC/rng), `main.rs` (RAM size, boot artifacts, load rootfs image), `loader_image.rs`, virtio device set (thêm virtio-rng).
- Create: `scripts/build-glibc-guest.sh`; guest RTC + virtio-rng device model trong cell.
- Depends: P04 image-file backing.

## Implementation Steps
1. P04 xong (image-file backing + resize). Xác nhận edge.
2. Build Debian minbase rootfs image (≥150MiB) theo dossier 3b.
3. Đổi bootargs sang root-on-blk; loader nạp rootfs image vào backing.
4. Bump guest RAM; boot tới shell với `init=/bin/sh` (MVP), rồi systemd.
5. Wire guest RTC + virtio-rng; verify `clock_gettime` đúng + `/dev/random` không block.
6. Chạy 1 binary glibc TLS thật (vd `python3 -c "import ssl; ..."` hoặc `apt update` qua network).
7. Success criterion network: outbound DHCP/DNS/HTTP từ guest Debian.
8. `haily-tester` + `haily-reviewer`.

## Todo
- [ ] Gate P04 (image-file backing) xong
- [ ] Debian minbase rootfs image ≥150MiB
- [ ] Bootargs root-on-blk + loader nạp image
- [ ] Guest RAM bump
- [ ] Boot init→shell (MVP init=/bin/sh, rồi systemd)
- [ ] Guest RTC wired + verify
- [ ] virtio-rng wired + verify /dev/random
- [ ] Binary glibc TLS chạy được
- [ ] Outbound network verify
- [ ] Regression Alpine (nếu giữ)

## Success Criteria
- Debian glibc guest boot tới shell qua root-on-blk.
- **TLS software chạy thật** (RTC + entropy đủ) — không chỉ boot tới shell.
- Outbound network hoạt động.
- Số RAM/boot-time thật vào doc.

## Risk Assessment
- **XL, rủi ro cao nhất của nửa compat.** Boot hang chờ device VMM không emulate → dùng UART sentinel + `earlycon` để chẩn đoán (như ARM64 P01 track M1).
- systemd đòi hỏi nhiều hơn init đơn giản → MVP `init=/bin/sh` trước để tách lỗi boot-path khỏi lỗi init.
- Nếu dossier phát hiện Debian cần device khác (nhiều PCI/ACPI/RTC MMIO) → escalate, có thể tách thành phase con.

## Security Considerations
- Guest lớn hơn = nhiều bề mặt trong-guest, cô lập host KHÔNG đổi (Stage-2 + bounds-check + backing-isolation P04).
- virtio-rng: entropy từ host GetRandom — không rò trạng thái host khác vào guest.

## Next Steps
- P08 tái dùng để có glibc guest trên x86 (mốc compat x86, gate sau P04+P05).
