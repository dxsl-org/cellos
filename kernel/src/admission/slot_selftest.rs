// SPDX-License-Identifier: MPL-2.0
//! Named tests for the A/B owner slot parser and verification.

use super::slot_format::{parse_slot, SlotParseError, FLAG_COMMITTED, SLOT_MAGIC};
use super::{AdmissionDecision, FloorPortOutcome, FloorState, SlotId, SlotObservation};
use alloc::vec::Vec;

#[cfg(feature = "dev-signing-key")]
fn build_valid_test_slot_a() -> Vec<u8> {
    let mut v = Vec::with_capacity(248);
    // Header (144 bytes)
    v.extend_from_slice(SLOT_MAGIC);
    v.extend_from_slice(&1u16.to_le_bytes()); // version 1
    v.extend_from_slice(&FLAG_COMMITTED.to_le_bytes()); // committed
    v.push(0); // Slot A
    v.extend_from_slice(&[0u8; 3]); // reserved
    v.extend_from_slice(&2u64.to_le_bytes()); // generation 2
    v.extend_from_slice(&1u64.to_le_bytes()); // expected generation 1
    v.extend_from_slice(&[2u8; 16]); // tx_id
    v.extend_from_slice(&[1u8; 16]); // backend_id
    v.extend_from_slice(&[2u8; 32]); // intent_digest
    v.extend_from_slice(&[3u8; 32]); // provenance_envelope_digest
    v.extend_from_slice(&1u32.to_le_bytes()); // admission count = 1
    v.extend_from_slice(&0u32.to_le_bytes()); // reserved
    assert_eq!(v.len(), 144);

    // Entry 0 (40 bytes)
    v.extend_from_slice(&[0xAAu8; 32]); // admitted elf hash
    v.extend_from_slice(&0u32.to_le_bytes()); // flags
    v.extend_from_slice(&0u32.to_le_bytes()); // reserved
    assert_eq!(v.len(), 184);

    // Precomputed signature using dev owner key (seed [0x4F; 32])
    const SIG: [u8; 64] = [
        0xcf, 0x5e, 0xc2, 0x51, 0x4c, 0x43, 0xaa, 0x18, 0xcf, 0x68, 0x00, 0x31, 0x06, 0x27, 0x30,
        0x0e, 0xd1, 0x43, 0xa3, 0x3c, 0xec, 0xcd, 0x13, 0xb3, 0xc7, 0xfe, 0x32, 0x10, 0x8b, 0xf8,
        0xcb, 0x8f, 0x07, 0x0d, 0x1f, 0x17, 0xf2, 0x62, 0x7d, 0x7f, 0x7b, 0xc3, 0x0a, 0xba, 0x8a,
        0xf5, 0x4d, 0x66, 0x33, 0xc9, 0x5b, 0x79, 0x71, 0xc3, 0x1f, 0xe2, 0x57, 0x5c, 0xc0, 0x6f,
        0xa1, 0x9c, 0xd9, 0x01,
    ];
    v.extend_from_slice(&SIG);
    assert_eq!(v.len(), 248);
    v
}

#[cfg(feature = "dev-signing-key")]
pub(super) fn run() -> bool {
    let mut ok = true;

    // 1. Parse valid committed Slot A
    let slot_a_bytes = build_valid_test_slot_a();
    let slot_a = match parse_slot(&slot_a_bytes) {
        Ok(parsed) => parsed,
        Err(e) => {
            log::error!("[selftest] OWNER-SLOT: failed to parse valid slot: {:?}", e);
            return false;
        }
    };
    assert_eq!(slot_a.slot_id, SlotId::A);
    assert!(slot_a.is_committed);
    assert_eq!(slot_a.floor_state.generation, 2);
    assert_eq!(slot_a.expected_generation, 1);
    assert!(slot_a.admits_elf(&[0xAAu8; 32]));
    assert!(!slot_a.admits_elf(&[0xBBu8; 32]));

    // 2. Tampered header byte must fail signature verification
    let mut bad_header = slot_a_bytes.clone();
    bad_header[24] ^= 1; // flip generation
    if parse_slot(&bad_header) != Err(SlotParseError::SignatureInvalid) {
        log::error!("[selftest] OWNER-SLOT: mutated header did not reject signature");
        ok = false;
    }

    // 3. Tampered admission entry must fail signature verification
    let mut bad_entry = slot_a_bytes.clone();
    bad_entry[144] ^= 1; // flip admitted ELF byte
    if parse_slot(&bad_entry) != Err(SlotParseError::SignatureInvalid) {
        log::error!("[selftest] OWNER-SLOT: mutated admission entry did not reject signature");
        ok = false;
    }

    // 4. Tampered signature byte must fail verification
    let mut bad_sig = slot_a_bytes.clone();
    bad_sig[247] ^= 1;
    if parse_slot(&bad_sig) != Err(SlotParseError::SignatureInvalid) {
        log::error!("[selftest] OWNER-SLOT: mutated signature did not reject");
        ok = false;
    }

    // 5. Short slice must fail with TooShort
    if parse_slot(&slot_a_bytes[..100]) != Err(SlotParseError::TooShort) {
        log::error!("[selftest] OWNER-SLOT: short slice did not return TooShort");
        ok = false;
    }

    // 6. Bad magic must fail with BadMagic
    let mut bad_magic = slot_a_bytes.clone();
    bad_magic[0] ^= 0xFF;
    if parse_slot(&bad_magic) != Err(SlotParseError::BadMagic) {
        log::error!("[selftest] OWNER-SLOT: bad magic did not return BadMagic");
        ok = false;
    }

    // 7. Integration with AdmissionDecision::decide()
    // Stale partner slot B (generation 1, committed)
    let partner_b = SlotObservation::AuthenticatedCommitted(FloorState {
        generation: 1,
        transaction_id: [1u8; 16],
        intent_digest: [1u8; 32],
        backend_identity: [1u8; 16],
    });
    let floor_outcome = FloorPortOutcome::Authenticated(slot_a.floor_state);
    let decision = super::decide(&floor_outcome, &[slot_a.observation(), partner_b]);
    if decision != AdmissionDecision::Admit(SlotId::A) {
        log::error!(
            "[selftest] OWNER-SLOT: decision did not Admit(SlotId::A): {:?}",
            decision
        );
        ok = false;
    }

    // 8. Integration with common task-creation admission gate (Step 6)
    {
        use super::gate::{evaluate_owner_admission, ADMISSION_REGISTRY};
        use types::ViError;

        // Permissive mode: unadmitted ELF allowed
        if evaluate_owner_admission(&[0x99u8; 32]).is_err() {
            log::error!("[selftest] OWNER-SLOT: permissive mode denied unadmitted ELF");
            ok = false;
        }

        // Enforce production admission with Slot A active
        {
            let mut registry = ADMISSION_REGISTRY.lock();
            registry.update(floor_outcome, Some(slot_a), None);
            registry.set_production_enforced(true);
        }

        // Admitted ELF (0xAA) must succeed
        if evaluate_owner_admission(&[0xAAu8; 32]).is_err() {
            log::error!("[selftest] OWNER-SLOT: admitted ELF was refused in production mode");
            ok = false;
        }

        // Unadmitted ELF (0xBB) must be denied
        if evaluate_owner_admission(&[0xBBu8; 32]) != Err(ViError::PermissionDenied) {
            log::error!("[selftest] OWNER-SLOT: unadmitted ELF was not denied in production mode");
            ok = false;
        }

        // Restore permissive mode
        ADMISSION_REGISTRY.lock().set_production_enforced(false);
    }

    if ok {
        log::info!("[selftest] OWNER-SLOT: PASS (parser, signature validation, tamper rejection, admission)");
    } else {
        log::error!("[selftest] OWNER-SLOT: FAIL");
    }
    ok
}

#[cfg(not(feature = "dev-signing-key"))]
pub(super) fn run() -> bool {
    true
}
