use crate::cli::{CliResult, Flags};
use manifest_core::{
    ComponentKind, ComponentLimit, ExpectedManifest, ManifestLimits, PhysicalRange, StagingLimits,
    MAX_COSE_LEN,
};

#[derive(Clone, Debug)]
pub struct ComponentPolicy {
    pub load_address: u64,
    pub max_load_end: u64,
    pub max_size: u64,
}

#[derive(Clone, Debug)]
pub struct Common {
    pub device_id: [u8; 32],
    pub authority_id: [u8; 32],
    pub approved_loader_sha256: [u8; 32],
    pub boot_epoch: u64,
    pub request_id: u64,
    pub entry_address: u64,
    pub components: [ComponentPolicy; 4],
    pub usable_dram: PhysicalRange,
    pub staging: PhysicalRange,
    pub forbidden: [PhysicalRange; 3],
    pub max_transfer_blocks: u32,
    pub manifest_bound: u32,
    pub max_component_region_length: u64,
}

impl Common {
    pub fn parse(flags: &mut Flags) -> CliResult<Self> {
        let device_id = flags.hex32("--device-id")?;
        let authority_id = flags.hex32("--authority-id")?;
        let approved_loader_sha256 = flags.hex32("--approved-loader-sha256")?;
        let boot_epoch = flags.nonzero("--boot-epoch")?;
        let request_id = flags.nonzero("--request-id")?;
        let entry_address = flags.u64("--entry-address")?;
        let components = [
            policy(flags, "opensbi")?,
            policy(flags, "dtb")?,
            policy(flags, "cellos")?,
            policy(flags, "vifs")?,
        ];
        let usable_dram = range(flags, "--usable-dram-base", "--usable-dram-end")?;
        let forbidden = [
            range(flags, "--loader-range-base", "--loader-range-end")?,
            range(flags, "--stack-range-base", "--stack-range-end")?,
            range(
                flags,
                "--manifest-scratch-range-base",
                "--manifest-scratch-range-end",
            )?,
        ];
        let staging_base = flags.u64("--staging-base")?;
        let staging_size = flags.nonzero("--staging-size")?;
        let staging_end = staging_base
            .checked_add(staging_size)
            .ok_or_else(|| "--staging-base + --staging-size overflows u64".to_owned())?;
        let max_transfer_blocks = u32::try_from(flags.nonzero("--max-transfer-blocks")?)
            .map_err(|_| "--max-transfer-blocks must fit u32".to_owned())?;
        let manifest_bound = u32::try_from(flags.nonzero("--manifest-bound")?)
            .map_err(|_| "--manifest-bound must fit u32".to_owned())?;
        if manifest_bound as usize > MAX_COSE_LEN {
            return Err(format!(
                "--manifest-bound exceeds core maximum {MAX_COSE_LEN}"
            ));
        }
        let max_component_region_length = flags.nonzero("--max-component-region-length")?;
        Ok(Self {
            device_id,
            authority_id,
            approved_loader_sha256,
            boot_epoch,
            request_id,
            entry_address,
            components,
            usable_dram,
            forbidden,
            staging: PhysicalRange {
                base: staging_base,
                end: staging_end,
            },
            max_transfer_blocks,
            manifest_bound,
            max_component_region_length,
        })
    }

    pub fn expected(&self) -> ExpectedManifest {
        ExpectedManifest {
            device_id: self.device_id,
            authority_id: self.authority_id,
            approved_loader_sha256: self.approved_loader_sha256,
            boot_epoch: self.boot_epoch,
            request_id: self.request_id,
        }
    }

    pub fn manifest_limits(&self) -> ManifestLimits {
        let kinds = [
            ComponentKind::OpenSbi,
            ComponentKind::Dtb,
            ComponentKind::Cellos,
            ComponentKind::Vifs,
        ];
        ManifestLimits {
            max_cose_length: self.manifest_bound,
            max_component_region_length: self.max_component_region_length,
            components: core::array::from_fn(|index| ComponentLimit {
                kind: kinds[index],
                load_address: self.components[index].load_address,
                max_load_end: self.components[index].max_load_end,
                max_size: self.components[index].max_size,
                entry_address: if index == 0 { self.entry_address } else { 0 },
            }),
        }
    }

    pub fn staging_limits(&self) -> StagingLimits {
        StagingLimits {
            usable_dram: self.usable_dram,
            staging: self.staging,
            max_transfer_blocks: self.max_transfer_blocks,
            manifest_bound: self.manifest_bound,
        }
    }
}

fn policy(flags: &mut Flags, kind: &str) -> CliResult<ComponentPolicy> {
    let load_address = flags.u64(&format!("--{kind}-load-address"))?;
    let max_load_end = flags.u64(&format!("--{kind}-max-load-end"))?;
    let max_size = flags.nonzero(&format!("--{kind}-max-size"))?;
    if max_load_end <= load_address {
        return Err(format!(
            "--{kind}-max-load-end must exceed --{kind}-load-address"
        ));
    }
    Ok(ComponentPolicy {
        load_address,
        max_load_end,
        max_size,
    })
}

fn range(flags: &mut Flags, base: &str, end: &str) -> CliResult<PhysicalRange> {
    let range = PhysicalRange {
        base: flags.u64(base)?,
        end: flags.u64(end)?,
    };
    if range.end <= range.base {
        return Err(format!("{end} must exceed {base}"));
    }
    Ok(range)
}
