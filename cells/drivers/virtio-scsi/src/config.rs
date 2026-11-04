//! `virtio_scsi_config` (virtio 1.2 §5.6.4): volatile read and validation
//! into the limits the request queue runs with.

use crate::ring::{DATA_LEN, MAX_CDB, MAX_SENSE};
use virtio_drivers::transport::Transport;

/// Targets probed: `0..=min(max_target, MAX_PROBE_TARGET)`.
const MAX_PROBE_TARGET: u16 = 7;
/// Smallest CDB area that carries READ(16)/WRITE(16).
const MIN_CDB: usize = 16;
/// `max_sectors` unit.
const SECTOR: u32 = 512;

/// The 36-byte configuration structure as little-endian 32-bit words:
/// 0 `num_queues`, 1 `seg_max`, 2 `max_sectors`, 3 `cmd_per_lun`,
/// 4 `event_info_size`, 5 `sense_size`, 6 `cdb_size`,
/// 7 `max_channel` (low half) / `max_target` (high half), 8 `max_lun`.
type ConfigWords = [u32; 9];

/// The configuration fields the cell uses.
pub struct RawConfig {
    pub num_queues: u32,
    pub max_sectors: u32,
    pub sense_size: u32,
    pub cdb_size: u32,
    pub max_target: u16,
}

/// Reads the device configuration structure word by word.
pub fn read(transport: &impl Transport) -> Result<RawConfig, virtio_drivers::Error> {
    let words = transport
        .config_space::<ConfigWords>()?
        .as_ptr()
        .cast::<u32>();
    let word = |index: usize| {
        // SAFETY: `config_space` proved the mapped device-config region holds
        // `ConfigWords` at a 4-aligned address and stays mapped while
        // `transport` lives; `index < 9`. Volatile: the device owns the fields.
        u32::from_le(unsafe { words.add(index).read_volatile() })
    };
    Ok(RawConfig {
        num_queues: word(0),
        max_sectors: word(2),
        sense_size: word(5),
        cdb_size: word(6),
        max_target: (word(7) >> 16) as u16,
    })
}

/// Validated limits the cell uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub cdb_size: usize,
    pub sense_size: usize,
    /// Highest target probed.
    pub last_target: u8,
    /// Largest data phase per command: `max_sectors * 512`, capped by the
    /// bounce buffer.
    pub max_transfer_bytes: u32,
}

/// A configuration the cell cannot drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// `num_queues == 0`: no request queue.
    NoRequestQueue,
    /// `cdb_size` below 16 or above the request area.
    CdbSize(u32),
    /// `sense_size` above the response area.
    SenseSize(u32),
    /// `max_sectors == 0`: no data phase may be issued.
    NoTransfer,
}

pub fn limits(cfg: &RawConfig) -> Result<Limits, ConfigError> {
    if cfg.num_queues == 0 {
        return Err(ConfigError::NoRequestQueue);
    }
    let cdb_size = cfg.cdb_size as usize;
    if !(MIN_CDB..=MAX_CDB).contains(&cdb_size) {
        return Err(ConfigError::CdbSize(cfg.cdb_size));
    }
    let sense_size = cfg.sense_size as usize;
    if sense_size > MAX_SENSE {
        return Err(ConfigError::SenseSize(cfg.sense_size));
    }
    if cfg.max_sectors == 0 {
        return Err(ConfigError::NoTransfer);
    }
    Ok(Limits {
        cdb_size,
        sense_size,
        last_target: cfg.max_target.min(MAX_PROBE_TARGET) as u8,
        max_transfer_bytes: cfg.max_sectors.saturating_mul(SECTOR).min(DATA_LEN as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QEMU `virtio-scsi-pci` defaults.
    fn qemu() -> RawConfig {
        RawConfig {
            num_queues: 1,
            max_sectors: 0xFFFF,
            sense_size: 96,
            cdb_size: 32,
            max_target: 255,
        }
    }

    #[test]
    fn qemu_defaults_are_accepted_and_capped() {
        let limits = limits(&qemu()).unwrap();
        assert_eq!(limits.cdb_size, 32);
        assert_eq!(limits.sense_size, 96);
        assert_eq!(limits.last_target, 7);
        assert_eq!(limits.max_transfer_bytes, DATA_LEN as u32);
        let small = RawConfig {
            max_sectors: 1,
            max_target: 2,
            ..qemu()
        };
        let limits = super::limits(&small).unwrap();
        assert_eq!((limits.max_transfer_bytes, limits.last_target), (512, 2));
    }

    #[test]
    fn unusable_configs_are_refused() {
        assert_eq!(
            limits(&RawConfig {
                num_queues: 0,
                ..qemu()
            }),
            Err(ConfigError::NoRequestQueue)
        );
        assert_eq!(
            limits(&RawConfig {
                cdb_size: 10,
                ..qemu()
            }),
            Err(ConfigError::CdbSize(10))
        );
        assert_eq!(
            limits(&RawConfig {
                cdb_size: 4096,
                ..qemu()
            }),
            Err(ConfigError::CdbSize(4096))
        );
        assert_eq!(
            limits(&RawConfig {
                sense_size: 4096,
                ..qemu()
            }),
            Err(ConfigError::SenseSize(4096))
        );
        assert_eq!(
            limits(&RawConfig {
                max_sectors: 0,
                ..qemu()
            }),
            Err(ConfigError::NoTransfer)
        );
    }
}
