use crate::task::cap::CapSet;

/// The pre-annex X/M/W rights every reviewed non-sandbox row keeps (annex
/// S2-T/S2-W §3, D-TW1). A future restricted row is written without it.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;
/// Outbound network client: `Resolve` plus outbound sockets (annex S2-N §5).
pub(super) const NET_CLIENT: CapSet = CapSet {
    net_resolve: true,
    net_connect: true,
    ..LEGACY
};

/// The child ceiling of a reviewed launch target; rows live in
/// [`crate::catalog`] (`user_target` field), the sandbox prefixes yield `EMPTY`.
pub(super) fn reviewed_user_target_ceiling(target: &str) -> Option<CapSet> {
    crate::catalog::user_target(target)
}
