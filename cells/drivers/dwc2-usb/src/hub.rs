//! USB 2.0 Hub configuration and port management (for LAN9514 internal hub).

use crate::usb_channel::UsbHostEngine;
use ostd::syscall::sys_yield;
use types::ViResult;

// Hub Class Feature Selectors
pub const PORT_CONNECTION: u16 = 0;
pub const PORT_ENABLE: u16 = 1;
pub const PORT_RESET: u16 = 4;
pub const PORT_POWER: u16 = 8;
pub const C_PORT_CONNECTION: u16 = 16;
pub const C_PORT_RESET: u16 = 20;

// Standard USB Requests
const USB_REQ_SET_ADDRESS: u8 = 0x05;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;

/// Hub class descriptor type (USB 2.0 §11.23).
const DT_HUB: u8 = 0x29;

// Hub Class Requests
const HUB_REQ_SET_FEATURE: u8 = 0x03;
const HUB_REQ_CLEAR_FEATURE: u8 = 0x01;
const HUB_REQ_GET_STATUS: u8 = 0x00;

pub struct UsbHub<'a> {
    engine: &'a UsbHostEngine<'a>,
    hub_addr: u8,
}

impl<'a> UsbHub<'a> {
    pub fn new(engine: &'a UsbHostEngine<'a>, hub_addr: u8) -> Self {
        Self { engine, hub_addr }
    }

    /// Set device address on USB bus for a device currently at address 0.
    pub fn set_address(engine: &UsbHostEngine<'_>, new_addr: u8) -> ViResult<()> {
        engine.control_transfer(
            0,    // Target currently at Address 0
            0x00, // Standard Device OUT
            USB_REQ_SET_ADDRESS,
            new_addr as u16,
            0,
            &mut [],
        )?;
        // Allow time for address to latch
        for _ in 0..1000 {
            sys_yield();
        }
        Ok(())
    }

    /// Set active configuration on a USB device.
    pub fn set_configuration(engine: &UsbHostEngine<'_>, dev_addr: u8, config: u8) -> ViResult<()> {
        engine.control_transfer(
            dev_addr,
            0x00, // Standard Device OUT
            USB_REQ_SET_CONFIGURATION,
            config as u16,
            0,
            &mut [],
        )?;
        for _ in 0..1000 {
            sys_yield();
        }
        Ok(())
    }

    /// Power on a downstream port on the hub.
    pub fn power_on_port(&self, port: u16) -> ViResult<()> {
        self.engine.control_transfer(
            self.hub_addr,
            0x23, // Class Port OUT
            HUB_REQ_SET_FEATURE,
            PORT_POWER,
            port,
            &mut [],
        )?;
        // Wait for power stabilization
        for _ in 0..5000 {
            sys_yield();
        }
        Ok(())
    }

    /// Reset a downstream port on the hub and wait for speed negotiation.
    pub fn reset_port(&self, port: u16) -> ViResult<()> {
        self.engine.control_transfer(
            self.hub_addr,
            0x23, // Class Port OUT
            HUB_REQ_SET_FEATURE,
            PORT_RESET,
            port,
            &mut [],
        )?;

        // Hold reset for ~50ms
        for _ in 0..5000 {
            sys_yield();
        }

        // Clear reset change flag
        let _ = self.engine.control_transfer(
            self.hub_addr,
            0x23, // Class Port OUT
            HUB_REQ_CLEAR_FEATURE,
            C_PORT_RESET,
            port,
            &mut [],
        );

        for _ in 0..1000 {
            sys_yield();
        }
        Ok(())
    }

    /// Read 4-byte port status and change bits.
    pub fn get_port_status(&self, port: u16) -> ViResult<(u16, u16)> {
        let mut buf = [0u8; 4];
        self.engine.control_transfer(
            self.hub_addr,
            0xA3, // Class Port IN
            HUB_REQ_GET_STATUS,
            0,
            port,
            &mut buf,
        )?;
        let status = u16::from_le_bytes([buf[0], buf[1]]);
        let change = u16::from_le_bytes([buf[2], buf[3]]);
        Ok((status, change))
    }

    /// True when the port reports a connected device (status bit 0).
    pub fn is_port_connected(&self, port: u16) -> bool {
        match self.get_port_status(port) {
            Ok((status, _)) => status & (1 << PORT_CONNECTION) != 0,
            Err(_) => false,
        }
    }

    /// Clear a port's change bit so the next connect/disconnect is reported once.
    ///
    /// A sticky change bit makes `is_port_connected` re-report the same attach
    /// on every poll, which would re-enumerate a device forever.
    pub fn clear_port_change(&self, port: u16, feature: u16) {
        let _ = self.engine.control_transfer(
            self.hub_addr,
            0x23, // Class Port OUT
            HUB_REQ_CLEAR_FEATURE,
            feature,
            port,
            &mut [],
        );
    }

    /// Number of downstream ports from the hub class descriptor.
    ///
    /// `bNbrPorts` sits at offset 2. Returning 0 on a failed read keeps callers
    /// from probing ports that were never described.
    pub fn num_ports(&self) -> u8 {
        let mut buf = [0u8; 9];
        let ok = self.engine.control_transfer(
            self.hub_addr,
            0xA0, // Standard Device-to-Host IN
            USB_REQ_GET_DESCRIPTOR,
            (DT_HUB as u16) << 8, // wValue = descriptor type << 8
            0,
            &mut buf,
        );
        match ok {
            Ok(n) if n >= 3 && buf[1] == DT_HUB => buf[2],
            _ => 0,
        }
    }

    /// True when the port reports a device at full/low speed (status bit 9).
    ///
    /// LAN9514 is a high-speed hub, so a full-speed device behind it comes from
    /// its own transaction translator. Diagnostics only.
    pub fn is_port_low_speed(&self, port: u16) -> bool {
        match self.get_port_status(port) {
            Ok((status, _)) => status & (1 << 9) != 0,
            Err(_) => false,
        }
    }
}
