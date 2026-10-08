#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::syscall::service;
use api::{declare_manifest, declare_syscalls};
#[cfg(feature = "loop-trace")]
use driver_dwc2_usb::dispatch::OP_TX;
use driver_dwc2_usb::dispatch::{handle, NicReply, REPLY_BUF, STATUS_NOT_READY};
use driver_dwc2_usb::hid::EvdevEvent;
use driver_dwc2_usb::hub::UsbHub;
use driver_dwc2_usb::lan9514::Lan9514Device;
use driver_dwc2_usb::lan_ipc;
use driver_dwc2_usb::usb_channel::{self, TransferMode, UsbHostEngine};
use driver_dwc2_usb::usb_hid;
use driver_dwc2_usb::Dwc2Controller;
use ostd::io::{print, println};
use ostd::syscall::{
    sys_force_exit, sys_lookup_service, sys_notify_on_exit, sys_recv_timeout,
    sys_register_nic_driver, sys_register_usb_hid_producer, sys_send, sys_spawn_from_path,
    sys_try_send, sys_yield, SyscallResult,
};

declare_manifest!(
    block_io = false,
    network = false,
    spawn = true,
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
    GetTime,
    SpawnFromPath,
    LookupService,
    RegisterNicDriver,
    RegisterUsbHidProducer,
    NotifyOnExit,
    ForceExit,
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

/// Serving-loop turns between PHY link checks.
///
/// One check is two or three synchronous control transfers (wait for MII idle,
/// write `MII_ADDR`, read `MII_DATA`) on the bus the NIC and both keyboards share,
/// so it belongs on its own cadence rather than in every turn. A turn is a 10 ms
/// receive timeout when the loop is idle and a transfer's worth of time when it is
/// busy, so a turn count is a floor on the interval: the poll can never cost more
/// than one check per this many turns of real work. On the board a per-turn check
/// was thousands of transfers a second of pure polling, and the ones the flaky bus
/// dropped printed `[dwc2] control ... failed` and were misread as link
/// transitions. A link only changes on the scale of auto-negotiation, so 64 turns
/// (≥ ~0.6 s idle) still catches one that comes up after bring-up.
const LINK_POLL_TURNS: u32 = 64;

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
    let mut lan_addr: Option<u8> = None;
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

                let (addr, ifaces, split) =
                    match usb_hid::attach_port(&engine, &hub, port, &mut next_addr) {
                        usb_hid::AttachOutcome::Attached(addr, ifaces, split) => {
                            (addr, ifaces, split)
                        }
                        usb_hid::AttachOutcome::Empty => continue,
                        usb_hid::AttachOutcome::Failed => {
                            // A hub that has stopped answering will not enumerate
                            // anything else, and every further request is one more
                            // STALL on a control pipe that is already stuck. Ask it a
                            // question it must answer — the *standard* device
                            // GET_STATUS — before trusting it with another port. The
                            // port-status form is a hub-class request whose wIndex is
                            // a port; asking it with wIndex = 0 stalls on this hub and
                            // once ended enumeration before the keyboard's port.
                            if !hub.is_responsive() {
                                println(
                                    "[usb-hid] hub stopped answering — ending port enumeration",
                                );
                                break;
                            }
                            continue;
                        }
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

                // The hub's own function device (on the LAN9514 that is the
                // Ethernet controller) is remembered here and initialized
                // *after* the port scan. Initializing it inline put a chip's
                // register waits in front of the remaining ports, and the
                // keyboard on a later port never enumerated: no LEDs, no keys.
                // Input devices are claimed before any NIC work now.
                if lan_addr.is_none() {
                    lan_addr = Some(addr);
                }

                // Back to direct addressing for the next port's hub traffic.
                engine.set_split(None);
            }

            if let Some(addr) = lan_addr {
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

    if lan.is_some() {
        println("[dwc2-usb] LAN9514 transport ready for isolated NIC front-end");
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

    // HID polling and decoding stay in this controller cell for the constrained
    // embedded profile. This preserves report ordering without cross-cell IPC.

    let lan_present = lan.is_some();
    // The NIC dispatch path needs a device even when none was found, so an
    // unattached controller answers net-service requests with a failure instead
    // of faulting on a missing endpoint.
    let mut lan = match lan {
        Some(dev) => dev,
        None => Lan9514Device::new(&engine, 0),
    };
    #[cfg(feature = "loopback-diag")]
    if lan_present {
        let (tx_ok, rx_bytes) = lan.loopback_self_test();
        println(&alloc::format!(
            "[lan9514] loopback diag: chip_tx_ok={} chip_rx_bytes={}",
            tx_ok,
            rx_bytes
        ));
    }

    // This host is the single kernel NIC endpoint even without an attached
    // LAN9514 — on RPi3 the LAN9514 *is* the NIC, so it must keep publishing
    // service::NIC_DRIVER.
    if sys_register_nic_driver().is_err() {
        println("[dwc2-usb] ERROR: failed to register DWC2 NIC endpoint");
    }
    // The input service's producer gate verifies service::USB_HID_PRODUCER, a
    // role distinct from the singleton NIC owner, so the HID event stream is
    // authorized in addition to — never instead of — the NIC registration.
    if sys_register_usb_hid_producer().is_err() {
        println("[dwc2-usb] ERROR: failed to register USB HID producer role");
    }
    let mut lan_worker_tid = if lan_present { spawn_lan_worker() } else { 0 };
    let mut lan_attach_pending = lan_worker_tid != 0;
    let mut lan_deferred = DeferredLan::new();
    // Last link state reported, so the transition prints exactly once (`init`
    // reports the state it measured, this tracks it afterwards). A bring-up read
    // that failed leaves this "down" as the conservative default: the first
    // successful poll then reports the transition once, which is the truth.
    let mut lan_link_up = lan_present && lan.link_bmsr().is_some_and(|bmsr| bmsr & 0x0004 != 0);
    let mut lan_poll_countdown: u32 = LINK_POLL_TURNS;
    // The port's change latches were set by this boot's own reset and connect;
    // clearing them here is what makes a later change (the core disabling the port
    // on a bus event) visible as a transition instead of lost in the boot-time
    // latches. `PRTENA` is not touched — see `clear_port_change_bits`.
    engine.clear_port_change_bits();
    let mut port_enabled = engine.port_enabled();
    // Bracket the window: this is the last moment the bring-up path knew the port
    // was alive, and the line below says whether it still is when the loop starts.
    // The board's run had the port already dead on the loop's first turn with no
    // line saying whether it died during enumeration or after it, and the *when* is
    // what decides between a bring-up bug and a bus event.
    println(&alloc::format!(
        "[dwc2] end of bring-up: root port {}",
        if port_enabled { "enabled" } else { "DISABLED" }
    ));
    if !port_enabled {
        println(
            "[dwc2] the serving loop starts with the root port already disabled \
             (no transfer can complete until it is reset)",
        );
    }

    // Input-service endpoint, re-resolved whenever it is missing: the service is
    // supervised and comes back under a new tid after a restart.
    let mut input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
    let mut source_registered = false;
    let mut events: alloc::vec::Vec<EvdevEvent> = alloc::vec::Vec::new();
    let mut in_buf = [0u8; 4096];
    let mut out_buf = [0u8; REPLY_BUF];

    println("[dwc2-usb] Entering NIC + HID serving loop...");

    loop {
        #[cfg(feature = "loop-trace")]
        let turn_start = ostd::syscall::sys_get_time_ms().unwrap_or(0);
        // Re-resolve both endpoints: the input service restarts under a new tid.
        if input_tid == 0 {
            input_tid = sys_lookup_service(service::INPUT).unwrap_or(0);
            source_registered = false;
        }
        if input_tid != 0 && !source_registered {
            source_registered =
                usb_hid::register_as_source(input_tid, api::ipc::input_source::USB_HID_HOST);
            if source_registered {
                println("[usb-hid] registered as an input event source");
            }
        }
        if lan_attach_pending {
            let mut attach = [0u8; 1];
            lan_ipc::encode_attach(&mut attach);
            if matches!(sys_try_send(lan_worker_tid, &attach), SyscallResult::Ok(0)) {
                lan_attach_pending = false;
            }
        } else if lan_worker_tid != 0 {
            // A request the front-end was not parked for waits here rather than
            // being lost; it goes first, before the client's next frame can
            // replace it.
            lan_deferred.retry(lan_worker_tid);
        }

        // The link can come up (or drop) long after `init` measured it: the chip
        // re-negotiated when its port was reset, and nothing else looks again.
        // Reported once per transition, and the data path is re-enabled on the way
        // up so a slow negotiation is not a permanently dead NIC. The cadence is
        // `LINK_POLL_TURNS`; see the constant for why a check does not belong in
        // every turn.
        // The port is a single local register read, so it is checked every turn
        // rather than on the poll cadence -- it is what decides whether any USB work
        // below is worth attempting at all. Its failure mode looks identical to
        // every other fault: with PRTENA clear, control, bulk and split transfers
        // all fail with no channel status, so the console showed "no status bit"
        // (and one NAK-retry line per attempt) for a NIC that was simply off the bus.
        // Reported once per transition, so a log says when it happened.
        let now_enabled = engine.port_enabled();
        if now_enabled != port_enabled {
            port_enabled = now_enabled;
            if now_enabled {
                println("[dwc2] root port enabled");
            } else {
                println(
                    "[dwc2] root port DISABLED (HPRT0.PRTENA=0) — every transfer fails until the \
                     port is reset and the bus re-enumerated",
                );
                // The state that says *what* was lost (see `core_state`): one loop
                // over four registers, once, at the transition.
                for (name, value) in engine.core_state() {
                    print("[dwc2]   ");
                    print(name);
                    print("=0x");
                    usb_channel::print_hex_val(value);
                    println("");
                }
            }
        }

        lan_poll_countdown = lan_poll_countdown.saturating_sub(1);
        if lan_poll_countdown == 0 {
            lan_poll_countdown = LINK_POLL_TURNS;

            // The link can come up (or drop) long after `init` measured it: the chip
            // re-negotiated when its port was reset, and nothing else looks again.
            // Reported once per transition, and the data path is re-enabled on the
            // way up so a slow negotiation is not a permanently dead NIC.
            //
            // An access that did not complete says nothing about the link: leave the
            // state and the console alone until one does. Reading it as `BMSR = 0` is
            // what printed the `PHY link down`/`PHY link up` pairs.
            if lan_present {
                if let Some(bmsr) = lan.link_bmsr() {
                    let now_up = bmsr & 0x0004 != 0;
                    if now_up != lan_link_up {
                        lan_link_up = now_up;
                        if now_up {
                            lan.enable_data_path();
                            print("[lan9514] PHY link up (BMSR=0x");
                        } else {
                            print("[lan9514] PHY link down (BMSR=0x");
                        }
                        usb_channel::print_hex_val(bmsr as u32);
                        println(")");
                    }
                }
            }
        }

        // Exit-watch wakeups name the dead LAN child but do not write a message.
        // Clearing the buffer keeps that distinct from a valid worker frame.
        in_buf.fill(0);
        match sys_recv_timeout(0, &mut in_buf, NIC_RECV_TICKS) {
            SyscallResult::Ok(sender_tid) if sender_tid > 0 => {
                if sender_tid == lan_worker_tid {
                    if let Some((client_tid, request)) = lan_ipc::decode_request(&in_buf) {
                        // Reply straight to the client. The request still only
                        // reaches `handle` after the front-end has decoded it,
                        // so the envelope check is intact, and a reply routed
                        // back through the front-end arrives from *its* tid -
                        // which a client waiting on this cell's tid never sees
                        // (the net bridge timed out on every TX that way).
                        match handle(&mut lan, request, &mut out_buf) {
                            NicReply::Status(code) => {
                                // The chip's verdict for a transmitted frame.
                                #[cfg(feature = "loop-trace")]
                                if request.first() == Some(&OP_TX) {
                                    let counter = if code == 0 { &NIC_TX_OK } else { &NIC_TX_FAIL };
                                    counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                }
                                let _ = sys_send(client_tid, &[code]);
                            }
                            NicReply::Frame { len, buf } => {
                                // A frame read out of the chip, or an empty read
                                // (the chip had nothing — the ordinary idle poll).
                                #[cfg(feature = "loop-trace")]
                                {
                                    let counter = if len > 0 { &NIC_RX_FRAMES } else { &NIC_RX_EMPTY };
                                    counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                }
                                let _ = sys_send(client_tid, &buf[..2 + len]);
                            }
                            NicReply::Mac(mac) => {
                                let _ = sys_send(client_tid, &mac);
                            }
                        }
                    } else if in_buf.iter().all(|&byte| byte == 0) {
                        // A zeroed receive buffer is the kernel exit-watch
                        // shape. Force-exit makes a malicious all-zero payload
                        // non-orphaning before the replacement is published.
                        let _ = sys_force_exit(lan_worker_tid);
                        lan_worker_tid = spawn_lan_worker();
                        lan_attach_pending = lan_worker_tid != 0;
                        // A replacement front-end only understands the attach
                        // frame first, so a deferred request cannot be handed to
                        // it: the client's retry is what gets served.
                        lan_deferred.clear();
                    }
                    // A NIC reply must not skip HID polling on this iteration.
                } else if sender_tid == input_tid && in_buf[0] == api::ipc::OP_SET_LEDS {
                    // Input owns lock state; translate it to each keyboard's
                    // descriptor-defined Output report.
                    let leds = in_buf[1];
                    for iface in hid_interfaces.iter_mut() {
                        usb_hid::request_leds(iface, leds);
                    }
                } else if lan_worker_tid == 0 || lan_attach_pending || !port_enabled {
                    // Not ready means three things: the front-end is not spawned
                    // yet (it is spawned after the chip is brought up, while the
                    // net service starts asking right away), or it is spawned but
                    // still waiting for the attach handshake. Forwarding in the
                    // second case loses the request - the front-end's first loop
                    // only recognises the attach frame - and the client then
                    // waits out its whole reply timeout for a request nobody
                    // ever answered. A status byte says "not ready" instead, and
                    // the client's own retry is what gets served.
                    //
                    // `STATUS_NOT_READY`, not a bare 1: the Net Cell keeps a refused
                    // frame and offers it again, and this code is what tells its log
                    // that nothing reached the chip (rather than the chip refusing).
                    //
                    // The third case is a disabled root port: the chip is off the bus,
                    // so a request handed to the front-end would spend the whole
                    // transfer budget failing (the board showed one NAK-retry line per
                    // attempt, hundreds of them, while the Net Cell waited seconds per
                    // frame). Saying "not ready" costs one byte and keeps the frame.
                    if nic_request_len(&in_buf).is_some() {
                        let _ = sys_send(sender_tid, &[STATUS_NOT_READY]);
                    }
                } else {
                    if let Some(request_len) = nic_request_len(&in_buf) {
                        let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
                        if let Some(len) = lan_ipc::encode_request(
                            sender_tid,
                            &in_buf[..request_len],
                            &mut request,
                        ) {
                            if !matches!(
                                sys_try_send(lan_worker_tid, &request[..len]),
                                SyscallResult::Ok(0)
                            ) {
                                // The front-end is mid-transfer: keep the request
                                // for the next turn instead of losing the frame
                                // (a dropped one costs the client its whole
                                // timeout and, on the board's multi-second USB
                                // turns, every retry as well).
                                static DEFERRED: core::sync::atomic::AtomicBool =
                                    core::sync::atomic::AtomicBool::new(false);
                                if lan_deferred.defer(&request[..len])
                                    && !DEFERRED.swap(true, core::sync::atomic::Ordering::Relaxed)
                                {
                                    ostd::io::println(
                                        "[dwc2-usb] NIC request deferred until the front-end is parked",
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Poll and decode one interface at a time so device identity remains
        // attached to every event and press/release ordering is preserved.
        //
        // Skipped entirely while the root port is disabled: the interrupts are on
        // that port, so every poll would spend its whole budget discovering what
        // `port_enabled` already says, and the board's console was a metre of
        // timeout dumps for it.
        for iface in hid_interfaces.iter_mut().filter(|_| port_enabled) {
            events.clear();
            usb_hid::poll_interface(&engine, iface, &mut events);
            if input_tid != 0 {
                for event in events.drain(..) {
                    let _ = usb_hid::forward_device_event(input_tid, iface.device, &event);
                }
            }
        }

        // Output reports share endpoint zero with HID polling, so run the retry
        // queue only after every interrupt-IN poll has completed -- and not at all
        // while the port is disabled, for the same reason as the polls above.
        for iface in hid_interfaces.iter_mut().filter(|_| port_enabled) {
            usb_hid::flush_leds(&engine, iface);
        }

        #[cfg(feature = "loop-trace")]
        trace_turn(ostd::syscall::sys_get_time_ms().unwrap_or(turn_start).saturating_sub(turn_start));

        sys_yield();
    }
}

/// Chip-level NIC outcomes (`loop-trace` images).
///
/// The client's `accepted=true` only says the driver served its request; what the
/// board still cannot tell is what the *chip* did — did the frame go out, and did
/// anything come back. These count `handle`'s own verdict (the USB transfer's
/// result) and ride the existing `[dwc2-loop]` line, so a diagnostic run answers
/// it without adding a line of noise to a quiet image.
#[cfg(feature = "loop-trace")]
static NIC_TX_OK: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "loop-trace")]
static NIC_TX_FAIL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "loop-trace")]
static NIC_RX_FRAMES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "loop-trace")]
static NIC_RX_EMPTY: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Report driver turn cost (`loop-trace` images).
///
/// The driver shares one loop between USB transfers and its mailbox, so a long
/// turn is exactly what keeps it out of `Recv` while a client is offering it a
/// request — the client's bounded offer then expires and its command is retried.
/// One line per ~2 s with the worst and latest turn, in milliseconds.
#[cfg(feature = "loop-trace")]
fn trace_turn(elapsed_ms: u64) {
    use core::sync::atomic::{AtomicU64, Ordering};
    /// Report cadence in milliseconds.
    const REPORT_INTERVAL_MS: u64 = 2000;
    static TURNS: AtomicU64 = AtomicU64::new(0);
    static MAX_MS: AtomicU64 = AtomicU64::new(0);
    static SLOW_TURNS: AtomicU64 = AtomicU64::new(0);
    static LAST_REPORT: AtomicU64 = AtomicU64::new(0);

    let turns = TURNS.fetch_add(1, Ordering::Relaxed) + 1;
    MAX_MS.fetch_max(elapsed_ms, Ordering::Relaxed);
    if elapsed_ms >= 100 {
        SLOW_TURNS.fetch_add(1, Ordering::Relaxed);
    }
    let Some(now) = ostd::syscall::sys_get_time_ms() else {
        return;
    };
    let last = LAST_REPORT.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < REPORT_INTERVAL_MS {
        return;
    }
    LAST_REPORT.store(now, Ordering::Relaxed);
    println(&alloc::format!(
        "[dwc2-loop] turns={} max_turn_ms={} last_turn_ms={} turns_over_100ms={} \
         nic_tx_ok={} nic_tx_fail={} nic_rx={} nic_rx_empty={} \
         ch_xacterr={} ch_stall={} ch_other={} ch_timeout={}",
        turns,
        MAX_MS.load(Ordering::Relaxed),
        elapsed_ms,
        SLOW_TURNS.load(Ordering::Relaxed),
        NIC_TX_OK.load(Ordering::Relaxed),
        NIC_TX_FAIL.load(Ordering::Relaxed),
        NIC_RX_FRAMES.load(Ordering::Relaxed),
        NIC_RX_EMPTY.load(Ordering::Relaxed),
        usb_channel::fault_counts()[0],
        usb_channel::fault_counts()[1],
        usb_channel::fault_counts()[2],
        usb_channel::fault_counts()[3]
    ));
}

/// Spawn the LAN9514 front-end. It receives no USB/DMA capability and can
/// restart without disturbing HID polling or the DWC2 controller.
fn spawn_lan_worker() -> usize {
    match sys_spawn_from_path("/bin/lan9514") {
        SyscallResult::Ok(tid) => {
            let _ = sys_notify_on_exit(tid);
            tid
        }
        _ => {
            println("[dwc2-usb] WARN: LAN9514 worker spawn failed");
            0
        }
    }
}

/// The NIC request the front-end was not parked to receive.
///
/// A request reaches the front-end by rendezvous `sys_try_send`, which completes
/// only while the front-end is parked in `Recv` — and the front-end is inside a
/// USB transfer for the whole of a slow turn. A request that arrived in that
/// window used to be dropped: the board printed
/// `[dwc2-usb] NIC request not forwarded to the front-end` and, immediately
/// after it, the net service's `NIC driver reply timeout; frame not
/// acknowledged`, so the frame the guest's ARP needed never left this cell. The
/// NIC protocol keeps one request outstanding per client, so one slot is enough;
/// the driver re-offers it every turn, exactly like the attach handshake above,
/// and a newer request replaces a deferred older one — that client has already
/// given up, and the net service keeps its frame at the head of its TX queue for
/// the retry anyway.
struct DeferredLan {
    bytes: [u8; lan_ipc::MAX_REQUEST],
    len: usize,
}

impl DeferredLan {
    fn new() -> Self {
        Self {
            bytes: [0u8; lan_ipc::MAX_REQUEST],
            len: 0,
        }
    }

    /// Keep `request` for the next turn. Anything the encoder produced fits;
    /// a request that does not is refused rather than truncated.
    fn defer(&mut self, request: &[u8]) -> bool {
        if request.len() > self.bytes.len() {
            return false;
        }
        self.bytes[..request.len()].copy_from_slice(request);
        self.len = request.len();
        true
    }

    /// Hand a deferred request to `front_end`; the slot clears when it lands.
    fn retry(&mut self, front_end: usize) {
        if self.len == 0 {
            return;
        }
        if matches!(
            sys_try_send(front_end, &self.bytes[..self.len]),
            SyscallResult::Ok(0)
        ) {
            self.len = 0;
        }
    }

    fn clear(&mut self) {
        self.len = 0;
    }
}

/// Exact byte length of a raw NIC request inside a padded IPC receive buffer.
fn nic_request_len(frame: &[u8]) -> Option<usize> {
    match *frame.first()? {
        0 => {
            if frame.len() < 3 {
                return None;
            }
            let bytes = u16::from_le_bytes([frame[1], frame[2]]) as usize;
            let len = 3 + bytes;
            (bytes > 0 && bytes <= driver_dwc2_usb::dispatch::FRAME_BUF && len <= frame.len())
                .then_some(len)
        }
        1 | 2 => Some(1),
        _ => None,
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
