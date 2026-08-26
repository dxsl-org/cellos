# Dossier 3b — glibc guest (Debian minimal) boot rework

**Cho:** P05 · **Nguồn:** research boot (aac57…) + rootfs-build (a2267…) 2026-07-12 · **Trạng thái:** ready-to-code

## Kết luận chốt (từ nghiên cứu, có nguồn)

### Boot model — root-on-blk, KHÔNG initramfs-to-tmpfs
- Bootargs: `console=ttyAMA0 root=/dev/vda1 rw rootfstype=ext4 <init>`. `root=/dev/vda` chỉ đúng nếu FS nằm thẳng trên block device không phân vùng; Debian phân vùng → phải `/dev/vda1`. Chốt layout khi build image.
- **Init staging (mỗi bước thêm 1 yêu cầu cứng — dễ cô lập lỗi):**
  1. `init=/bin/sh` trên real root — rẻ nhất, gương của `rdinit=/bin/sh` Alpine đã thắng. **MVP milestone.**
  2. `sysvinit-core` — inittab-driven PID1, không cgroup/dbus/udev. **Milestone trung gian, khuyến nghị làm guest chính.**
  3. systemd — chỉ nếu thật cần; đòi guest-kernel `.config`: DEVTMPFS, CGROUPS(v2), INOTIFY_USER, SIGNALFD, TIMERFD, EPOLL, UNIX(+NET), SYSFS, PROC_FS, FHANDLE, FUTEX; kernel ≥5.10. Thiếu bất kỳ = `PID 1 exited` panic. Rootfs phải có `/run` + symlink `/var/run→/run`.
- Debian initrd (initramfs-tools) mong đợi chạy → `switch_root` → exec `/sbin/init`. MVP bỏ qua initrd, dùng `init=/bin/sh` thẳng trên real root.

### Device set — DTB+virtio-mmio+GICv2 ĐỦ (không ACPI/PCI/GICv3)
- Stock Debian arm64 kernel boot bằng device-tree; ACPI trên arm64 là `CONFIG_ACPI` optional. QEMU `-M virt -kernel -initrd` (không UEFI) boot Debian arm64 = tiền lệ trực tiếp. Ecosystem nhầm vì Debian **installer/cloud image** giả định UEFI — đó là packaging, không phải yêu cầu kernel.
- **cloud-hypervisor bỏ virtio-mmio+GICv2+DT sang ACPI+PCI+GICv3-ITS** — nhưng release-note của họ nói là để boot *UEFI cloud image chưa sửa*, không phải yêu cầu kernel. Đọc như "lựa chọn của họ", KHÔNG bắt chước.
- ⇒ Giữ nguyên đường DTB+virtio-mmio+GICv2 hiện có; chỉ mở rộng DTB thêm node.

### virtio-rng — THÊM, ưu tiên trước blk
- Không có rng KHÔNG treo vĩnh viễn, nhưng gây stall "crng init done" từ vài giây tới vài phút (nặng trên ARM64 guest entropy yếu). Gate systemd-udevd, sshd host-key, DNS.
- Fix ngành: virtio-rng device (virtio device-id 4) trên transport virtio-mmio đã có → guest nhận entropy dump sớm. Rẻ, nguồn entropy = host GetRandom.
- **Rủi ro demo:** thiếu rng nhìn như "VMM bug" (boot lúc nhanh lúc chậm). Làm sớm.

### RTC — ưu tiên THẤP NHẤT
- Firecracker (peer gần nhất) có PL031 nhưng tắt interrupt; kernel hiện đại set time từ kvmclock/paravirt, không RTC. systemd degrade bằng `fake-hwclock` + timesyncd (chuẩn Raspberry Pi). RTC = "đúng wall-clock, không cần để tới shell". Defer sau milestone boot-to-shell.

### RAM — bump 128→256 MiB (floor), 512 cho apt
- 128MiB chỉ marginal cho installer; systemd+apt cần 256 floor, 512 nếu `apt` trong scope. (Estimate community, validate thực nghiệm khi virtio-blk xong — rẻ: so `-m 256` vs `-m 512`.)

### Rootfs build
- **mmdebstrap** (không debootstrap): ~2× nhanh, minbase gzipped ~34M (27M có apt), tự xử foreign-arch qemu-user (bớt 1 mảnh so với `qemu-debootstrap`). minbase = Essential+Priority:required, KHÔNG kernel/init.
- Package tối thiểu boot-to-shell + `apt update`: minbase + **sysvinit-core** (skip systemd cho gọn; skip busybox-as-PID1 vì lệch chuẩn Debian) + kernel package + netbase/ifupdown.
- **Kernel arm64 KHÔNG cần trích** (khác x86 PVH!): Debian `linux-image-arm64` ship `/boot/vmlinuz` = raw `arch/arm64/boot/Image` (Debian override `image-file: arch/arm64/boot/Image`). Loader chỉ cần đặt Image cách base 2MB-aligned `text_offset` byte + jump. Dùng kernel package Debian, KHÔNG tự build (tự build ra `Image.gz`).
- **Ảnh disk rootless (CI-friendly):** `fakeroot` bọc CẢ mmdebstrap + `mkfs.ext4 -m 0 -d $ROOTFS image.img <size>` (dùng lại fakeroot state file) — không loopback, không root. Nếu không bọc fakeroot: file bake sai uid → hỏng setuid/`/etc/shadow`. Tránh genext2fs (ext2, no journal) + virt-make-fs (nặng, cần root).
- Tổng ảnh: ~300-600MB (minbase 90-220MB + sysvinit + Image 30-60MB + ext4, `-m 0` bỏ 5% reserved).

## Ready-to-code checklist (P05)
- [ ] Edge P04 (image-file backing ≥512MB) xong trước.
- [ ] Script `scripts/build-glibc-guest.sh`: mmdebstrap minbase+sysvinit-core+linux-image-arm64 → `fakeroot mkfs.ext4 -m 0 -d` → `debian-arm64.img`.
- [ ] Đổi bootargs `dtb.rs:22` → `root=/dev/vda1 rw rootfstype=ext4 init=/bin/sh` (MVP).
- [ ] Loader nạp raw arm64 Image (text_offset, 2MB-align) — tái dùng `loader_image.rs`.
- [ ] Bump `GUEST_RAM_SIZE` `main.rs:55` → 256MiB (512 nếu test apt).
- [ ] Thêm virtio-rng device model (device-id 4, virtio-mmio slot mới) + DTB node; backend = host GetRandom.
- [ ] (Sau boot-to-shell) sysvinit milestone; systemd chỉ nếu cần + guest-kernel .config đủ.
- [ ] rootfs selection Alpine|Debian khi `vm create` (user chốt GIỮ CẢ HAI → 2 lane CI).
- [ ] RTC defer.

## Rủi ro / mở
- systemd cgroup v1/v2 mount + udev khi tới "apt install thật" — ngoài scope pass này, kiểm ở milestone sau boot-to-shell.
- Số RAM/size là estimate → validate thực nghiệm khi blk xong.
