//! ViTlsClock — authenticated-time gate for relay mTLS.
//!
//! The platform/QEMU RTC is not authenticated and a process-local monotonic
//! latch would reset on restart, so neither can establish certificate time.
//! Until a protected authenticated time and persisted floor are supplied,
//! [`observe`] always returns `None`. Connection and handshake entry points
//! explicitly refuse that state before embedded-tls can treat `None` as
//! permission to skip validity checks.

use embedded_tls::TlsClock;

/// Apply the current fail-closed policy to an untrusted RTC observation.
///
/// Raw wall-clock values never become TLS certificate time. The argument is
/// retained only to make that security boundary explicit and testable.
fn reject_untrusted_time(_raw_rtc_secs: Option<u64>) -> Option<u64> {
    None
}

/// Return authenticated certificate time when the protected source exists.
///
/// Phase 3 deliberately has no such source, so relay TLS remains unavailable.
pub fn observe() -> Option<u64> {
    reject_untrusted_time(None)
}

/// A TLS clock that remains unavailable until protected time is integrated.
///
/// Callers gate connection/handshake entry with [`observe`] because
/// embedded-tls interprets `None` as "skip certificate validity".
pub struct ViTlsClock;

impl TlsClock for ViTlsClock {
    fn now() -> Option<u64> {
        observe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_platform_rtc_cannot_enable_tls() {
        assert_eq!(reject_untrusted_time(None), None);
        assert_eq!(reject_untrusted_time(Some(0)), None);
        assert_eq!(reject_untrusted_time(Some(1_800_000_000)), None);
        assert_eq!(reject_untrusted_time(Some(u64::MAX)), None);
    }

    #[test]
    fn raw_rtc_rollback_cannot_create_a_trusted_floor() {
        assert_eq!(reject_untrusted_time(Some(1_800_000_000)), None);
        assert_eq!(reject_untrusted_time(Some(1_700_000_000)), None);
    }

    #[test]
    fn default_tls_clock_is_unavailable() {
        assert_eq!(observe(), None);
        assert_eq!(ViTlsClock::now(), None);
    }
}
