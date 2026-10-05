//! AHCI 1.3.1 — HBA reset, port bring-up, IDENTIFY DEVICE, data path.
//!
//! PCI binding is done by the caller; this module resets the HBA, starts one
//! SATA port with a device attached, completes a single polled IDENTIFY DEVICE,
//! and then serves 512-byte sector transfers over the command list built here:
//! READ DMA EXT (0x25) / WRITE DMA EXT (0x35) with a 48-bit LBA, and FLUSH CACHE
//! EXT (0xEA). Every transfer is one PRDT entry over a caller-supplied authorized
//! DMA IOVA, and the capacity is taken from the IDENTIFY response — a transfer
//! past it is rejected with a typed error, never clamped.
//!
//! The q35 ICH9 AHCI is register-compatible with PCH AHCI; port count, remap and
//! NCQ behaviour are not validated on the QEMU model (plan assumption A-02).
//!
//! Law 4 exception: Driver Cells may use `unsafe` for DMA memory access. MMIO
//! goes through the bounds-checked `ostd::mmio::MmioRegion`; every `unsafe`
//! block carries a `// SAFETY:` comment.

use crate::dma::AuthorizedDma;
use alloc::string::String;
use core::sync::atomic::{fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::io::print_fmt;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

// ── Generic host control (ABAR offsets) ───────────────────────────────────────

const REG_CAP: usize = 0x00;
const REG_GHC: usize = 0x04;
const REG_PI: usize = 0x0C;
const REG_VS: usize = 0x10;

const GHC_HR: u32 = 1 << 0;
const GHC_AE: u32 = 1 << 31;

// ── Port register block (base 0x100, stride 0x80) ─────────────────────────────

const PORT_BASE: usize = 0x100;
const PORT_STRIDE: usize = 0x80;

const PX_CLB: usize = 0x00;
const PX_CLBU: usize = 0x04;
const PX_FB: usize = 0x08;
const PX_FBU: usize = 0x0C;
const PX_IS: usize = 0x10;
const PX_IE: usize = 0x14;
const PX_CMD: usize = 0x18;
const PX_TFD: usize = 0x20;
const PX_SIG: usize = 0x24;
const PX_SSTS: usize = 0x28;
const PX_SCTL: usize = 0x2C;
const PX_SERR: usize = 0x30;
const PX_CI: usize = 0x38;

const CMD_ST: u32 = 1 << 0;
const CMD_SUD: u32 = 1 << 1;
const CMD_POD: u32 = 1 << 2;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;

const SSTS_DET_MASK: u32 = 0x0F;
const SSTS_DET_PRESENT: u32 = 0x03;
const SSTS_IPM_MASK: u32 = 0x0F_00;
const SSTS_IPM_ACTIVE: u32 = 0x01_00;

const TFD_ERR: u32 = 1 << 0;
/// Task file busy (BSY) — must be clear before PxCMD.ST is set.
const TFD_BSY: u32 = 1 << 7;
/// Task file data-request (DRQ) — must be clear before PxCMD.ST is set.
const TFD_DRQ: u32 = 1 << 3;

const IS_TFES: u32 = 1 << 30;
/// Fatal port conditions that are **not** task-file errors: host-bus fatal (29),
/// host-bus data error (28), interface fatal (27) and overflow (24). AHCI reports
/// these independently of `TFES`, and a command can clear `PxCI` while one is
/// latched — treating that as success would return stale DMA contents for a read
/// and acknowledge a write that never reached the device.
const IS_HBFS: u32 = 1 << 29;
const IS_HBDS: u32 = 1 << 28;
const IS_IFS: u32 = 1 << 27;
const IS_OFS: u32 = 1 << 24;
/// Every latched condition that makes a completed command untrustworthy.
const IS_FATAL: u32 = IS_TFES | IS_HBFS | IS_HBDS | IS_IFS | IS_OFS;

/// AHCI port signature for an ATA (LBA) disk — the only signature IDENTIFY
/// DEVICE is valid for.
const SIG_ATA: u32 = 0x0000_0101;

/// Every signature a device may publish in PxSIG once its initial D2H FIS has
/// landed (AHCI 1.3.1 §3.3.9): ATA, ATAPI, SEMB, port multiplier.
const SIG_KNOWN: [u32; 4] = [0x0000_0101, 0xEB14_0101, 0xC33C_0101, 0x9669_0101];

// ── Command / FIS constants (AHCI 1.3.1 §4.2) ─────────────────────────────────

/// Register Host-to-Device FIS type.
const FIS_TYPE_H2D: u8 = 0x27;
/// ATA IDENTIFY DEVICE command.
const ATA_CMD_IDENTIFY: u8 = 0xEC;
/// ATA READ DMA EXT — 48-bit LBA, DMA into a PRDT-listed buffer.
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
/// ATA WRITE DMA EXT — 48-bit LBA, DMA out of a PRDT-listed buffer.
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
/// ATA FLUSH CACHE EXT — 48-bit LBA, non-data; drains the device's write cache.
const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA;
/// H2D FIS length in DWORDs (20 bytes) for a command with no data payload in
/// the FIS itself.
const H2D_FIS_DWORDS: u32 = 5;
/// One PRDT entry for a single 512-byte sector response or payload.
const IDENTIFY_BYTES: u32 = 512;
/// Fixed 512-byte logical sector.
const SECTOR_BYTES: u32 = 512;
/// Command-header DWORD0 bit 6: the host-to-device (write) flag of a data
/// transfer. Bit 5 is the ATAPI flag (AHCI 1.3.1 §4.2.1), which is why this is
/// not `1 << 5`.
const CMD_HDR_W: u32 = 1 << 6;
/// ATA device register value selecting LBA addressing.
const ATA_DEVICE_LBA: u8 = 0x40;

// ── Poll budgets ──────────────────────────────────────────────────────────────

/// Bounded poll for register transitions that should complete in microseconds.
/// Every poll is an MMIO read (a VM exit under QEMU), so the budget is kept
/// small enough that a failing gate reports instead of blowing the boot window.
const POLL_LIMIT: u64 = 2_000_000;
/// Link-up after COMRESET can take longer than a register transition. Each poll
/// is an MMIO read (a VM exit under QEMU), so the budget stays modest.
const LINK_POLL_LIMIT: u64 = 200_000;
/// Yield every this many polls so a long wait does not starve the system.
const POLL_YIELD_EVERY: u64 = 8192;

pub struct AhciController {
    mmio: MmioRegion,
    port: u8,
    /// Set when a timeout or fatal port error could not be recovered by stopping
    /// and restarting the engine. Every later request is refused with a typed
    /// error: the single command slot's structures are shared with the next
    /// request, so reusing them while the HBA may still own tag 0 would let a
    /// retry race an in-flight DMA.
    faulted: bool,
    // The HBA fetches every one of these structures by DMA; all four are
    // retained for the controller's lifetime so the device-visible IOVAs stay
    // valid.
    cmd_list: AuthorizedDma<DmaBuf>,
    /// Retained for the controller's lifetime: the HBA writes received FISes
    /// here continuously, so freeing it would be a use-after-free. It is only
    /// read (via `iova()`) while the port is being started.
    _fis: AuthorizedDma<DmaBuf>,
    cmd_table: AuthorizedDma<DmaBuf>,
    identify: AuthorizedDma<DmaBuf>,
}

impl AhciController {
    /// Reset the HBA reachable via `mmio` (the AHCI ABAR), start the first port
    /// with an ATA device, and complete IDENTIFY DEVICE.
    ///
    /// Returns a typed error on any device error; never panics.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // The Platform Cell scans ECAM concurrently with this cell's spawn, and
        // its BAR size probe temporarily clears the PCI command register's
        // memory-decode bit. A register read inside that window returns 0, so
        // wait (bounded) for a sane CAP/PI before trusting any register — a
        // zero CAP would otherwise look like "HBA reset already complete" and
        // "no ports implemented".
        // AHCI 1.3.1 §10.1.2: while CAP.SAM is 0 the host must enable AHCI mode
        // (GHC.AE) before touching any register other than GHC. Best effort
        // here; re-asserted after the Platform Cell's BAR-probe window below.
        let ghc = rd(&mmio, REG_GHC)?;
        if ghc & GHC_AE == 0 && ghc != 0xFFFF_FFFF {
            wr(&mmio, REG_GHC, ghc | GHC_AE)?;
        }

        let (cap, version, pi) = {
            let mut attempt = 0u32;
            loop {
                let cap = rd(&mmio, REG_CAP)?;
                let version = rd(&mmio, REG_VS)?;
                let pi = rd(&mmio, REG_PI)?;
                if cap != 0 && cap != 0xFFFF_FFFF && pi != 0 {
                    break (cap, version, pi);
                }
                attempt += 1;
                if attempt >= 20_000 {
                    let _ = print_fmt(format_args!(
                        "[ahci] ABAR not readable: cap=0x{:08x} version=0x{:08x} pi=0x{:08x}\n",
                        cap, version, pi
                    ));
                    return Err(ViError::IO);
                }
                ostd::task::yield_now();
            }
        };
        // CAP.NP is bits 4:0 and holds "number of ports - 1".
        let n_ports = ((cap & 0x1F) + 1) as u8;
        let _ = print_fmt(format_args!(
            "[ahci] controller bound bdf={:02x}:{:02x}.{} cap=0x{:08x} version=0x{:08x} pi=0x{:08x}\n",
            (bdf >> 8) & 0xFF,
            (bdf >> 3) & 0x1F,
            bdf & 0x07,
            cap,
            version,
            pi
        ));

        // 1. HBA reset. GHC.HR self-clears when the reset completes, and it is
        // only defined while AE is set — writing HR alone would clear AHCI mode
        // first. Keep AE in the same write, then re-assert it after the reset.
        let ghc = rd(&mmio, REG_GHC)?;
        wr(&mmio, REG_GHC, ghc | GHC_AE | GHC_HR)?;
        let mut spins = 0u64;
        loop {
            if rd(&mmio, REG_GHC)? & GHC_HR == 0 {
                break;
            }
            spins = poll_tick(spins, POLL_LIMIT, "[ahci] HBA reset never completed")?;
        }
        wr(&mmio, REG_GHC, GHC_AE)?;
        let _ = print_fmt(format_args!("[ahci] HBA reset complete (AE enabled)\n"));

        // 2. Command list, FIS receive area, command table and IDENTIFY buffer.
        let cmd_list = alloc_dma(bdf, 1)?;
        let fis = alloc_dma(bdf, 1)?;
        let cmd_table = alloc_dma(bdf, 1)?;
        let identify = alloc_dma(bdf, 1)?;
        // DmaBuf is not guaranteed zeroed; the HBA reads the FIS receive area and
        // the command table, so both must start in a defined state.
        for buf in [&cmd_list, &fis, &cmd_table, &identify] {
            // SAFETY: each buffer is a live, page-sized DMA allocation owned by
            // this cell; write_bytes covers exactly its size.
            unsafe { core::ptr::write_bytes(buf.inner().virt(), 0, buf.inner().size()) };
        }

        // 3. Reset is done; start a port with a device and keep it running.
        let port = Self::start_port(&mmio, n_ports, pi, &cmd_list, &fis)?;

        let mut ctrl = AhciController {
            mmio,
            port,
            faulted: false,
            cmd_list,
            _fis: fis,
            cmd_table,
            identify,
        };
        ctrl.identify()?;
        // Word 0 is the ATA general-configuration word: 0x0040 means a fixed,
        // non-removable ATA device, which proves the 512-byte DMA landed (a
        // completed-but-empty transfer would leave this zero). The model field
        // is only set by QEMU when `model=` is passed, so it is reported but
        // not required.
        let w0 = dma_read_u16(&ctrl.identify, 0);
        let model = ctrl.model();
        let firmware = ata_string(&ctrl.identify, 23, 4);
        let _ = print_fmt(format_args!(
            "[ahci] IDENTIFY DEVICE ok port={} w0=0x{:04x} sectors={} model=\"{}\" fw=\"{}\"\n",
            ctrl.port,
            w0,
            ctrl.sectors(),
            model,
            firmware
        ));
        Ok(ctrl)
    }

    pub fn port(&self) -> u8 {
        self.port
    }

    /// Model string from the IDENTIFY DEVICE response (`""` if not run).
    pub fn model(&self) -> String {
        ata_string(&self.identify, 27, 20)
    }

    /// LBA28 sector count (IDENTIFY words 60:61, i.e. byte offsets 120/122).
    pub fn sectors(&self) -> u64 {
        let lo = dma_read_u16(&self.identify, 60 * 2) as u64;
        let hi = dma_read_u16(&self.identify, 61 * 2) as u64;
        lo | (hi << 16)
    }

    /// Device capacity in 512-byte sectors, from the IDENTIFY response.
    ///
    /// Prefers the 48-bit field (IDENTIFY words 100:103) when the device
    /// declares LBA48 support (word 83 bit 10); otherwise falls back to the
    /// LBA28 field. The LBA28 field is clamped at `0x0FFF_FFFF` on a large
    /// LBA48 device, so using it unconditionally would under-report a modern
    /// disk and make an in-range LBA look out of range.
    pub fn capacity_sectors(&self) -> u64 {
        if dma_read_u16(&self.identify, 83 * 2) & (1 << 10) != 0 {
            let mut cap = 0u64;
            for i in 0..4usize {
                cap |= (dma_read_u16(&self.identify, (100 + i) * 2) as u64) << (16 * i);
            }
            if cap != 0 {
                return cap;
            }
        }
        self.sectors()
    }
}

/// Split out so `new` can build the struct before the borrow-heavy IDENTIFY step.
impl AhciController {
    /// Program PxCLB/PxFB, enable the port, and issue IDENTIFY DEVICE.
    fn identify(&mut self) -> ViResult<()> {
        let p = self.port_offset();

        // Command header slot 0: CFL=5 DWORDs, PRDTL=1 entry.
        let ctba = self.cmd_table.iova();
        dma_write_u32(&self.cmd_list, 0, H2D_FIS_DWORDS | (1 << 16));
        dma_write_u32(&self.cmd_list, 4, 0);
        dma_write_u32(&self.cmd_list, 8, ctba as u32);
        dma_write_u32(&self.cmd_list, 12, (ctba >> 32) as u32);

        // Command table: H2D register FIS at offset 0 (C=1, command = IDENTIFY),
        // PRDT entry 0 at offset 128. The PRDT entry is `DBA(64) | reserved(32)
        // | DBC(32)` — DBC is the *last* dword (offset 12), not offset 8 (QEMU
        // `AHCI_SG`, Fuchsia `ahci_prd_t`), and holds byte-count-minus-one.
        dma_write_u8(&self.cmd_table, 0, FIS_TYPE_H2D);
        dma_write_u8(&self.cmd_table, 1, 0x80); // C bit: update the command register
        dma_write_u8(&self.cmd_table, 2, ATA_CMD_IDENTIFY);
        dma_write_u64(&self.cmd_table, 128, self.identify.iova());
        dma_write_u32(&self.cmd_table, 136, 0); // reserved
        dma_write_u32(&self.cmd_table, 140, IDENTIFY_BYTES - 1); // DBC = byte count - 1

        // Clear latched errors, then issue. The fence makes the command
        // structure writes visible before the HBA is told to fetch them.
        wr(&self.mmio, p + PX_SERR, 0xFFFF_FFFF)?;
        wr(&self.mmio, p + PX_IS, 0xFFFF_FFFF)?;
        fence(Ordering::SeqCst);
        wr(&self.mmio, p + PX_CI, 1)?;

        let mut spins = 0u64;
        loop {
            if rd(&self.mmio, p + PX_CI)? & 1 == 0 {
                break;
            }
            spins = poll_tick(spins, POLL_LIMIT, "[ahci] IDENTIFY never completed")?;
        }

        let is = rd(&self.mmio, p + PX_IS)?;
        let tfd = rd(&self.mmio, p + PX_TFD)?;
        if tfd & TFD_ERR != 0 || is & IS_FATAL != 0 {
            let _ = print_fmt(format_args!(
                "[ahci] IDENTIFY failed: PxIS=0x{:08x} PxTFD=0x{:08x}\n",
                is, tfd
            ));
            return Err(ViError::IO);
        }

        // A completed-but-empty transfer must not be reported as a successful
        // IDENTIFY: the command header's transferred byte count (PRDBC, command
        // header DWORD1 at byte offset 4 — AHCI 1.3.1 §4.2.1; QEMU's
        // `AHCICmdHdr.status` and Linux's `ahci_cmd_hdr.status`, at the same
        // offset) must equal the requested 512 and the payload's
        // general-configuration word must be a real value.
        let prdbc = dma_read_u32(&self.cmd_list, 4);
        let w0 = dma_read_u16(&self.identify, 0);
        if prdbc != IDENTIFY_BYTES || w0 == 0 || w0 == 0xFFFF {
            let _ = print_fmt(format_args!(
                "[ahci] IDENTIFY payload invalid: PRDBC={} w0=0x{:04x}\n",
                prdbc, w0
            ));
            return Err(ViError::IO);
        }
        Ok(())
    }

    fn port_offset(&self) -> usize {
        PORT_BASE + self.port as usize * PORT_STRIDE
    }

    // ── Data path (phase 02b) ─────────────────────────────────────────────────

    /// Read one 512-byte sector at the given 48-bit LBA into `iova`.
    ///
    /// `iova` must be the device-visible address of an authorized DMA buffer
    /// (one PRDT entry covers that single page).
    pub fn read_sector(&mut self, lba: u64, iova: u64) -> ViResult<()> {
        self.ensure_live()?;
        self.issue_dma(ATA_CMD_READ_DMA_EXT, lba, iova, false, "READ DMA EXT")
    }

    /// Write one 512-byte sector at the given 48-bit LBA from `iova`.
    pub fn write_sector(&mut self, lba: u64, iova: u64) -> ViResult<()> {
        self.ensure_live()?;
        self.issue_dma(ATA_CMD_WRITE_DMA_EXT, lba, iova, true, "WRITE DMA EXT")
    }

    /// Drain the device's write cache (FLUSH CACHE EXT, non-data).
    pub fn flush(&mut self) -> ViResult<()> {
        self.ensure_live()?;
        let p = self.port_offset();
        // Non-data command: PRDTL 0, and no PRDT entry programmed. The HBA only
        // reads the FIS words it is told about via CFL, so the stale PRDT left
        // by the previous data command is not fetched.
        dma_write_u32(&self.cmd_list, 0, H2D_FIS_DWORDS);
        dma_write_u32(&self.cmd_list, 4, 0);
        dma_write_u32(&self.cmd_list, 8, self.cmd_table.iova() as u32);
        dma_write_u32(&self.cmd_list, 12, (self.cmd_table.iova() >> 32) as u32);

        dma_write_u8(&self.cmd_table, 0, FIS_TYPE_H2D);
        dma_write_u8(&self.cmd_table, 1, 0x80); // C bit
        dma_write_u8(&self.cmd_table, 2, ATA_CMD_FLUSH_CACHE_EXT);
        dma_write_u8(&self.cmd_table, 7, ATA_DEVICE_LBA);
        self.issue_and_wait(p, 0, "FLUSH CACHE EXT")
    }

    /// Program command slot 0 for a one-sector READ/WRITE DMA EXT and poll it to
    /// completion.
    fn issue_dma(
        &mut self,
        cmd: u8,
        lba: u64,
        iova: u64,
        write: bool,
        what: &str,
    ) -> ViResult<()> {
        let p = self.port_offset();
        let tag = 0u8;

        // Reject a transfer beyond the IDENTIFY-reported capacity instead of
        // letting the device answer with an opaque ABRT and no address.
        let capacity = self.capacity_sectors();
        if lba >= capacity {
            let _ = print_fmt(format_args!(
                "[ahci] port={} tag={} {} rejected: LBA {} >= capacity {} sectors\n",
                self.port, tag, what, lba, capacity
            ));
            return Err(ViError::InvalidInput);
        }
        // Only 48 bits of LBA exist in the FIS; an LBA that needs more cannot be
        // expressed, so it is out of range by definition.
        if lba >> 48 != 0 {
            let _ = print_fmt(format_args!(
                "[ahci] port={} tag={} {} rejected: LBA {} exceeds 48 bits\n",
                self.port, tag, what, lba
            ));
            return Err(ViError::InvalidInput);
        }

        // Command header slot 0: CFL=5 DWORDs, direction bit, PRDTL=1 entry.
        let header = H2D_FIS_DWORDS | (if write { CMD_HDR_W } else { 0 }) | (1 << 16);
        dma_write_u32(&self.cmd_list, 0, header);
        dma_write_u32(&self.cmd_list, 4, 0); // PRDBC cleared before issue
        dma_write_u32(&self.cmd_list, 8, self.cmd_table.iova() as u32);
        dma_write_u32(&self.cmd_list, 12, (self.cmd_table.iova() >> 32) as u32);

        // H2D register FIS at offset 0 (C=1, 48-bit LBA fields), PRDT entry 0 at
        // offset 128 (DBA 64-bit, reserved 32-bit, DBC = byte count - 1).
        dma_write_u8(&self.cmd_table, 0, FIS_TYPE_H2D);
        dma_write_u8(&self.cmd_table, 1, 0x80); // C bit: update the command register
        dma_write_u8(&self.cmd_table, 2, cmd);
        dma_write_u8(&self.cmd_table, 3, 0); // features high
        dma_write_u8(&self.cmd_table, 4, (lba & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 5, ((lba >> 8) & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 6, ((lba >> 16) & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 7, ATA_DEVICE_LBA);
        dma_write_u8(&self.cmd_table, 8, ((lba >> 24) & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 9, ((lba >> 32) & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 10, ((lba >> 40) & 0xFF) as u8);
        dma_write_u8(&self.cmd_table, 11, 0); // features exp
        dma_write_u8(&self.cmd_table, 12, 1); // sector count low: one sector
        dma_write_u8(&self.cmd_table, 13, 0); // sector count high
        dma_write_u32(&self.cmd_table, 128, iova as u32);
        dma_write_u32(&self.cmd_table, 132, (iova >> 32) as u32);
        dma_write_u32(&self.cmd_table, 136, 0); // reserved
        dma_write_u32(&self.cmd_table, 140, SECTOR_BYTES - 1); // DBC = byte count - 1

        self.issue_and_wait(p, tag, what)
    }

    /// Clear latched port errors, kick PxCI bit `tag`, and poll it back to 0.
    ///
    /// Any device fault or timeout is a typed `ViError::IO` naming the port and
    /// tag; a stall is never reported as success.
    fn issue_and_wait(&mut self, p: usize, tag: u8, what: &str) -> ViResult<()> {
        wr(&self.mmio, p + PX_SERR, 0xFFFF_FFFF)?;
        wr(&self.mmio, p + PX_IS, 0xFFFF_FFFF)?;
        // The fence makes the command structure writes visible before the HBA is
        // told to fetch them.
        fence(Ordering::SeqCst);
        let bit = 1u32 << tag;
        wr(&self.mmio, p + PX_CI, bit)?;

        let mut spins = 0u64;
        loop {
            if rd(&self.mmio, p + PX_CI)? & bit == 0 {
                break;
            }
            match poll_tick(spins, POLL_LIMIT, "") {
                Ok(next) => spins = next,
                Err(_) => {
                    let _ = print_fmt(format_args!(
                        "[ahci] port={} tag={} {} timeout after {} polls\n",
                        self.port, tag, what, POLL_LIMIT
                    ));
                    // Retire the slot before returning: the caller (VFS) retries,
                    // and tag 0's structures are shared with that retry.
                    self.recover_after_fault(p);
                    return Err(ViError::IO);
                }
            }
        }

        let is = rd(&self.mmio, p + PX_IS)?;
        let tfd = rd(&self.mmio, p + PX_TFD)?;
        if tfd & TFD_ERR != 0 || is & IS_FATAL != 0 {
            let _ = print_fmt(format_args!(
                "[ahci] port={} tag={} {} device fault: PxIS=0x{:08x} PxTFD=0x{:08x}\n",
                self.port, tag, what, is, tfd
            ));
            self.recover_after_fault(p);
            return Err(ViError::IO);
        }
        fence(Ordering::Acquire);
        Ok(())
    }

    /// Refuse a request on a controller that could not be recovered.
    fn ensure_live(&self) -> ViResult<()> {
        if self.faulted {
            let _ = print_fmt(format_args!(
                "[ahci] port={} controller faulted; request refused\n",
                self.port
            ));
            return Err(ViError::IO);
        }
        Ok(())
    }

    /// Retire the outstanding slot after a timeout or a fatal port error.
    ///
    /// Single-slot design: tag 0's command structures are shared with the next
    /// request, so leaving the HBA owning the slot would let a caller's retry
    /// race a still-running DMA. Stop the engine (which retires the slot), clear
    /// the latched error bits and restart it; if the restart does not come back
    /// clean, poison the controller so nothing reuses those structures.
    fn recover_after_fault(&mut self, p: usize) {
        let _ = Self::stop_port_engine(&self.mmio, p);
        let _ = wr(&self.mmio, p + PX_SERR, 0xFFFF_FFFF);
        let _ = wr(&self.mmio, p + PX_IS, 0xFFFF_FFFF);
        let restarted = {
            let Self {
                mmio,
                cmd_list,
                _fis,
                ..
            } = &*self;
            Self::start_fis_receive(mmio, p, cmd_list, _fis).is_ok()
                && Self::wait_task_file_idle(mmio, p).is_ok()
                && Self::start_command_engine(mmio, p).is_ok()
        };
        if !restarted {
            self.faulted = true;
            let _ = print_fmt(format_args!(
                "[ahci] port={} controller poisoned after an unrecoverable fault\n",
                self.port
            ));
        }
    }

    /// Bring one implemented port up and return its index.
    ///
    /// PxSIG is only valid once the port is started (AHCI 1.3.1 §3.3.9: the
    /// signature is valid when PxCMD.ST and PxCMD.FRE are set — and QEMU
    /// returns 0xffff_ffff before that, including on q35), so the signature
    /// check happens *after* the port engine is running. A started port whose
    /// signature is not an ATA disk (the q35 ATAPI CD-ROM shares the
    /// controller) is stopped again and the scan continues.
    fn start_port(
        mmio: &MmioRegion,
        n_ports: u8,
        pi: u32,
        cmd_list: &AuthorizedDma<DmaBuf>,
        fis: &AuthorizedDma<DmaBuf>,
    ) -> ViResult<u8> {
        for port in 0..n_ports {
            if pi & (1u32 << port) == 0 {
                continue;
            }
            let p = PORT_BASE + port as usize * PORT_STRIDE;

            // Power the port and request spin-up before reading the link state;
            // the HBA only reports DET/IPM once the port is powered.
            let cmd = rd(mmio, p + PX_CMD)?;
            wr(mmio, p + PX_CMD, cmd | CMD_POD | CMD_SUD)?;

            let mut ssts = rd(mmio, p + PX_SSTS)?;
            if ssts & SSTS_DET_MASK != SSTS_DET_PRESENT {
                // DET=0 means no device on this port: nothing to reset, so do
                // not spend the (MMIO-exit-bound) link budget on it. DET=1 means
                // a device is detected but communication is not established, so
                // COMRESET it.
                if ssts & SSTS_DET_MASK == 0 {
                    continue;
                }
                let sctl = rd(mmio, p + PX_SCTL)?;
                wr(mmio, p + PX_SCTL, (sctl & !0xF) | 1)?;
                delay_ms();
                wr(mmio, p + PX_SCTL, sctl & !0xF)?;
                let mut spins = 0u64;
                loop {
                    ssts = rd(mmio, p + PX_SSTS)?;
                    if ssts & SSTS_DET_MASK == SSTS_DET_PRESENT
                        && ssts & SSTS_IPM_MASK == SSTS_IPM_ACTIVE
                    {
                        break;
                    }
                    match poll_tick(spins, LINK_POLL_LIMIT, "") {
                        Ok(next) => spins = next,
                        Err(_) => {
                            // An unresponsive port must not abort the scan for
                            // the ports after it.
                            let _ = print_fmt(format_args!(
                                "[ahci] port {} link did not come up (PxSSTS=0x{:08x})\n",
                                port, ssts
                            ));
                            break;
                        }
                    }
                }
            }
            if ssts & SSTS_DET_MASK != SSTS_DET_PRESENT
                || ssts & SSTS_IPM_MASK != SSTS_IPM_ACTIVE
            {
                continue;
            }

            Self::start_fis_receive(mmio, p, cmd_list, fis)?;
            Self::wait_task_file_idle(mmio, p)?;
            // AHCI 1.3.1 §3.3.9: PxSIG becomes valid once the device's initial
            // D2H FIS lands, which with FRE on can precede the command engine.
            // Prefer that order; QEMU's ICH9 model only publishes the signature
            // after PxCMD.ST, so fall back to the started-engine read.
            let sig = match Self::wait_device_signature(mmio, p) {
                Some(sig) => {
                    if sig != SIG_ATA {
                        let _ = print_fmt(format_args!(
                            "[ahci] port {} is not an ATA disk (SSTS=0x{:08x} SIG=0x{:08x}); skipping\n",
                            port, ssts, sig
                        ));
                        Self::stop_port_engine(mmio, p)?;
                        continue;
                    }
                    Self::start_command_engine(mmio, p)?;
                    sig
                }
                None => {
                    Self::start_command_engine(mmio, p)?;
                    let sig = rd(mmio, p + PX_SIG)?;
                    if sig != SIG_ATA {
                        let _ = print_fmt(format_args!(
                            "[ahci] port {} is not an ATA disk (SSTS=0x{:08x} SIG=0x{:08x}); skipping\n",
                            port, ssts, sig
                        ));
                        Self::stop_port_engine(mmio, p)?;
                        continue;
                    }
                    sig
                }
            };

            let _ = print_fmt(format_args!(
                "[ahci] port {} link up (SSTS=0x{:08x} SIG=0x{:08x})\n",
                port, ssts, sig
            ));
            return Ok(port);
        }

        Err(ViError::NotFound)
    }

    /// Program PxCLB/PxFB and start FIS receive (PxCMD.FRE), leaving the command
    /// engine stopped so the device's initial signature can be read first.
    fn start_fis_receive(
        mmio: &MmioRegion,
        p: usize,
        cmd_list: &AuthorizedDma<DmaBuf>,
        fis: &AuthorizedDma<DmaBuf>,
    ) -> ViResult<()> {
        Self::stop_port_engine(mmio, p)?;

        wr(mmio, p + PX_SERR, 0xFFFF_FFFF)?;
        wr(mmio, p + PX_CLB, cmd_list.iova() as u32)?;
        wr(mmio, p + PX_CLBU, (cmd_list.iova() >> 32) as u32)?;
        wr(mmio, p + PX_FB, fis.iova() as u32)?;
        wr(mmio, p + PX_FBU, (fis.iova() >> 32) as u32)?;

        let cmd = rd(mmio, p + PX_CMD)?;
        wr(mmio, p + PX_CMD, cmd | CMD_FRE | CMD_POD | CMD_SUD)?;
        let mut spins = 0u64;
        loop {
            if rd(mmio, p + PX_CMD)? & CMD_FR != 0 {
                break;
            }
            spins = poll_tick(spins, POLL_LIMIT, "[ahci] FIS receive never started")?;
        }
        Ok(())
    }

    /// Start the command engine (PxCMD.ST) and wait for CR.
    fn start_command_engine(mmio: &MmioRegion, p: usize) -> ViResult<()> {
        let cmd = rd(mmio, p + PX_CMD)?;
        wr(mmio, p + PX_CMD, cmd | CMD_ST)?;
        let mut spins = 0u64;
        loop {
            if rd(mmio, p + PX_CMD)? & CMD_CR != 0 {
                break;
            }
            spins = poll_tick(spins, POLL_LIMIT, "[ahci] command list never started")?;
        }

        // Polled completion path only: no interrupts.
        wr(mmio, p + PX_IE, 0)?;
        Ok(())
    }

    /// Poll PxTFD until BSY and DRQ are clear (bounded).
    ///
    /// AHCI 1.3.1 §10.3.1: software must not set PxCMD.ST while the task file
    /// reports the device busy or holding a data request.
    fn wait_task_file_idle(mmio: &MmioRegion, p: usize) -> ViResult<()> {
        let mut spins = 0u64;
        loop {
            let tfd = rd(mmio, p + PX_TFD)?;
            if tfd & (TFD_BSY | TFD_DRQ) == 0 {
                return Ok(());
            }
            spins = poll_tick(spins, LINK_POLL_LIMIT, "[ahci] task file never went idle")?;
        }
    }

    /// Poll PxSIG until a known device signature appears (bounded).
    ///
    /// Returns `None` when none appears before the budget expires — which is the
    /// QEMU ICH9 model's behaviour — so the caller reads the signature after
    /// starting the command engine instead of failing the port.
    fn wait_device_signature(mmio: &MmioRegion, p: usize) -> Option<u32> {
        let mut spins = 0u64;
        loop {
            let Ok(sig) = rd(mmio, p + PX_SIG) else {
                return None;
            };
            if SIG_KNOWN.contains(&sig) {
                return Some(sig);
            }
            match poll_tick(spins, LINK_POLL_LIMIT, "") {
                Ok(next) => spins = next,
                Err(_) => return None,
            }
        }
    }

    /// Clear PxCMD.ST then PxCMD.FRE, waiting for CR/FR to drop.
    fn stop_port_engine(mmio: &MmioRegion, p: usize) -> ViResult<()> {
        let cmd = rd(mmio, p + PX_CMD)?;
        if cmd & CMD_ST != 0 {
            wr(mmio, p + PX_CMD, cmd & !CMD_ST)?;
            let mut spins = 0u64;
            loop {
                if rd(mmio, p + PX_CMD)? & CMD_CR == 0 {
                    break;
                }
                spins = poll_tick(spins, POLL_LIMIT, "[ahci] port stop never completed")?;
            }
        }
        let cmd = rd(mmio, p + PX_CMD)?;
        if cmd & CMD_FRE != 0 {
            wr(mmio, p + PX_CMD, cmd & !CMD_FRE)?;
            let mut spins = 0u64;
            loop {
                if rd(mmio, p + PX_CMD)? & CMD_FR == 0 {
                    break;
                }
                spins = poll_tick(spins, POLL_LIMIT, "[ahci] FIS receive never stopped")?;
            }
        }
        Ok(())
    }
}

// ── Register access (bounds-checked by MmioRegion) ────────────────────────────

#[inline]
fn rd(mmio: &MmioRegion, off: usize) -> ViResult<u32> {
    mmio.read_u32(off)
}

#[inline]
fn wr(mmio: &MmioRegion, off: usize, val: u32) -> ViResult<()> {
    mmio.write_u32(off, val)
}

/// Count one poll iteration and enforce the budget.
#[inline]
fn poll_tick(mut spins: u64, limit: u64, what: &str) -> ViResult<u64> {
    spins += 1;
    if spins % POLL_YIELD_EVERY == 0 {
        ostd::task::yield_now();
    }
    if spins >= limit {
        if !what.is_empty() {
            let _ = print_fmt(format_args!("[ahci] timeout: {} ({} polls)\n", what, limit));
        }
        return Err(ViError::IO);
    }
    fence(Ordering::Acquire);
    Ok(spins)
}

/// Hold COMRESET for at least the AHCI-mandated ~1 ms.
///
/// Uses the monotonic millisecond clock when the platform exposes one (`None`
/// on a profile without a timer); an iteration count alone is not a time lower
/// bound, since its duration depends on CPU frequency, `PAUSE`, and scheduler
/// state — a fast physical CPU could deassert COMRESET too early.
fn delay_ms() {
    let start = ostd::syscall::sys_get_time_ms();
    loop {
        for _ in 0..16 {
            ostd::task::yield_now();
        }
        for _ in 0..100_000 {
            core::hint::spin_loop();
        }
        match (start, ostd::syscall::sys_get_time_ms()) {
            (Some(start), Some(now)) if now.saturating_sub(start) >= 1 => return,
            (Some(_), Some(_)) => continue,
            // No monotonic clock: the yield/spin pass above is the whole bound.
            _ => return,
        }
    }
}

// ── DMA access ────────────────────────────────────────────────────────────────

fn alloc_dma(bdf: u32, pages: usize) -> ViResult<AuthorizedDma<DmaBuf>> {
    let buf = DmaBuf::alloc(pages).ok_or(ViError::OutOfMemory)?;
    AuthorizedDma::authorize(buf, |b| b.authorize(bdf)).map_err(|_| ViError::PermissionDenied)
}

fn dma_write_u8(buf: &AuthorizedDma<DmaBuf>, off: usize, val: u8) {
    // SAFETY: `buf` is a live DMA allocation owned by this cell and `off` is
    // within it by construction (fixed command-table layout).
    unsafe { core::ptr::write_volatile(buf.inner().virt().add(off), val) };
}

fn dma_write_u32(buf: &AuthorizedDma<DmaBuf>, off: usize, val: u32) {
    // SAFETY: same contract as `dma_write_u8`; `off` is 4-byte aligned.
    unsafe {
        core::ptr::write_volatile(buf.inner().virt().add(off) as *mut u32, val);
    }
}

fn dma_write_u64(buf: &AuthorizedDma<DmaBuf>, off: usize, val: u64) {
    // SAFETY: same contract as `dma_write_u8`; `off` is 8-byte aligned.
    unsafe {
        core::ptr::write_volatile(buf.inner().virt().add(off) as *mut u64, val);
    }
}

fn dma_read_u16(buf: &AuthorizedDma<DmaBuf>, off: usize) -> u16 {
    // SAFETY: same contract as `dma_write_u8`; `off` is 2-byte aligned.
    unsafe { core::ptr::read_volatile(buf.inner().virt().add(off) as *const u16) }
}

fn dma_read_u32(buf: &AuthorizedDma<DmaBuf>, off: usize) -> u32 {
    // SAFETY: same contract as `dma_write_u8`; `off` is 4-byte aligned.
    unsafe { core::ptr::read_volatile(buf.inner().virt().add(off) as *const u32) }
}

/// Decode an ATA string field (`words` little-endian words starting at `word`).
///
/// ATA strings are byte-swapped inside each word, so the high byte holds the
/// earlier character; a NUL or non-ASCII byte is dropped so a malformed
/// response cannot produce garbage in the log.
fn ata_string(buf: &AuthorizedDma<DmaBuf>, word: usize, words: usize) -> String {
    let mut s = String::with_capacity(words * 2);
    for w in 0..words {
        let raw = dma_read_u16(buf, (word + w) * 2);
        for b in [(raw >> 8) as u8, (raw & 0xFF) as u8] {
            if (0x20..0x7F).contains(&b) {
                s.push(b as char);
            }
        }
    }
    let trimmed = s.trim();
    let mut out = String::with_capacity(trimmed.len());
    out.push_str(trimmed);
    out
}
