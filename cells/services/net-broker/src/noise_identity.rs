/// Build the Noise prologue with identities ordered by protocol role.
///
/// Both peers must hash `cluster_id || initiator_id || responder_id`; local
/// endpoint order would reverse the identities on the responder and make the
/// handshake transcripts diverge.
///
/// `cluster_id` selects the cluster transcript. `local_node_id` and
/// `remote_node_id` describe this endpoint's view; `is_initiator` converts
/// that view into protocol-role order. The returned 72 bytes contain the
/// little-endian cluster ID followed by initiator and responder NodeIds.
pub fn handshake_prologue(
    cluster_id: u64,
    local_node_id: &[u8; 32],
    remote_node_id: &[u8; 32],
    is_initiator: bool,
) -> [u8; 72] {
    let (initiator_id, responder_id) = if is_initiator {
        (local_node_id, remote_node_id)
    } else {
        (remote_node_id, local_node_id)
    };
    let mut prologue = [0u8; 72];
    prologue[..8].copy_from_slice(&cluster_id.to_le_bytes());
    prologue[8..40].copy_from_slice(initiator_id);
    prologue[40..72].copy_from_slice(responder_id);
    prologue
}

#[cfg(test)]
mod tests;
