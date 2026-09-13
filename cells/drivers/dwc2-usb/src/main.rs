#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::{declare_manifest, declare_syscalls};
use driver_dwc2_usb::dispatch::{handle, NicReply, REPLY_BUF};
use driver_dwc2_usb::hub::UsbHub;
use driver_dwc2_usb::lan9514::Lan9514Device;
use driver_dwc2_usb::usb_channel::UsbHostEngine;
use driver_dwc2_usb::Dwc2Controller;
use ostd::io::{print, println};
use ostd::syscall::{sys_recv, sys_register_nic_driver, sys_try_send, sys_yield, SyscallResult};

declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false,
    i2c = false,
    spi = false
);

declare_syscalls![
    Send,
    TrySend,
    Recv,
    Reply,
    Log,
    RequestMmio,
    WaitIrq,
    RegisterNicDriver
];

const DWC2_BASE: usize = 0x3F98_0000;
const DWC2_LEN: usize = 0x20000;

#[no_mangle]
pub fn main() {
    println("[dwc2] Synopsys DWC2 USB Host & SMSC LAN9514 Driver starting...");

    let dwc2 = match Dwc2Controller::open(DWC2_BASE, DWC2_LEN) {
        Ok(d) => d,
        Err(_) => {
            println("[dwc2] ERROR: failed to request DWC2 MMIO region");
            loop {
                sys_yield();
            }
        }
    };

    let core_id = match dwc2.probe_core_id() {
        Ok(id) => id,
        Err(_) => {
            println("[dwc2] ERROR: Synopsys Core ID check failed");
            loop {
                sys_yield();
            }
        }
    };

    print("[dwc2] Synopsys Core ID: 0x");
    print_hex_u32(core_id);
    println(" (OTG Host Core Verified)");

    if dwc2.init_host().is_err() {
        println("[dwc2] ERROR: DWC2 host mode initialization failed");
        loop {
            sys_yield();
        }
    }
    println("[dwc2] Host Mode configured successfully");

    dwc2.power_on_port();
    println("[dwc2] Root Port 0 powered ON");

    // Wait for downstream connection
    let mut connected = false;
    for _ in 0..10_000 {
        if dwc2.is_port_connected() {
            connected = true;
            break;
        }
        sys_yield();
    }

    if connected {
        println("[dwc2] Downstream connection detected on Root Port 0! Resetting port...");
        match dwc2.reset_port() {
            Ok(0) => println("[dwc2] Port 0 enabled: High-Speed (480 Mbps) - LAN9514 Hub attached"),
            Ok(1) => println("[dwc2] Port 0 enabled: Full-Speed (12 Mbps)"),
            Ok(2) => println("[dwc2] Port 0 enabled: Low-Speed (1.5 Mbps)"),
            _ => println("[dwc2] Port 0 enabled: Unknown speed"),
        }
    } else {
        println("[dwc2] WARN: No connection detected on Root Port 0");
    }

    let engine = UsbHostEngine::new(dwc2.mmio());

    // ── Phase 1: Enumerate LAN9514 USB Hub (Root device at Address 0) ─────────
    println("[dwc2-usb] Configuring LAN9514 internal USB 2.0 Hub (Address 1)...");
    let _ = UsbHub::set_address(&engine, 1);
    let _ = UsbHub::set_configuration(&engine, 1, 1);
    let hub = UsbHub::new(&engine, 1);
    println("[dwc2-usb] LAN9514 Hub active at USB Address 1");

    // ── Phase 2: Power on and Reset Hub Downstream Port 1 (Ethernet) ───────────
    println("[dwc2-usb] Powering ON Hub Downstream Port 1 (Internal Ethernet)...");
    let _ = hub.power_on_port(1);
    println("[dwc2-usb] Resetting Hub Downstream Port 1...");
    let _ = hub.reset_port(1);
    println("[dwc2-usb] Hub Port 1 active (Internal Ethernet connected)");

    // ── Phase 3: Enumerate Ethernet Controller (Now at Address 0 on Port 1) ───
    println("[dwc2-usb] Configuring LAN9514 Ethernet Controller (Address 2)...");
    let _ = UsbHub::set_address(&engine, 2);
    let _ = UsbHub::set_configuration(&engine, 2, 1);
    println("[dwc2-usb] LAN9514 Ethernet Controller active at USB Address 2");

    // ── Phase 4: Initialize LAN9514 MAC, PHY and Read Hardware MAC ───────────
    let mut lan = Lan9514Device::new(&engine, 2);
    println("[lan9514] Initializing SMSC LAN9514 Ethernet Controller...");
    if lan.init().is_ok() {
        let mac = lan.mac_address();
        print("[lan9514] Hardware MAC: ");
        print_mac(&mac);
        println(" (Ready)");

        // Register as the active NIC Driver Cell in kernel
        if sys_register_nic_driver().is_ok() {
            println("[dwc2-usb] Successfully registered as system NIC Driver Cell!");
        } else {
            println("[dwc2-usb] WARN: sys_register_nic_driver failed");
        }
    } else {
        println("[lan9514] WARN: Ethernet MAC init deferred/failed");
    }

    println("[dwc2-usb] Entering NIC IPC serving loop...");

    // ── Phase 5: Serving loop for raw NIC IPC protocol from service-net ───────
    let mut in_buf = [0u8; 4096];
    let mut out_buf = [0u8; REPLY_BUF];

    loop {
        match sys_recv(0, &mut in_buf) {
            SyscallResult::Ok(sender_tid) if sender_tid > 0 => {
                match handle(&mut lan, &in_buf, &mut out_buf) {
                    NicReply::Status(code) => {
                        let _ = sys_try_send(sender_tid, &[code]);
                    }
                    NicReply::Frame { len, buf } => {
                        let _ = sys_try_send(sender_tid, &buf[..2 + len]);
                    }
                    NicReply::Mac(mac) => {
                        let _ = sys_try_send(sender_tid, &mac);
                    }
                }
            }
            _ => {
                sys_yield();
            }
        }
    }
}

fn print_hex_u32(val: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 8];
    for i in (0..8).rev() {
        buf[i] = HEX[((val >> ((7 - i) * 4)) & 0xF) as usize];
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        print(s);
    }
}

fn print_mac(mac: &[u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, &b) in mac.iter().enumerate() {
        if i > 0 {
            print(":");
        }
        let h = [HEX[(b >> 4) as usize], HEX[(b & 0xF) as usize]];
        if let Ok(s) = core::str::from_utf8(&h) {
            print(s);
        }
    }
}
