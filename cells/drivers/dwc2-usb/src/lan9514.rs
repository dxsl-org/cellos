//! SMSC/Microchip LAN9514 USB 2.0 10/100 Ethernet Driver.
#![allow(dead_code)]

use crate::usb_channel::UsbHostEngine;
use ostd::io::println;
use types::{ViError, ViResult};
// ── LAN95xx Register Offsets ──────────────────────────────────────────────────
//
// Byte offsets, and the same values U-Boot's `smsc95xx` driver writes on this
// board when it netboots it. This file previously used a second, invented map
// (`ID_REV = 0x50`, `HW_CFG = 0x74`) plus an indirect "MAC CSR" window that the
// chip does not have: every read landed on some other register, which is why
// the ID never matched and the MAC address never came back.

const ID_REV: u32 = 0x00;
const INT_STS: u32 = 0x08;
const TX_CFG: u32 = 0x10;
const HW_CFG: u32 = 0x14;
const PM_CTRL: u32 = 0x20;
const LED_GPIO_CFG: u32 = 0x24;
const GPIO_CFG: u32 = 0x28;
const AFC_CFG: u32 = 0x2C;
const E2P_CMD: u32 = 0x30;
const E2P_DATA: u32 = 0x34;
const BURST_CAP: u32 = 0x38;
const BULK_IN_DLY: u32 = 0x6C;
const MAC_CR: u32 = 0x100;
const ADDRH: u32 = 0x104;
const ADDRL: u32 = 0x108;
const MII_ADDR: u32 = 0x114;
const MII_DATA: u32 = 0x118;
const FLOW: u32 = 0x11C;
const VLAN1: u32 = 0x120;
const COE_CR: u32 = 0x130;

// Bit Constants
const HW_CFG_LRST: u32 = 0x0000_0008; // Lite reset (U-Boot resets with this, not SRST)
const HW_CFG_BIR: u32 = 0x0000_1000; // Bulk In Empty Response
const HW_CFG_MEF: u32 = 0x0000_0020;
const HW_CFG_BCE: u32 = 0x0000_0002;
const HW_CFG_RXDOFF: u32 = 0x0000_0600;
const PM_CTL_PHY_RST: u32 = 0x0000_0010;
const TX_CFG_ON: u32 = 0x0000_0004;
const MAC_CR_RXEN: u32 = 0x0000_0004;
const MAC_CR_TXEN: u32 = 0x0000_0008;
const MAC_CR_MCPAS: u32 = 0x0008_0000; // pass all multicast: the guest needs ND
const MAC_CR_PRMS: u32 = 0x0004_0000;
const MAC_CR_HPFILT: u32 = 0x0000_2000;
const MII_BUSY: u32 = 0x0000_0001;
const MII_WRITE: u32 = 0x0000_0002;
const AFC_CFG_DEFAULT: u32 = 0x00F8_30A1;
const DEFAULT_BULK_IN_DELAY: u32 = 0x0000_2000;
const BURST_CAP_HIGH_SPEED: u32 = 5; // 5 x 512-byte packets, U-Boot's turbo value
const LED_GPIO_CFG_LEDS: u32 = 0x0111_0000;
const PHY_ADDRESS: u32 = 1; // SMSC95XX_INTERNAL_PHY_ID

/// Control transfers one register access retries before giving up.
///
/// A transfer that did not complete is not a register that reads zero, and the
/// board's USB layer fails some of them (`[dwc2] control ... failed ... XACTERR`
/// on the LAN9514's own register reads). The first cut returned the untouched
/// buffer, so a failed read of `HW_CFG`, `MII_ADDR` or `MII_DATA` read as "reset
/// finished", "MII bus idle" and "PHY reports no link" — and the last one is what
/// made the serving loop print `PHY link down`/`PHY link up` pairs. Retrying a few
/// times costs one transfer and keeps a single bus hiccup from changing chip or
/// link state.
const REGISTER_ATTEMPTS: usize = 4;

pub struct Lan9514Device<'a> {
    engine: &'a UsbHostEngine<'a>,
    dev_addr: u8,
    mac: [u8; 6],
}

impl<'a> Lan9514Device<'a> {
    pub fn new(engine: &'a UsbHostEngine<'a>, dev_addr: u8) -> Self {
        Self {
            engine,
            dev_addr,
            mac: [0xb8, 0x27, 0xeb, 0x12, 0x34, 0x56], // Default RPi OUI fallback
        }
    }

    /// Read a 32-bit hardware register from the LAN9514 over USB control transfer.
    /// Read a register and report how many bytes the data phase actually moved.
    ///
    /// `control_transfer` returns the byte count and every caller here drops
    /// it, so a short or empty data phase is indistinguishable from a register
    /// that reads zero. The probe needs the difference.
    pub fn read_reg_ex(&self, reg: u32) -> (u32, usize) {
        let mut buf = [0u8; 4];
        let moved = self
            .engine
            .control_transfer(self.dev_addr, 0xC0, 0xA1, 0, reg as u16, &mut buf)
            .unwrap_or(0);
        (u32::from_le_bytes(buf), moved)
    }

    /// Write a register and report whether the transfer completed.
    pub fn write_reg_result(&self, reg: u32, val: u32) -> bool {
        let mut buf = val.to_le_bytes();
        self.engine
            .control_transfer(self.dev_addr, 0x40, 0xA0, 0, reg as u16, &mut buf)
            .is_ok()
    }

    pub fn read_reg(&self, reg: u32) -> u32 {
        let mut buf = [0u8; 4];
        // U-Boot's smsc95xx (the driver that netboots this board) uses vendor
        // request 0xA1 to read and 0xA0 to write, with the register offset in
        // wIndex and wValue zero. Both halves matter: the original code had the
        // right requests but put the offset in wValue, and a later "fix" here
        // swapped the requests instead, which made every read a write-direction
        // request and returned one meaningless value for every offset.
        let _ = self.engine.control_transfer(
            self.dev_addr,
            0xC0, // Vendor IN
            0xA1, // Read Register
            0,
            reg as u16,
            &mut buf,
        );
        u32::from_le_bytes(buf)
    }

    /// Write a 32-bit hardware register on the LAN9514 over USB control transfer.
    pub fn write_reg(&self, reg: u32, val: u32) {
        let mut buf = val.to_le_bytes();
        let _ = self.engine.control_transfer(
            self.dev_addr,
            0x40, // Vendor OUT
            0xA0, // Write Register
            0,
            reg as u16,
            &mut buf,
        );
    }

    /// Read a register, retrying a control transfer that did not complete.
    ///
    /// `None` means the bus refused `REGISTER_ATTEMPTS` transfers in a row, not
    /// that the register holds zero: a failed read leaves the buffer as it was,
    /// and every poll in this file asks about state that does not change on its
    /// own (reset completion, MII idle, link), where a zero is a plausible-looking
    /// wrong answer. The board's link poll read exactly that.
    fn read_reg_retry(&self, reg: u32) -> Option<u32> {
        for _ in 0..REGISTER_ATTEMPTS {
            let (value, moved) = self.read_reg_ex(reg);
            if moved == 4 {
                return Some(value);
            }
            ostd::syscall::sys_yield();
        }
        None
    }

    /// Read a 16-bit register from the internal MII PHY (PHY address 1).
    ///
    /// `None` when any phase of the access failed: see [`Self::read_reg_retry`]
    /// for why that must not become `BMSR = 0`.
    pub fn read_phy_reg(&self, reg: u8) -> Option<u16> {
        if !self.wait_mii_idle() {
            return None;
        }
        let cmd = (PHY_ADDRESS << 11) | ((reg as u32) << 6) | MII_BUSY;
        if !self.write_reg_result(MII_ADDR, cmd) {
            return None;
        }
        if !self.wait_mii_idle() {
            return None;
        }
        self.read_reg_retry(MII_DATA)
            .map(|value| (value & 0xFFFF) as u16)
    }

    /// Write a 16-bit register to the internal MII PHY (PHY address 1).
    ///
    /// Returns whether every phase completed. A write that did not leaves the PHY
    /// exactly as it was — the one that restarts auto-negotiation is the one that
    /// matters — and the caller reports that instead of assuming it landed.
    pub fn write_phy_reg(&self, reg: u8, val: u16) -> bool {
        if !self.wait_mii_idle() {
            return false;
        }
        if !self.write_reg_result(MII_DATA, val as u32) {
            return false;
        }
        let cmd = (PHY_ADDRESS << 11) | ((reg as u32) << 6) | MII_WRITE | MII_BUSY;
        if !self.write_reg_result(MII_ADDR, cmd) {
            return false;
        }
        self.wait_mii_idle()
    }

    /// Wait (bounded) for the MII interface to report idle.
    ///
    /// A transfer that failed is not an idle interface: [`Self::read_reg_retry`]
    /// reports `None` and the access is abandoned rather than the MII bus being
    /// read as free.
    fn wait_mii_idle(&self) -> bool {
        for _ in 0..100 {
            match self.read_reg_retry(MII_ADDR) {
                Some(value) if value & MII_BUSY == 0 => return true,
                Some(_) => {}
                None => return false,
            }
            ostd::syscall::sys_yield();
        }
        false
    }

    /// Initialize the LAN9514 Ethernet controller.
    /// Bring the chip up the way U-Boot's `smsc95xx` does on this board.
    ///
    /// The sequence is deliberately the reference driver's: lite reset, PHY
    /// reset, MAC address, burst/bulk configuration, `HW_CFG` flags, interrupt
    /// acknowledge, LED configuration, flow control, MAC control, VLAN tag and
    /// the checksum-offload engines, then the TX and RX paths and a bounded wait
    /// for link. Anything else is guesswork against a chip that already works
    /// under the firmware that netboots this board.
    pub fn init(&mut self) -> ViResult<()> {
        // Read-only snapshot of what the *firmware* left in the chip, taken before
        // any write of ours (`loopback-diag` images).
        //
        // The board netboots over this same Ethernet — so under U-Boot the wire,
        // the PHY and the chip all work — and the loopback self-test shows the
        // chip's own transmit and receive paths work under Cellos too, yet nothing
        // arrives from the wire once we have initialised it. The registers below
        // are what U-Boot left behind: comparing them with the read-backs after
        // our init names whatever this driver changes out from under a working
        // link. Nothing here writes.
        #[cfg(feature = "loopback-diag")]
        {
            // `print` + `print_hex_val`, not `format!`: this crate's lib target has
            // no allocator wiring, and every line here is a handful of registers.
            ostd::io::print("[lan9514] pre-init: HW_CFG=0x");
            crate::usb_channel::print_hex_val(self.read_reg(HW_CFG));
            ostd::io::print(" MAC_CR=0x");
            crate::usb_channel::print_hex_val(self.read_reg(MAC_CR));
            ostd::io::print(" PM_CTL=0x");
            crate::usb_channel::print_hex_val(self.read_reg(PM_CTRL));
            ostd::io::print(" ADDRL=0x");
            crate::usb_channel::print_hex_val(self.read_reg(ADDRL));
            ostd::io::print(" ADDRH=0x");
            crate::usb_channel::print_hex_val(self.read_reg(ADDRH));
            ostd::io::println("");
            ostd::io::print("[lan9514] pre-init phy: BMCR=0x");
            crate::usb_channel::print_hex_val(self.read_phy_reg(0).unwrap_or(0) as u32);
            ostd::io::print(" BMSR=0x");
            crate::usb_channel::print_hex_val(self.read_phy_reg(1).unwrap_or(0) as u32);
            ostd::io::print(" ANAR=0x");
            crate::usb_channel::print_hex_val(self.read_phy_reg(4).unwrap_or(0) as u32);
            ostd::io::print(" ANLPAR=0x");
            crate::usb_channel::print_hex_val(self.read_phy_reg(5).unwrap_or(0) as u32);
            ostd::io::print(" PHYSCS=0x");
            crate::usb_channel::print_hex_val(self.read_phy_reg(31).unwrap_or(0) as u32);
            ostd::io::println("");
        }

        // 1. Lite reset, then PHY reset. Both come first, as in U-Boot: reading
        //    registers before the lite reset returns values the chip has not
        //    settled into (the board read `ID_REV = 0xEC000002` there, which is
        //    the USB function's own idProduct/bcdDevice pair, not a chip ID).
        self.write_reg(HW_CFG, HW_CFG_LRST);
        let mut count = 0;
        loop {
            // A read that did not complete is not "reset finished" (see
            // `read_reg_retry`); give up rather than configure a chip mid-reset.
            match self.read_reg_retry(HW_CFG) {
                Some(value) if value & HW_CFG_LRST == 0 => break,
                Some(_) => {}
                None => {
                    println("[lan9514] WARN: lite reset status unreadable");
                    return Err(ViError::IO);
                }
            }
            count += 1;
            if count > 200 {
                println("[lan9514] WARN: lite reset did not complete");
                return Err(ViError::IO);
            }
            ostd::syscall::sys_yield();
        }
        self.write_reg(PM_CTRL, PM_CTL_PHY_RST);
        count = 0;
        loop {
            match self.read_reg_retry(PM_CTRL) {
                Some(value) if value & PM_CTL_PHY_RST == 0 => break,
                Some(_) => {}
                None => {
                    println("[lan9514] WARN: PHY reset status unreadable");
                    return Err(ViError::IO);
                }
            }
            count += 1;
            if count > 200 {
                println("[lan9514] WARN: PHY reset did not complete");
                return Err(ViError::IO);
            }
            ostd::syscall::sys_yield();
        }

        // 2. What the chip says about itself, now that it is out of reset. Logged
        //    rather than enforced: U-Boot's driver does not check this value
        //    either, and a chip that answers its register file is usable whatever
        //    its ID revision reads.
        let (id, id_bytes) = self.read_reg_ex(ID_REV);
        ostd::io::print("[lan9514] ID_REV=0x");
        crate::usb_channel::print_hex_val(id);
        ostd::io::print(" bytes=0x");
        crate::usb_channel::print_hex_val(id_bytes as u32);
        let chip = id >> 16;
        // Linux's `smsc95xx` names this register's chip ids: `9500` for the
        // LAN9500 family and `EC00` for the LAN9512/9514 the Pi carries (the two
        // share the value), so `EC00` is the recognised case here. The board reads
        // `0xEC000002`; a check that only accepted a literal `0x9514` called a
        // working LAN9514 unrecognized on every boot.
        if chip == 0xEC00 {
            println(" (SMSC/Microchip LAN9512/9514)");
        } else if chip == 0x9500 {
            println(" (SMSC/Microchip LAN9500)");
        } else {
            println(" (unrecognized revision, continuing)");
        }

        // 3. Prove the register file before trusting anything written below: a
        //    known value written to a spare register must come back. `VLAN1`
        //    holds a 16-bit tag, so the comparison is on the low half - the
        //    first cut compared 32 bits and rejected a chip that was answering.
        let scratch = VLAN1;
        let wrote = self.write_reg_result(scratch, 0x5A5A_A5A5);
        let (back, back_bytes) = self.read_reg_ex(scratch);
        ostd::io::print("[lan9514] scratch write=0x");
        crate::usb_channel::print_hex_val(wrote as u32);
        ostd::io::print(" read=0x");
        crate::usb_channel::print_hex_val(back);
        ostd::io::print(" bytes=0x");
        crate::usb_channel::print_hex_val(back_bytes as u32);
        println("");
        if !wrote || back & 0xFFFF != 0xA5A5 {
            println("[lan9514] WARN: register file does not read back — leaving it unconfigured");
            return Err(ViError::IO);
        }

        // 4. MAC address: the firmware/U-Boot programs it from OTP, so read it
        //    and keep it when it is a plausible unicast address. A bare-metal
        //    boot without that leaves the registers unprogrammed, and a chip with
        //    no address receives nothing, so fall back to a locally administered
        //    one and say so.
        let addrl = self.read_reg(ADDRL);
        let addrh = self.read_reg(ADDRH);
        ostd::io::print("[lan9514] ADDRL=0x");
        crate::usb_channel::print_hex_val(addrl);
        ostd::io::print(" ADDRH=0x");
        crate::usb_channel::print_hex_val(addrh);
        println("");
        let read_mac = [
            (addrl & 0xFF) as u8,
            ((addrl >> 8) & 0xFF) as u8,
            ((addrl >> 16) & 0xFF) as u8,
            ((addrl >> 24) & 0xFF) as u8,
            (addrh & 0xFF) as u8,
            ((addrh >> 8) & 0xFF) as u8,
        ];
        let plausible = read_mac != [0; 6]
            && read_mac != [0xFF; 6]
            && read_mac[0] & 0x01 == 0; // unicast
        if plausible {
            self.mac = read_mac;
        } else {
            self.mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
            println("[lan9514] WARN: no MAC in the chip — programming a local one");
        }
        let lo = u32::from_le_bytes([self.mac[0], self.mac[1], self.mac[2], self.mac[3]]);
        let hi = u16::from_le_bytes([self.mac[4], self.mac[5]]) as u32;
        self.write_reg(ADDRL, lo);
        self.write_reg(ADDRH, hi);

        // 5. Buffers and hardware configuration.
        self.write_reg(BURST_CAP, BURST_CAP_HIGH_SPEED);
        self.write_reg(BULK_IN_DLY, DEFAULT_BULK_IN_DELAY);
        let mut hw_cfg = self.read_reg(HW_CFG);
        hw_cfg |= HW_CFG_BIR | HW_CFG_MEF | HW_CFG_BCE;
        hw_cfg &= !HW_CFG_RXDOFF;
        self.write_reg(HW_CFG, hw_cfg);
        self.write_reg(INT_STS, 0xFFFF_FFFF);
        self.write_reg(LED_GPIO_CFG, LED_GPIO_CFG_LEDS);

        // 6. Flow control, MAC control, VLAN tag, checksum offload off.
        self.write_reg(FLOW, 0);
        self.write_reg(AFC_CFG, AFC_CFG_DEFAULT);
        let mut mac_cr = self.read_reg(MAC_CR);
        mac_cr &= !MAC_CR_HPFILT;
        // Pass *every* unicast, not only the chip's own address. This front-end
        // bridges two stacks that each hold their own MAC — the net service's
        // smoltcp interface and the guest's virtio MAC (`GUEST_MAC`) — while the
        // chip's perfect-match filter holds exactly one address (its own, and a
        // bare-metal boot leaves it unprogrammed so the driver writes a local
        // one). With `PRMS` clear the chip drops a reply addressed to either
        // stack: the board showed the host stack never receiving a DHCP offer
        // (`[net] DHCP: deconfigured`), the guest's ping drawing no reply,
        // `guest_q=0` in every `[net-loop]` sample, and `first e1000 RX len=64`
        // (a multicast frame) as the only inbound witness. Multicast stays
        // passed for the guest's IPv6 neighbour discovery. Which stack *sees* a
        // frame is still decided per frame by the splitter's destination-MAC
        // routing, so this widens the chip's filter, not the guests' view.
        mac_cr |= MAC_CR_PRMS | MAC_CR_MCPAS;
        self.write_reg(MAC_CR, mac_cr);
        self.write_reg(VLAN1, 0x0000_8100);
        self.write_reg(COE_CR, 0);

        // 7. PHY: soft reset, advertise 10/100 full+half, restart auto-negotiation.
        //    A PHY write that does not complete leaves the PHY as it was, which the
        //    link measurement below reports as a link that never comes up — say so
        //    once instead of letting the link line carry the blame.
        let mut phy_wrote = self.write_phy_reg(0, 0x8000);
        for _ in 0..100 {
            if self.read_phy_reg(0).is_some_and(|bmcr| bmcr & 0x8000 == 0) {
                break;
            }
            ostd::syscall::sys_yield();
        }
        phy_wrote &= self.write_phy_reg(4, 0x01E1); // ANAR
        phy_wrote &= self.write_phy_reg(0, 0x1200); // BMCR: enable + restart AN
        if !phy_wrote {
            println("[lan9514] WARN: a PHY register write did not complete");
        }

        // 8. Start the TX and RX paths.
        self.write_reg(MAC_CR, mac_cr | MAC_CR_TXEN);
        self.write_reg(TX_CFG, TX_CFG_ON);
        self.write_reg(MAC_CR, mac_cr | MAC_CR_TXEN | MAC_CR_RXEN);

        // 9. Link: report what the PHY says. "No cable" and "chip never
        //    configured" used to look identical from the outside.
        let mut bmsr = 0u16;
        // The chip is a USB device behind the hub: the port reset that enumerated
        // it restarts PHY auto-negotiation, which takes seconds. The board read the
        // link inside a ~2 s window twice and concluded "cable" both times while
        // the PHY was merely still negotiating — and nothing ever looked again, so
        // a link that came up later was never used. Wait longer here, and the
        // serving loop keeps checking on its own cadence.
        for _ in 0..600 {
            // A failed access says nothing about the link. The serving loop keeps
            // polling (`link_bmsr`), so this is only the measurement the bring-up
            // line reports.
            if let Some(value) = self.read_phy_reg(1) {
                bmsr = value;
                if value & 0x0004 != 0 {
                    break;
                }
            }
            ostd::syscall::sys_yield();
        }
        if bmsr & 0x0004 != 0 {
            ostd::io::print("[lan9514] PHY link up (BMSR=0x");
        } else {
            // Not seen as a cable fault: an incomplete negotiation is expected
            // right after the port reset, and the serving loop reports the
            // transition when it completes.
            ostd::io::print("[lan9514] PHY link not up yet (BMSR=0x");
        }
        crate::usb_channel::print_hex_val(bmsr as u32);
        if bmsr & 0x0020 != 0 {
            println("), auto-negotiation complete");
        } else {
            println("), auto-negotiation incomplete");
        }
        // Confirm the configuration latched instead of assuming a write that
        // returned is a write the chip kept.
        let mac_cr_back = self.read_reg(MAC_CR);
        let tx_cfg_back = self.read_reg(TX_CFG);
        let hw_cfg_back = self.read_reg(HW_CFG);
        ostd::io::print("[lan9514] read-back MAC_CR=0x");
        crate::usb_channel::print_hex_val(mac_cr_back);
        ostd::io::print(" TX_CFG=0x");
        crate::usb_channel::print_hex_val(tx_cfg_back);
        ostd::io::print(" HW_CFG=0x");
        crate::usb_channel::print_hex_val(hw_cfg_back);
        println("");
        ostd::io::print("[lan9514] chip MAC ");
        print_mac(&self.mac);
        println("");
        Ok(())
    }

    /// Return the active 6-byte MAC address.
    /// PHY-loopback self-test: transmit one frame and see whether the chip reads
    /// it back.
    ///
    /// The board's receive direction delivered nothing at all while the netboot
    /// proves the wire and the PHY work under U-Boot, so the question left is
    /// inside the chip. With `BMCR.LOOPBACK` set the PHY hands a transmitted frame
    /// straight to the receive path, so a frame that comes back proves MAC TX +
    /// MAC RX + RX FIFO + bulk-IN all work and points the fault at the cable or
    /// the peer; a frame that does not come back points it at the receive
    /// configuration or at the receive transfer itself. The PHY is restored before
    /// returning, so a diagnostic image still serves traffic afterwards.
    #[cfg(feature = "loopback-diag")]
    pub fn loopback_self_test(&self) -> (bool, usize) {
        /// `BMCR` bit 14: loop the PHY's transmit back into its receive.
        const BMCR_LOOPBACK: u16 = 1 << 14;

        let Some(bmcr) = self.read_phy_reg(0) else {
            println("[lan9514] loopback diag: PHY unreadable, test skipped");
            return (false, 0);
        };
        let _ = self.write_phy_reg(0, bmcr | BMCR_LOOPBACK);
        for _ in 0..20 {
            if self.read_phy_reg(0).is_some_and(|value| value & BMCR_LOOPBACK != 0) {
                break;
            }
            ostd::syscall::sys_yield();
        }

        // Broadcast destination, this chip's address as source, and a locally
        // assigned EtherType so nothing upstream acts on it if it ever escapes.
        let mut frame = [0u8; 60];
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&self.mac);
        frame[12..14].copy_from_slice(&[0x88, 0xb5]);
        for (i, byte) in frame[14..].iter_mut().enumerate() {
            *byte = i as u8;
        }

        let sent = self.send_frame(&frame);
        let mut received = 0usize;
        let mut buf = [0u8; 1600];
        for _ in 0..50 {
            let n = self.recv_frame(&mut buf);
            if n > 0 {
                received = n;
                break;
            }
            ostd::syscall::sys_yield();
        }

        let _ = self.write_phy_reg(0, bmcr);
        (sent, received)
    }

    /// Raw `BMSR`, so the serving loop can print the state a transition was read
    /// from — and can tell a failed access from a link that went down.
    ///
    /// `None` when the access failed (see [`Self::read_reg_retry`]): a zero read
    /// from a transfer that never completed is not `BMSR`, and treating it as one
    /// is what made the board print `PHY link down`/`PHY link up` pairs.
    pub fn link_bmsr(&self) -> Option<u16> {
        self.read_phy_reg(1)
    }

    /// Re-enable the MAC's transmit and receive paths for a link that came up.
    ///
    /// Idempotent: `init` already sets these, and this only matters for a link
    /// that was not up when it ran.
    pub fn enable_data_path(&self) {
        let mac_cr = self.read_reg(MAC_CR);
        self.write_reg(MAC_CR, mac_cr | MAC_CR_TXEN | MAC_CR_RXEN);
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    /// Send a raw Ethernet frame over Bulk OUT EP 2.
    pub fn send_frame(&self, frame: &[u8]) -> bool {
        // The first failure is printed once: `accepted=false` in the net bridge
        // cannot say whether the transfer failed or the chip refused, and the
        // retry loop would flood the console either way.
        if frame.is_empty() || frame.len() > 1514 {
            return false;
        }

        // LAN95xx TX Command: 8-byte header prepended to frame
        // Word 0: (1 << 13) [First Segment] | (1 << 12) [Last Segment] | (length & 0x7FF)
        let tx_cmd_a = (1u32 << 13) | (1u32 << 12) | (frame.len() as u32);
        // Word 1: length
        let tx_cmd_b = frame.len() as u32;

        let mut packet = [0u8; 8 + 1514];
        let w0 = tx_cmd_a.to_le_bytes();
        let w1 = tx_cmd_b.to_le_bytes();
        packet[..4].copy_from_slice(&w0);
        packet[4..8].copy_from_slice(&w1);
        packet[8..8 + frame.len()].copy_from_slice(frame);

        let total = 8 + frame.len();
        let result = self.engine.bulk_transmit(self.dev_addr, 2, &packet[..total]);
        if let Err(error) = result {
            static FIRST_TX_ERROR: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !FIRST_TX_ERROR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                // The cause, not just "IO": a device `STALL` (an endpoint the host
                // must clear), a bus `XACTERR` and a channel that never reported are
                // three different faults, and the board's console named none of them.
                // The port state goes with it, because a port the core disabled
                // fails every transfer with no channel status at all.
                ostd::io::print("[lan9514] first bulk-OUT failure: ");
                ostd::io::print(self.engine.failure_name(&error));
                if !self.engine.port_enabled() {
                    ostd::io::print(" (the root port is DISABLED, PRTENA=0)");
                }
                ostd::io::println("");
            }
            return false;
        }
        true
    }

    /// Receive a raw Ethernet frame from Bulk IN EP 1.
    pub fn recv_frame(&self, out: &mut [u8]) -> usize {
        let mut raw = [0u8; 4 + 1518];
        let n = match self.engine.bulk_receive(self.dev_addr, 1, &mut raw) {
            Ok(bytes) => bytes,
            Err(error) => {
                static FIRST_RX_ERROR: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !FIRST_RX_ERROR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    ostd::io::print("[lan9514] first bulk-IN failure: ");
                    ostd::io::print(self.engine.failure_name(&error));
                    if !self.engine.port_enabled() {
                        ostd::io::print(" (the root port is DISABLED, PRTENA=0)");
                    }
                    ostd::io::println("");
                }
                return 0;
            }
        };

        if n < 4 {
            return 0;
        }

        // LAN95xx RX Status: 4-byte header
        let status = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let frame_len = ((status >> 16) & 0x3FFF) as usize;

        if frame_len == 0 || frame_len > 1514 || (4 + frame_len) > n {
            return 0;
        }

        let copy_len = frame_len.min(out.len());
        out[..copy_len].copy_from_slice(&raw[4..4 + copy_len]);
        copy_len
    }
}

/// Print a MAC address the way a reader expects to see one.
fn print_mac(mac: &[u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (index, byte) in mac.iter().enumerate() {
        if index > 0 {
            ostd::io::print(":");
        }
        let digits = [HEX[(byte >> 4) as usize], HEX[(byte & 0x0F) as usize]];
        ostd::io::print(core::str::from_utf8(&digits).unwrap_or("?"));
    }
}
