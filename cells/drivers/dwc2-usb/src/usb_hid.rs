//! USB HID class driver over the DWC2 host controller.
//!
//! Brings up whatever HID device is attached to a LAN9514 downstream port and
//! feeds its reports into the input service. The full path is:
//!
//! ```text
//!   port connect → SET_ADDRESS → GET_DESCRIPTOR(DEVICE)
//!                → GET_DESCRIPTOR(CONFIGURATION) → walk interfaces
//!                → per HID interface: GET_DESCRIPTOR(REPORT) → parse
//!                → SET_CONFIGURATION → SET_IDLE(0) → SET_PROTOCOL
//!                → poll Interrupt IN → decode → forward to input service
//! ```
//!
//! **Composite devices.** A wireless receiver presents several interfaces — its
//! keyboard and its mouse — and each carries its own report descriptor and
//! interrupt endpoint. They are enumerated independently and polled round-robin;
//! treating only the first interface would surface a keyboard with no pointer
//! (or the reverse), which is the usual failure mode for combo dongles.
//!
//! **Report vs boot protocol.** The descriptor path runs when a report
//! descriptor parses, because it is the only way to decode a device that uses
//! report IDs or a non-standard layout. Boot protocol (fixed 8-byte keyboard /
//! 3-byte mouse) is the fallback for a descriptor that is absent or unparseable,
//! and it is what most combo receivers implement anyway so a BIOS can use them.

extern crate alloc;

use alloc::vec::Vec;

use crate::delay_ms;
use crate::hid::{
    decode_boot_report, BootState, EvdevEvent, HidDecoder, HidDeviceId, HidKind, LedOutput,
    DEVICE_EVENT_LEN, MAX_LED_REPORT,
};
use crate::hub::UsbHub;
use crate::usb_channel::UsbHostEngine;
use crate::usb_desc::{
    self, EndpointDesc, InterfaceDesc, RT_CLASS_INTERFACE_OUT, RT_DEV_TO_HOST_STANDARD,
};
use ostd::io::{print, println};
use ostd::syscall::SyscallResult;

/// First host channel reserved for HID. Channels 0-2 are control and the
/// Ethernet bulk pair, so HID devices take 3..=6 (four concurrently attached).
pub const HID_CHANNEL_BASE: usize = 3;
/// Maximum HID interfaces driven at once across all ports.
pub const MAX_HID_INTERFACES: usize = 4;
/// Buffer size for one interrupt IN report.
const REPORT_BUF: usize = 64;
/// Control-transfer attempts before an enumeration stage gives up.
///
/// A port reset and a mode switch are both followed by a brief window where the
/// first SETUP is lost, so one attempt is not enough to tell a dead device from
/// a slow one.
const ENUM_ATTEMPTS: usize = 3;
/// Frame interval between retried HID Output reports.
///
/// A split-control request can transiently receive NYET while the hub's
/// translator is serving the interrupt endpoint. Retrying at 20 Hz keeps a
/// failed LED update visible to the user without competing continuously with
/// keyboard polling.
const LED_RETRY_FRAMES: u32 = 50;

/// The retry disposition for a failed HID Output report.
///
/// A STALL is the device rejecting this report request; only bus or hub
/// availability failures are eligible for the bounded retry queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LedFailureDisposition {
    Retry,
    Reject,
}

#[inline]
fn led_failure_disposition(stalled: bool) -> LedFailureDisposition {
    if stalled {
        LedFailureDisposition::Reject
    } else {
        LedFailureDisposition::Retry
    }
}

/// One enumerated HID interface with one in-host decoder state.
pub struct HidInterface {
    /// Host-local logical interface identity, scoped to this host-cell lifetime.
    pub device: HidDeviceId,
    pub channel: usize,
    pub dev_addr: u8,
    pub interface: u8,
    pub endpoint: EndpointDesc,
    pub decoder: Option<HidDecoder>,
    pub boot_state: BootState,
    pub kind: HidKind,
    /// The output report that lights this device's lock LEDs, when it has one.
    pub leds: Option<LedOutput>,
    /// Last LED bitmap programmed into the device, so a repeat costs no traffic.
    pub leds_set: Option<u8>,
    /// Latest lock bitmap requested by the input service, retained until it is
    /// accepted by the device.
    pub desired_leds: Option<u8>,
    /// Full USB frame of the most recent Output-report attempt.
    led_retry_frame: Option<u32>,

    /// How this device is reached: through the hub, or directly.
    ///
    /// Carried per interface because polling sets the context on the engine
    /// before each poll, and a full- or low-speed device behind a hub is not
    /// reachable without it.
    pub split: Option<crate::usb_channel::Split>,
    /// Frame number of the last poll, so the endpoint's interval is honoured.
    pub last_poll_frame: u32,
    /// A start-split is outstanding and the next poll collects its result.
    ///
    /// The two halves of a split reach the hub in different microframes, and this
    /// driver issues one half per call rather than waiting out that gap on the
    /// machine's time.
    pub split_pending: bool,
}

/// Hub port status speed codes (bits 9:10 of `wPortStatus`).
const HUB_PORT_SPEED_HIGH: u8 = 2;
const HUB_PORT_SPEED_LOW: u8 = 1;

/// Probe `port` for a device, reset it, and assign the next free address.
///
/// Returns the device's interfaces so the caller can decide what it is: a hub
/// port may hold a HID device, and on the LAN9514 the first port holds the
/// Ethernet controller. Classification is deliberately the caller's job — this
/// function only brings the device to a state where descriptors are readable.
///
/// Returns `None` when the port is empty or the device does not answer; a flaky
/// port must not take the whole controller down.
pub fn attach_port(
    engine: &UsbHostEngine<'_>,
    hub: &UsbHub<'_>,
    port: u16,
    next_addr: &mut u8,
) -> Option<(u8, Vec<InterfaceDesc>, Option<crate::usb_channel::Split>)> {
    // Everything before the device answers is traffic to the hub itself, which
    // sits on the root port and must not be reached through a split. Whatever
    // the previous port left behind is cleared before the first of them.
    engine.set_split(None);

    if !hub.is_port_connected(port) {
        return None;
    }

    print("[usb-hid] device on hub port ");
    print_u8(port as u8);
    println("");

    if hub.reset_port(port).is_err() {
        println("[usb-hid] WARN: port reset failed");
        hub.clear_port_change(port, crate::hub::C_PORT_RESET);
        return None;
    }
    hub.clear_port_change(port, crate::hub::C_PORT_CONNECTION);

    let addr = *next_addr;

    // A device behind the hub can be slower than the hub itself, and the core
    // will not run a high-speed channel programmed with 8-byte packets (nor
    // frame a full-speed device that answers in 8 bytes at 64). Take EP0's
    // starting size from this port's negotiated speed.
    let hub_speed = hub.port_speed(port);
    let ep0_mps = match hub_speed {
        HUB_PORT_SPEED_HIGH => {
            crate::usb_channel::initial_control_mps(crate::usb_channel::PORT_SPEED_HIGH)
        }
        _ => crate::usb_channel::initial_control_mps(crate::usb_channel::PORT_SPEED_FULL),
    };
    engine.set_control_mps(ep0_mps);

    // A full- or low-speed device behind this high-speed hub can only be reached
    // through it: the host asks the hub to buffer each transaction and then asks
    // for the result. Nothing on the bus answers otherwise, and the channel
    // reports XACTERR, which reads like a protocol error and is really a missing
    // transaction. A high-speed device is addressed directly and needs none of
    // this.
    let split = if hub_speed == HUB_PORT_SPEED_HIGH {
        None
    } else {
        Some(crate::usb_channel::Split {
            hub_addr: hub.address(),
            port: port as u8,
            low_speed: hub_speed == HUB_PORT_SPEED_LOW,
        })
    };
    engine.set_split(split);

    // Bring the device up. Every failure past this point has to clear the split
    // context on its way out: leaving it set routes the *next* transfer -- which
    // is hub traffic for the next port -- through a hub port, and the hub then
    // reports transaction errors for requests it never received.
    let brought_up = (|| -> Option<Vec<InterfaceDesc>> {
        // The device answers at address 0 until SET_ADDRESS latches.
        let device = read_device_descriptor(engine, 0)?;
        print("[usb-hid] vendor:product ");
        print_hex16(device.vendor_id);
        print(":");
        print_hex16(device.product_id);
        println("");

        UsbHub::set_address(engine, addr).ok()?;
        let (config_value, descriptors) = read_configuration(engine, addr)?;

        // Activate the configuration before any interface-level request: SET_IDLE
        // and SET_PROTOCOL are only answered in the configured state.
        UsbHub::set_configuration(engine, addr, config_value).ok()?;

        Some(descriptors)
    })();

    match brought_up {
        Some(descriptors) => {
            *next_addr = next_addr.saturating_add(1);
            Some((addr, descriptors, split))
        }
        None => {
            engine.set_split(None);
            None
        }
    }
}

/// Read a device descriptor from `addr`, retrying the short-read case.
///
/// USB 2.0 §9.6.1 lets a device answer the first 8 bytes of its device
/// descriptor and expect a second request for the full 18, so one short reply is
/// normal and only a second short reply means failure.
pub fn read_device_descriptor(
    engine: &UsbHostEngine<'_>,
    addr: u8,
) -> Option<usb_desc::DeviceDescriptor> {
    // The first SETUP after a port reset is regularly lost, and a failed
    // transfer must be retried rather than abandoned: an abandoned control
    // transaction leaves the device mid-stage, and its *next* transaction then
    // stalls in the status phase. A `.ok()?` here would end the whole
    // enumeration on that one transient failure.
    for attempt in 0..ENUM_ATTEMPTS {
        let mut buf = [0u8; 18];
        let result = engine.control_transfer(
            addr,
            RT_DEV_TO_HOST_STANDARD,
            usb_desc::REQ_GET_DESCRIPTOR,
            (usb_desc::DT_DEVICE as u16) << 8,
            0,
            &mut buf,
        );

        match result {
            // USB 2.0 §9.6.1 lets a device answer the first 8 bytes of its
            // device descriptor and expect a second request for the full 18;
            // a short reply is therefore normal and worth retrying.
            Ok(n) if n >= 18 => {
                let device = usb_desc::DeviceDescriptor::parse(&buf)?;
                // The device descriptor is the only place a device reports its
                // control-endpoint packet size, and every later transfer on this
                // engine depends on it — adopt it before returning.
                engine.set_control_mps(device.max_packet_size_0);
                return Some(device);
            }
            _ => {
                if attempt + 1 < ENUM_ATTEMPTS {
                    delay_ms(10);
                }
            }
        }
    }
    None
}

/// Read a device's active configuration, returning its config value and the
/// interfaces it declares.
///
/// The 9-byte header is read first because `wTotalLength` is authoritative: a
/// fixed-size read either truncates a large configuration or mis-frames a small
/// one, and either way the interface walk then reads the wrong bytes.
pub fn read_configuration(
    engine: &UsbHostEngine<'_>,
    addr: u8,
) -> Option<(u8, Vec<InterfaceDesc>)> {
    let mut header = [0u8; 9];
    let mut header_len = 0usize;
    for attempt in 0..ENUM_ATTEMPTS {
        header = [0u8; 9];
        match engine.control_transfer(
            addr,
            RT_DEV_TO_HOST_STANDARD,
            usb_desc::REQ_GET_DESCRIPTOR,
            (usb_desc::DT_CONFIGURATION as u16) << 8,
            0,
            &mut header,
        ) {
            Ok(n) if n >= 9 => {
                header_len = n;
                break;
            }
            _ => {
                if attempt + 1 < ENUM_ATTEMPTS {
                    delay_ms(10);
                }
            }
        }
    }
    if header_len < 9 {
        return None;
    }
    let total = usb_desc::configuration_total_length(&header)? as usize;
    if !(9..=512).contains(&total) {
        return None;
    }

    let mut cfg = alloc::vec![0u8; total];
    let mut cn = 0usize;
    for attempt in 0..ENUM_ATTEMPTS {
        match engine.control_transfer(
            addr,
            RT_DEV_TO_HOST_STANDARD,
            usb_desc::REQ_GET_DESCRIPTOR,
            (usb_desc::DT_CONFIGURATION as u16) << 8,
            0,
            &mut cfg,
        ) {
            Ok(n) if n >= 9 => {
                cn = n;
                break;
            }
            _ => {
                if attempt + 1 < ENUM_ATTEMPTS {
                    delay_ms(10);
                }
            }
        }
    }
    if cn < 9 {
        return None;
    }

    let config_value = cfg.get(5).copied().unwrap_or(1);
    Some((
        config_value,
        usb_desc::parse_configuration(&cfg[..cn.min(total)]),
    ))
}

/// What the root device turned out to be.
pub enum RootClass {
    /// A hub — its downstream ports must be scanned for devices.
    Hub,
    /// A HID device sitting directly on the root port.
    Hid,
    /// Something this driver does not claim.
    Other,
}

/// Classify a device from its device and interface descriptors.
///
/// A hub declares class 0x09 either at device level (typical for a wired hub
/// like the LAN9514) or per interface, so both are checked. Interface classes
/// decide everything else, because §9.6.1 leaves the device-level class at 0 for
/// a device that mixes classes — which a combo receiver does.
pub fn classify(device: &usb_desc::DeviceDescriptor, interfaces: &[InterfaceDesc]) -> RootClass {
    const CLASS_HUB: u8 = 0x09;
    if device.class == CLASS_HUB || interfaces.iter().any(|i| i.class == CLASS_HUB) {
        return RootClass::Hub;
    }
    if interfaces.iter().any(|i| i.is_hid()) {
        return RootClass::Hid;
    }
    RootClass::Other
}

/// Bring up one HID interface: fetch its report descriptor, choose a decode
/// path, and select the protocol the decoder expects.
fn start_interface(
    engine: &UsbHostEngine<'_>,
    dev_addr: u8,
    iface: &InterfaceDesc,
) -> Option<HidInterface> {
    let ep = *iface.interrupt_in.first()?;
    let kind = match iface.protocol {
        usb_desc::PROTOCOL_KEYBOARD => HidKind::Keyboard,
        usb_desc::PROTOCOL_MOUSE => HidKind::Mouse,
        _ => HidKind::Unknown,
    };

    print("[usb-hid] HID interface ");
    print_u8(iface.number);
    print(" class=");
    print_u8(iface.class);
    print(" boot=");
    print_u8(iface.subclass);
    println("");

    // ── Report descriptor → full decoder ─────────────────────────────────────
    //
    // The HID descriptor's wDescriptorLength is authoritative; the same request
    // with a short buffer is what truncated descriptors come from.
    let mut decoder = None;
    let mut descriptor_leds = None;
    if let Some(len) = iface.report_descriptor_len {
        let len = len as usize;
        if len > 0 && len <= 1024 {
            let mut rd = alloc::vec![0u8; len];
            let got = engine
                .control_transfer(
                    dev_addr,
                    RT_DEV_TO_HOST_STANDARD,
                    usb_desc::REQ_GET_DESCRIPTOR,
                    (usb_desc::DT_HID_REPORT as u16) << 8,
                    iface.number as u16,
                    &mut rd,
                )
                .unwrap_or(0);
            if got > 0 {
                let map = crate::hid::parse_report_descriptor(&rd[..got]);
                if !map.reports.is_empty() {
                    println("[usb-hid] report descriptor parsed");
                    descriptor_leds = LedOutput::from_map(&map);
                    decoder = Some(HidDecoder::new(map));
                } else {
                    println("[usb-hid] WARN: report descriptor declared no fields");
                }
            }
        }
    }

    // ── Lock LEDs ────────────────────────────────────────────────────────────
    //
    // The keyboard lights Caps/Num/Scroll Lock itself, and nothing but an Output
    // report can set them. The report the descriptor declares wins, report ID
    // and bit order included; a boot-capable interface that declared none takes
    // the fixed boot byte, which is what the fallback path is polling through.
    let leds = descriptor_leds.or_else(|| iface.supports_boot().then(LedOutput::boot));
    match leds.as_ref() {
        Some(l) => {
            print("[usb-hid] iface ");
            print_u8(iface.number);
            print(" LED output report=");
            print_usize_hid(l.report_id() as usize);
            print(" len=");
            print_usize_hid(l.payload_len());
            println("");
        }
        None => {
            print("[usb-hid] iface ");
            print_u8(iface.number);
            println(" declares no LED output report");
        }
    }

    // ── Idle + protocol ──────────────────────────────────────────────────────
    //
    // SET_IDLE(0) stops the device resending an unchanged report forever, which
    // would otherwise flood the poll loop and the input service.
    let _ = engine.control_transfer(
        dev_addr,
        RT_CLASS_INTERFACE_OUT,
        usb_desc::REQ_HID_SET_IDLE,
        0,
        iface.number as u16,
        &mut [],
    );

    // Keep report protocol when a descriptor drives decoding (boot protocol
    // would change the report shape and desynchronise it). Only a device using
    // the boot fallback is switched to boot protocol.
    if iface.supports_boot() && decoder.is_none() {
        let _ = engine.control_transfer(
            dev_addr,
            RT_CLASS_INTERFACE_OUT,
            usb_desc::REQ_HID_SET_PROTOCOL,
            0, // 0 = boot protocol
            iface.number as u16,
            &mut [],
        );
    }

    // The split context is captured here and re-applied before every poll, so a
    // context that does not match the device is a split aimed at the wrong hub
    // port -- which the hub answers NYET forever, because the translator there
    // has no such device. That is indistinguishable from a slow device unless
    // the pairing is printed.
    let split = engine.split();
    print("[usb-hid] iface ");
    print_u8(iface.number);
    print(" addr=");
    print_usize_hid(dev_addr as usize);
    print(" port=");
    print_usize_hid(split.map_or(0, |s| s.port as usize));
    print(" hub=");
    print_usize_hid(split.map_or(0, |s| s.hub_addr as usize));
    println("");

    Some(HidInterface {
        device: HidDeviceId(0),
        split,
        last_poll_frame: 0,
        split_pending: false,
        channel: 0, // assigned by the caller
        dev_addr,
        interface: iface.number,
        endpoint: ep,
        decoder,
        boot_state: BootState::default(),
        kind,
        leds,
        leds_set: None,
        desired_leds: None,
        led_retry_frame: None,
    })
}

/// Start every drivable HID interface of one already-configured device.
pub fn start_hid_interfaces(
    engine: &UsbHostEngine<'_>,
    addr: u8,
    interfaces: &[InterfaceDesc],
    channel_cursor: usize,
    out: &mut Vec<HidInterface>,
) -> usize {
    let mut started = 0usize;
    for iface in interfaces.iter().filter(|i| i.is_hid() && i.alternate == 0) {
        if out.len() >= MAX_HID_INTERFACES {
            break;
        }
        let Some(mut hid) = start_interface(engine, addr, iface) else {
            continue;
        };
        hid.device = HidDeviceId((out.len() + 1) as u32);
        hid.channel = channel_cursor + out.len();
        out.push(hid);
        started += 1;
    }
    started
}

/// Enumerate every HID device behind `hub`, filling `out`.
///
/// Ports are scanned in order and each device takes the next free host channel.
/// `next_addr` carries the USB address counter across ports so two devices never
/// collide on one address.
pub fn enumerate(
    engine: &UsbHostEngine<'_>,
    hub: &UsbHub<'_>,
    port_count: u8,
    next_addr: &mut u8,
    out: &mut Vec<HidInterface>,
) {
    for port in 1..=port_count as u16 {
        if out.len() >= MAX_HID_INTERFACES {
            return;
        }
        let Some((addr, descriptors, split)) = attach_port(engine, hub, port, next_addr) else {
            continue;
        };
        // The interfaces are brought up through the same path the device was.
        engine.set_split(split);
        let started = start_hid_interfaces(engine, addr, &descriptors, HID_CHANNEL_BASE, out);
        // Back to direct addressing: the next thing talked to may be the hub
        // itself, and a stale context would send that through a hub port.
        engine.set_split(None);
        if started == 0 {
            println("[usb-hid] port holds no drivable HID interface");
        }
    }
}

/// Poll one interface and append the events it produced.
///
/// A NAK (no report queued) is the normal idle case and yields nothing.
pub fn poll_interface(
    engine: &UsbHostEngine<'_>,
    iface: &mut HidInterface,
    out: &mut Vec<EvdevEvent>,
) {
    // The endpoint states how often it wants to be asked and the host is expected
    // to honour it: polling faster than that replaces whatever the hub still has
    // buffered before a complete-split can collect it, and the endpoint then reads
    // as permanently idle. `HFNUM` counts the same frames the interval is stated
    // in, so the value is used as given.
    //
    // A poll with half a split outstanding is exempt, because its other half is
    // what collects the report and the hub pairs the two for only a few frames.
    // Frames, not microframes. The endpoint's interval is a millisecond count, and
    // a poll loop that ignores it polls at whatever phase it happens to arrive at --
    // which is what kept the hub's translator without a schedule to run the
    // transaction on. It answered every complete-split with NYET, because at that
    // point in the transfer it had nothing to collect.
    let now = engine.full_frame_number();
    let interval = (iface.endpoint.interval as u32).max(1);
    if !iface.split_pending
        && iface.last_poll_frame != 0
        && now.wrapping_sub(iface.last_poll_frame) & 0xFFFF < interval
    {
        return;
    }
    iface.last_poll_frame = now;

    let mut buf = [0u8; REPORT_BUF];
    // The device is addressed the way it is wired for the whole of this call,
    // including the recovery below. A full- or low-speed device behind a hub is
    // reached only through a split, and a transfer sent without one is answered
    // by nobody -- so clearing the context before the recovery turns one stalled
    // endpoint into a device that can never be reached again.
    engine.set_split(iface.split);
    let result = engine.interrupt_receive(
        iface.channel,
        iface.dev_addr,
        iface.endpoint.number(),
        iface.endpoint.max_packet_size,
        &mut buf,
        &mut iface.split_pending,
    );

    let got = match result {
        Ok(n) => n,
        // Only a stall needs clearing. Everything else a poll can end on is a
        // transfer that did not deliver this time: a NYET is the hub asking for
        // time, a NAK is a keyboard with no key down, and a halt with no status is
        // the core ending a periodic channel at its frame boundary. Clearing a
        // halt that was never set cost a control transfer on every poll and
        // interrupted the polling it was meant to help.
        Err(_) if !engine.last_was_stall() => {
            engine.set_split(None);
            return;
        }
        Err(_) => {
            // A STALL leaves the endpoint halted until the host clears it.
            // Dropping the device here would lose a working keyboard over one
            // bad report, so clear the stall and keep polling.
            let _ = engine.control_transfer(
                iface.dev_addr,
                0x02, // Standard endpoint OUT
                0x01, // CLEAR_FEATURE
                0,    // ENDPOINT_HALT
                iface.endpoint.address as u16,
                &mut [],
            );
            engine.set_split(None);
            return;
        }
    };

    // Back to direct addressing: a later transfer to the hub or to the
    // controller's own device must not be routed through a hub port.
    engine.set_split(None);
    if got == 0 {
        return;
    }

    match iface.decoder.as_mut() {
        Some(dec) => dec.process(&buf[..got], out),
        None => {
            // Boot fallback: the interface protocol says which shape to expect.
            let kind = match iface.kind {
                HidKind::Unknown => detect_boot_kind(&buf[..got]),
                k => k,
            };
            decode_boot_report(kind, &buf[..got], &mut iface.boot_state, out);
        }
    }
}

/// Guess a boot report's class from its length when the interface said nothing.
///
/// A boot keyboard report is always 8 bytes; a boot mouse is 3 or 4.
fn detect_boot_kind(report: &[u8]) -> HidKind {
    match report.len() {
        8 => HidKind::Keyboard,
        3 | 4 => HidKind::Mouse,
        _ => HidKind::Unknown,
    }
}

/// Retain the latest lock bitmap requested by the input service.
///
/// A failed Output transfer is transient on a split hub path. Keeping only the
/// latest desired state means rapid CAPS/NUM presses never replay stale states,
/// while the next service pass can retry the state the user actually sees.
pub fn request_leds(iface: &mut HidInterface, leds: u8) {
    if iface.leds.is_none() || iface.leds_set == Some(leds) {
        iface.desired_leds = None;
        return;
    }
    iface.desired_leds = Some(leds);
    // A changed lock state is eligible immediately; only failed retries are
    // rate-limited.
    iface.led_retry_frame = None;
}

/// Retry the latest pending lock-key LED state after HID polling.
///
/// The retry is deliberately serviced after interrupt IN polling, so the
/// complete-split of an in-flight keyboard report cannot be displaced by an
/// endpoint-zero transaction. `desired_leds` clears only after the keyboard
/// accepts the report.
pub fn flush_leds(engine: &UsbHostEngine<'_>, iface: &mut HidInterface) {
    let Some(leds) = iface.desired_leds else {
        return;
    };
    let Some(output) = iface.leds.as_ref() else {
        iface.desired_leds = None;
        return;
    };

    let now = engine.full_frame_number();
    if let Some(last) = iface.led_retry_frame {
        if now.wrapping_sub(last) & 0xFFFF < LED_RETRY_FRAMES {
            return;
        }
    }
    iface.led_retry_frame = Some(now);

    let mut payload = [0u8; MAX_LED_REPORT];
    let len = output.encode(leds, &mut payload);
    let report_id = output.report_id() as u16;

    // The device is addressed the way it is wired, exactly as polling addresses
    // it: a full- or low-speed keyboard behind a hub is unreachable without the
    // split context, and this request would be answered by nobody.
    engine.set_split(iface.split);
    let result = engine.control_transfer(
        iface.dev_addr,
        RT_CLASS_INTERFACE_OUT,
        usb_desc::REQ_HID_SET_REPORT,
        ((usb_desc::REPORT_TYPE_OUTPUT as u16) << 8) | report_id,
        iface.interface as u16,
        &mut payload[..len],
    );
    engine.set_split(None);

    match result {
        Ok(_) => {
            iface.leds_set = Some(leds);
            iface.desired_leds = None;
            print("[usb-hid] iface ");
            print_u8(iface.interface);
            print(" LEDs 0x");
            print_hex_u8(leds);
            println("");
        }
        Err(_) => match led_failure_disposition(engine.last_was_stall()) {
            LedFailureDisposition::Retry => {
                print("[usb-hid] WARN: iface ");
                print_u8(iface.interface);
                println(" deferred the LED output report");
            }
            LedFailureDisposition::Reject => {
                // USB STALL is the device declining this report, not a busy hub.
                // Do not repeatedly consume endpoint zero or flood the console;
                // a later lock-state transition is still a fresh request.
                iface.desired_leds = None;
                iface.led_retry_frame = None;
                print("[usb-hid] WARN: iface ");
                print_u8(iface.interface);
                println(" rejected the LED output report; waiting for lock-state change");
            }
        },
    }
}

/// Send one device-identified event to the input service.
pub fn forward_device_event(tid: usize, device: HidDeviceId, ev: &EvdevEvent) -> bool {
    if tid == 0 {
        return false;
    }
    let mut buf = [0u8; DEVICE_EVENT_LEN];
    ev.encode_device(device, &mut buf);
    matches!(ostd::syscall::sys_send(tid, &buf), SyscallResult::Ok(_))
}

/// Register this cell as a raw event producer with the input service.
///
/// One typed request, sent before any event. After it the service routes this
/// sender's short `[opcode][code][value]` messages down its event path.
pub fn register_as_source(tid: usize, kind: u8) -> bool {
    if tid == 0 {
        return false;
    }
    let mut buf = [0u8; 32];
    match api::ipc::encode(
        &api::ipc::InputRequest::RegisterEventSource { kind },
        &mut buf,
    ) {
        // TrySend reports a refused non-blocking delivery as Ok(usize::MAX),
        // not Err. Only Ok(0) means the input service received or queued the
        // registration; otherwise the serving loop must retry it.
        Ok(encoded) => matches!(
            ostd::syscall::sys_try_send(tid, encoded),
            SyscallResult::Ok(0)
        ),
        Err(_) => false,
    }
}

// ─── Tiny formatting helpers (no `format!` in the report hot path) ────────────

fn print_usize_hid(v: usize) {
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

fn print_hex_u8(v: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let out = [HEX[(v >> 4) as usize], HEX[(v & 0xF) as usize]];
    if let Ok(s) = core::str::from_utf8(&out) {
        print(s);
    }
}

fn print_hex16(v: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        let nibble = (v >> (12 - i * 4)) & 0xF;
        *slot = HEX[nibble as usize];
    }
    if let Ok(s) = core::str::from_utf8(&out) {
        print(s);
    }
}

fn print_u8(v: u8) {
    let mut out = [0u8; 3];
    let mut n = v;
    let mut len = 0;
    if n == 0 {
        print("0");
        return;
    }
    while n > 0 && len < out.len() {
        out[len] = b'0' + (n % 10);
        n /= 10;
        len += 1;
    }
    out[..len].reverse();
    if let Ok(s) = core::str::from_utf8(&out[..len]) {
        print(s);
    }
}

/// Keep the compiler honest about the unused-import warning when the descriptor
/// helpers are only touched on some paths.
#[allow(dead_code)]
fn _assert_desc_used(e: &EndpointDesc) -> u8 {
    e.interval
}

#[cfg(test)]
mod tests {
    use super::{led_failure_disposition, LedFailureDisposition};

    #[test]
    fn stalled_led_output_is_terminal_for_the_current_lock_state() {
        assert_eq!(
            led_failure_disposition(true),
            LedFailureDisposition::Reject,
            "a device-rejected SET_REPORT must not remain in the retry queue"
        );
    }

    #[test]
    fn transient_led_output_failure_remains_retryable() {
        assert_eq!(
            led_failure_disposition(false),
            LedFailureDisposition::Retry,
            "a non-STALL transfer failure can be retried after the hub is ready"
        );
    }
}
