use crate::cli::{CliResult, Flags};
use crate::common::Common;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug)]
pub struct BundlerArgs {
    pub common: Common,
    pub components: [PathBuf; 4],
    pub seed: PathBuf,
    pub transcript_out: PathBuf,
    pub summary_out: PathBuf,
}

impl BundlerArgs {
    pub fn parse<I>(args: I) -> CliResult<Self>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut flags = Flags::parse(args)?;
        let components = [
            flags.path("--opensbi")?,
            flags.path("--dtb")?,
            flags.path("--cellos")?,
            flags.path("--vifs")?,
        ];
        let seed = flags.path("--seed")?;
        let transcript_out = flags.path("--transcript-out")?;
        let summary_out = flags.path("--summary-out")?;
        let common = Common::parse(&mut flags)?;
        flags.finish()?;
        if transcript_out == summary_out {
            return Err("--transcript-out and --summary-out must differ".to_owned());
        }
        if components.iter().any(|path| path == &seed) {
            return Err("--seed must not name a component input".to_owned());
        }
        Ok(Self {
            common,
            components,
            seed,
            transcript_out,
            summary_out,
        })
    }
}
