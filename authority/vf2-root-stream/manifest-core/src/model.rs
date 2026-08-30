use crate::{Error, Result};

pub const DIGEST_LEN: usize = 32;
pub const COMPONENT_COUNT: usize = 4;
pub const MAX_PAYLOAD_LEN: usize = 437;
pub const MAX_COSE_LEN: usize = 549;
pub const MAX_SIG_STRUCTURE_LEN: usize = 528;
pub const EXTERNAL_AAD: &[u8] = b"cellos.vf2-root-stream.manifest/v1";
pub const LANE: &str = "DEV_REFERENCE";
pub const EVIDENCE_BOUNDARY: &str = "SOFTWARE_HARNESS";

/// The only admitted component kinds, in required wire order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ComponentKind {
    OpenSbi = 1,
    Dtb = 2,
    Cellos = 3,
    Vifs = 4,
}

impl ComponentKind {
    pub(crate) fn from_u64(value: u64) -> Result<Self> {
        match value {
            1 => Ok(Self::OpenSbi),
            2 => Ok(Self::Dtb),
            3 => Ok(Self::Cellos),
            4 => Ok(Self::Vifs),
            _ => Err(Error::WrongComponent),
        }
    }
}

/// One signed, region-relative component descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Component {
    pub kind: ComponentKind,
    pub offset: u64,
    pub length: u64,
    pub load_address: u64,
    pub sha256: [u8; DIGEST_LEN],
}

/// The complete signed payload represented without allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub request_id: u64,
    pub approved_loader_sha256: [u8; DIGEST_LEN],
    pub component_region_length: u64,
    pub entry_address: u64,
    pub components: [Component; COMPONENT_COUNT],
}

/// Exact signed bindings expected for this one authority request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedManifest {
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub approved_loader_sha256: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub request_id: u64,
}

/// Caller-supplied bounds for one component kind; no defaults are provided.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentLimit {
    pub kind: ComponentKind,
    pub load_address: u64,
    pub max_load_end: u64,
    pub max_size: u64,
    pub entry_address: u64,
}

/// All caller-supplied manifest and component bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestLimits {
    pub max_cose_length: u32,
    pub max_component_region_length: u64,
    pub components: [ComponentLimit; COMPONENT_COUNT],
}
