//! The transport seam: one SCSI command in, one completion out.

use crate::error::ScsiError;
use crate::sense::Sense;

/// SAM-5 status codes the disk layer distinguishes.
pub mod status {
    /// Command completed successfully.
    pub const GOOD: u8 = 0x00;
    /// Command failed; sense data describes why.
    pub const CHECK_CONDITION: u8 = 0x02;
    /// Logical unit is busy.
    pub const BUSY: u8 = 0x08;
    /// Logical unit is reserved by another initiator.
    pub const RESERVATION_CONFLICT: u8 = 0x18;
    /// Task set is full.
    pub const TASK_SET_FULL: u8 = 0x28;
    /// Command aborted by another initiator.
    pub const TASK_ABORTED: u8 = 0x40;
}

/// The 8-byte SAM LUN of logical unit 0; also used when REPORT LUNS is
/// unsupported.
pub const LUN0: [u8; 8] = [0; 8];

/// The data phase of one command, carrying the buffer the transport DMAs
/// into or out of.
#[derive(Debug)]
pub enum DataPhase<'a> {
    /// No data transfer.
    None,
    /// Device-to-host transfer into the buffer.
    In(&'a mut [u8]),
    /// Host-to-device transfer from the buffer.
    Out(&'a [u8]),
}

impl DataPhase<'_> {
    /// Buffer length in bytes; 0 for [`DataPhase::None`].
    pub fn len(&self) -> usize {
        match self {
            DataPhase::None => 0,
            DataPhase::In(buf) => buf.len(),
            DataPhase::Out(buf) => buf.len(),
        }
    }

    /// Whether the phase transfers no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What the device returned for one command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    /// SAM status byte (see [`status`]).
    pub status: u8,
    /// Sense decoded with [`Sense::decode`] when the transport received sense
    /// bytes it could decode; `None` otherwise.
    pub sense: Option<Sense>,
    /// Bytes of the data phase NOT transferred. Device-reported and
    /// untrusted: the disk layer rejects values above the buffer length.
    pub residual: u32,
}

/// Executes SCSI commands over one transport (virtio-scsi request queue,
/// PVSCSI ring, ...).
pub trait ScsiExecutor {
    /// Transport failure: queue full, device gone, host/target-level
    /// response codes, or anything else that prevented a SAM status.
    type Error;

    /// Issues `cdb` to `lun` on `target` and blocks until it completes.
    ///
    /// `lun` is the 8-byte SAM LUN as returned by REPORT LUNS; the transport
    /// re-encodes it if its wire format differs. Returns `Ok` whenever the
    /// device produced a SAM status, including CHECK CONDITION; `Err` only
    /// when no status was obtained.
    fn execute(
        &mut self,
        target: u8,
        lun: [u8; 8],
        cdb: &[u8],
        data: DataPhase<'_>,
    ) -> Result<Completion, Self::Error>;
}

/// Lets a caller probe several targets with one executor (`&mut exec`)
/// without giving it up when a probe fails.
impl<T: ScsiExecutor + ?Sized> ScsiExecutor for &mut T {
    type Error = T::Error;

    fn execute(
        &mut self,
        target: u8,
        lun: [u8; 8],
        cdb: &[u8],
        data: DataPhase<'_>,
    ) -> Result<Completion, Self::Error> {
        (**self).execute(target, lun, cdb, data)
    }
}

/// Runs one command and maps non-GOOD outcomes to [`ScsiError`].
///
/// Returns the number of bytes actually transferred (buffer length minus
/// residual).
pub(crate) fn run<E: ScsiExecutor>(
    exec: &mut E,
    target: u8,
    lun: [u8; 8],
    cdb: &[u8],
    data: DataPhase<'_>,
) -> Result<usize, ScsiError<E::Error>> {
    let len = data.len();
    let done = exec
        .execute(target, lun, cdb, data)
        .map_err(ScsiError::Transport)?;
    match (done.status, done.sense) {
        (status::GOOD, _) => {}
        (status::CHECK_CONDITION, Some(sense)) => return Err(ScsiError::CheckCondition(sense)),
        (status::CHECK_CONDITION, None) => return Err(ScsiError::CheckConditionNoSense),
        (other, _) => return Err(ScsiError::Status(other)),
    }
    usize::try_from(done.residual)
        .ok()
        .and_then(|residual| len.checked_sub(residual))
        .ok_or(ScsiError::BadResidual {
            len,
            residual: done.residual,
        })
}
