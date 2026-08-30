/// One canonical positive X.509 serial number denied by trusted policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeniedSerial<'a> {
    bytes: &'a [u8],
}

impl<'a> DeniedSerial<'a> {
    /// Creates a denylist entry from unsigned big-endian serial bytes.
    /// Empty, zero, leading-zero, and over-20-byte values return `None`.
    pub fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > 20 || bytes[0] == 0 {
            None
        } else {
            Some(Self { bytes })
        }
    }

    /// Returns the canonical unsigned big-endian serial bytes.
    pub const fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }
}

/// Authenticated, trusted inputs that close profile validation policy.
#[derive(Clone, Copy)]
pub struct TrustedPolicy<'a> {
    /// DER trust-anchor certificate. It is never accepted inside the uploaded profile.
    pub trust_anchor_der: &'a [u8],
    /// Exact case-sensitive DNS identity required as the leaf's sole SAN.
    pub expected_dns_name: &'a [u8],
    /// Trusted signed Unix time used for every certificate validity check.
    pub signed_time_unix: i64,
    /// Currently authorized physical profile slot.
    pub expected_slot: u8,
    /// Currently authorized monotonic profile generation.
    pub expected_generation: u64,
    /// Currently authorized policy epoch.
    pub expected_policy_epoch: u64,
    /// Exact authenticated journal counter and protected binding revision.
    pub expected_journal_revision: u64,
    /// SHA-256 NodeIds denied by authenticated policy.
    pub denied_node_ids: &'a [[u8; 32]],
    /// Canonical positive serial numbers denied by authenticated policy.
    pub denied_serials: &'a [DeniedSerial<'a>],
}
