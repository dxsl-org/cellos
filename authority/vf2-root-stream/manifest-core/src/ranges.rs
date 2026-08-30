use crate::{Error, ExpectedManifest, Manifest, ManifestLimits, Result, COMPONENT_COUNT};

/// A checked half-open physical range `[base, end)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRange {
    pub base: u64,
    pub end: u64,
}

impl PhysicalRange {
    /// Constructs a nonempty half-open range, rejecting wrap and reversal.
    pub fn new(base: u64, length: u64) -> Result<Self> {
        if length == 0 {
            return Err(Error::ZeroLength);
        }
        let end = base.checked_add(length).ok_or(Error::Overflow)?;
        Ok(Self { base, end })
    }
    pub fn len(self) -> Result<u64> {
        self.end
            .checked_sub(self.base)
            .filter(|n| *n != 0)
            .ok_or(Error::InvalidStaging)
    }
    pub fn contains(self, other: Self) -> bool {
        self.base <= other.base && other.end <= self.end && other.base < other.end
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.base < other.end && other.base < self.end
    }
    pub fn contains_address(self, address: u64) -> bool {
        self.base <= address && address < self.end
    }
}

/// Frozen caller-supplied quarantine bounds; this crate supplies no hardware defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StagingLimits {
    pub usable_dram: PhysicalRange,
    pub staging: PhysicalRange,
    pub max_transfer_blocks: u32,
    pub manifest_bound: u32,
}

/// Checks exact request bindings and all component order, address, size, and overlap limits.
pub fn validate_manifest(
    m: &Manifest,
    expected: &ExpectedManifest,
    limits: &ManifestLimits,
) -> Result<()> {
    if expected.boot_epoch == 0
        || expected.request_id == 0
        || m.boot_epoch != expected.boot_epoch
        || m.request_id != expected.request_id
    {
        return Err(Error::WrongFreshness);
    }
    if m.device_id != expected.device_id
        || m.authority_id != expected.authority_id
        || m.approved_loader_sha256 != expected.approved_loader_sha256
    {
        return Err(Error::WrongIdentity);
    }
    if m.component_region_length == 0
        || m.component_region_length > limits.max_component_region_length
    {
        return Err(Error::LimitExceeded);
    }
    let mut ranges = [PhysicalRange { base: 0, end: 0 }; COMPONENT_COUNT];
    let mut relative_end = 0u64;
    for i in 0..COMPONENT_COUNT {
        let c = m.components[i];
        let limit = limits.components[i];
        if c.kind != limit.kind || c.kind as usize != i + 1 {
            return Err(Error::WrongComponent);
        }
        if c.offset != relative_end
            || c.length == 0
            || c.length > limit.max_size
            || c.load_address != limit.load_address
        {
            return Err(Error::LimitExceeded);
        }
        relative_end = c.offset.checked_add(c.length).ok_or(Error::Overflow)?;
        ranges[i] = PhysicalRange::new(c.load_address, c.length)?;
        if ranges[i].end > limit.max_load_end {
            return Err(Error::LimitExceeded);
        }
        for prior in &ranges[..i] {
            if ranges[i].overlaps(*prior) {
                return Err(Error::RangeOverlap);
            }
        }
    }
    if relative_end != m.component_region_length {
        return Err(Error::WrongRegionLength);
    }
    let entry = limits.components[0].entry_address;
    if m.entry_address != entry || !ranges[0].contains_address(entry) {
        return Err(Error::WrongEntry);
    }
    Ok(())
}

/// Validates immutable pre-receive quarantine and worst-case final windows.
/// It performs no writes and requires no manifest bytes from the transfer.
pub fn validate_staging(
    limits: &StagingLimits,
    forbidden: &[PhysicalRange],
    manifest_limits: &ManifestLimits,
) -> Result<()> {
    limits.usable_dram.len()?;
    let staging_len = limits.staging.len()?;
    if limits.staging.base & 4095 != 0
        || limits.staging.end & 4095 != 0
        || staging_len & 1023 != 0
        || limits.max_transfer_blocks == 0
        || limits.manifest_bound == 0
        || limits.manifest_bound != manifest_limits.max_cose_length
        || manifest_limits.max_component_region_length == 0
        || !limits.usable_dram.contains(limits.staging)
    {
        return Err(Error::InvalidStaging);
    }
    let capacity = u64::from(limits.max_transfer_blocks)
        .checked_mul(1024)
        .ok_or(Error::Overflow)?;
    let logical_max = 4u64
        .checked_add(u64::from(limits.manifest_bound))
        .and_then(|length| length.checked_add(manifest_limits.max_component_region_length))
        .ok_or(Error::Overflow)?;
    if staging_len < capacity || logical_max > capacity {
        return Err(Error::InvalidStaging);
    }
    for (index, range) in forbidden.iter().enumerate() {
        range.len()?;
        if limits.staging.overlaps(*range) {
            return Err(Error::RangeOverlap);
        }
        for prior in &forbidden[..index] {
            if range.overlaps(*prior) {
                return Err(Error::RangeOverlap);
            }
        }
    }
    let mut windows = [PhysicalRange { base: 0, end: 0 }; COMPONENT_COUNT];
    for (index, component) in manifest_limits.components.iter().enumerate() {
        if component.kind as usize != index + 1 || component.max_size == 0 {
            return Err(Error::InvalidStaging);
        }
        windows[index] = PhysicalRange {
            base: component.load_address,
            end: component.max_load_end,
        };
        let window_len = windows[index].len()?;
        if component.max_size > window_len
            || !limits.usable_dram.contains(windows[index])
            || (index == 0 && !windows[index].contains_address(component.entry_address))
            || (index != 0 && component.entry_address != 0)
        {
            return Err(Error::InvalidStaging);
        }
        if limits.staging.overlaps(windows[index]) {
            return Err(Error::RangeOverlap);
        }
        for prior in &windows[..index] {
            if windows[index].overlaps(*prior) {
                return Err(Error::RangeOverlap);
            }
        }
        for immutable in forbidden {
            if windows[index].overlaps(*immutable) {
                return Err(Error::RangeOverlap);
            }
        }
    }
    Ok(())
}
