use crate::LANE_DEV_REFERENCE;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    OpenBoot = 1,
    ReadCommittedRelayState = 2,
    RequestSignedTime = 3,
    AcceptSignedTime = 4,
    BeginRelayEnrollment = 5,
    ReadRelayCsrChunk = 6,
    ValidateAndStageRelayProfile = 7,
    ConsumeStagedRelayProfile = 8,
    CommitRelayGeneration = 9,
    AbortRelayEnrollment = 10,
    GetRelayActivePublicKey = 11,
    SignTls13ClientCertificateVerify = 12,
    BeginRelayProfileUpload = 13,
    WriteRelayProfileChunk = 14,
}

impl TryFrom<u8> for Operation {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use Operation::*;
        Ok(match value {
            1 => OpenBoot,
            2 => ReadCommittedRelayState,
            3 => RequestSignedTime,
            4 => AcceptSignedTime,
            5 => BeginRelayEnrollment,
            6 => ReadRelayCsrChunk,
            7 => ValidateAndStageRelayProfile,
            8 => ConsumeStagedRelayProfile,
            9 => CommitRelayGeneration,
            10 => AbortRelayEnrollment,
            11 => GetRelayActivePublicKey,
            12 => SignTls13ClientCertificateVerify,
            13 => BeginRelayProfileUpload,
            14 => WriteRelayProfileChunk,
            _ => return Err(value),
        })
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameClass {
    Request = 1,
    Response = 2,
    Fault = 3,
}

pub type MessageKind = FrameClass;

impl TryFrom<u8> for FrameClass {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Request),
            2 => Ok(Self::Response),
            3 => Ok(Self::Fault),
            _ => Err(value),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneTag {
    DevReference = LANE_DEV_REFERENCE,
}

pub struct FrameHeaderLayout;
impl FrameHeaderLayout {
    pub const VERSION: usize = 4;
    pub const LANE: usize = 5;
    pub const CLASS: usize = 6;
    pub const OPERATION: usize = 7;
    pub const PAYLOAD_LEN: usize = 8;
    pub const RESERVED: usize = 10;
    pub const REQUEST_ID: usize = 12;
    pub const AUTHENTICATOR: usize = 20;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub class: FrameClass,
    pub operation: Operation,
    pub payload_len: u16,
    pub request_id: u64,
    pub authenticator: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedFrame<'a> {
    pub header: FrameHeader,
    pub payload: &'a [u8],
}
