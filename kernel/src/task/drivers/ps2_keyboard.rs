//! Polled i8042 keyboard and PS/2 mouse, with VMware/QEMU absolute pointer support.
//!
//! Physical input stays in Linux evdev form until service-input translates it.
//! Only the headless console fallback performs ASCII/ANSI translation.

use crate::sync::Spinlock;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const STATUS_OUTPUT_FULL: u8 = 0x01;
const STATUS_INPUT_FULL: u8 = 0x02;
const STATUS_MOUSE_DATA: u8 = 0x20;
const STATUS_ERROR: u8 = 0xC0;
const WAIT_LIMIT: usize = 100_000;
const POLL_LIMIT: usize = 64;

/// Kernel input-service wire fields: EV_KEY=0, EV_REL=1, EV_ABS=2 (guest pixels).
#[derive(Clone, Copy, Debug)]
pub struct Ps2Event {
    pub opcode: u8,
    pub code: u32,
    pub value: i32,
}

struct Ps2State {
    extended: bool,
    pause_idx: usize,
    pressed: [bool; 144],
    caps_lock: bool,
    buf: [u8; 8],
    buf_len: usize,
    buf_pos: usize,
    mouse_enabled: bool,
    mouse_buf: [u8; 3],
    mouse_idx: usize,
    mouse_buttons: u8,
    absolute_mouse: bool,
    // One packet produces at most six events (X/Y, wheel, three buttons).
    // Hardware packets are read only when this queue is empty.
    events: [Option<Ps2Event>; 8],
    event_head: usize,
    event_len: usize,
}

static STATE: Spinlock<Ps2State> = Spinlock::new(Ps2State {
    extended: false,
    pause_idx: 0,
    pressed: [false; 144],
    caps_lock: false,
    buf: [0; 8],
    buf_len: 0,
    buf_pos: 0,
    mouse_enabled: false,
    mouse_buf: [0; 3],
    mouse_idx: 0,
    mouse_buttons: 0,
    absolute_mouse: false,
    events: [None; 8],
    event_head: 0,
    event_len: 0,
});

#[inline]
fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY: reading I/O port on x86 is a Ring 0 instruction without memory aliasing.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    }
    val
}

#[inline]
fn outb(port: u16, val: u8) {
    // SAFETY: writing I/O port on x86 is a Ring 0 instruction without memory aliasing.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
    }
}

fn wait_write() -> bool {
    for _ in 0..WAIT_LIMIT {
        if inb(STATUS_PORT) & STATUS_INPUT_FULL == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn command(byte: u8) -> bool {
    if !wait_write() {
        return false;
    }
    outb(STATUS_PORT, byte);
    true
}

fn write_data(byte: u8) -> bool {
    if !wait_write() {
        return false;
    }
    outb(DATA_PORT, byte);
    true
}

/// Initialization holds both device clocks disabled while reading the command
/// byte. Device responses must come from the requested port and pass status checks.
fn read_response(aux: bool) -> Option<u8> {
    for _ in 0..WAIT_LIMIT {
        let status = inb(STATUS_PORT);
        if status & STATUS_OUTPUT_FULL != 0 {
            let byte = inb(DATA_PORT);
            if status & STATUS_ERROR != 0 || (status & STATUS_MOUSE_DATA != 0) != aux {
                return None;
            }
            return Some(byte);
        }
        core::hint::spin_loop();
    }
    None
}

fn mouse_command(byte: u8) -> bool {
    // RESEND is part of the PS/2 device protocol; never treat it as an ACK.
    for _ in 0..3 {
        if !command(0xD4) || !write_data(byte) {
            return false;
        }
        match read_response(true) {
            Some(0xFA) => return true,
            Some(0xFE) => continue,
            _ => return false,
        }
    }
    false
}

fn init_controller() -> Result<(), &'static str> {
    if !command(0xAD) || !command(0xA7) || !wait_write() {
        return Err("disabling device clocks timed out");
    }
    // Discard firmware leftovers only at boot, with both producers stopped.
    for _ in 0..32 {
        if inb(STATUS_PORT) & STATUS_OUTPUT_FULL == 0 {
            break;
        }
        let _ = inb(DATA_PORT);
    }
    if inb(STATUS_PORT) & STATUS_OUTPUT_FULL != 0 || !command(0x20) {
        return Err("reading controller configuration timed out");
    }
    let config = read_response(false).ok_or("missing controller configuration")?;
    // Poll both ports: IRQ1/IRQ12 must remain disabled. The kernel has no PS/2
    // interrupt handlers. Enable Set-2 -> Set-1 keyboard translation explicitly (bit 6 = 1),
    // and enable both keyboard clock (bit 4 = 0) and mouse clock (bit 5 = 0).
    let config = (config | 0x40) & !0x33;
    if !command(0x60) || !write_data(config) || !command(0xAE) || !command(0xA8) {
        return Err("programming polling configuration timed out");
    }
    // Defaults select the standard stream settings; no wheel negotiation is
    // performed, so the parser consumes three-byte packets.
    if !mouse_command(0xF6) || !mouse_command(0xF4) {
        return Err("mouse defaults/reporting command was not acknowledged");
    }
    Ok(())
}

// VMware's low-bandwidth backdoor protocol, independently implemented from the
// register/command definitions in QEMU hw/i386/{vmport,vmmouse}.c and
// include/hw/i386/vmport.h. No external implementation is copied.
const VM_MAGIC: u32 = 0x564D5868;
const VM_MOUSE_COMMAND: u32 = 41;
const VM_MOUSE_STATUS: u32 = 40;
const VM_MOUSE_DATA: u32 = 39;

#[cfg(target_arch = "x86_64")]
fn vm_command(command: u32, argument: u32) -> [u32; 4] {
    let mut a = VM_MAGIC;
    let mut b = argument;
    let mut c = command;
    let mut d = 0x5658u32;
    // SAFETY: called at ring 0 only after CPUID reports a hypervisor. The
    // GETVERSION magic handshake precedes mouse commands. RBX is reserved by
    // LLVM on some targets, so preserve it explicitly and exchange EBX via ESI.
    // These commands return at most four words; ESI/EDI are otherwise unchanged.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov ebx, esi",
            "in eax, dx",
            "mov esi, ebx",
            "pop rbx",
            inout("eax") a,
            inout("esi") b,
            inout("ecx") c,
            inout("edx") d,
            inout("edi") 0u32 => _,
            options(preserves_flags),
        );
    }
    [a, b, c, d]
}

#[cfg(not(target_arch = "x86_64"))]
fn vm_command(_command: u32, _argument: u32) -> [u32; 4] {
    [u32::MAX; 4]
}

fn init_absolute_mouse() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        // Never touch the VMware I/O port on bare metal.
        if !crate::cpu_features::hypervisor_present() {
            return false;
        }
        let version = vm_command(10, !VM_MAGIC);
        if version[0] == u32::MAX || version[1] != VM_MAGIC {
            return false;
        }
        vm_command(VM_MOUSE_COMMAND, 0x45414552); // READ_ID
        let status = vm_command(VM_MOUSE_STATUS, 0)[0];
        if status != 1 {
            vm_command(VM_MOUSE_COMMAND, 0xF5);
            return false;
        }
        if vm_command(VM_MOUSE_DATA, 1)[0] != 0x3442554A {
            vm_command(VM_MOUSE_COMMAND, 0xF5);
            return false;
        }
        vm_command(VM_MOUSE_COMMAND, 0x53424152); // REQUEST_ABSOLUTE
        return true;
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

fn absolute_pixel(value: u32, extent: u16) -> i32 {
    // Both endpoints are inclusive: 0 -> 0, 65535 -> extent - 1.
    // Round to nearest pixel, avoiding a systematic one-pixel bias.
    ((u64::from(value.min(65535)) * u64::from(extent.saturating_sub(1)) + 32767)
        / 65535) as i32
}

pub fn init() {
    let mut state = STATE.lock();
    match init_controller() {
        Ok(()) => state.mouse_enabled = true,
        Err(reason) => {
            state.mouse_enabled = false;
            let _ = command(0xA7);
            log::warn!("PS/2 mouse unavailable: {}", reason);
        }
    }
    // Keyboard fallback remains available even if the auxiliary port is absent.
    if !command(0xAE) {
        log::warn!("PS/2 keyboard port enable timed out");
    }
    if state.mouse_enabled && init_absolute_mouse() {
        state.absolute_mouse = true;
        log::info!("VMware/QEMU absolute pointer enabled (EV_ABS guest pixels)");
    }
}

impl Ps2State {
    fn push(&mut self, event: Ps2Event) {
        // poll_event never reads another byte until the preceding batch drains.
        let tail = (self.event_head + self.event_len) % self.events.len();
        self.events[tail] = Some(event);
        self.event_len += 1;
    }

    fn pop(&mut self) -> Option<Ps2Event> {
        if self.event_len == 0 {
            return None;
        }
        let event = self.events[self.event_head].take();
        self.event_head = (self.event_head + 1) % self.events.len();
        self.event_len -= 1;
        event
    }

    fn key(&mut self, code: u32, released: bool) {
        let pressed = &mut self.pressed[code as usize];
        let value = if released { 0 } else if *pressed { 2 } else { 1 };
        *pressed = !released;
        if code == 58 && value == 1 {
            self.caps_lock = !self.caps_lock;
        }
        self.push(Ps2Event { opcode: 0, code, value });
    }

    fn keyboard_byte(&mut self, byte: u8) {
        // Pause has no break code: E1 1D 45 E1 9D C5. Do not expose its fake
        // Ctrl/NumLock bytes to the modifier tracker.
        const PAUSE: [u8; 5] = [0x1D, 0x45, 0xE1, 0x9D, 0xC5];
        if self.pause_idx != 0 {
            if byte == PAUSE[self.pause_idx - 1] {
                self.pause_idx += 1;
                if self.pause_idx == 6 {
                    self.pause_idx = 0;
                    self.key(119, false);
                    self.key(119, true);
                }
                return;
            }
            self.pause_idx = 0;
        }
        if byte == 0xE1 {
            self.extended = false;
            self.pause_idx = 1;
            return;
        }
        if byte == 0xE0 {
            self.extended = true;
            return;
        }
        let extended = core::mem::replace(&mut self.extended, false);
        let scan = byte & 0x7F;
        let code = if extended {
            match scan {
                0x1C => 96,  // keypad Enter
                0x1D => 97,  // right Ctrl
                0x35 => 98,  // keypad /
                0x37 => 99,  // PrintScreen (ignore its E0 fake Shift bytes)
                0x38 => 100, // right Alt
                0x46 => 119, // Ctrl-Break
                0x47 => 102, // Home
                0x48 => 103, // Up
                0x49 => 104, // PageUp
                0x4B => 105, // Left
                0x4D => 106, // Right
                0x4F => 107, // End
                0x50 => 108, // Down
                0x51 => 109, // PageDown
                0x52 => 110, // Insert
                0x53 => 111, // Delete
                0x5B => 125, // left Meta
                0x5C => 126, // right Meta
                0x5D => 127, // Compose/Menu
                0x5E => 116, // Power
                0x5F => 142, // Sleep
                0x63 => 143, // Wake
                _ => return,
            }
        } else {
            match scan {
                0x01..=0x53 | 0x56..=0x58 => scan as u32,
                0x54 => 99, // SysRq
                _ => return, // ACK, BAT, and invalid scancodes are not keys.
            }
        };
        self.key(code, byte & 0x80 != 0);
    }

    fn mouse_byte(&mut self, byte: u8) {
        if self.absolute_mouse || !self.mouse_enabled || (self.mouse_idx == 0 && byte & 0x08 == 0) {
            return;
        }
        self.mouse_buf[self.mouse_idx] = byte;
        self.mouse_idx += 1;
        if self.mouse_idx != 3 {
            return;
        }
        self.mouse_idx = 0;
        let [flags, x, y] = self.mouse_buf;
        // Overflow makes that axis unreliable. Keep button transitions, but do
        // not turn overflow bytes into huge cursor jumps.
        let dx = if flags & 0x40 != 0 { 0 } else {
            x as i32 - if flags & 0x10 != 0 { 256 } else { 0 }
        };
        let dy = if flags & 0x80 != 0 { 0 } else {
            -(y as i32 - if flags & 0x20 != 0 { 256 } else { 0 })
        };
        // Move before buttons: the compositor must hit-test the packet's new
        // cursor position, not the position preceding the click.
        if dx != 0 {
            self.push(Ps2Event { opcode: 1, code: 0, value: dx });
        }
        if dy != 0 {
            self.push(Ps2Event { opcode: 1, code: 1, value: dy });
        }
        self.buttons(flags & 0x07);
    }

    fn buttons(&mut self, buttons: u8) {
        let changed = self.mouse_buttons ^ buttons;
        for bit in 0..3 {
            let mask = 1 << bit;
            if changed & mask != 0 {
                self.push(Ps2Event {
                    opcode: 0,
                    code: 0x110 + bit,
                    value: i32::from(buttons & mask != 0),
                });
            }
        }
        self.mouse_buttons = buttons;
    }

    fn poll_absolute_mouse(&mut self) {
        if !self.absolute_mouse {
            return;
        }
        // Use the same authoritative geometry as GpuGetResolution/compositor.
        // Until scanout is registered, leave packets in the host queue.
        let Some((width, height)) =
            crate::resource_registry::display_framebuffer_resolution()
        else {
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        let status = vm_command(VM_MOUSE_STATUS, 0)[0];
        if status >> 16 != 0 || (status & 0xFFFF) % 4 != 0 {
            vm_command(VM_MOUSE_COMMAND, 0xF5); // DISABLE, reactivate PS/2 producer
            self.absolute_mouse = false;
            self.mouse_idx = 0;
            log::warn!("VMware mouse queue unavailable; using relative PS/2 fallback");
            return;
        }
        if status < 4 {
            return;
        }
        // Read exactly one packet and drain our queue before consuming another.
        // IPC backpressure therefore cannot overwrite a queued button release.
        let [flags, x, y, wheel] = vm_command(VM_MOUSE_DATA, 4);
        if flags & 0x10000 != 0 {
            self.push(Ps2Event { opcode: 1, code: 0, value: x as i32 });
            self.push(Ps2Event { opcode: 1, code: 1, value: y as i32 });
        } else {
            self.push(Ps2Event { opcode: 2, code: 0, value: absolute_pixel(x, width) });
            self.push(Ps2Event { opcode: 2, code: 1, value: absolute_pixel(y, height) });
        }
        // VMware Z is positive down; EV_REL/REL_WHEEL is positive up.
        if wheel != 0 {
            self.push(Ps2Event { opcode: 1, code: 8, value: (wheel as i32).saturating_neg() });
        }
        // Packet movement must reach hit-testing before its button transitions.
        let buttons = ((flags & 0x20) >> 5)
            | ((flags & 0x10) >> 3)
            | ((flags & 0x08) >> 1);
        self.buttons(buttons as u8);
    }

    fn poll_event(&mut self) -> Option<Ps2Event> {
        if let Some(event) = self.pop() {
            return Some(event);
        }
        // Prefixes, modifiers and AUX bytes are progress, not an empty FIFO.
        // Bound interrupt-side work while allowing a whole packet per poll.
        for _ in 0..POLL_LIMIT {
            let status = inb(STATUS_PORT);
            if status & STATUS_OUTPUT_FULL == 0 {
                break;
            }
            let byte = inb(DATA_PORT);
            if status & STATUS_ERROR != 0 {
                if status & STATUS_MOUSE_DATA != 0 {
                    self.mouse_idx = 0;
                } else {
                    self.extended = false;
                    self.pause_idx = 0;
                }
                continue;
            }
            if status & STATUS_MOUSE_DATA != 0 {
                self.mouse_byte(byte);
            } else {
                self.keyboard_byte(byte);
            }
            if let Some(event) = self.pop() {
                return Some(event);
            }
        }
        self.poll_absolute_mouse();
        self.pop()
    }

    fn ascii(&mut self, event: Ps2Event) -> Option<u8> {
        if event.opcode != 0 || event.value == 0 || event.code >= 144 {
            return None;
        }
        let sequence: &[u8] = match event.code {
            59 => b"\x1b[11~", 60 => b"\x1b[12~", 61 => b"\x1b[13~",
            62 => b"\x1b[14~", 63 => b"\x1b[15~", 64 => b"\x1b[17~",
            65 => b"\x1b[18~", 66 => b"\x1b[19~", 67 => b"\x1b[20~",
            68 => b"\x1b[21~", 87 => b"\x1b[23~", 88 => b"\x1b[24~",
            102 => b"\x1b[H", 103 => b"\x1b[A", 104 => b"\x1b[5~",
            105 => b"\x1b[D", 106 => b"\x1b[C", 107 => b"\x1b[F",
            108 => b"\x1b[B", 109 => b"\x1b[6~", 110 => b"\x1b[2~",
            111 => b"\x1b[3~",
            _ => b"",
        };
        if !sequence.is_empty() {
            self.buf[..sequence.len()].copy_from_slice(sequence);
            self.buf_len = sequence.len();
            self.buf_pos = 1;
            return Some(sequence[0]);
        }
        let shift = self.pressed[42] || self.pressed[54];
        let ch = match event.code {
            1 => Some(0x1B),
            14 => Some(0x08),
            15 => Some(b'\t'),
            28 | 96 => Some(b'\n'),
            57 => Some(b' '),
            98 => Some(b'/'),
            code => translate_scancode(code as u8, shift, self.caps_lock),
        };
        if self.pressed[29] || self.pressed[97] {
            if let Some(c) = ch {
                if c.is_ascii_alphabetic() {
                    return Some(c.to_ascii_uppercase() - b'A' + 1);
                }
            }
        }
        ch
    }
}

/// Consume one typed event. Caller must retain it if IPC delivery is blocked,
/// and must not call again until that event has been delivered.
pub fn poll_event() -> Option<Ps2Event> {
    STATE.lock().poll_event()
}

/// Headless/early-boot ASCII fallback; never used when service-input is online.
pub fn poll() -> Option<u8> {
    let mut state = STATE.lock();
    if state.buf_pos < state.buf_len {
        let byte = state.buf[state.buf_pos];
        state.buf_pos += 1;
        return Some(byte);
    }
    for _ in 0..POLL_LIMIT {
        let event = state.poll_event()?;
        if let Some(byte) = state.ascii(event) {
            return Some(byte);
        }
    }
    None
}

fn translate_scancode(scancode: u8, shift: bool, caps: bool) -> Option<u8> {
    let upper = shift ^ caps;
    match scancode {
        // Numbers and symbols row
        0x02 => Some(if shift { b'!' } else { b'1' }),
        0x03 => Some(if shift { b'@' } else { b'2' }),
        0x04 => Some(if shift { b'#' } else { b'3' }),
        0x05 => Some(if shift { b'$' } else { b'4' }),
        0x06 => Some(if shift { b'%' } else { b'5' }),
        0x07 => Some(if shift { b'^' } else { b'6' }),
        0x08 => Some(if shift { b'&' } else { b'7' }),
        0x09 => Some(if shift { b'*' } else { b'8' }),
        0x0A => Some(if shift { b'(' } else { b'9' }),
        0x0B => Some(if shift { b')' } else { b'0' }),
        0x0C => Some(if shift { b'_' } else { b'-' }),
        0x0D => Some(if shift { b'+' } else { b'=' }),

        // Letters
        0x1E => Some(if upper { b'A' } else { b'a' }),
        0x30 => Some(if upper { b'B' } else { b'b' }),
        0x2E => Some(if upper { b'C' } else { b'c' }),
        0x20 => Some(if upper { b'D' } else { b'd' }),
        0x12 => Some(if upper { b'E' } else { b'e' }),
        0x21 => Some(if upper { b'F' } else { b'f' }),
        0x22 => Some(if upper { b'G' } else { b'g' }),
        0x23 => Some(if upper { b'H' } else { b'h' }),
        0x17 => Some(if upper { b'I' } else { b'i' }),
        0x24 => Some(if upper { b'J' } else { b'j' }),
        0x25 => Some(if upper { b'K' } else { b'k' }),
        0x26 => Some(if upper { b'L' } else { b'l' }),
        0x32 => Some(if upper { b'M' } else { b'm' }),
        0x31 => Some(if upper { b'N' } else { b'n' }),
        0x18 => Some(if upper { b'O' } else { b'o' }),
        0x19 => Some(if upper { b'P' } else { b'p' }),
        0x10 => Some(if upper { b'Q' } else { b'q' }),
        0x13 => Some(if upper { b'R' } else { b'r' }),
        0x1F => Some(if upper { b'S' } else { b's' }),
        0x14 => Some(if upper { b'T' } else { b't' }),
        0x16 => Some(if upper { b'U' } else { b'u' }),
        0x2F => Some(if upper { b'V' } else { b'v' }),
        0x11 => Some(if upper { b'W' } else { b'w' }),
        0x2D => Some(if upper { b'X' } else { b'x' }),
        0x15 => Some(if upper { b'Y' } else { b'y' }),
        0x2C => Some(if upper { b'Z' } else { b'z' }),

        // Punctuation
        0x1A => Some(if shift { b'{' } else { b'[' }),
        0x1B => Some(if shift { b'}' } else { b']' }),
        0x27 => Some(if shift { b':' } else { b';' }),
        0x28 => Some(if shift { b'"' } else { b'\'' }),
        0x29 => Some(if shift { b'~' } else { b'`' }),
        0x2B => Some(if shift { b'|' } else { b'\\' }),
        0x33 => Some(if shift { b'<' } else { b',' }),
        0x34 => Some(if shift { b'>' } else { b'.' }),
        0x35 => Some(if shift { b'?' } else { b'/' }),

        _ => None,
    }
}

#[cfg(test)]
mod pointer_tests {
    use super::absolute_pixel;

    #[test]
    fn normalized_coordinates_use_actual_inclusive_extent() {
        for extent in [1, 800, 1080, 1280, 1920, u16::MAX] {
            assert_eq!(absolute_pixel(0, extent), 0);
            assert_eq!(absolute_pixel(65535, extent), i32::from(extent - 1));
            assert_eq!(absolute_pixel(u32::MAX, extent), i32::from(extent - 1));
        }
        assert_eq!(absolute_pixel(32768, 1280), 640);
        assert_eq!(absolute_pixel(32768, 800), 400);
        assert_eq!(absolute_pixel(0, 0), 0);
    }
}
