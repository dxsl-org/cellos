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
