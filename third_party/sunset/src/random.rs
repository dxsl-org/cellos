// Cellos patch 0001 (see PROVENANCE): upstream draws from `getrandom::SysRng`,
// whose backend on a bare-metal target is chosen by a global `--cfg`. Sunset here draws
// from one source the embedding program installs once. Until a source is installed, and
// whenever it reports failure, every draw fails: there is no fallback generator.
use core::fmt;

use rand_core::{CryptoRng, TryCryptoRng, TryRng, UnwrapErr};
#[allow(unused_imports)]
use {
    crate::error::{Error, Result, TrapBug},
    log::{debug, error, info, log, trace, warn},
};

/// Fills the whole buffer with cryptographic random bytes, or returns `false`.
pub type EntropySource = fn(&mut [u8]) -> bool;

static SOURCE: spin::Once<EntropySource> = spin::Once::new();

/// Install the process entropy source. The first installation wins; later calls
/// return `false` and change nothing.
pub fn set_entropy_source(source: EntropySource) -> bool {
    let mut installed = false;
    SOURCE.call_once(|| {
        installed = true;
        source
    });
    installed
}

#[derive(Debug)]
struct EntropyFailed;

impl fmt::Display for EntropyFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("entropy source unavailable")
    }
}

impl core::error::Error for EntropyFailed {}

struct Source;

impl TryRng for Source {
    type Error = EntropyFailed;

    fn try_next_u32(&mut self) -> Result<u32, EntropyFailed> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn try_next_u64(&mut self) -> Result<u64, EntropyFailed> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), EntropyFailed> {
        match SOURCE.get() {
            Some(fill) if fill(dst) => Ok(()),
            _ => Err(EntropyFailed),
        }
    }
}

impl TryCryptoRng for Source {}

/// Generator for key generation inside the protocol. As upstream (`UnwrapErr`), a
/// failed draw panics, which aborts the program instead of producing a key.
pub(crate) fn rng() -> impl CryptoRng {
    UnwrapErr(Source)
}

pub fn fill_random(buf: &mut [u8]) -> Result<(), Error> {
    Source.try_fill_bytes(buf).map_err(|_| Error::msg("RNG failed"))
}
