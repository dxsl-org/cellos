use super::ProtectedAuthorityRecord;
use crate::{
    AuthorityMode, BootState, ProfileUploadIntent, RelayIntent, RelayProfileState, TimeState,
    PROFILE_MAX_LEN,
};

impl ProtectedAuthorityRecord {
    pub(super) fn invariants_hold(&self) -> bool {
        if !mode_boot_valid(self.mode, self.boot)
            || matches!(self.boot, BootState::Open { epoch } if epoch != self.boot_floor)
            || (matches!(self.boot, BootState::Open { .. })
                && self.approved_loader_digest == [0; 32])
            || (self.pending_time.is_some() && self.time != TimeState::Unavailable)
        {
            return false;
        }
        if let TimeState::Valid {
            source_epoch,
            sequence,
            expires_at,
            ..
        } = self.time
        {
            if self.mode != AuthorityMode::Serving
                || source_epoch != self.time_floors.source_epoch
                || sequence != self.time_floors.source_sequence
                || expires_at <= self.time_floors.unix_seconds
            {
                return false;
            }
        }
        let in_flight = matches!(
            self.relay,
            RelayProfileState::Pending { .. }
                | RelayProfileState::Uploading(_)
                | RelayProfileState::Staged(_)
                | RelayProfileState::ReceiptConsumed(_)
                | RelayProfileState::Prepared(_)
                | RelayProfileState::Promoted { .. }
        );
        if let Some(intent) = self.previous_active {
            if !in_flight
                || !self.intent_valid(&intent)
                || intent.generation >= self.generation_floor
            {
                return false;
            }
        }
        match self.relay {
            RelayProfileState::Empty => self.previous_active.is_none(),
            RelayProfileState::Pending {
                generation,
                csr_handle,
                pending_slot,
            } => {
                generation != 0
                    && generation == csr_handle
                    && generation == self.generation_floor
                    && pending_slot == self.expected_pending_slot()
            }
            RelayProfileState::Uploading(intent) => self.upload_valid(&intent),
            RelayProfileState::Staged(intent)
            | RelayProfileState::ReceiptConsumed(intent)
            | RelayProfileState::Prepared(intent) => self.current_intent_valid(&intent),
            RelayProfileState::Promoted { intent, .. } => self.current_intent_valid(&intent),
            RelayProfileState::Active(intent) => {
                self.previous_active.is_none() && self.intent_valid(&intent)
            }
        }
    }

    fn current_intent_valid(&self, intent: &RelayIntent) -> bool {
        self.intent_valid(intent) && intent.generation == self.generation_floor
    }

    fn intent_valid(&self, intent: &RelayIntent) -> bool {
        intent.device_id == self.device_id
            && intent.authority_id == self.authority_id
            && intent.authority_epoch == self.authority_epoch
            && intent.generation != 0
            && intent.generation <= self.generation_floor
            && intent.csr_handle != 0
            && intent.pending_slot <= 1
            && intent.boot_epoch <= self.boot_floor
            && intent.upload_handle != 0
            && intent.profile_len != 0
            && intent.profile_len as usize <= PROFILE_MAX_LEN
    }

    fn upload_valid(&self, intent: &ProfileUploadIntent) -> bool {
        intent.device_id == self.device_id
            && intent.authority_id == self.authority_id
            && intent.authority_epoch == self.authority_epoch
            && intent.boot_epoch <= self.boot_floor
            && intent.generation == self.generation_floor
            && intent.generation != 0
            && intent.csr_handle != 0
            && intent.pending_slot == self.expected_pending_slot()
            && intent.pending_slot <= 1
            && intent.upload_handle != 0
            && intent.profile_len != 0
            && intent.profile_len as usize <= PROFILE_MAX_LEN
            && intent.next_index <= intent.chunk_count()
    }

    fn expected_pending_slot(&self) -> u8 {
        self.previous_active
            .map_or(0, |intent| intent.pending_slot ^ 1)
    }
}

fn mode_boot_valid(mode: AuthorityMode, boot: BootState) -> bool {
    match mode {
        AuthorityMode::Ready => boot == BootState::Closed,
        AuthorityMode::Serving => matches!(boot, BootState::Open { .. }),
        AuthorityMode::Sealed => true,
    }
}
