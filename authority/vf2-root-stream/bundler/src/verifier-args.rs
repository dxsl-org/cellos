use crate::cli::{CliResult, Flags};
use crate::common::Common;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug)]
pub struct VerifierArgs {
    pub common: Common,
    pub transcript: PathBuf,
    pub public_key: PathBuf,
}

impl VerifierArgs {
    pub fn parse<I>(args: I) -> CliResult<Self>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut flags = Flags::parse(args)?;
        let transcript = flags.path("--transcript")?;
        let public_key = flags.path("--public-key")?;
        let common = Common::parse(&mut flags)?;
        flags.finish()?;
        Ok(Self {
            common,
            transcript,
            public_key,
        })
    }
}
