#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::syscall::service;
use api::{declare_manifest, declare_syscalls};
use driver_dwc2_usb::dispatch::{handle, NicReply, REPLY_BUF};
use driver_dwc2_usb::hid::EvdevEvent;
use driver_dwc2_usb::hub::UsbHub;
use driver_dwc2_usb::lan9514::Lan9514Device;
use driver_dwc2_usb::usb_channel::{self, TransferMode, UsbHostEngine};
use driver_dwc2_usb::usb_hid;
use driver_dwc2_usb::Dwc2Controller;
use ostd::io::{print, println};
use ostd::syscall::{
    sys_lookup_service, sys_recv_timeout, sys_register_nic_driver, sys_try_send, sys_yield,
    SyscallResult,
};

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
    RecvTimeout,
    Reply,
    Log,
    RequestMmio,
    WaitIrq,
    LookupService,
    RegisterNicDriver,
    GetTime,
    // DMA payload slots: the core reads and writes them directly.
    GrantAlloc,
    GrantFree,
    GrantCacheSyncBegin,
    GrantCacheSyncComplete
];

/// Poll budget for the NIC IPC receive, in 10 ms ticks.
///
/// One tick keeps the HID poll loop at ~100 Hz while an idle NIC costs the
/// driver nothing: `RecvTimeout` returns as soon as a message arrives, so the
/// delay only applies when there is no traffic to service.
const NIC_RECV_TICKS: u64 = 1;

const DWC2_BASE: usize = 0x3F98_0000;
const DWC2_LEN: usize = 0x20000;

// `cell_main!` exports the linker-visible `main` symbol from an external macro
// expansion, which is how a cell keeps `#![forbid(unsafe_code)]`: rustc treats a
// hand-written `#[no_mangle]` as an unsafe attribute (see libs/ostd/src/entry.rs).
ostd::cell_main!(cell_main);

fn cell_main() {
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

    let engine = UsbHostEngine::new(dwc2.mmio());

    // ── Bring the bus up ─────────────────────────────────────────────────────
    //
    // DMA first, FIFO as fallback. Linux drives this SoC's DWC2 in DMA mode,
    // and the board has only ever completed control transfers that carry no
    // data phase in FIFO mode: every data-phase read stalled with the channel
    // enabled and no status bit set. Each mode is therefore tried against a
    // real descriptor read rather than assumed.
    let mut root_class = usb_hid::RootClass::Other;

    for mode in [TransferMode::Dma, TransferMode::Fifo] {
        if !engine.set_mode(mode) {
            println("[dwc2] DMA scratch allocation failed; falling back to FIFO");
            continue;
        }

        if dwc2.init_host(mode).is_err() {
            println("[dwc2] host mode initialization failed");
            continue;
        }

        dwc2.power_on_port();

        let mut connected = false;
        for _ in 0..10_000 {
            if dwc2.is_port_connected() {
                connected = true;
                break;
            }
            sys_yield();
        }
        if !connected {
            println("[dwc2] WARN: no downstream connection on Root Port 0");
        } else {
            match dwc2.reset_port() {
                Ok(speed) => {
                    match speed {
                        0 => println("[dwc2] Port 0 enabled: High-Speed (480 Mbps)"),
                        1 => println("[dwc2] Port 0 enabled: Full-Speed (12 Mbps)"),
                        2 => println("[dwc2] Port 0 enabled: Low-Speed (1.5 Mbps)"),
                        _ => println("[dwc2] Port 0 enabled: Unknown speed"),
                    }
                    engine.set_control_mps(usb_channel::initial_control_mps(speed));
                }
                Err(_) => println("[dwc2] WARN: port reset failed"),
            }
        }

        print("[dwc2] mode=");
        print(match mode {
            TransferMode::Dma => "dma",
            TransferMode::Fifo => "fifo",
        });
        print(" ep0_mps=");
        print_usize(engine.control_mps() as usize);
        println("");

        // A readable device descriptor proves this mode moves bytes.
        let root_device = usb_hid::read_device_descriptor(&engine, 0);
        let root_interfaces = usb_hid::read_configuration(&engine, 0).map(|(_, i)| i);

        match (root_device.as_ref(), root_interfaces.as_ref()) {
            (Some(dev), Some(ifaces)) => {
                print("[dwc2] root device class=");
                print_usize(dev.class as usize);
                print(" mps0=");
                print_usize(dev.max_packet_size_0 as usize);
                println("");
                root_class = usb_hid::classify(dev, ifaces);
                /* keep this mode */
                break;
            }
            _ => {
                println(match mode {
                    TransferMode::Dma => "[dwc2] DMA mode did not enumerate; retrying with FIFO",
                    TransferMode::Fifo => "[dwc2] FIFO mode did not enumerate either",
                });
                root_class = usb_hid::RootClass::Other;
            }
        }
    }

    // ── Classify the root device ─────────────────────────────────────────────
    //
    // On a Pi 3 the root port holds the LAN9514 compound hub, whose first
    // downstream port is the Ethernet controller. Reading the descriptors and
    // branching on the class keeps a directly attached device working too, and
    // — more importantly — turns "nothing answered" into a printed diagnostic
    // instead of a silent no-op.
    let mut hid_interfaces: alloc::vec::Vec<usb_hid::HidInterface> = alloc::vec::Vec::new();
    let mut lan: Option<Lan9514Device<'_>> = None;

    match root_class {
        // ── Hub: scan its downstream ports ───────────────────────────────────
        usb_hid::RootClass::Hub => {
            println("[dwc2-usb] Configuring USB hub at Address 1...");
            let _ = UsbHub::set_address(&engine, 1);
            let _ = UsbHub::set_configuration(&engine, 1, 1);
            let hub = UsbHub::new(&engine, 1);

            let port_count = hub.num_ports();
            if port_count == 0 {
                println("[dwc2-usb] WARN: hub descriptor unavailable; assuming 1 port");
            }
            let ports = if port_count == 0 { 1 } else { port_count };

            print("[dwc2-usb] hub reports ");
            print_usize(ports as usize);
            println(" downstream port(s)");

            // Addresses 1 is the hub; downstream devices start at 2.
            let mut next_addr: u8 = 2;
            for port in 1..=ports as u16 {
                let _ = hub.power_on_port(port);

                let Some((addr, ifaces, split)) =
                    usb_hid::attach_port(&engine, &hub, port, &mut next_addr)
                else {
                    continue;
                };

                // Address whatever is on this port the way the port is wired:
                // a full- or low-speed device behind the hub is reached only
                // through it.
                engine.set_split(split);

                // A HID device is claimed here; anything else on this port is
                // the hub's own function device — on the LAN9514 that is the
                // Ethernet controller, which owns no HID interface.
                let started = usb_hid::start_hid_interfaces(
                    &engine,
                    addr,
                    &ifaces,
                    usb_hid::HID_CHANNEL_BASE,
                    &mut hid_interfaces,
                );
                if started > 0 {
                    engine.set_split(None);
                    continue;
                }

                if lan.is_none() {
                    println("[lan9514] Initializing SMSC LAN9514 Ethernet Controller...");
                    let mut candidate = Lan9514Device::new(&engine, addr);
                    if candidate.init().is_ok() {
                        let mac = candidate.mac_address();
                        print("[lan9514] Hardware MAC: ");
                        print_mac(&mac);
                        println(" (Ready)");
                        lan = Some(candidate);
                    } else {
                        println("[lan9514] WARN: Ethernet MAC init failed on this port");
                    }
                }

                // Back to direct addressing for the next port's hub traffic.
                engine.set_split(None);
            }
        }

        // ── HID device directly on the root port ─────────────────────────────
        usb_hid::RootClass::Hid => {
            println("[dwc2-usb] HID device attached directly to the root port");
            if UsbHub::set_address(&engine, 1).is_ok() {
                if let Some((config_value, ifaces)) = usb_hid::read_configuration(&engine, 1) {
                    let _ = UsbHub::set_configuration(&engine, 1, config_value);
                    let started = usb_hid::start_hid_interfaces(
                        &engine,
                        1,
                        &ifaces,
                        usb_hid::HID_CHANNEL_BASE,
                        &mut hid_interfaces,
                    );
                    if started == 0 {
                        println("[usb-hid] WARN: no drivable HID interface on the root device");
                    }
                }
            }
        }

        usb_hid::RootClass::Other => {
            println("[dwc2-usb] root device is not a hub or HID device; no NIC/HID to drive");
        }
    }

    if let Some(dev) = lan.as_ref() {
        let mac = dev.mac_address();
        let _ = mac;
        if sys_register_nic_driver().is_ok() {
            println("[dwc2-usb] Successfully registered as system NIC Driver Cell!");
        } else {
            println("[dwc2-usb] WARN: sys_register_nic_driver failed");
        }
    } else {
        println("[dwc2-usb] no LAN9514 Ethernet controller found");
    }

    if hid_interfaces.is_empty() {
        println("[usb-hid] no HID device attached");
    } else {
        print("[usb-hid] driving ");
        print_usize(hid_interfaces.len());
        println(" HID interface(s)");
    }

    // The NIC dispatch path needs a device even when none was found, so an
    // unattached controller answers net-service requests with a failure instead
    // of faulting on a missing endpoint.
    let mut lan = match lan {
        Some(dev) => dev,
        None => Lan9514Device::new(&engine, 0),
    };

    // Input-service endpoint, re-resolved whenever it is missing: the service is
    // supervised and comes back under a new tid after a restart.
    let mut input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
    let mut source_registered = false;
    let mut events: alloc::vec::Vec<EvdevEvent> = alloc::vec::Vec::new();
    let mut in_buf = [0u8; 4096];
    let mut out_buf = [0u8; REPLY_BUF];

    println("[dwc2-usb] Entering NIC + HID serving loop...");

    loop {
        // Re-resolve both endpoints: the input service restarts under a new tid.
        if input_tid == 0 {
            input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
            source_registered = false;
        }
        if input_tid != 0 && !source_registered {
            source_registered =
                usb_hid::register_as_source(input_tid, api::ipc::input_source::USB_HID);
            if source_registered {
                println("[usb-hid] registered as an input event source");
            }
        }

        match sys_recv_timeout(0, &mut in_buf, NIC_RECV_TICKS) {
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
            _ => {}
        }

        // Poll every HID interface, then flush the batch to the input service.
        for iface in hid_interfaces.iter_mut() {
            usb_hid::poll_interface(&engine, iface, &mut events);
        }
        if !events.is_empty() {
            if input_tid != 0 {
                for ev in events.iter() {
                    usb_hid::forward_event(input_tid, ev);
                }
            } else {
                // No consumer yet: drop rather than grow without bound. The
                // next loop iteration retries the lookup.
                let _ = sys_lookup_service(service::INPUT);
            }
            events.clear();
        }

        sys_yield();
    }
}

fn print_usize(v: usize) {
    let mut out = [0u8; 20];
    let mut n = v;
    let mut len = 0;
    if n == 0 {
        print("0");
        return;
    }
    while n > 0 && len < out.len() {
        out[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    out[..len].reverse();
    if let Ok(s) = core::str::from_utf8(&out[..len]) {
        print(s);
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
