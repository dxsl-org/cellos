macro_rules! wire_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident : $repr:ty {
        $($(#[$variant_meta:meta])* $variant:ident = $value:expr),+ $(,)?
    }) => {
        $(#[$meta])*
        #[repr($repr)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant = $value),+
        }

        impl TryFrom<$repr> for $name {
            type Error = $repr;

            fn try_from(value: $repr) -> Result<Self, $repr> {
                match value {
                    $(x if x == Self::$variant as $repr => Ok(Self::$variant),)+
                    _ => Err(value),
                }
            }
        }
    };
}

wire_enum!(
    /// Operations supported by KMS ABI version 1.
    pub enum KmsOpcode: u8 {
        /// Bind authority to the live registered broker generation.
        RegisterBrokerInstance = 1,
        /// Read fail-closed identity readiness and public metadata.
        GetNodeIdentityStatus = 2,
        /// Open or provision the stable node identity.
        AcquireNodeIdentity = 3,
        /// Perform static X25519 DH without exporting the private scalar.
        NoiseStaticDh = 4,
        /// Rotate identity under live supervisor authority.
        RotateNodeIdentity = 5,
        /// Bind TLS signing authority to the live service-net generation.
        RegisterServiceNetInstance = 6,
        /// Read independent Relay P-256 readiness and protected metadata.
        GetRelayP256Status = 7,
        /// Sign a TLS 1.3 client CertificateVerify transcript.
        SignTls13ClientCertificateVerify = 8,
        /// Open a supervisor-only relay enrollment and publish its CSR handle.
        BeginRelayEnrollment = 9,
        /// Read the next ordered chunk of the pending canonical relay CSR.
        ReadRelayCsrChunk = 10,
        /// Atomically activate the pending relay generation.
        CommitRelayGeneration = 11,
        /// Destroy the pending relay generation without activating it.
        AbortRelayEnrollment = 12,
        /// Bind a validated service-net profile digest to the pending slot.
        StageRelayProfile = 13,
        /// Read the active generation's public SPKI and its SHA-256.
        GetRelayActivePublicKey = 14,
    }
);
wire_enum!(
    /// Whether a response succeeded or contains a typed error.
    pub enum KmsResponseStatus: u8 { Ok = 0, Error = 1 }
);
wire_enum!(
    /// Root provider serving the active identity.
    pub enum KmsProviderKind: u8 {
        None = 0,
        TestHooks = 1,
        SiloWrapped = 2,
        DiceSealed = 3,
        HardwareSealed = 4,
        /// Hardware relay signing capability.
        HardwareRelay = 5,
    }
);
wire_enum!(
    /// Fail-closed readiness state reported to the broker.
    pub enum NodeIdentityState: u8 {
        Uninitialized = 0,
        Ready = 1,
        RemoteDisabled = 2,
        CloneDetected = 3,
        ProviderUnavailable = 4,
        NoAntiRollback = 5,
        PolicyMismatch = 6,
        BindingInvalid = 7,
    }
);
wire_enum!(
    /// Auditable reason for supervisor-authorized rotation.
    pub enum RotateNodeIdentityReason: u8 {
        CloneRecovery = 1,
        LostKeyRecovery = 2,
        OperatorRekey = 3,
    }
);
wire_enum!(
    /// Cryptographic algorithm owned by one provider capability leaf.
    pub enum KmsKeyAlgorithm: u8 {
        C2cX25519 = 1,
        RelayP256Sha256 = 2,
    }
);
wire_enum!(
    /// Independent readiness for one provider capability leaf.
    pub enum KmsCapabilityReadiness: u8 {
        Unavailable = 0,
        Ready = 1,
        RemoteDisabled = 2,
        ProviderError = 3,
        PolicyMismatch = 4,
    }
);
wire_enum!(
    /// Protected qualification state of the relay signing provider.
    pub enum RelayProviderAssessment: u8 {
        Unassessed = 0,
        DevelopmentReference = 1,
        QualificationTest = 2,
        ProductionQualified = 3,
    }
);
wire_enum!(
    /// Stable numeric service errors; responses never contain free-form secrets.
    pub enum KmsErrorCode: u16 {
        CallerUnattested = 1,
        PermissionDenied = 2,
        BindingRequired = 3,
        BindingStale = 4,
        SecureRootRequired = 5,
        CloneDetected = 6,
        InvalidHandle = 7,
        InvalidPeerKey = 8,
        UnknownOpcode = 9,
        UnsupportedVersion = 10,
        PersistFailed = 11,
        ProviderFailure = 12,
        Busy = 13,
        ServiceBindingRequired = 14,
        ServiceBindingStale = 15,
        RelayUnavailable = 16,
        RelayGenerationMismatch = 17,
        ActiveProfileMismatch = 18,
        InvalidSignature = 19,
        QualificationRequired = 20,
        InvalidRequest = 21,
        /// A relay enrollment is already pending.
        EnrollmentPendingExists = 22,
        /// CSR handle is stale, foreign, or already consumed.
        CsrHandleInvalid = 23,
        /// CSR chunks must be read strictly in order.
        CsrOrderInvalid = 24,
        /// Authenticated time is missing or rolled back below a protected floor.
        TimeUntrusted = 25,
        /// Policy epoch moved backward against the protected monotonic floor.
        PolicyEpochRegressed = 26,
    }
);

/// Structural decoding failure. No state-changing operation may run after one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmsWireError {
    InvalidLength(usize),
    UnsupportedVersion(u8),
    UnknownOpcode(u8),
    UnknownStatus(u8),
    UnknownErrorCode(u16),
    PayloadTooLong(u16),
    NonZeroReserved,
    NonCanonicalPayload,
    UnexpectedErrorCode(u16),
    MissingErrorCode,
}

/// Opaque KMS-local identity handle. Zero is invalid.
#[repr(transparent)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeIdentityHandle(pub u32);

/// KMS-issued broker binding generation. Zero is never remotely ready.
#[repr(transparent)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingEpoch(pub u64);

/// KMS-issued service-net binding generation. Zero is never authorized.
#[repr(transparent)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceNetBindingEpoch(pub u64);
