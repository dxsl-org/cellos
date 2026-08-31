use super::*;
use authority_protocol::RelayProfileState;
use core::cell::Cell;
use stm32_authority_journal::PendingEnrollmentSnapshot;

struct Snapshots {
    pending: PendingEnrollmentSnapshot,
    current_revision: u64,
    loads: usize,
    rechecks: usize,
}

impl PendingSnapshotSource for Snapshots {
    type Error = ();

    fn load_pending_snapshot(&mut self) -> Result<PendingEnrollmentSnapshot, Self::Error> {
        self.loads += 1;
        Ok(self.pending.clone())
    }

    fn current_journal_revision(&mut self) -> Result<u64, Self::Error> {
        self.rechecks += 1;
        Ok(self.current_revision)
    }
}

fn run_transaction(
    revision_delta: u64,
) -> (
    Result<authority_protocol::RelayIntent, ProfileStageError<()>>,
    RelayProfileState,
    usize,
    usize,
    usize,
) {
    let fixture = fixture::chain(0);
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let (mut state, _bank, request) = fixture::uploaded_state(
        &fixture,
        fixture::Store {
            revision: &revision,
            record: &record,
        },
    );
    let pending = fixture::snapshot(&fixture);
    let mut trusted = policy(&fixture, &[], &[]);
    trusted.expected_journal_revision = pending.journal_revision();
    let mut snapshots = Snapshots {
        current_revision: pending.journal_revision() + revision_delta,
        pending,
        loads: 0,
        rechecks: 0,
    };
    let mut reader = Reader {
        first: fixture.tpm.clone(),
        second: None,
        calls: 0,
    };

    let result = validate_and_stage_profile(
        &mut state,
        &request,
        &fixture.profile,
        trusted,
        &mut snapshots,
        &mut reader,
    );
    (
        result,
        state.relay_state(),
        reader.calls,
        snapshots.loads,
        snapshots.rechecks,
    )
}

#[test]
fn transaction_stages_only_after_snapshot_validation_and_revision_recheck() {
    let (result, relay, reads, loads, rechecks) = run_transaction(0);
    assert!(result.is_ok());
    assert!(matches!(relay, RelayProfileState::Staged(_)));
    assert_eq!((reads, loads, rechecks), (2, 1, 1));
}

#[test]
fn transaction_rejects_a_journal_revision_race_before_staging() {
    let (result, relay, reads, loads, rechecks) = run_transaction(1);
    assert_eq!(result, Err(ProfileStageError::JournalChanged));
    assert!(matches!(relay, RelayProfileState::Uploading(_)));
    assert_eq!((reads, loads, rechecks), (2, 1, 1));
}

#[test]
fn staged_retry_returns_persisted_intent_without_media_or_tpm_work() {
    let fixture = fixture::chain(0);
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let (mut state, _bank, request) = fixture::uploaded_state(
        &fixture,
        fixture::Store {
            revision: &revision,
            record: &record,
        },
    );
    let pending = fixture::snapshot(&fixture);
    let retry = fixture::validation_request(
        &fixture,
        7 + fixture
            .profile
            .len()
            .div_ceil(stm32_authority_journal::PROFILE_CHUNK_SIZE) as u64,
        *pending.profile_digest(),
        *pending.tpm_public_digest(),
    );
    let mut trusted = policy(&fixture, &[], &[]);
    trusted.expected_journal_revision = pending.journal_revision();
    let mut snapshots = Snapshots {
        current_revision: pending.journal_revision(),
        pending,
        loads: 0,
        rechecks: 0,
    };
    let mut reader = Reader {
        first: fixture.tpm.clone(),
        second: None,
        calls: 0,
    };
    let first = validate_and_stage_profile(
        &mut state,
        &request,
        &fixture.profile,
        trusted,
        &mut snapshots,
        &mut reader,
    )
    .unwrap();
    snapshots.loads = 0;
    snapshots.rechecks = 0;
    reader.calls = 0;

    let recovered = validate_and_stage_profile(
        &mut state,
        &retry,
        &fixture.profile,
        trusted,
        &mut snapshots,
        &mut reader,
    );

    assert_eq!(recovered, Ok(first));
    assert_eq!(
        (reader.calls, snapshots.loads, snapshots.rechecks),
        (0, 0, 0)
    );
    assert!(matches!(state.relay_state(), RelayProfileState::Staged(_)));
}
