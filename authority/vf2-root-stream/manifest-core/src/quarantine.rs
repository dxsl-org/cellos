use crate::{validate_staging, ManifestLimits, PhysicalRange, Result, StagingLimits};
use zeroize::Zeroize;

/// Platform hook invoked after logical clearing.
///
/// Host harnesses record ordering only. A hardware loader must implement the
/// ADR-0010 uncached or clean-to-coherency path and architectural fence.
pub trait CleanupHook {
    /// Makes the completed zero writes visible at the profile's coherency point.
    fn make_visible(&mut self, bytes: &mut [u8]) -> Result<()>;
}

/// Validated logical quarantine lifecycle for host and loader integration.
///
/// Construction validates physical bounds before touching `storage`, then clears
/// the complete range. Dropping or finishing after construction clears it again.
pub struct LogicalQuarantine<'a, 'h, H: CleanupHook> {
    storage: &'a mut [u8],
    cleanup: &'h mut H,
    cleaned: bool,
}

impl<'a, 'h, H: CleanupHook> LogicalQuarantine<'a, 'h, H> {
    /// Validates immutable limits before any write and performs the mandatory
    /// pre-clear without requiring bytes from the incoming manifest.
    pub fn prepare(
        storage: &'a mut [u8],
        cleanup: &'h mut H,
        limits: &StagingLimits,
        forbidden: &[PhysicalRange],
        manifest_limits: &ManifestLimits,
    ) -> Result<Self> {
        validate_staging(limits, forbidden, manifest_limits)?;
        let expected =
            usize::try_from(limits.staging.len()?).map_err(|_| crate::Error::InvalidStaging)?;
        if storage.len() != expected {
            return Err(crate::Error::InvalidStaging);
        }
        storage.zeroize();
        cleanup.make_visible(storage)?;
        Ok(Self {
            storage,
            cleanup,
            cleaned: false,
        })
    }

    /// Returns the validated quarantine storage used only for transfer staging.
    pub fn receive_buffer(&mut self) -> &mut [u8] {
        self.storage
    }

    /// Performs complete post-validation cleanup before handoff or reset release.
    pub fn finish(mut self) -> Result<()> {
        self.clear()?;
        self.cleaned = true;
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.storage.zeroize();
        self.cleanup.make_visible(self.storage)
    }
}

impl<H: CleanupHook> Drop for LogicalQuarantine<'_, '_, H> {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.clear();
        }
    }
}
