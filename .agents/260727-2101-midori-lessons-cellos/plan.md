# Midori Lessons → Cellos: Async, No-Root, Lightweight

**Created**: 2026-07-27 · **Red-teamed**: 2026-07-27 · **Validated**: 2026-07-27
**Status**: COMPLETED CONVERGENCE PROGRAM (D39, 2026-08-06). Complete: 01, 02, 03, 04,
05, 06, 07, 08, 09, 10, 11. Deferred boundaries stay documented in the phase notes:
init-respawn proof, async VFS/DMA/RecvScatter, unmeasured paths remain 64, and the
W^X cross-hart limitation.
· **Source**: analysis session 2026-07-27

> **WIP-limit exception:** P0 security fixes, broken-build/CI repairs, and
> verification-only closures may proceed when they do not open another feature program.
> Supervisory migration, package distribution, and Trust & Identity remainder are queued.

Áp dụng 3 tính năng lõi của Midori vào Cellos theo Scope Doctrine (cả 3 đều qua Gate 1 —
leverage SAS/LBI, không replicate Linux). Plan này xử lý các phát hiện cụ thể từ codebase,
không phải một cuộc port kiến trúc.

## Nguồn phát hiện

| # | Tính năng Midori | Thực trạng Cellos | Phase |
|---|---|---|---|
| 1 | Không có root / no ambient authority | VFS bỏ trống authorization trên op destructive + read; hai bảng handle không so owner | 01, 02, 06 |
| 2 | Không leo thang quyền | `init` = `CapSet::ALL`, shell = sudo, POLICY.BIN chưa bake + không phủ 3 cap mạnh | 03, 04 |
| 3 | Nhẹ tựa lông hồng | 4 stack cấp mỗi lần spawn (2 bị bỏ), ~1 MB peak liên tục/cell | 05, 08 |
| 4 | Async toàn tập | 5 `.await`/45K LOC; executor là busy-poll dummy waker | 07 |

## Phases

| Phase | Tên | Ưu tiên | Trạng thái | ABI gate |
|-------|-----|---------|-----------|----------|
| [01](phase-01-vfs-destructive-authz.md) | VFS destructive-op authorization | P1 | **MERGED 2026-07-28** (PR #10, đủ tiêu chí) | Không |
| [02](phase-02-vfs-read-gating.md) | Read gating + handle owner-check + ACL | P1 | **Runtime-closed under amended criteria 2026-08-05** — `0c83ce0f` (owner-check) + `7a525538` (attested cell_id, 7/7 governed message-path op gated) + QEMU markers for metadata-only pre-seal governed `GetFile` and `ReadFileGrant` clamp/nonzero/deny. **Rescoped out of this phase**: real `ReadGrant` producer → future Law 1 `OpenAt`/file-handle/close design; direct fast-IPC `GetFile` proof → future Tier-1 transport rewrite. `DataPtr` vẫn same-SAS only, không Tier-2-safe | Law 1 ✅ confirm 2× |
| [03](phase-03-policy-cap-coverage.md) | POLICY.BIN v2 + enumeration + bít escape hatch | P1 | **Done 2026-07-28** — `d7bb53869` + bake 8/8 lane (`f7e4bb4e7`, `a8516c767`, `edbb20ba5`); còn `hv-x86` chưa verify + `sensor-demo` không có trong image | Không |
| [04](phase-04-deprivilege-init-shell.md) | Deprivilege init + shell; fold `/bin/vfs` region | P2 | **Completed 2026-07-30** — launch-edge authority landed; init-respawn proof remains deferred. | Law 1 ✅ confirm 2× (service ID chưa dùng) |
| [05](phase-05-stack-deadalloc-binary.md) | Xoá stack cấp trùng + 3 `.expect` + binary footprint | P1 | **Done 2026-07-28** (`c9bb6f2fc`) | Không |
| [06](phase-06-directory-capabilities.md) | Directory capability thay path string | P3 | **XONG 2026-07-31** — ADR [09c](../../docs/specs/09c-vfs-directory-capabilities-adr.md) accepted; kernel `6d3bcc10` + VFS `658cc398`. Criterion chính **chứng minh trên QEMU**: pioneer `/bin/vfs-test` sealed → 57 PASS/0 FAIL, `Write{path}` bị từ chối, widening bị từ chối, op theo handle vẫn chạy. Còn mở: `GetFile` vẫn trả con trỏ thô (4 caller); xoá variant path-string chờ shell/lua/wasm/ostd | Law 1 ✅ ×2 (gộp cả spawn ABI, ADR chọn phương án (a)) |
| [07](phase-07-async-reactor.md) | Reactor thật + Async Pinning Registry | P3 | **Completed 2026-08-06** — verified substrate closed; generic reactor, `RecvScatter`, async VFS/DMA remain deferred. | Law 1 cho event/syscall ABI mới — **vẫn chưa xin thêm** |
| [08](phase-08-stack-sizing-table.md) | Bảng stack sizing per-path (tách khỏi 05) | P2 | **Completed 2026-08-06** — six measured paths fixed at 16 usable pages plus two guards; unmeasured paths remain 64. | Không |
| [09](phase-09-noentry-fail-closed.md) | `NoEntry` fail-closed cho path P-TRUST (từ mục Deferred) | P2 | **Runtime-verified 2026-07-31** — incomplete signed policy strip đúng P-TRUST, complete policy không có false positive, shell smoke 3 arch + AArch64 `periph-demo` xanh. **CÒN MỞ**: ARM image không package `sensor-demo`/`robot-demo`; full RV64 serial rerun timeout | Không |
| [10](phase-10-wx-post-reloc.md) | W^X post-reloc — hạ quyền trang cell về p_flags | P2 | **Completed 2026-07-31** — code and runtime verified via QEMU; cross-hart TLB shootdown remains a known SMP limitation. | Không |
| [11](phase-11-cellos-sign-f1.md) | `cellos-sign` — build+kiểm F1+ký một bước | P1 | **Complete + runtime-verified 2026-07-31** — F1/F5, signer 35/35, real RV64 sign→verify→tamper rejection, signed-image boot, W^X 2/2; prior RV64 boot evidence 54/54 retained with provenance | Không (stretch manifest-bit là Law 1, PR riêng) |

## Thứ tự thực thi

```
01 ──► 02 ──► 06          (02 gated: Law 1 cell_id; 02 cũng là prerequisite owner-check của 06)
03 ──► 04 ──► 09          (04 hấp thụ req5 cũ của 03; 09 cần ceiling per-path của 04 để không lặp C1)
05 ──► 07 ──► 08          (08 phải đo watermark LẠI sau khi shim block_on thêm frame)
10, 11 độc lập            (10 chỉ đụng loader+paging; 11 toàn tooling/CI — song song với mọi nhánh)
```

- **Bổ sung 2026-07-30** (nguồn: `reports/gap-analysis-midori-lessons-260730-0849.md`, user
  đã duyệt): 09 đóng finding Deferred (`NoEntry` dev-permissive); 10 = Layer A của
  `docs/specs/19-hardware-isolation-layers.md`; 11 hiện thực Tier-1 admission của
  `docs/specs/18-cell-trust-tiers.md`. Tier 2 (per-domain page table) và các gap IPC/mailbox/
  scale thuộc **plan sau**, không thêm vào đây.

- **Phát hiện mở khi làm 10 + 11 (2026-07-30)** — không sửa trong hai commit đó, cần phase/ticket riêng.
  Nguồn: `reports/wave1-review-260730.md`.
  1. ✅ **ĐÃ ĐÓNG 2026-07-30** (`a7ef17e2`) — `SpawnFromMem` giờ đi qua `spawn_gated`. Name do caller
     truyền **không** được dùng làm path: rút về label `/mem/<component>` (chỉ component cuối, lọc
     `[A-Za-z0-9._-]`, cap 64) nên không match được `/bin/` prefix, exact-path của policy/trusted-core,
     hay `ends_with("/bin/…")` → cap yêu cầu là `EMPTY` by construction. Đây là chỗ dễ đổi một lỗ
     thành một lỗ tệ hơn (signature bypass → privilege escalation qua tên tự chọn), nên invariant của
     label là phần load-bearing, đã test 60+ tên thù địch + mutation check.
     **Đổi hành vi cần biết**: `exec` một image có manifest khai privilege giờ bị **từ chối** thay vì
     spawn cell không cap; `PermissionDenied` được propagate thay vì bị làm phẳng thành `InvalidInput`.
     **Chưa verify**: đường spawn thành công chưa từng chạy (không boot được).
     Còn mở, tách ticket: syscall này **vẫn không đòi `SpawnCap`** (khác 2 spawn syscall còn lại) và
     **vẫn bỏ `args_ptr`/`args_len`**.
     — mô tả gốc: không verify Ed25519, không gate
     manifest; shell phơi ra thành `exec <file>` chỉ kiểm 4 byte magic
     (`cells/tools/shell/src/commands.rs:78-97` → `sys_spawn_from_mem`).
     **Xác minh trực tiếp 2026-07-30**: signature gate nằm **duy nhất** trong `spawn_gated`
     ([loader.rs:116-140](../../kernel/src/loader.rs#L116-L140)); `SpawnFromMem` gọi
     `task::spawn_from_mem` thẳng nên không bao giờ đi qua đó.
     ⇒ Hệ quả nặng hơn mô tả ban đầu: **bật feature `signing-required` cũng KHÔNG chặn được** —
     `exec` vẫn nạp ELF chưa ký. Đây vừa là **điều kiện kích hoạt** của 2/3 Critical review tìm ra,
     vừa **vô hiệu hoá posture ký** mà phase 11 vừa dựng: gate đường ký vô nghĩa khi có một syscall
     nạp thẳng ELF không ký. **Ưu tiên cao nhất trong danh sách này** — nên đóng trước 02/04, và
     trước khi release checklist bật `signing_required`.
  2. ✅ **ĐÃ ĐÓNG 2026-07-30** (`98a08325`) — dùng `#[target_feature(enable = "mte")]` **per-function**
     thay vì thêm `+mte` vào rustflags, nên codegen phần còn lại của kernel không đổi. An toàn vì mọi
     đường tới `stg`/`ldg` đều đã gated runtime (`init()` return sớm khi `!is_available()`;
     selftest bail khi MTE field < 2) → máy không có MTE (ARMv8.0 như raspi3b) assemble nhưng không bao
     giờ execute. **Kiểm chứng**: `cargo build` aarch64 giờ **link được** (binary 22 MB thật);
     `#[target_feature]` trên trait impl method compile OK và `stg` được emit thật (`d9200800`,
     disassemble xác nhận).
     Lane `test-hooks` aarch64 **vẫn đỏ vì lý do khác, pre-existing**: `qemu_exit` không có
     `AArch64Semihosting`, `task/user_hello.rs` 3 lỗi type mismatch. MTE không còn nằm trong số đó.
     **ĐÍNH CHÍNH 2026-07-30** (điều tra lại sau khi commit — commit message của `98a08325` nói sai
     chỗ này): CI **ĐÃ CÓ** lane build kernel aarch64 —
     [ci.yml:170](../../.github/workflows/ci.yml#L170) `cargo build --release -p vicell-kernel`
     chạy cho **mọi** matrix row, không có `continue-on-error`. Nên lỗi này **không hề vô hình**:
     job `Build (aarch64-unknown-none-softfloat)` đơn giản là **đang ĐỎ**.
     ⇒ Không cần thêm lane mới (khuyến nghị cũ sai). Fix `98a08325` **sửa một CI job đang đỏ thật**.
     ⇒ Xác minh bằng đúng rustflags của CI (`+bti,+paca,+pacg`): trước fix `error: instruction
     requires: mte`; sau fix `Finished release profile`.
     Điều đúng trong bài học cũ: `cargo check` **không** assemble inline asm, nên `cargo check` xanh
     không nói gì về asm — dùng `cargo build` khi verify thay đổi asm. Nhưng nguyên nhân sống sót là
     **lane đỏ bị bỏ qua**, không phải thiếu lane.
     — mô tả gốc: `hal/arch/arm/src/aarch64/mte.rs:61,78` dùng `stg`/`ldg`
     nhưng rustflags aarch64 của CI là `+bti,+paca,+pacg`, thiếu `+mte` → mọi job link thật kernel
     aarch64 đang đỏ (pre-existing, `mte.rs` không bị hai commit này đụng). Hệ quả phụ: `cargo check`
     không assemble inline asm, nên asm aarch64 **không** được CI kiểm.
  3. **Không có cross-hart TLB shootdown** trong cây. `sfence.vma` (riscv64) và `invlpg` (x86_64)
     chỉ local-hart; aarch64 `tlbi ...is` mới là broadcast. Mitigation mà plan giả định ("IPI/sfence
     broadcast sẵn có của SMP phase 32") **không tồn tại** — chỉ có `sbi_send_ipi` trần.
  4. `tlb_flush_all` (`kernel/src/memory/paging.rs:53-66`) thiếu companion `alle2is`; `unmap`
     (`:157-162`) thiếu `dsb ishst` dẫn, dùng `dsb sy` thay `dsb ish`, không `isb`, không nhánh EL2.
  5. **Macro cross-crate lách F1**: `macro_rules!` export từ `ostd` expand `unsafe` sạch trong crate
     có `forbid(unsafe_code)`, và không lớp nào của F1 thấy. Dùng có chủ ý 1 lần (`cell_main!`);
     không có gì chặn macro sau mở rộng lỗ. Đề xuất: quét thân `macro_rules!` dưới `libs/ostd`.
  6. `task::spawn_synthetic` (`kernel/src/task.rs:1763`) vẫn nhận cell id do caller đưa —
     **0 call site**, latent, sẽ tái tạo lỗi CellId(0) nếu hồi sinh.

- **01, 03, 05 độc lập → song song được**, với một ngoại lệ: 03 và 04 cùng sửa `kernel/src/loader.rs`
  và `kernel/src/task/cap.rs`; 05 cũng sửa `loader.rs`. Serialize các commit đụng 2 file này.
- **Số đo RAM của 05 không còn là metric whole-system** nếu 01/03 land song song — 05 báo cáo
  peak contiguous demand per spawn (đo được cục bộ), không phải "RAM free sau boot".
- Phase 08 KHÔNG chạy trước representative post-shim executor evidence:
  `block_on` pin future trên stack của caller (`libs/ostd/src/executor.rs:20`), nên
  watermark đo trước executor/generic-wait migration là dữ liệu của một thế giới khác.
  NET_RX-only substrate evidence from phase 07 là điều kiện honesty, không phải sizing input đủ.

## Dependencies & gates

- **Law 1 (libs/api = 2× user confirmation)**: phase 02 (cell_id kèm IPC — đã chọn làm mặc định,
  bỏ phương án GetProcs), 04 (service ID cho broker trong `libs/api/src/abi/syscall.rs:718-743`),
  06 (`VfsRequest` shape), 07 (syscall mới). Phase 03 và 05 không có ABI gate.
- ~~**Blocking dependency ngoài plan**: phase 02 phụ thuộc `.agents/260712-1903-thread-cellid-quota-fix`~~
  → **XOÁ (validation V1)**: fix kernel-side đã land (`kernel/src/task/syscall.rs:1415-1450` — thread
  inherit `cell_id` của cha, fail-safe deny khi không resolve được). Plan sibling đã được đóng (D3).
  Phần còn mở là **VFS-side** (V2) và nó đã được gộp vào phase 02 (D1) — không còn là dependency ngoài.
- **Cross-plan conflict**: `.agents/260712-1000-cell-package-distribution/phase-01-writable-cell-store.md`
  muốn mở `/bin/` writable (đụng đúng `access.rs:33` + `backend_bin_overlay.rs:63-68` mà phase
  01/02 siết). Precedence đã chốt: **phase 02 làm rule per-cell trước**, pkg plan dùng rule
  per-cell cho `/bin/`, KHÔNG flip `allow_write_all` toàn prefix.
- **Cross-plan conflict**: phase 07 rewrite đường grant-reap, đụng
  `.agents/260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md`.
- **Async Pinning Registry** (`docs/specs/03-runtime.md:22-24`, chưa hiện thực) là prerequisite
  cứng của phase 07, và phạm vi phải phủ cả completion queue, không chỉ DMA buffer.
- **POLICY.BIN hiện KHÔNG có trong bất kỳ embedded VIFS1 nào** (verified 2026-07-27) → nhánh
  `Absent` → dev-permissive. Phase 03 step bake là **lần bake đầu tiên**, nên blob phải
  behaviour-neutral.

## Không làm (đã cân nhắc và loại)

- **"Async toàn tập" kiểu Midori** (cấm blocking toàn hệ): không có compiler barrier để enforce
  trong SAS, phá mọi cell hiện có, xung đột Law 8 (`Drop` không async được). Phase 07 chỉ làm
  reactor + concurrency *trong* cell.
- **Direct-call IPC qua vtable** (`docs/specs/03-runtime.md:8-12`): cần ADR riêng, đụng spec 17.
- **Whole-program LTO xuyên kernel+cells**: `lto = true` + `opt-level = "z"` đã bật
  (`Cargo.toml:165-167`); lever còn lại là feature-gate ostd, nằm trong phase 05.
- **Phương án A của phase 02** (VFS tra `GetProcs`/`GetProcs2` để map CellId → path):
  **rejected** — `ProcessInfoV2` không có path/cell_id (`libs/api/src/abi/syscall.rs:777-786`),
  và thứ gần nhất (`name`) dẫn xuất từ path_hint do caller truyền (`kernel/src/loader.rs:177`)
  nên forge được. Xem `## Red Team Review` → C2.

---

## Red Team Review

4 reviewer (Security Adversary · Assumption Destroyer · Failure Mode Analyst · Dependency Trap
Hunter), 31 finding → 13 sau dedupe + evidence filter. Mọi claim dưới đây đã được xác minh lại
trực tiếp trên code, không lấy nguyên từ báo cáo reviewer.

### Critical

| # | Finding | Xử lý |
|---|---------|-------|
| **C1** | Phase 03 req 5 bất khả thi hai lần: `REGION_MASK=0b111` (`policy.rs:36`) làm entry `0b1111` fail domain-validation (`:173`) → `parse`=None → `PolicyState::Invalid` → `DenyAll` **mọi** path (`:288`) → `CapSet::EMPTY` cho tất cả ngoài 3 cell trusted-core. Độc lập: init `CapSet::ALL` có `block_regions: 0b111` (`cap.rs:181`) và ceiling intersect chạy TRƯỚC policy (`loader.rs:272`→`:289`) nên fold `0b1000` bị zero bất kể blob. Kèm: `MMIO_MASK` thiếu `DEV_CAN`/`DEV_ADC` mà `from_manifest` vẫn mint (`cap.rs:223-228`). | **Accept — chuyển req 5 sang phase 04** (fold cần sửa init ceiling = việc của 04). Phase 04 gánh cả 3 bước widen mask + host-side parse self-test trước khi bake. |
| **C2** | Phase 02 "không cần ABI" không khả thi: `ProcessInfo`/`ProcessInfoV2` chỉ có `id`/`state`/`name` (`syscall.rs:769-786`); `name` = `path.rsplit('/').next()` từ path_hint **do caller truyền** (`loader.rs:177`) → SpawnCap holder gọi `sys_spawn_from_elf(elf, "/bin/vfs")` là con tên `vfs` → thừa hưởng ACL `/srv/`. Thêm: CellId==tid và init respawn service bằng tid mới → bảng static keyed CellId tự trỏ lại sau auto-restart. | **Accept — Law 1 Approach B thành mặc định**: kernel truyền cell_id kèm IPC. Gate 0 đổi thành **forgeability test**. Thêm edge cứng tới thread-cellid plan. Rule key theo **path**, không theo CellId. |
| **C3** | Phase 05 sizing sẽ ghi OOB: memset lấy size từ hằng `STACK_FRAMES` không từ stack đã cấp (`scheduler.rs:208-214`, `:278-285`) → cấp 16 page vẫn zero 64 page = 192 KiB ghi vào frame cell khác, guard page ở **đáy** không bắt được. Kèm: **4 stack/spawn**, 2 bị bỏ (`task.rs:572-575` → `scheduler.rs:197`/`:220` → `task.rs:590-592`) → peak ~1 MB không phải 520 KiB; **3** site `.expect` (`scheduler.rs:197`/`:220`/`:271`) không phải 2; `Scheduler::spawn` nhận `name` không nhận `path`, trả `usize` không trả `Result`. | **Accept — tách phase**. Phase 05 = phần an toàn (xoá cấp trùng, 3 `.expect`, binary). Phase 08 = sizing table, sau 07, với ràng buộc memset phải derive từ `Stack` đã cấp. |

### Major

| # | Finding | Xử lý |
|---|---------|-------|
| **M1** | Phase 04 broker thành spawn service ambient: `LookupService` mở cho mọi cell, `sys_send` không cần cap → mọi cell tới được broker; cổng thật hôm nay là `caller_has_spawn` (`syscall.rs:2092`). Mitigation "broker kiểm cap của caller" **không thoả được** — sau phase 04 shell cố tình không còn gpio/uart nên đòi caller giữ cap là từ chối mọi request hợp lệ. Kèm: service ID mới ở `libs/api` → có Law 1 gate; auto-restart là của init (`[_; NSVC=9]`, `init/src/main.rs:87-128`) không phải supervisor; `boot_authority()` = hợp cap các con ⊇ mọi con nên ceiling không bao giờ bind. | **Accept — giữ broker, sửa authorization model**: bảng đã ký (caller-identity → path được phép), broker nhận **index** vào allowlist cố định không nhận path tự do, chỉ phục vụ tid đã đăng ký. `boot_authority` thành bảng **per-path**. Mark Law 1 gate. Thêm `init/src/main.rs` (NSVC 9→10, chèn trước `/bin/shell`). |
| **M2** | POLICY.BIN **absent ở mọi embedded VIFS1** (verified: hit "POLICY" duy nhất trong `embedded/kernel_fs.img` là log string `" policy says no restart"`; aarch64/x86_64 zero hit) → nhánh `Absent`, dev-permissive. Nên bake của phase 03 là lần đầu, và `sign-policy.py:39` cho `/bin/shell` mmio=0 → `Permit ∩` zero gpio\|uart (`cap.rs:287`) → 3 peripheral demo vỡ trong dev build thường, `policy-required` còn tắt. Rồi criterion phase 04 đo trên baseline đã vỡ. | **Accept**: blob của phase 03 phải **behaviour-neutral** (giữ `/bin/shell` mmio=3). Việc hạ mmio của shell chuyển sang phase 04 cùng lần re-bake. Thêm "chạy 3 peripheral demo ngay sau bake" vào validation phase 03. |
| **M3** | Phase 03 `policy-required` ảnh hưởng ~14 cell không phải 3: `sign-policy.py:37-41` có 4 entry, init spawn ~14-20 path (`init/src/main.rs:88-98`, `:142-143`, `:164-165`, `:183`, `:232`, `:238`). `/bin/block` là Block Driver Cell phục vụ cell-store tại `/bin` — mất `block_io` là mọi spawn non-ramdisk fail. Và `/bin/platform` spawn `Spawner::Root` (`kernel/src/main.rs:682`) → **đã** miễn policy, liệt kê sai trong bản draft. | **Accept**: thay framing "3 cell" bằng **enumeration deliverable** — mọi path init spawn + mọi path match trong `with_path_caps`. Bỏ `/bin/platform` khỏi danh sách fail-closed. |
| **M4** | Phase 07 completion queue trên grant cell tự free được: `GrantUnregister`/`GrantFree` chỉ kiểm `owner == caller` rồi `free_grant_pages` (`syscall.rs:3445-3462`) → kernel/ISR ghi vào frame đã cấp lại. Pinning Registry req 1 chỉ phủ op buffer. Kèm lock-order: `waker.rs:9-10` nói caller trong sweep đã giữ SCHEDULER, `free_grant_pages` lấy FRAME_ALLOCATOR — thứ tự đã được ghi là đảo (`scheduler.rs:86-90`). Kèm: reaper chạy vô điều kiện khi exit/force-exit (`task.rs:334`, `:392`) + init auto-restart Permanent → cell panic giữa DMA là ca **thường**, reaper không biết pin. | **Accept**: CQ thành **kernel-owned memory tham chiếu từ TCB**, không phải grant thu hồi được; pin lifetime = cell lifetime; reap path tra pinning registry và **quarantine** frame đã pin; ghi lock-order của đường append vào ADR. Thêm edge tới cap-revocation plan. |
| **M5** | Phase 07 shim `block_on` phá 3 bất biến: (a) `ipc_try_send` chỉ giao khi target ở `TaskState::Recv{mask,buf_ptr,buf_len}` (`task.rs:1314-1326`) và đường input async của shell dựa đúng vào đó (`shell/src/async_utils.rs:36-44`) → park kiểu CQ = mọi TrySend drop âm thầm = bàn phím chết; (b) `exit_task` unblock peer bằng match `TaskState::Sending{target}` → CQ-park không match ⇒ caller treo vĩnh viễn thay vì nhận `usize::MAX`, supervisor treo ⇒ never-die không fire; (c) hai khối `unsafe` của VFS ghi vào grant caller với lý lẽ *"caller's ipc_call blocks so it cannot free the grant"* (`dispatch.rs:229-232`, `:214-215`) → future huỷ được = corruption âm thầm. | **Accept**: giữ rendezvous `Recv{buf_ptr}` HOẶC migrate `ipc_try_send` sang CQ trong **cùng step**; register waiter theo tid nó phụ thuộc + `exit_task` push synthetic error completion; **audit mọi `unsafe` có lý lẽ "caller blocks" TRƯỚC khi đổi executor**; ADR chốt cancel = chờ-rồi-bỏ. Thêm acceptance test: burst bàn phím tới shell 0 drop, 3 arch. |

### Minor

| # | Finding | Xử lý |
|---|---------|-------|
| **m1** | Ví dụ động lực của phase 01 bất khả thi: `BinOverlay` trả `false` cho `unlink`/`rmdir`/`rmdir_recursive` (`backend_bin_overlay.rs:82-90`) → `Unlink("/bin/shell")` đã fail `Err(1)`. Lỗ thật ở path gốc `/` (RamFS, `manager.rs:54`) nơi RamFS **có** hiện thực xoá và rule `/` `allow_write_all: false` không được tham chiếu. `MountEntry::writable` là `#[allow(dead_code)]` → authorization nhân đôi ở hai nơi. | **Accept**: hạ P0→P1, restate ví dụ, đổi negative test sang `/`, thêm bước enforce-hoặc-xoá `writable`. |
| **m2** | Phase 02 bỏ sót `Poll`, và **cả hai** bảng handle không so owner: `PendingTable` không có field owner, id tuần tự từ 1 (`pending.rs:24-43`); `HandleTable::get_mut` chỉ tra `cap.0` (`handle_table.rs:54-56`), `dispatch.rs:173` cũng không so → cell A quét id đọc data của cell B + DoS (`slots.remove` lấy luôn). | **Accept**: thêm `Poll` + `ReadGrant` + op close-class vào danh sách gate; key cả hai bảng theo `(sender, handle)` hoặc so `entry.owner`. Đây là **prerequisite của phase 06**, không phải "hạ tầng đã có". |
| **m3** | `Syscall::Spawn` (tạo thread) không qua `caller_has_spawn` → cell không đặc quyền loop spawn thread, mỗi thread đòi 65 frame liên tục → phân mảnh → panic tại site thứ 3 (`scheduler.rs:271`). Kèm: chỉ **một** guard page, không stack probe, và `unmap_page` fail thì code chạy tiếp **không guard**, chỉ log (`stack.rs:128-139`). | **Accept**: gộp vào phase 05 (3 site) + phase 08 (guard/probe requirement, unmap fail ⇒ spawn fail). Cap số thread/cell + charge stack vào cell quota. |
| **m4** | Claim quota-skew của phase 01 sai: `can_write` bỏ qua cell (`access.rs:80-87`) và `/data`,`/tmp`,`/mnt/sd`,`/srv` đều `allow_write_all: true` → sau phase 01 cell A vẫn xoá file của B và `release(A,size)` ghi vào sổ A (`quota.rs:60-64`) trong khi B vẫn bị charge. | **Accept**: xoá claim khỏi phase 01, ghi thành open finding (fix thật = release phải credit owner đã charge, thuộc phase 02 khi đã có danh tính). |
| **m5** | Phase 06 giữ op path-string song song mà không có step nào flip sang handle-only; `handle_request` không có per-cell mode (`dispatch.rs:25-240`) → "cell không thể diễn đạt path ngoài handle" sai với mọi cell suốt migration và không bao giờ thành đúng. Kèm: Req 4 (kế thừa thu-hẹp) cần lineage kernel không phơi (`Task` không có field parent; `SpawnFromPath` ABI chỉ có `path_ptr`/`path_len`). | **Accept**: thêm per-cell handle-only flag; success criterion = pioneer cell nhận `Err(3)` cho `Write{path}`; **xoá** (không phải deprecate) op path-string thành step cuối tường minh; thêm ADR item 4 (authenticate handle set lúc spawn). |

### Rejected

| Finding | Lý do |
|---------|-------|
| "POLICY.BIN đã bake sẵn nên gpio/uart của shell đã bị zero" (Assumption Destroyer M6) | Xác minh trực tiếp: không có FAT entry POLICY.BIN trong bất kỳ `kernel_fs*.img` embedded nào. Dòng roadmap "PolicyLoaded, 4 entries" nói về disk image test, không phải VIFS1 embedded đang boot. Bị M2 (ngược lại, và đúng) thay thế. |

### Deferred

| Finding | Lý do defer |
|---------|-------------|
| Mở `CAP_BYTES` cho 3 cap P-TRUST là net-negative khi `NoEntry` còn dev-permissive: sau phase 03, `with_path_caps` vẫn mint theo path (`cap.rs:259-266`), nên POLICY.BIN thiếu entry (typo, bake sót) → `NoEntry` → dev-permissive → cell **giữ** cap DMA-anywhere; trước đó ít nhất bị `Permit ∩` zero về false. | ~~Defer~~ → **Đã nhận vào plan (2026-07-30) thành [phase 09](phase-09-noentry-fail-closed.md)**, xếp sau 04. Ràng buộc giữ nguyên: chưa land 09 thì plan không claim "bít escape hatch". |

**Consistency**: OK — 8 phase file khớp bảng phase; edge `05→07→08` khớp "Thứ tự thực thi"; không còn tham chiếu tới `phase-05-footprint-stacks-binary.md` (đã tách thành 05 + 08).

## Closure

All original phases now satisfy the amended closure criteria. Phase 04 stays closed on launch-edge authority with init-respawn proof deferred; phase 07 stays closed on the verified substrate only, with generic reactor / RecvScatter / async VFS-DMA still deferred; phase 08 closes with the 16-page table and 64-page fallback preserved; phase 10 closes with QEMU runtime proof while the cross-hart W^X limitation remains documented.

Validation:
- Phase 04/07/08 closure status cross-checked against `.agents/260806-1026-midori-reactor-stack-closure/plan.md` and its `reports/phase-05-test-review.md`, `reports/phase-07-test-review.md`, and `reports/stack-sizing-evidence.md`.
- Phase 10 runtime closure cross-checked against `.agents/reports/qemu-build-unblock-260731.md` and `.agents/reports/HANDOFF-260731.md` §D7.

---

## Validation Log

### Verification Results

```
Claims checked: 6 | Verified: 4 | Failed: 2 | Unverified: 0
Tier: Full (8 phase) — thu hẹp theo skip-condition vì Red Team Review đã có evidence
```

**FAILED — V1: edge "blocking dependency" của phase 02 sai. Fix đã land, plan sibling stale.**
Red-team để lại một tranh chấp chưa xử: một reviewer nói thread-CellId(0) đã vá, một nói
`Syscall::Spawn` hardcode `CellId(0)` tại `kernel/src/task/syscall.rs:1153`. Xác minh:
`Syscall::Spawn` ở [syscall.rs:1415-1450](../../kernel/src/task/syscall.rs#L1415-L1450) **đã**
inherit `cell_id` của cell cha, kèm comment tường minh *"it must never fall back to CellId(0), which
is exactly the quota-escape this closes"* và fail-safe deny khi không resolve được caller. Nhưng
`.agents/260712-1903-thread-cellid-quota-fix/plan.md` now records
`done (kernel-side)` with its 2026-07-27 closure note. Phase 02 is therefore **not**
blocked on that sibling plan; the remaining VFS-side identity work is tracked here.

**FAILED — V2 (finding MỚI, cả 4 reviewer đều nói sai): VFS bịa `CellId` từ tid → quota escape qua
thread ở tầng VFS. LATENT, không phải live** — `sys_spawn` (thread) có trong
[ostd/syscall.rs:233](../../libs/ostd/src/syscall.rs#L233) nhưng **chưa cell nào gọi** (grep toàn
`cells/`), nên chưa khai thác được. Nó sẽ cắn cell đầu tiên dùng thread.
Ba dữ kiện: (a) loader đặt `cell_id = CellId(tid)` ([loader.rs:190](../../kernel/src/loader.rs#L190));
(b) thread inherit `cell_id` của **cha** nhưng nhận **tid riêng** (syscall.rs:1415-1450); (c) VFS dựng
`owner = types::CellId(sender as u64)` với `sender` là **tid**
([dispatch.rs:49](../../cells/services/vfs/src/dispatch.rs#L49), `:113`, `:124`).
⇒ Với cell do loader spawn, `CellId(tid) == cell_id` **tình cờ** đúng. Với **thread**, VFS bịa ra
`CellId(thread_tid)` — một identity không ứng với cell nào. Nên: quota của thread ghi vào một sổ ảo
thay vì sổ của cell cha (**escape kernel-side đã vá, VFS-side thì chưa, vì VFS không dùng `cell_id`
của kernel mà tự dẫn xuất từ tid**); và ACL của phase 02 với luật "unknown identity → deny" sẽ deny
sạch mọi traffic từ thread.
⇒ Đây chính là lý lẽ mạnh nhất cho quyết định Law 1 của phase 02: fix đúng là **VFS nhận `cell_id`
do kernel attest**, không phải dẫn xuất từ `sender`.

**VERIFIED — V3**: `/bin/platform` spawn `Spawner::Root` tại
[main.rs:682](../../kernel/src/main.rs#L682) → đã miễn policy. Bổ sung nuance chưa ghi: nó nằm sau
`#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]` → **trên ARM64 không spawn**, và
failure là non-fatal by design.

**VERIFIED — V4**: init spawn qua `sys_spawn_from_path` ([init/src/main.rs:142-165](../../cells/tools/init/src/main.rs#L142-L165))
→ `Spawner::User(init_tid)` → ceiling = cap của init. Xác nhận C1: fold `/bin/vfs` bị ceiling của init
zero trước khi policy chạy.

**VERIFIED — V5**: `.agents/260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md` và
`.agents/260712-1000-cell-package-distribution/phase-01-writable-cell-store.md` tồn tại → hai
cross-plan conflict trong Dependencies là thật, không phải suy đoán. (pkg plan còn có
`threat-model-bin-write-gate.md` — đọc trước khi làm phase 01/02.)

**VERIFIED (một phần) — V6**: `fontdue` string còn trong `service-vfs` (1 hit) và `app-shell` (1 hit),
`hello-cell` 0 hit → LTO **không** strip sạch khỏi một cell non-GUI. Bằng chứng yếu (1 string có thể
chỉ là panic location, không phải cả rasterizer) nên workstream B của phase 05 giữ nguyên "đo trước",
nhưng tín hiệu ban đầu là **có việc làm**, đừng giả định LTO đã dọn.

### Decisions

| # | Câu hỏi | Quyết định | Thay đổi plan |
|---|---------|-----------|---------------|
| D1 | V2 (VFS bịa CellId từ tid) fix ở đâu? | **Gộp vào phase 02** | Fix đúng chính là Law 1 change phase 02 đã xin: VFS nhận `cell_id` do kernel attest thay vì dẫn xuất từ `sender`. Một ABI change phục vụ cả ACL lẫn quota. An toàn vì chưa cell nào dùng thread → "unknown → deny" không phá gì hôm nay. |
| D2 | Bake POLICY.BIN có trong scope phase 03? | **Có — bake 1 image rồi lan** | Giữ nguyên plan: host-side parse self-test → bake riscv64 → boot + 3 peripheral demo → mới bake phần còn lại. Phase 03 giữ được tiêu chí thật (boot `policy-required`). |
| D3 | Plan sibling stale (`260712-1903-thread-cellid-quota-fix`) | **Đóng, ghi VFS-side còn mở** | Mark phần kernel done; thêm con trỏ sang plan này cho phần VFS-side (V2). Xoá edge "blocking dependency" khỏi Dependencies & gates của plan này. |
| D4 | Enumeration + POLICY.BIN per-arch hay union? | **Một blob union cho cả 3 arch** | Enumeration = hợp mọi path trên mọi arch; entry của path không tồn tại trên một arch là vô hại (không ai spawn nó). Một blob, một script sign, một thứ để review. Đánh đổi đã nhận: blob mang entry không dùng, và không biểu diễn được "cell này chỉ được tồn tại trên x86". |
