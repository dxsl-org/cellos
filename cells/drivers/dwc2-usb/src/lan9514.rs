//! SMSC/Microchip LAN9514 USB 2.0 10/100 Ethernet Driver.
#![allow(dead_code)]

use crate::usb_channel::UsbHostEngine;
use ostd::io::println;
use types::{ViError, ViResult};
// ── LAN95xx Register Offsets ──────────────────────────────────────────────────

const ID_REV: u32 = 0x50;
const INT_STS: u32 = 0x54;
const HW_CFG: u32 = 0x74;
const PMT_CTRL: u32 = 0x20;
const LED_GPIO_CFG: u32 = 0x24;
const GPIO_CFG: u32 = 0x28;
const AFC_CFG: u32 = 0x2C;
const E2P_CMD: u32 = 0x30;
const E2P_DATA: u32 = 0x34;
const BURST_CAP: u32 = 0x38;
const BULK_IN_DLY: u32 = 0x6C;

// MAC Registers (accessed via MAC_CSR)
const MAC_CSR_CMD: u32 = 0xA0;
const MAC_CSR_DATA: u32 = 0xA4;

const MAC_CR: u32 = 0x01;
const ADDRH: u32 = 0x02;
const ADDRL: u32 = 0x03;
const HASHH: u32 = 0x04;
const HASHL: u32 = 0x05;
const MII_ACCESS: u32 = 0x06;
const MII_DATA: u32 = 0x07;
const FLOW: u32 = 0x08;

// Bit Constants
const HW_CFG_SRST: u32 = 1 << 0;
const HW_CFG_BIR: u32 = 1 << 12; // Bulk In Empty Response
const MAC_CR_RXEN: u32 = 1 << 2;
const MAC_CR_TXEN: u32 = 1 << 3;
const MAC_CR_MCPAS: u32 = 1 << 19;
const MAC_CR_PRMS: u32 = 1 << 18;
const MAC_CSR_BUSY: u32 = 1 << 31;
const MAC_CSR_READ: u32 = 0;
const MAC_CSR_WRITE: u32 = 1 << 30;

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
    pub fn read_reg(&self, reg: u32) -> u32 {
        let mut buf = [0u8; 4];
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

    /// Read an internal MAC CSR register.
    pub fn read_mac_reg(&self, mac_reg: u32) -> u32 {
        // Wait for not busy
        while self.read_reg(MAC_CSR_CMD) & MAC_CSR_BUSY != 0 {
            ostd::syscall::sys_yield();
        }
        self.write_reg(MAC_CSR_CMD, MAC_CSR_BUSY | MAC_CSR_READ | (mac_reg & 0xFF));
        while self.read_reg(MAC_CSR_CMD) & MAC_CSR_BUSY != 0 {
            ostd::syscall::sys_yield();
        }
        self.read_reg(MAC_CSR_DATA)
    }

    /// Write an internal MAC CSR register.
    pub fn write_mac_reg(&self, mac_reg: u32, val: u32) {
        while self.read_reg(MAC_CSR_CMD) & MAC_CSR_BUSY != 0 {
            ostd::syscall::sys_yield();
        }
        self.write_reg(MAC_CSR_DATA, val);
        self.write_reg(MAC_CSR_CMD, MAC_CSR_BUSY | MAC_CSR_WRITE | (mac_reg & 0xFF));
        while self.read_reg(MAC_CSR_CMD) & MAC_CSR_BUSY != 0 {
            ostd::syscall::sys_yield();
        }
    }

    /// Read a 16-bit register from the internal MII PHY (PHY address 1).
    pub fn read_phy_reg(&self, reg: u8) -> u16 {
        while self.read_mac_reg(MII_ACCESS) & 0x01 != 0 {
            ostd::syscall::sys_yield();
        }
        let cmd = (1u32 << 11) | ((reg as u32) << 6) | 0x01; // PHY=1, REG=reg, READ=0, BUSY=1
        self.write_mac_reg(MII_ACCESS, cmd);
        while self.read_mac_reg(MII_ACCESS) & 0x01 != 0 {
            ostd::syscall::sys_yield();
        }
        (self.read_mac_reg(MII_DATA) & 0xFFFF) as u16
    }

    /// Write a 16-bit register to the internal MII PHY (PHY address 1).
    pub fn write_phy_reg(&self, reg: u8, val: u16) {
        while self.read_mac_reg(MII_ACCESS) & 0x01 != 0 {
            ostd::syscall::sys_yield();
        }
        self.write_mac_reg(MII_DATA, val as u32);
        let cmd = (1u32 << 11) | ((reg as u32) << 6) | 0x02 | 0x01; // PHY=1, REG=reg, WRITE=1, BUSY=1
        self.write_mac_reg(MII_ACCESS, cmd);
        while self.read_mac_reg(MII_ACCESS) & 0x01 != 0 {
            ostd::syscall::sys_yield();
        }
    }

    /// Initialize the LAN9514 Ethernet controller.
    pub fn init(&mut self) -> ViResult<()> {
        // 1. Verify Chip ID
        let id = self.read_reg(ID_REV);
        let chip = id >> 16;
        if chip != 0x9514 && chip != 0x9500 && chip != 0x950A {
            // Print the value: a warning that hides the number it is warning
            // about cannot be acted on.
            ostd::io::print("[lan9514] WARN: unexpected chip ID: 0x");
            crate::usb_channel::print_hex_val(id);
            println("");
        } else {
            println("[lan9514] Verified SMSC/Microchip LAN9514 Ethernet Controller");
        }

        // 2. Soft Reset MAC
        self.write_reg(HW_CFG, HW_CFG_SRST);
        let mut count = 0;
        while self.read_reg(HW_CFG) & HW_CFG_SRST != 0 {
            count += 1;
            if count > 10_000 {
                return Err(ViError::IO);
            }
            ostd::syscall::sys_yield();
        }

        // 3. Configure Hardware Buffers
        self.write_reg(BURST_CAP, 0x20); // 32 * 512 bytes burst
        self.write_reg(BULK_IN_DLY, 0x08);
        self.write_reg(HW_CFG, HW_CFG_BIR); // Allow empty bulk-in response

        // 4. Read Hardware MAC Address from ADDRL / ADDRH
        let addrl = self.read_mac_reg(ADDRL);
        let addrh = self.read_mac_reg(ADDRH);
        if addrl != 0 && addrl != 0xFFFF_FFFF {
            self.mac[0] = (addrl & 0xFF) as u8;
            self.mac[1] = ((addrl >> 8) & 0xFF) as u8;
            self.mac[2] = ((addrl >> 16) & 0xFF) as u8;
            self.mac[3] = ((addrl >> 24) & 0xFF) as u8;
            self.mac[4] = (addrh & 0xFF) as u8;
            self.mac[5] = ((addrh >> 8) & 0xFF) as u8;
        }

        // 5. Initialize internal MII PHY: Soft Reset & Auto-Negotiation
        self.write_phy_reg(0, 0x8000); // BMCR Soft Reset
        for _ in 0..1000 {
            if self.read_phy_reg(0) & 0x8000 == 0 {
                break;
            }
            ostd::syscall::sys_yield();
        }
        self.write_phy_reg(4, 0x01E1); // ANAR: 10/100 Full/Half Duplex + 802.3
        self.write_phy_reg(0, 0x1200); // BMCR: Enable & Restart Auto-Negotiation
        println("[lan9514] Internal MII PHY initialized, Auto-Negotiation active");

        // 6. Enable RJ45 Status LEDs (Activity + Link)
        self.write_reg(LED_GPIO_CFG, 0x0000_0070);

        // 7. Configure MAC Control Register: Enable TX and RX
        let mac_cr = MAC_CR_TXEN | MAC_CR_RXEN | MAC_CR_MCPAS;
        self.write_mac_reg(MAC_CR, mac_cr);

        println("[lan9514] Ethernet MAC configured, TX and RX enabled");
        Ok(())
    }

    /// Return the active 6-byte MAC address.
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    /// Send a raw Ethernet frame over Bulk OUT EP 2.
    pub fn send_frame(&self, frame: &[u8]) -> bool {
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
        self.engine
            .bulk_transmit(self.dev_addr, 2, &packet[..total])
            .is_ok()
    }

    /// Receive a raw Ethernet frame from Bulk IN EP 1.
    pub fn recv_frame(&self, out: &mut [u8]) -> usize {
        let mut raw = [0u8; 4 + 1518];
        let n = match self.engine.bulk_receive(self.dev_addr, 1, &mut raw) {
            Ok(bytes) => bytes,
            Err(_) => return 0,
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
