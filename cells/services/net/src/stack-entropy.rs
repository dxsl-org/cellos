//! Boot entropy for the smoltcp stack: the interface RNG seed and the
//! ephemeral-port cursor.
//!
//! smoltcp draws TCP initial sequence numbers, DNS transaction ids, DNS source
//! ports and IPv4 ids from `Config::random_seed`. Left at its default of 0,
//! every boot replays the same sequence and an off-path peer can predict them
//! (spoofed connections, RST/data injection, DNS poisoning). Connect-side source
//! ports were a plain counter from 49152 for the same reason.
//!
//! Both come from [`ViRng`], the same fail-closed source TLS uses: a kernel that
//! serves no entropy stops the service here rather than running a predictable
//! stack. A development kernel built with `dev-weak-rng` still serves bytes.

use core::sync::atomic::{AtomicU16, Ordering};

use ostd::sync::Mutex;
use rand_core::RngCore;
use smoltcp::{iface::Config, wire::HardwareAddress};

use crate::tls::rng::ViRng;

/// IANA dynamic-port range (RFC 6335), inclusive.
const EPHEMERAL_FIRST: u16 = 49152;
const EPHEMERAL_SPAN: u32 = (u16::MAX - EPHEMERAL_FIRST) as u32 + 1;

/// Largest single cursor advance (RFC 6056 §3.3.5, random increments). Small
/// enough that a port is not handed out again until ~500 allocations later,
/// large enough that one observed port does not name the next.
const MAX_PORT_STEP: u32 = 64;

/// Port stream drawn after the interface seed; `None` until [`seed_stack`] runs.
static PORT_RNG: Mutex<Option<ViRng>> = Mutex::new(None);
/// Next ephemeral port. Single-threaded service, so load/store is not racing.
static NEXT_PORT: AtomicU16 = AtomicU16::new(EPHEMERAL_FIRST);

/// Draw the smoltcp `Config::random_seed` and start the ephemeral-port cursor at
/// a random point. Call once, before the interface exists.
///
/// # Panics
/// When the kernel serves no entropy (see [`ViRng::new`]).
fn seed_stack() -> u64 {
    let mut rng = ViRng::new();
    let seed = rng.next_u64();
    NEXT_PORT.store(
        advance_port(EPHEMERAL_FIRST, rng.next_u32()),
        Ordering::Relaxed,
    );
    *PORT_RNG.lock() = Some(rng);
    seed
}

/// The smoltcp interface configuration the service runs with: `mac`, and a
/// seed drawn by [`seed_stack`].
///
/// # Panics
/// When the kernel serves no entropy (see [`ViRng::new`]).
pub(crate) fn seeded_config(mac: HardwareAddress) -> Config {
    let mut config = Config::new(mac);
    config.random_seed = seed_stack();
    config
}

/// Hand out the next connect-side source port.
pub(crate) fn next_ephemeral_port() -> u16 {
    let draw = PORT_RNG.lock().as_mut().map(|rng| rng.next_u32());
    let port = NEXT_PORT.load(Ordering::Relaxed);
    NEXT_PORT.store(advance_port(port, port_step(draw)), Ordering::Relaxed);
    port
}

/// Cursor advance for one allocation: 1..=[`MAX_PORT_STEP`] from the port
/// stream, or 1 before the stack is seeded (host tests only — `run()` seeds
/// before any socket exists).
fn port_step(draw: Option<u32>) -> u32 {
    draw.map_or(1, |draw| 1 + draw % MAX_PORT_STEP)
}

/// `port + step`, wrapped inside the dynamic range. A port below the range
/// (never stored, but cheap to rule out) is treated as its first port.
fn advance_port(port: u16, step: u32) -> u16 {
    let offset = u32::from(port.saturating_sub(EPHEMERAL_FIRST));
    // Both terms are below EPHEMERAL_SPAN, so the wrapped sum fits in u16.
    EPHEMERAL_FIRST + ((offset + step % EPHEMERAL_SPAN) % EPHEMERAL_SPAN) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::wire::EthernetAddress;

    #[test]
    fn service_interface_config_is_seeded() {
        // Regression: the interface was built with `Config::new` alone, whose
        // `random_seed` is 0 on every boot.
        let mac = HardwareAddress::Ethernet(EthernetAddress([0x52, 0x54, 0, 0x12, 0x34, 0x56]));
        assert_ne!(Config::new(mac).random_seed, seeded_config(mac).random_seed);
    }

    #[test]
    fn advance_stays_inside_the_dynamic_range() {
        assert_eq!(advance_port(EPHEMERAL_FIRST, 0), EPHEMERAL_FIRST);
        assert_eq!(advance_port(u16::MAX, 1), EPHEMERAL_FIRST);
        assert_eq!(advance_port(u16::MAX - 1, 1), u16::MAX);
        assert_eq!(
            advance_port(u16::MAX, EPHEMERAL_SPAN + 2),
            EPHEMERAL_FIRST + 1
        );
        assert_eq!(advance_port(1024, 3), EPHEMERAL_FIRST + 3);
    }

    #[test]
    fn steps_are_bounded_random_increments() {
        assert_eq!(port_step(None), 1);
        assert_eq!(port_step(Some(0)), 1);
        assert_eq!(port_step(Some(MAX_PORT_STEP - 1)), MAX_PORT_STEP);
        assert_eq!(port_step(Some(u32::MAX)), 1 + u32::MAX % MAX_PORT_STEP);
    }
}
