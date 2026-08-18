/// SoC-level access constraints applied by the shared SDHCI mechanism.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdhciAccessPolicy {
    pub word_access_only: bool,
    pub minimum_write_spacing_us: u32,
}
