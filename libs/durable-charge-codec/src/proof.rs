//! Fixed native signed statements, not mutation permissions or durability proofs.
//!
//! Pins and every expected binding must come from an independently admitted current
//! policy. This module neither selects that policy nor supplies a signing endpoint.
use crate::record::{schema_id, Record};
use crate::Limits;
use ed25519_compact::{PublicKey, Signature};

/// Exact Ed25519 message prefix, including its terminal NUL.
pub const SIGNING_DOMAIN: &[u8] = b"cellos/native/signed-fact/v1\0";
pub const HEADER_LEN: usize = 32;
pub const BODY_LEN: usize = 1564;
pub const SIGNED_LEN: usize = HEADER_LEN + BODY_LEN;
pub const CARRIER_LEN: usize = SIGNED_LEN + 64;

macro_rules! discriminants {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u16)]
        pub enum $name { $($variant = $value),+ }
        impl $name {
            fn parse(value: u16) -> Result<Self, ProofError> {
                match value { $($value => Ok(Self::$variant),)+ _ => Err(ProofError::Discriminant) }
            }
        }
    };
}

discriminants!(Role {
    OwnerCommit = 1, NonregressionFloor = 2, WriterFence = 3,
    CompleteCoverage = 4, Destination = 5, Closure = 6,
    ResourceOutcome = 7, StateEffectOutcome = 8, Succession = 9,
    Issuer = 10, Lookup = 11, BootLifecycle = 12,
});
discriminants!(Purpose {
    CommitResult = 1, Floor = 2, InstallFence = 3, CompleteCoverage = 4,
    DestinationBinding = 5, FullClosure = 6, LocalReservation = 7,
    WholeVectorApplied = 8, ShrinkQuiescence = 9, NoApplication = 10,
    AuthorizedSuccession = 11, ImmutableMaximumProfile = 12,
    CurrentLookup = 13, BootGone = 14, GenesisAuthorization = 15,
    CompleteReconstruction = 16,
});
discriminants!(Lane { Native = 1, Fixture = 2 });
discriminants!(Side { Local = 0, Source = 1, Destination = 2 });
discriminants!(Status {
    Committed = 1, TerminalNonaccepted = 2, Unresolved = 3,
    RecoveryFailed = 4, LowerBound = 5, FenceInstalled = 6,
    Complete = 7, Bound = 8, Quiescent = 9, Authorized = 10,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofError {
    Length, Limit, Magic, Version, Suite, Reserved, Discriminant,
    RolePurpose, Identity, Counter, Context, PublicPin, Record, Signature,
}

/// A full owner identity. Provenance and current custody are separate fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub principal: [u8; 32],
    pub incarnation: [u8; 32],
    pub home: [u8; 32],
    pub epoch: u64,
}

/// Exact retained DCO1 reference; the schema is additionally bound in the carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordBinding {
    pub encoded_length: u32,
    pub kind: u16,
    pub digest: [u8; 32],
}

/// All fields are mandatory. Zero O/A is legal only where the bound DCO1 kind
/// permits it. Zero operation keys explicitly mean inapplicable, not permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings {
    pub role: Role,
    pub purpose: Purpose,
    pub lane: Lane,
    pub side: Side,
    pub status: Status,
    pub producer: Owner,
    pub key_id: [u8; 32],
    pub roster_id: [u8; 32],
    pub roster_revision: u64,
    pub revocation_revision: u64,
    pub audience: [u8; 32],
    pub admission_decision: [u8; 32],
    pub admission_revision: u64,
    pub policy_id: [u8; 32],
    pub policy_revision: u64,
    pub application: [u8; 32],
    pub workload: [u8; 32],
    pub scope_purpose: [u8; 32],
    pub journal: [u8; 32],
    pub genesis: [u8; 32],
    pub volume: [u8; 32],
    pub floor_namespace: [u8; 32],
    pub obligation: [u8; 16],
    pub adoption: [u8; 16],
    pub transaction: [u8; 16],
    pub origin_journal: [u8; 32],
    pub origin_genesis: [u8; 32],
    pub origin_transaction: [u8; 16],
    pub origin_revision: u64,
    pub create_key: [u8; 16],
    pub operation_key: [u8; 16],
    pub release_key: [u8; 16],
    pub transition_map: [u8; 32],
    pub request_schema: [u8; 32],
    pub request_length: u64,
    pub request_digest: [u8; 32],
    pub effect: [u8; 32],
    pub profile: [u8; 32],
    pub destination: [u8; 32],
    pub requesting_endpoint: [u8; 32],
    pub responding_endpoint: [u8; 32],
    pub correlation: [u8; 32],
    pub provenance: Owner,
    pub custodian: Owner,
    pub writer_epoch: u64,
    pub authority_fence: u64,
    pub controller_epoch: u64,
    pub custody_revision: u64,
    /// Zero explicitly denotes a pre-successor/no-allocation statement.
    pub allocation_revision: u64,
    pub input_revision: u64,
    pub output_revision: u64,
    pub input_floor_revision: u64,
    pub output_floor_revision: u64,
    pub input_record: RecordBinding,
    pub record: RecordBinding,
    pub record_schema: [u8; 32],
    pub coverage_roster: [u8; 32],
    pub coverage_revision: u64,
    pub coverage_count: u32,
    pub checkpoint: [u8; 32],
    pub backend_admission: [u8; 32],
    pub failure_model: [u8; 32],
}

/// Caller-owned current expectations, never inferred from the presented carrier.
/// Supplying this value asserts no admission: native consumers must establish it
/// through their real policy, current-roster, custody, floor and gate protocols.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyContext {
    pub expected: Bindings,
    pub public_key: [u8; 32],
    /// Explicit finite admission bound; there is no implicit production default.
    pub max_carrier_bytes: usize,
    pub record_limits: Limits,
}

/// An authenticated statement by the exact assigned producer, with borrowed
/// carrier/record bytes and borrowed expectations. Deliberately no permit method.
#[derive(Debug)]
pub struct VerifiedFact<'a, 'c> {
    carrier: &'a [u8],
    record_bytes: &'a [u8],
    bindings: &'c Bindings,
}
impl<'a, 'c> VerifiedFact<'a, 'c> {
    pub fn carrier(&self) -> &'a [u8] { self.carrier }
    pub fn record_bytes(&self) -> &'a [u8] { self.record_bytes }
    pub fn statement(&self) -> &'c Bindings { self.bindings }
    pub fn role(&self) -> Role { self.bindings.role }
    pub fn purpose(&self) -> Purpose { self.bindings.purpose }
    pub fn status(&self) -> Status { self.bindings.status }
}

fn role_purpose(role: Role, purpose: Purpose) -> bool {
    matches!((role, purpose),
        (Role::OwnerCommit, Purpose::CommitResult)
        | (Role::NonregressionFloor, Purpose::Floor)
        | (Role::WriterFence, Purpose::InstallFence)
        | (Role::CompleteCoverage, Purpose::CompleteCoverage | Purpose::CompleteReconstruction)
        | (Role::Destination, Purpose::DestinationBinding | Purpose::LocalReservation | Purpose::WholeVectorApplied)
        | (Role::Closure, Purpose::FullClosure)
        | (Role::ResourceOutcome | Role::StateEffectOutcome,
            Purpose::WholeVectorApplied | Purpose::ShrinkQuiescence | Purpose::NoApplication)
        | (Role::Succession, Purpose::AuthorizedSuccession)
        | (Role::Issuer, Purpose::ImmutableMaximumProfile | Purpose::GenesisAuthorization)
        | (Role::Lookup, Purpose::CurrentLookup)
        | (Role::BootLifecycle, Purpose::BootGone))
}

struct Check<'a> { bytes: &'a [u8], at: usize }
impl Check<'_> {
    fn field(&mut self, expected: &[u8]) -> Result<(), ProofError> {
        let end = self.at.checked_add(expected.len()).ok_or(ProofError::Length)?;
        if self.bytes.get(self.at..end) != Some(expected) { return Err(ProofError::Context); }
        self.at = end;
        Ok(())
    }
    fn identity(&mut self, expected: &[u8]) -> Result<(), ProofError> {
        if expected.iter().all(|b| *b == 0) { return Err(ProofError::Identity); }
        self.field(expected)
    }
    fn counter(&mut self, expected: u64) -> Result<(), ProofError> {
        if expected == 0 || expected == u64::MAX { return Err(ProofError::Counter); }
        self.field(&expected.to_le_bytes())
    }
    fn owner(&mut self, expected: &Owner) -> Result<(), ProofError> {
        self.identity(&expected.principal)?;
        self.identity(&expected.incarnation)?;
        self.identity(&expected.home)?;
        self.counter(expected.epoch)
    }
    fn reference(&mut self, expected: &RecordBinding) -> Result<(), ProofError> {
        if expected.encoded_length < 248 || schema_id(expected.kind).is_none() {
            return Err(ProofError::Record);
        }
        self.field(&expected.encoded_length.to_le_bytes())?;
        self.field(&expected.kind.to_le_bytes())?;
        if self.bytes.get(self.at..self.at + 2) != Some(&[0, 0]) { return Err(ProofError::Reserved); }
        self.field(&[0, 0])?;
        self.identity(&expected.digest)
    }
}

/// Verify the fixed carrier and exact record against independently supplied
/// expectations. Non-success statuses remain statements, never safe absence.
pub fn verify<'a, 'c>(
    carrier: &'a [u8], record: &Record<'a>, context: &'c VerifyContext,
) -> Result<VerifiedFact<'a, 'c>, ProofError> {
    if carrier.len() > context.max_carrier_bytes { return Err(ProofError::Limit); }
    if carrier.len() != CARRIER_LEN { return Err(ProofError::Length); }
    if &carrier[..4] != b"NCF1" { return Err(ProofError::Magic); }
    if carrier[4..6] != 1u16.to_le_bytes() { return Err(ProofError::Version); }
    if carrier[6..8] != 1u16.to_le_bytes() { return Err(ProofError::Suite); }
    if carrier[8..12] != (CARRIER_LEN as u32).to_le_bytes()
        || carrier[12..16] != (BODY_LEN as u32).to_le_bytes() { return Err(ProofError::Length); }
    if carrier[26..28] != 1u16.to_le_bytes() { return Err(ProofError::Discriminant); }
    if carrier[28..32] != [0; 4] { return Err(ProofError::Reserved); }
    let read16 = |at| u16::from_le_bytes([carrier[at], carrier[at + 1]]);
    let role = Role::parse(read16(16))?;
    let purpose = Purpose::parse(read16(18))?;
    let lane = Lane::parse(read16(20))?;
    let side = Side::parse(read16(22))?;
    let status = Status::parse(read16(24))?;
    if !role_purpose(role, purpose) { return Err(ProofError::RolePurpose); }
    let b = &context.expected;
    if (role, purpose, lane, side, status) != (b.role, b.purpose, b.lane, b.side, b.status) {
        return Err(ProofError::Context);
    }
    let mut c = Check { bytes: &carrier[..SIGNED_LEN], at: HEADER_LEN };
    c.owner(&b.producer)?;
    c.identity(&b.key_id)?;
    c.identity(&b.roster_id)?;
    c.counter(b.roster_revision)?;
    c.counter(b.revocation_revision)?;
    c.identity(&b.audience)?;
    c.identity(&b.admission_decision)?;
    c.counter(b.admission_revision)?;
    c.identity(&b.policy_id)?;
    c.counter(b.policy_revision)?;
    c.identity(&b.application)?;
    c.identity(&b.workload)?;
    c.identity(&b.scope_purpose)?;
    c.identity(&b.journal)?;
    c.identity(&b.genesis)?;
    c.identity(&b.volume)?;
    c.identity(&b.floor_namespace)?;
    c.field(&b.obligation)?;
    c.field(&b.adoption)?;
    c.identity(&b.transaction)?;
    if b.obligation == [0; 16] {
        if b.origin_journal != [0; 32] || b.origin_genesis != [0; 32]
            || b.origin_transaction != [0; 16] || b.origin_revision != 0
            || b.create_key != [0; 16] || b.operation_key != [0; 16]
            || b.release_key != [0; 16] { return Err(ProofError::Identity); }
        c.field(&b.origin_journal)?;
        c.field(&b.origin_genesis)?;
        c.field(&b.origin_transaction)?;
        c.field(&b.origin_revision.to_le_bytes())?;
        c.field(&b.create_key)?;
    } else {
        c.identity(&b.origin_journal)?;
        c.identity(&b.origin_genesis)?;
        c.identity(&b.origin_transaction)?;
        c.counter(b.origin_revision)?;
        c.identity(&b.create_key)?;
    }
    c.field(&b.operation_key)?;
    c.field(&b.release_key)?;
    if b.adoption == [0; 16] {
        if b.transition_map != [0; 32] { return Err(ProofError::Identity); }
        c.field(&b.transition_map)?;
    } else {
        c.identity(&b.transition_map)?;
    }
    c.identity(&b.request_schema)?;
    c.field(&b.request_length.to_le_bytes())?;
    c.identity(&b.request_digest)?;
    c.identity(&b.effect)?;
    c.identity(&b.profile)?;
    c.identity(&b.destination)?;
    c.identity(&b.requesting_endpoint)?;
    c.identity(&b.responding_endpoint)?;
    c.identity(&b.correlation)?;
    c.owner(&b.provenance)?;
    c.owner(&b.custodian)?;
    c.counter(b.writer_epoch)?;
    c.counter(b.authority_fence)?;
    c.counter(b.controller_epoch)?;
    c.counter(b.custody_revision)?;
    if b.allocation_revision == u64::MAX { return Err(ProofError::Counter); }
    c.field(&b.allocation_revision.to_le_bytes())?;
    c.counter(b.input_revision)?;
    c.counter(b.output_revision)?;
    c.counter(b.input_floor_revision)?;
    c.counter(b.output_floor_revision)?;
    c.reference(&b.input_record)?;
    c.reference(&b.record)?;
    c.identity(&b.record_schema)?;
    c.identity(&b.coverage_roster)?;
    c.counter(b.coverage_revision)?;
    c.field(&b.coverage_count.to_le_bytes())?;
    c.identity(&b.checkpoint)?;
    c.identity(&b.backend_admission)?;
    c.identity(&b.failure_model)?;
    if context.public_key.iter().all(|x| *x == 0) { return Err(ProofError::PublicPin); }
    if c.bytes.get(c.at..c.at + 32) != Some(context.public_key.as_slice()) {
        return Err(ProofError::PublicPin);
    }
    c.field(&context.public_key)?;
    // Record has public structural fields: reparse its actual retained bytes,
    // rather than treating caller-supplied metadata as an authority seal.
    let parsed = crate::decode(record.bytes, &context.record_limits).map_err(|_| ProofError::Record)?;
    let record = &parsed;
    if c.at != SIGNED_LEN { return Err(ProofError::Length); }
    let h = &record.header;
    if record.kind != b.record.kind || record.bytes.len() != b.record.encoded_length as usize
        || record.digest != b.record.digest || schema_id(record.kind) != Some(b.record_schema)
        || h.journal_ledger != b.journal || h.journal_genesis != b.genesis
        || h.obligation_id != b.obligation || h.adoption_id != b.adoption
        || h.local_transaction != b.transaction || h.local_revision != b.output_revision
        || h.journal_writer_epoch != b.writer_epoch
        || h.previous_record_digest != b.input_record.digest {
        return Err(ProofError::Record);
    }
    let p = record.payload;
    if !matches!(record.kind, 32 | 34 | 52) {
        if p[..32] != b.application || p[32..64] != b.workload
            || p[64..96] != b.scope_purpose
            || p[96..100] != (b.lane as u32).to_le_bytes()
            || p[100..108] != b.policy_revision.to_le_bytes() {
            return Err(ProofError::Record);
        }
    }
    if record.kind == 32 && p[..32] != b.profile { return Err(ProofError::Record); }
    if matches!(record.kind, 1 | 10 | 48 | 49 | 50 | 57 | 58) {
        if p[108..140] != b.origin_journal || p[140..172] != b.origin_genesis
            || p[172..188] != b.origin_transaction
            || p[188..196] != b.origin_revision.to_le_bytes()
            || p[196..212] != b.create_key { return Err(ProofError::Record); }
    }
    if matches!(record.kind, 48 | 58) {
        if p[436..444] != b.allocation_revision.to_le_bytes()
            || p[444..452] != b.custody_revision.to_le_bytes() {
            return Err(ProofError::Record);
        }
        let mut owner = Check { bytes: p, at: 252 };
        owner.owner(&b.custodian).map_err(|_| ProofError::Record)?;
    }
    if b.input_revision.checked_add(1) != Some(b.output_revision) {
        return Err(ProofError::Counter);
    }
    let key = PublicKey::new(context.public_key);
    let signature = Signature::from_slice(&carrier[SIGNED_LEN..]).map_err(|_| ProofError::Signature)?;
    let mut verifier = key.verify_incremental(&signature).map_err(|_| ProofError::Signature)?;
    verifier.absorb(SIGNING_DOMAIN);
    verifier.absorb(&carrier[..SIGNED_LEN]);
    verifier.verify().map_err(|_| ProofError::Signature)?;
    Ok(VerifiedFact { carrier, record_bytes: record.bytes, bindings: b })
}
