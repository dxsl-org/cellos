//! ViTlsClock — authenticated-time gate for relay mTLS.
//!
//! The platform/QEMU RTC is not authenticated. Default/fleet builds refuse TLS
//! until protected authenticated time is integrated. The explicit QEMU DEV
//! profile trusts a local host companion to verify the independent time source
//! and retain its floor outside guest snapshots; it does not protect host rollback.

use embedded_tls::TlsClock;

/// Apply the current fail-closed policy to an untrusted RTC observation.
///
/// Raw wall-clock values never become TLS certificate time. The argument is
/// retained only to make that security boundary explicit and testable.
fn reject_untrusted_time(_raw_rtc_secs: Option<u64>) -> Option<u64> {
    None
}

/// Return authenticated certificate time, never raw platform wall time.
pub fn observe() -> Option<u64> {
    #[cfg(feature = "qemu-dev-https")]
    { super::qemu_time::observe() }
    #[cfg(not(feature = "qemu-dev-https"))]
    { reject_untrusted_time(None) }
}

/// Conservative certificate-validity bounds for the selected time authority.
pub fn validity_window() -> Option<(u64, u64)> {
    #[cfg(feature = "qemu-dev-https")]
    { super::qemu_time::validity_window() }
    #[cfg(not(feature = "qemu-dev-https"))]
    { observe().map(|now| (now, now)) }
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
